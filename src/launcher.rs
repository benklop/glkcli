use crate::config::GameFormat;
use crate::detect::*;
use crate::storage::{Checkpoint, GameStorage};
use crate::pty::{spawn_in_pty, InterceptedKey, PtyHandle};
use crate::criu;
use anyhow::{anyhow, Context, Result};
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

    /// Run a game in a PTY with checkpoint support
    ///
    /// This replaces the traditional run_game for CRIU-based checkpoint functionality.
    /// It intercepts F1/F2/F3/Escape keys to manage checkpoints.
    ///
    /// # Arguments
    /// * `game_path` - Path to the game file
    /// * `format` - Detected game format
    /// * `tuid` - Game identifier for checkpoint storage
    /// * `storage` - Storage manager for checkpoint persistence
    /// * `restore_checkpoint_id` - Optional checkpoint ID to restore from
    ///
    /// # Returns
    /// Returns Ok(()) when the game exits normally
    pub fn run_game_with_checkpoints(
        &self,
        game_path: &Path,
        format: GameFormat,
        tuid: &str,
        storage: &GameStorage,
        restore_checkpoint_id: Option<String>,
    ) -> Result<()> {
        let interpreter_name = format.interpreter()
            .ok_or_else(|| anyhow!("No interpreter configured for format: {}", format))?;

        let interpreter_path = self.find_interpreter_path(interpreter_name)
            .ok_or_else(|| anyhow!("Interpreter '{}' not found", interpreter_name))?;

        let game_dir = game_path.parent()
            .ok_or_else(|| anyhow!("Could not determine game directory"))?;

        // Prepare interpreter arguments
        let mut args: Vec<String> = format.flags().iter().map(|s| s.to_string()).collect();
        args.push(game_path.display().to_string());
        
        let args_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();

        // Track playtime
        let mut cumulative_playtime: u64 = 0;
        let mut session_start: Instant;

        // If restoring from checkpoint, do that first
        let mut pty_handle = if let Some(checkpoint_id) = restore_checkpoint_id {
            let checkpoints = storage.get_checkpoints(tuid)?;
            let checkpoint = checkpoints.iter()
                .find(|c| c.id == checkpoint_id)
                .ok_or_else(|| anyhow!("Checkpoint not found: {}", checkpoint_id))?;
            
            log::info!("Restoring from checkpoint: {}", checkpoint.name);
            cumulative_playtime = checkpoint.playtime_seconds;
            session_start = Instant::now();
            
            // Restore the CRIU checkpoint
            let _restored_pid = criu::restore_checkpoint(&checkpoint.checkpoint_path)?;
            
            // We need to attach to the restored process via PTY
            // This is complex - for now, we'll start fresh and note this limitation
            // TODO: Implement proper PTY attachment to restored CRIU process
            log::warn!("CRIU restore successful but PTY re-attachment not yet implemented");
            log::warn!("Starting fresh session instead");
            
            spawn_in_pty(
                interpreter_path.to_str().unwrap(),
                &args_refs,
                Some(game_dir),
            )?
        } else {
            session_start = Instant::now();
            
            // Start fresh game
            spawn_in_pty(
                interpreter_path.to_str().unwrap(),
                &args_refs,
                Some(game_dir),
            )?
        };

        log::info!("Game started with PID: {}", pty_handle.pid);

        // Main game loop - monitor for intercepted keys
        loop {
            // Check for intercepted keystrokes
            match pty_handle.keystroke_rx.recv_timeout(std::time::Duration::from_millis(100)) {
                Ok(intercepted_key) => {
                    let elapsed = session_start.elapsed().as_secs();
                    let total_playtime = cumulative_playtime + elapsed;
                    
                    match intercepted_key {
                        InterceptedKey::SaveAndExit => {
                            log::info!("F1 pressed: Save and exit");
                            
                            // Create checkpoint
                            if let Err(e) = self.create_checkpoint(
                                &mut pty_handle,
                                tuid,
                                storage,
                                total_playtime,
                                "Auto-save (F1)"
                            ) {
                                log::error!("Failed to create checkpoint: {}", e);
                            }
                            
                            // Exit to menu
                            break;
                        }
                        InterceptedKey::SaveAndContinue => {
                            log::info!("F2 pressed: Save and continue");
                            
                            // Create checkpoint but continue playing
                            if let Err(e) = self.create_checkpoint(
                                &mut pty_handle,
                                tuid,
                                storage,
                                total_playtime,
                                "Quick-save (F2)"
                            ) {
                                log::error!("Failed to create checkpoint: {}", e);
                                // Continue playing even if checkpoint fails
                            } else {
                                // Update cumulative playtime and reset session timer
                                cumulative_playtime = total_playtime;
                                session_start = Instant::now();
                            }
                        }
                        InterceptedKey::QuickReload => {
                            log::info!("F3 pressed: Quick reload");
                            
                            // Get latest checkpoint and restore
                            if let Ok(Some(checkpoint)) = storage.get_latest_checkpoint(tuid) {
                                log::info!("Restoring checkpoint: {}", checkpoint.name);
                                
                                // Kill current process
                                let _ = pty_handle.signal(nix::sys::signal::Signal::SIGTERM);
                                let _ = pty_handle.wait();
                                
                                // Restore from checkpoint
                                // TODO: Implement proper restoration
                                log::warn!("Quick reload not fully implemented yet");
                                break;
                            } else {
                                log::warn!("No checkpoints available to restore");
                            }
                        }
                        InterceptedKey::ExitPrompt => {
                            log::info!("Escape pressed: Exit prompt");
                            
                            // TODO: Show dialog asking if user wants to save
                            // For now, just exit
                            break;
                        }
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    // No keys intercepted, continue
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    // PTY closed, game exited
                    log::info!("Game process exited");
                    break;
                }
            }
        }

        // Wait for game to fully exit
        pty_handle.wait()?;
        
        Ok(())
    }

    /// Create a checkpoint of the current game session
    fn create_checkpoint(
        &self,
        pty_handle: &mut PtyHandle,
        tuid: &str,
        storage: &GameStorage,
        playtime_seconds: u64,
        name: &str,
    ) -> Result<()> {
        // Generate checkpoint ID
        let checkpoint_id = format!("checkpoint_{}", SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)?
            .as_secs());
        
        // Create checkpoint directory
        let checkpoint_dir = storage.create_checkpoint_dir(tuid, &checkpoint_id)?;
        
        log::info!("Creating checkpoint: {} (playtime: {}s)", name, playtime_seconds);
        
        // Create CRIU checkpoint (leave process running for F2)
        criu::checkpoint_process(pty_handle.pid, &checkpoint_dir, true)?;
        
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
                return Some(install_path);
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
                return Some(path);
            }
        }

        // Finally check PATH
        if let Ok(path_env) = env::var("PATH") {
            for dir in path_env.split(':') {
                let full_path = PathBuf::from(dir).join(interpreter_name);
                if full_path.exists() {
                    return Some(full_path);
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
