use crate::config::GameFormat;
use crate::detect::*;
use crate::storage::{Checkpoint, GameStorage};
use crate::pty::InterceptedKey;
use crate::pty_embedded::EmbeddedPty;
use crate::criu;
use anyhow::{anyhow, Context, Result};
use crossterm::execute;
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Instant, SystemTime};
use chrono::{DateTime, Local};

/// Interactive Fiction game launcher
///
/// The Launcher is responsible for detecting game formats and executing
/// appropriate interpreters for Interactive Fiction games.
pub struct Launcher {
    // Could hold configuration or state in the future
}

impl Default for Launcher {
    fn default() -> Self {
        Self::new().expect("Launcher initialization should not fail")
    }
}

impl Launcher {
    /// Creates a new Launcher instance
    ///
    /// # Examples
    ///
    /// ```
    /// use glkcli::launcher::Launcher;
    ///
    /// let launcher = Launcher::new().unwrap();
    /// ```
    pub fn new() -> Result<Self> {
        Ok(Launcher {})
    }

    /// Detects the format of a game file
    ///
    /// This method attempts to determine the game format first by examining
    /// the file header (most reliable), then falls back to extension-based
    /// detection if the header doesn't match any known patterns.
    ///
    /// # Arguments
    ///
    /// * `game_path` - Path to the game file to analyze
    ///
    /// # Returns
    ///
    /// Returns the detected [`GameFormat`] or [`GameFormat::Unknown`] if
    /// the format could not be determined.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The game file does not exist
    /// - The file cannot be opened or read
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use glkcli::launcher::Launcher;
    /// use std::path::Path;
    ///
    /// let launcher = Launcher::new().unwrap();
    /// let format = launcher.detect_format(Path::new("zork1.z5")).unwrap();
    /// println!("Detected format: {}", format);
    /// ```
    pub fn detect_format(&self, game_path: &Path) -> Result<GameFormat> {
        if !game_path.exists() {
            return Err(anyhow!("Game file does not exist: {}", game_path.display()));
        }

        // Try header detection first (most reliable)
        let format = detect_format_by_header(game_path)
            .context("Failed to detect format by header")?;
        
        if format != GameFormat::Unknown {
            return Ok(format);
        }

        // Fall back to extension detection
        Ok(detect_format_by_extension(game_path))
    }

    pub fn detect_and_run(&self, game_path: &Path, verbose: bool) -> Result<()> {
        if verbose {
            println!("Info: Detecting game format...");
        }

        let format = self.detect_format(game_path)?;
        if format == GameFormat::Unknown {
            return Err(anyhow!("Unable to detect game format"));
        }

        if verbose {
            println!("Info: Detected format: {}", format);
        }

        self.run_game(game_path, format)
            .context("Failed to run game")
    }

    pub fn run_game(&self, game_path: &Path, format: GameFormat) -> Result<()> {
        let interpreter_name = format.interpreter()
            .ok_or_else(|| anyhow!("No interpreter configured for format: {}", format))?;

        let interpreter_path = self.find_interpreter_path(interpreter_name)
            .ok_or_else(|| anyhow!("Interpreter '{}' not found", interpreter_name))?;

        // Change to the game's directory (where save files will be created)
        let game_dir = game_path.parent()
            .ok_or_else(|| anyhow!("Could not determine game directory"))?;
        
        // Build command arguments
        let mut cmd = Command::new(&interpreter_path);
        
        // Add interpreter-specific flags
        for flag in format.flags() {
            cmd.arg(flag);
        }
        
        // Add game file
        cmd.arg(game_path);

        // Set working directory to the game's directory
        cmd.current_dir(game_dir);

        // Execute the interpreter
        let status = cmd.status()
            .with_context(|| format!("Failed to execute interpreter: {}", interpreter_path.display()))?;

        // Some interpreters return non-zero exit codes even after successful gameplay
        // We only treat it as a real error if the exit code indicates a serious problem
        // (e.g., 127 = command not found, 126 = not executable, negative = killed by signal)
        if !status.success() {
            if let Some(code) = status.code() {
                // Exit codes 1-3 are often used by interpreters for normal gameplay completion
                // Only treat high error codes or execution failures as real errors
                if code >= 100 || code < 0 {
                    return Err(anyhow!("Interpreter exited with error code: {}", code));
                }
                // For codes 1-99, we assume the game ran (user might have quit, saved, etc.)
                // and don't treat it as an error
            } else {
                // No exit code (killed by signal?) - this is a real error
                return Err(anyhow!("Interpreter was terminated by signal"));
            }
        }

        Ok(())
    }

    /// Run a game in an embedded PTY with status bar
    ///
    /// This improves on run_game_with_checkpoints by embedding the PTY in a ratatui
    /// interface with a status bar showing elapsed time and key reminders.
    ///
    /// # Arguments
    /// * `game_path` - Path to the game file
    /// * `format` - Detected game format
    /// * `tuid` - Game identifier for checkpoint storage
    /// * `storage` - Storage manager for checkpoint persistence
    /// * `restore_checkpoint_id` - Optional checkpoint ID to restore from
    /// * `game_name` - Display name for the game
    ///
    /// # Returns
    /// Returns Ok(()) when the game exits normally
    pub fn run_game_embedded(
        &self,
        game_path: &Path,
        format: GameFormat,
        tuid: &str,
        storage: &GameStorage,
        restore_checkpoint_id: Option<String>,
        game_name: &str,
    ) -> Result<()> {
        use crossterm::{
            execute,
            terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
        };
        use ratatui::{backend::CrosstermBackend, Terminal};
        use std::io;

        let interpreter_name = format.interpreter()
            .ok_or_else(|| anyhow!("No interpreter configured for format: {}", format))?;

        let interpreter_path = self.find_interpreter_path(interpreter_name)
            .ok_or_else(|| anyhow!("Interpreter '{}' not found", interpreter_name))?;

        let game_dir = game_path.parent()
            .ok_or_else(|| anyhow!("Could not determine game directory"))?;

        // Verify interpreter exists and is executable
        if !interpreter_path.exists() {
            return Err(anyhow!("Interpreter not found at: {}", interpreter_path.display()));
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = std::fs::metadata(&interpreter_path)
                .context("Failed to read interpreter metadata")?;
            let permissions = metadata.permissions();
            if permissions.mode() & 0o111 == 0 {
                return Err(anyhow!("Interpreter is not executable: {}", interpreter_path.display()));
            }
        }

        log::info!("Using interpreter: {} for format: {}", interpreter_path.display(), format);

        // Prepare interpreter arguments
        let mut args: Vec<String> = format.flags().iter().map(|s| s.to_string()).collect();
        args.push(game_path.display().to_string());
        
        let args_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();

        log::debug!("Launching game in embedded PTY: interpreter={} args={:?} cwd={:?}", 
                   interpreter_path.display(), args_refs, game_dir);

        // Track playtime
        let mut cumulative_playtime: u64 = 0;

        // Handle checkpoint restore
        if let Some(checkpoint_id) = restore_checkpoint_id {
            let checkpoints = storage.get_checkpoints(tuid)?;
            let checkpoint = checkpoints.iter()
                .find(|c| c.id == checkpoint_id)
                .ok_or_else(|| anyhow!("Checkpoint not found: {}", checkpoint_id))?;
            
            log::info!("Restoring from checkpoint: {}", checkpoint.name);
            cumulative_playtime = checkpoint.playtime_seconds;
            
            // Restore the CRIU checkpoint
            log::info!("Restoring CRIU checkpoint from: {:?}", checkpoint.checkpoint_path);
            let restored_pid = criu::restore_checkpoint(&checkpoint.checkpoint_path)?;
            log::info!("Successfully restored process with PID: {}", restored_pid);
            
            // Enable sound after restore using SIGUSR1
            use nix::sys::signal::{kill, Signal};
            use nix::unistd::Pid;
            
            // Give the process a moment to fully restore
            std::thread::sleep(std::time::Duration::from_millis(200));
            
            log::info!("Enabling sound after restore (SIGUSR1 to PID {})", restored_pid);
            if let Err(e) = kill(Pid::from_raw(restored_pid), Signal::SIGUSR1) {
                log::warn!("Failed to send SIGUSR1 to enable sound: {}", e);
                // Don't fail the restore, just warn
            }
            
            // Since CRIU restore runs detached, we need to monitor it differently
            // For now, we'll use a simpler approach: run in foreground without embedded PTY
            println!("\n=== Game Restored from Checkpoint: {} ===", checkpoint.name);
            println!("Press Ctrl+C to exit and return to menu\n");
            
            // Wait for the restored process to complete
            use nix::sys::wait::{waitpid, WaitStatus};
            
            loop {
                match waitpid(Pid::from_raw(restored_pid), None) {
                    Ok(WaitStatus::Exited(_, _)) => {
                        log::info!("Restored process {} exited normally", restored_pid);
                        break;
                    }
                    Ok(WaitStatus::Signaled(_, signal, _)) => {
                        log::info!("Restored process {} was terminated by signal: {:?}", restored_pid, signal);
                        break;
                    }
                    Ok(_) => {
                        // Process still running or other status
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                    Err(e) => {
                        log::error!("Error waiting for restored process: {}", e);
                        break;
                    }
                }
            }
            
            println!("\n=== Game Session Ended ===");
            println!("Returning to menu...\n");
            return Ok(());
        }

        let interpreter_str = interpreter_path.to_str()
            .ok_or_else(|| anyhow!("Interpreter path contains invalid UTF-8: {}", interpreter_path.display()))?;

        // Setup terminal for ratatui FIRST, before spawning PTY
        // This prevents any early output from going to the wrong place
        enable_raw_mode().context("Failed to enable raw mode")?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)
            .context("Failed to enter alternate screen")?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)
            .context("Failed to create terminal")?;

        // Clear the terminal before starting
        terminal.clear()?;

        // NOW create embedded PTY
        let mut pty = EmbeddedPty::spawn(
            interpreter_str,
            &args_refs,
            Some(game_dir),
            game_name.to_string(),
        )?;

        log::info!("Game started with PID: {}", pty.pid());

        // Run the embedded PTY
        let result = self.run_embedded_loop(&mut terminal, &mut pty, tuid, storage, cumulative_playtime);

        // Restore terminal
        disable_raw_mode().context("Failed to disable raw mode")?;
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen
        ).context("Failed to leave alternate screen")?;
        terminal.show_cursor().context("Failed to show cursor")?;

        result
    }

    /// Main event loop for embedded PTY
    fn run_embedded_loop<B: ratatui::backend::Backend>(
        &self,
        terminal: &mut ratatui::Terminal<B>,
        pty: &mut EmbeddedPty,
        tuid: &str,
        storage: &GameStorage,
        initial_playtime: u64,
    ) -> Result<()> {
        use crossterm::event::{self, Event};
        use crossterm::terminal::disable_raw_mode;
        use std::time::Duration;

        let mut cumulative_playtime = initial_playtime;
        let mut session_start = Instant::now();

        loop {
            // Draw the current state
            terminal.draw(|f| pty.render(f))?;

            // Poll for events (faster polling for better responsiveness)
            if event::poll(Duration::from_millis(50))? {
                match event::read()? {
                    Event::Key(key) => {
                        // Handle key input
                        match pty.handle_key(&key)? {
                            Some(InterceptedKey::SaveAndExit) => {
                                log::info!("F1 pressed: Save and exit");
                                
                                let elapsed = session_start.elapsed().as_secs();
                                let total_playtime = cumulative_playtime + elapsed;
                                
                                // Create checkpoint - CRIU will stop the process
                                match self.create_checkpoint_embedded(
                                    pty,
                                    tuid,
                                    storage,
                                    total_playtime,
                                    "Auto-save (F1)",
                                    false  // CRIU stops the process
                                ) {
                                    Ok(_) => {
                                        // Don't send SIGKILL - CRIU already stopped the process
                                        log::info!("Checkpoint created, process stopped by CRIU");
                                    }
                                    Err(e) => {
                                        log::error!("Failed to create checkpoint: {}", e);
                                    }
                                }
                                
                                break;
                            }
                            Some(InterceptedKey::SaveAndContinue) => {
                                log::info!("F2 pressed: Save and continue");
                                
                                let elapsed = session_start.elapsed().as_secs();
                                let total_playtime = cumulative_playtime + elapsed;
                                
                                // Create checkpoint but continue playing
                                if let Err(e) = self.create_checkpoint_embedded(
                                    pty,
                                    tuid,
                                    storage,
                                    total_playtime,
                                    "Quick-save (F2)",
                                    true  // Keep process running
                                ) {
                                    log::error!("Failed to create checkpoint: {}", e);
                                } else {
                                    cumulative_playtime = total_playtime;
                                    session_start = Instant::now();
                                }
                            }
                            Some(InterceptedKey::QuickReload) => {
                                log::info!("F3 pressed: Quick reload");
                                
                                // Get latest checkpoint and restore
                                if let Ok(Some(checkpoint)) = storage.get_latest_checkpoint(tuid) {
                                    log::info!("Restoring from checkpoint: {}", checkpoint.name);
                                    
                                    // Kill the current process
                                    log::info!("Terminating current game process for restore");
                                    let _ = pty.signal(nix::sys::signal::Signal::SIGKILL);
                                    std::thread::sleep(std::time::Duration::from_millis(100));
                                    
                                    // Exit the embedded PTY and restore terminal
                                    use crossterm::terminal::LeaveAlternateScreen;
                                    use std::io;
                                    execute!(io::stdout(), LeaveAlternateScreen)
                                        .context("Failed to leave alternate screen")?;
                                    disable_raw_mode().context("Failed to disable raw mode")?;
                                    
                                    // Restore the checkpoint
                                    match criu::restore_checkpoint(&checkpoint.checkpoint_path) {
                                        Ok(restored_pid) => {
                                            log::info!("Successfully restored process with PID: {}", restored_pid);
                                            println!("\n=== Quick Reload from: {} ===", checkpoint.name);
                                            println!("Press Ctrl+C to exit\n");
                                            
                                            // Wait for restored process
                                            use nix::sys::wait::{waitpid, WaitStatus};
                                            use nix::unistd::Pid;
                                            
                                            loop {
                                                match waitpid(Pid::from_raw(restored_pid), None) {
                                                    Ok(WaitStatus::Exited(_, _)) | Ok(WaitStatus::Signaled(_, _, _)) => break,
                                                    Err(e) => {
                                                        log::error!("Error waiting for restored process: {}", e);
                                                        break;
                                                    }
                                                    _ => std::thread::sleep(std::time::Duration::from_millis(100)),
                                                }
                                            }
                                            
                                            println!("\n=== Restored Session Ended ===");
                                            return Ok(());
                                        }
                                        Err(e) => {
                                            eprintln!("Failed to restore checkpoint: {}", e);
                                            log::error!("Failed to restore checkpoint: {}", e);
                                            return Err(e);
                                        }
                                    }
                                } else {
                                    log::warn!("No checkpoints available to restore");
                                }
                            }
                            Some(InterceptedKey::QuickReloadNoConfirm) => {
                                log::info!("Shift+F3 pressed: Quick reload without confirmation");
                                
                                // Get latest checkpoint and restore immediately
                                if let Ok(Some(checkpoint)) = storage.get_latest_checkpoint(tuid) {
                                    log::info!("Restoring from checkpoint: {}", checkpoint.name);
                                    
                                    // Kill the current process
                                    log::info!("Terminating current game process for restore");
                                    let _ = pty.signal(nix::sys::signal::Signal::SIGKILL);
                                    std::thread::sleep(std::time::Duration::from_millis(100));
                                    
                                    // Exit the embedded PTY and restore terminal
                                    use crossterm::terminal::LeaveAlternateScreen;
                                    use std::io;
                                    execute!(io::stdout(), LeaveAlternateScreen)
                                        .context("Failed to leave alternate screen")?;
                                    disable_raw_mode().context("Failed to disable raw mode")?;
                                    
                                    // Restore the checkpoint
                                    match criu::restore_checkpoint(&checkpoint.checkpoint_path) {
                                        Ok(restored_pid) => {
                                            log::info!("Successfully restored process with PID: {}", restored_pid);
                                            println!("\n=== Quick Reload from: {} ===", checkpoint.name);
                                            println!("Press Ctrl+C to exit\n");
                                            
                                            // Wait for restored process
                                            use nix::sys::wait::{waitpid, WaitStatus};
                                            use nix::unistd::Pid;
                                            
                                            loop {
                                                match waitpid(Pid::from_raw(restored_pid), None) {
                                                    Ok(WaitStatus::Exited(_, _)) | Ok(WaitStatus::Signaled(_, _, _)) => break,
                                                    Err(e) => {
                                                        log::error!("Error waiting for restored process: {}", e);
                                                        break;
                                                    }
                                                    _ => std::thread::sleep(std::time::Duration::from_millis(100)),
                                                }
                                            }
                                            
                                            println!("\n=== Restored Session Ended ===");
                                            return Ok(());
                                        }
                                        Err(e) => {
                                            eprintln!("Failed to restore checkpoint: {}", e);
                                            log::error!("Failed to restore checkpoint: {}", e);
                                            return Err(e);
                                        }
                                    }
                                } else {
                                    log::warn!("No checkpoints available to restore");
                                }
                            }
                            Some(InterceptedKey::QuitPrompt) => {
                                // User confirmed quit (pressed Y after F4)
                                log::info!("Quit confirmed by user");
                                // Terminate the process gracefully
                                if let Err(e) = pty.signal(nix::sys::signal::Signal::SIGTERM) {
                                    log::warn!("Failed to send SIGTERM: {}", e);
                                    let _ = pty.signal(nix::sys::signal::Signal::SIGKILL);
                                }
                                break;
                            }
                            Some(InterceptedKey::ExitPrompt) => {
                                // This shouldn't happen anymore since ESC is no longer intercepted
                                log::info!("Exit prompt (should not occur)");
                                break;
                            }
                            None => {
                                // Key was forwarded to PTY or confirmation was cancelled
                            }
                        }
                    }
                    Event::Resize(cols, rows) => {
                        pty.resize(cols, rows)?;
                    }
                    _ => {}
                }
            }

            // Check if process has exited
            if !pty.is_running() {
                log::info!("Game process has exited");
                break;
            }
        }

        // Only wait if process is still running
        if pty.is_running() {
            log::debug!("Waiting for PTY process to exit");
            pty.wait()?;
        } else {
            log::debug!("PTY process already exited, skipping wait");
        }
        Ok(())
    }

    /// Create a checkpoint for the embedded PTY
    fn create_checkpoint_embedded(
        &self,
        pty: &mut EmbeddedPty,
        tuid: &str,
        storage: &GameStorage,
        playtime_seconds: u64,
        name: &str,
        leave_running: bool,
    ) -> Result<()> {
        // Generate checkpoint ID
        let checkpoint_id = format!("checkpoint_{}", SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)?
            .as_secs());
        
        // Create checkpoint directory
        let checkpoint_dir = storage.create_checkpoint_dir(tuid, &checkpoint_id)?;
        
        log::info!("Creating checkpoint: {} (playtime: {}s, leave_running: {})", name, playtime_seconds, leave_running);

        // CRIU handles process freezing/thawing internally via SIGSTOP/SIGCONT.
        // No pre/post signalling needed on our side.
        criu::checkpoint_process(pty.pid(), &checkpoint_dir, leave_running)?;
        
        // Save checkpoint metadata
        let checkpoint = Checkpoint {
            id: checkpoint_id,
            game_tuid: tuid.to_string(),
            name: name.to_string(),
            checkpoint_path: checkpoint_dir,
            created_at: SystemTime::now(),
            playtime_seconds,
            description: Some(format!(
                "Created on {}",
                DateTime::<Local>::from(SystemTime::now()).format("%Y-%m-%d %H:%M:%S")
            )),
        };
        
        storage.save_checkpoint(checkpoint)?;
        
        log::info!("Checkpoint created successfully");
        Ok(())
    }

    fn find_interpreter_path(&self, interpreter_name: &str) -> Option<PathBuf> {
        // First check configured installation directory (set at compile time)
        // This is typically /usr/share/glkterm/bin for system installations
        if let Some(install_dir) = option_env!("GLKTERM_BIN_DIR") {
            let install_path = PathBuf::from(install_dir).join(interpreter_name);
            if install_path.exists() {
                // Return canonicalized absolute path
                return install_path.canonicalize().ok();
            }
        }

        // Then check if it exists in the current build directory (for development)
        let build_paths = [
            format!("./build/terps/{}", interpreter_name),
            format!("./terps/{}", interpreter_name),
            format!("../terps/{}", interpreter_name),
        ];

        for path_str in &build_paths {
            let path = PathBuf::from(path_str);
            if path.exists() {
                // Return canonicalized absolute path
                if let Ok(canonical) = path.canonicalize() {
                    return Some(canonical);
                }
            }
        }

        // Finally check PATH
        if let Ok(path_env) = env::var("PATH") {
            for dir in path_env.split(':') {
                let full_path = PathBuf::from(dir).join(interpreter_name);
                if full_path.exists() {
                    // PATH entries should already be absolute, but canonicalize anyway
                    return full_path.canonicalize().ok();
                }
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::{NamedTempFile, TempDir};

    fn create_test_game_file(format: GameFormat) -> NamedTempFile {
        let mut file = match format {
            GameFormat::ZCode => NamedTempFile::with_suffix(".z5").unwrap(),
            GameFormat::Glulx => NamedTempFile::with_suffix(".ulx").unwrap(),
            GameFormat::Tads => NamedTempFile::with_suffix(".gam").unwrap(),
            _ => NamedTempFile::new().unwrap(),
        };

        // Write appropriate header
        let header: Vec<u8> = match format {
            GameFormat::ZCode => {
                let mut data = vec![0u8; 32];
                data[0] = 5; // Z-code version 5
                data
            }
            GameFormat::Glulx => {
                let mut data = vec![0u8; 32];
                data[0..4].copy_from_slice(b"Glul");
                data
            }
            GameFormat::Tads => {
                let mut data = vec![0u8; 32];
                data[0..12].copy_from_slice(b"TADS2 bin\x0A\x0D\x1A");
                data
            }
            _ => vec![0u8; 32],
        };

        file.write_all(&header).unwrap();
        file.flush().unwrap();
        file
    }

    #[test]
    fn test_launcher_new() {
        let launcher = Launcher::new();
        assert!(launcher.is_ok());
    }

    #[test]
    fn test_detect_format_zcode() {
        let launcher = Launcher::new().unwrap();
        let file = create_test_game_file(GameFormat::ZCode);
        
        let format = launcher.detect_format(file.path()).unwrap();
        assert_eq!(format, GameFormat::ZCode);
    }

    #[test]
    fn test_detect_format_glulx() {
        let launcher = Launcher::new().unwrap();
        let file = create_test_game_file(GameFormat::Glulx);
        
        let format = launcher.detect_format(file.path()).unwrap();
        assert_eq!(format, GameFormat::Glulx);
    }

    #[test]
    fn test_detect_format_tads() {
        let launcher = Launcher::new().unwrap();
        let file = create_test_game_file(GameFormat::Tads);
        
        let format = launcher.detect_format(file.path()).unwrap();
        assert_eq!(format, GameFormat::Tads);
    }

    #[test]
    fn test_detect_format_nonexistent_file() {
        let launcher = Launcher::new().unwrap();
        let result = launcher.detect_format(Path::new("/nonexistent/game.z5"));
        
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("does not exist"));
    }

    #[test]
    fn test_detect_format_by_extension_fallback() {
        let launcher = Launcher::new().unwrap();
        
        // Create a file with Z-code extension but unknown header
        let mut file = NamedTempFile::with_suffix(".z5").unwrap();
        file.write_all(&[0xFF; 32]).unwrap();
        file.flush().unwrap();
        
        let format = launcher.detect_format(file.path()).unwrap();
        // Should fall back to extension detection
        assert_eq!(format, GameFormat::ZCode);
    }

    #[test]
    fn test_find_interpreter_path_in_path_env() {
        let launcher = Launcher::new().unwrap();
        
        // Create a temporary directory and add it to PATH for testing
        let temp_dir = TempDir::new().unwrap();
        let interpreter_path = temp_dir.path().join("test_interpreter_xyz123");
        
        // Create a dummy executable file with unique name to avoid conflicts
        std::fs::write(&interpreter_path, "#!/bin/sh\necho test").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&interpreter_path).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&interpreter_path, perms).unwrap();
        }
        
        // Add temp directory to PATH
        let original_path = env::var("PATH").unwrap_or_default();
        env::set_var("PATH", format!("{}:{}", temp_dir.path().display(), original_path));
        
        let result = launcher.find_interpreter_path("test_interpreter_xyz123");
        
        // Restore original PATH
        env::set_var("PATH", original_path);
        
        assert!(result.is_some());
        assert_eq!(result.unwrap(), interpreter_path);
    }

    #[test]
    fn test_find_interpreter_path_not_found() {
        let launcher = Launcher::new().unwrap();
        let result = launcher.find_interpreter_path("nonexistent_interpreter_xyz");
        assert!(result.is_none());
    }

    #[test]
    fn test_game_format_interpreter_mapping() {
        // Verify all supported formats have interpreters except Unknown
        assert_eq!(GameFormat::ZCode.interpreter(), Some("bocfel"));
        assert_eq!(GameFormat::Glulx.interpreter(), Some("git"));
        assert_eq!(GameFormat::Tads.interpreter(), Some("tadsr"));
        assert_eq!(GameFormat::Unknown.interpreter(), None);
    }
}
