use anyhow::{anyhow, Context, Result};
use std::path::Path;
use std::process::Command;
use nix::unistd::geteuid;

/// Check if we're running as root
fn is_root() -> bool {
    geteuid().is_root()
}

/// Check if CRIU is available and usable on this system
pub fn check_criu_available() -> Result<()> {
    // Try to run 'criu check'
    let mut cmd = Command::new("criu");
    cmd.arg("check");
    
    // Add --unprivileged flag if not running as root
    if !is_root() {
        cmd.arg("--unprivileged");
    }
    
    let output = cmd
        .output()
        .context("Failed to execute 'criu check'. Is CRIU installed?")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(
            "CRIU check failed. You may need to:\n\
             1. Install CRIU: sudo apt install criu\n\
             2. Set capabilities: sudo setcap cap_checkpoint_restore+eip $(readlink -f $(which criu))\n\
                (For older kernels < 5.9: sudo setcap cap_sys_admin,cap_sys_ptrace,cap_dac_override+eip $(readlink -f $(which criu)))\n\
                Note: Use 'readlink -f' to resolve symlinks\n\
             3. Or run with sufficient privileges\n\
             Error: {}",
            stderr
        ));
    }

    log::info!("CRIU is available and configured correctly");
    Ok(())
}

/// Create a checkpoint of a running process
///
/// # Arguments
/// * `pid` - Process ID to checkpoint
/// * `checkpoint_dir` - Directory to store checkpoint files
/// * `leave_running` - If true, process continues running after checkpoint (pre-dump)
///
/// # Returns
/// Ok(()) if checkpoint was created successfully
pub fn checkpoint_process(pid: i32, checkpoint_dir: &Path, leave_running: bool) -> Result<()> {
    // Create checkpoint directory if it doesn't exist
    std::fs::create_dir_all(checkpoint_dir)
        .context("Failed to create checkpoint directory")?;

    log::info!(
        "Creating CRIU checkpoint for PID {} in {:?} (leave_running: {})",
        pid,
        checkpoint_dir,
        leave_running
    );

    let mut cmd = Command::new("criu");
    cmd.arg("dump")
        .arg("--shell-job") // For processes attached to terminal
        .arg("--tcp-close")
        .arg("--tree")
        .arg(pid.to_string())
        .arg("--images-dir")
        .arg(checkpoint_dir)
        .arg("--log-file")
        .arg("dump.log");

    if leave_running {
        cmd.arg("--leave-running");
    }

    // Add --unprivileged flag if not running as root
    if !is_root() {
        cmd.arg("--unprivileged");
    }

    // Execute CRIU dump
    let output = cmd
        .output()
        .context("Failed to execute 'criu dump'")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        
        // Read the dump log if available for more details
        let log_path = checkpoint_dir.join("dump.log");
        let dump_log = std::fs::read_to_string(&log_path)
            .unwrap_or_else(|_| String::from("(log file not available)"));

        return Err(anyhow!(
            "CRIU checkpoint failed for PID {}\n\
             Stdout: {}\n\
             Stderr: {}\n\
             Dump log:\n{}",
            pid, stdout, stderr, dump_log
        ));
    }

    log::info!("Successfully created checkpoint for PID {}", pid);
    Ok(())
}

/// Restore a process from a checkpoint
///
/// # Arguments
/// * `checkpoint_dir` - Directory containing checkpoint files
///
/// # Returns
/// Ok(pid) - The PID of the restored process
pub fn restore_checkpoint(checkpoint_dir: &Path) -> Result<i32> {
    if !checkpoint_dir.exists() {
        return Err(anyhow!(
            "Checkpoint directory does not exist: {:?}",
            checkpoint_dir
        ));
    }

    log::info!("Restoring CRIU checkpoint from {:?}", checkpoint_dir);

    let mut cmd = Command::new("criu");
    cmd.arg("restore")
        .arg("--shell-job")
        .arg("--images-dir")
        .arg(checkpoint_dir)
        .arg("--log-file")
        .arg("restore.log")
        .arg("-v4")
        .arg("--restore-detached"); // Don't block on completion

    // Add --unprivileged flag if not running as root
    if !is_root() {
        cmd.arg("--unprivileged");
        // Close TCP connections instead of trying to restore them
        // This avoids needing permissions for nftables/iptables manipulation
        cmd.arg("--tcp-close");
        // Allow external unix sockets (don't restore them)
        cmd.arg("--ext-unix-sk");
    }

    // Skip network namespace operations for processes without network access
    cmd.arg("--empty-ns")
        .arg("net");

    // Execute CRIU restore
    let output = cmd
        .output()
        .context("Failed to execute 'criu restore'")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        
        // Read the restore log if available
        let log_path = checkpoint_dir.join("restore.log");
        let restore_log = std::fs::read_to_string(&log_path)
            .unwrap_or_else(|_| String::from("(log file not available)"));

        return Err(anyhow!(
            "CRIU restore failed\n\
             Stdout: {}\n\
             Stderr: {}\n\
             Restore log:\n{}",
            stdout, stderr, restore_log
        ));
    }

    // Parse the PID from CRIU output
    // CRIU typically outputs "Restored process with pid XXXX" or similar
    let stdout = String::from_utf8_lossy(&output.stdout);
    
    // Try to extract PID from output or log file
    let log_path = checkpoint_dir.join("restore.log");
    let restore_log = std::fs::read_to_string(&log_path)
        .unwrap_or_else(|_| String::from(""));
    
    // Look for "Restoring processes (pid: XXXX)" in the log
    for line in restore_log.lines().chain(stdout.lines()) {
        if line.contains("Restoring processes") || line.contains("root task") {
            if let Some(pid_str) = extract_pid_from_line(line) {
                let pid: i32 = pid_str.parse()
                    .context("Failed to parse PID from CRIU output")?;
                log::info!("Successfully restored checkpoint with PID {}", pid);
                return Ok(pid);
            }
        }
    }

    // If we can't find the PID in the output, read it from stats file
    let stats_path = checkpoint_dir.join("stats-restore");
    if stats_path.exists() {
        // Try to get PID from the checkpoint metadata
        // Fall back to reading from core files
        if let Ok(pid) = get_pid_from_checkpoint_files(checkpoint_dir) {
            log::info!("Successfully restored checkpoint with PID {}", pid);
            return Ok(pid);
        }
    }

    Err(anyhow!(
        "CRIU restore succeeded but could not determine restored PID"
    ))
}

/// Extract PID from a CRIU log line
fn extract_pid_from_line(line: &str) -> Option<&str> {
    // Look for patterns like "pid: 1234" or "(1234)"
    if let Some(pos) = line.find("pid:") {
        let after = &line[pos + 4..].trim();
        if let Some(end) = after.find(|c: char| !c.is_numeric()) {
            return Some(&after[..end]);
        }
        return Some(after);
    }
    
    if let Some(start) = line.find('(') {
        if let Some(end) = line[start..].find(')') {
            let pid_str = &line[start + 1..start + end];
            if pid_str.chars().all(|c| c.is_numeric()) {
                return Some(pid_str);
            }
        }
    }
    
    None
}

/// Get the root PID from checkpoint image files
fn get_pid_from_checkpoint_files(checkpoint_dir: &Path) -> Result<i32> {
    // CRIU stores the root PID in pstree.img or inventory.img
    // For now, we'll look for core-*.img files and extract the PID
    for entry in std::fs::read_dir(checkpoint_dir)? {
        let entry = entry?;
        let filename = entry.file_name();
        let filename_str = filename.to_string_lossy();
        
        if filename_str.starts_with("core-") && filename_str.ends_with(".img") {
            // Extract PID from filename like "core-1234.img"
            let pid_str = &filename_str[5..filename_str.len() - 4];
            if let Ok(pid) = pid_str.parse::<i32>() {
                return Ok(pid);
            }
        }
    }
    
    Err(anyhow!("Could not find PID in checkpoint files"))
}

/// Delete a checkpoint directory and all its contents
pub fn delete_checkpoint(checkpoint_dir: &Path) -> Result<()> {
    if checkpoint_dir.exists() {
        std::fs::remove_dir_all(checkpoint_dir)
            .context("Failed to delete checkpoint directory")?;
        log::info!("Deleted checkpoint: {:?}", checkpoint_dir);
    }
    Ok(())
}

/// Get the size of a checkpoint in bytes
pub fn get_checkpoint_size(checkpoint_dir: &Path) -> Result<u64> {
    let mut total_size = 0u64;
    
    if !checkpoint_dir.exists() {
        return Ok(0);
    }
    
    for entry in std::fs::read_dir(checkpoint_dir)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        if metadata.is_file() {
            total_size += metadata.len();
        }
    }
    
    Ok(total_size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_pid_from_line() {
        assert_eq!(
            extract_pid_from_line("Restoring processes (pid: 1234)"),
            Some("1234")
        );
        assert_eq!(
            extract_pid_from_line("root task pid: 5678 ..."),
            Some("5678")
        );
    }
}
