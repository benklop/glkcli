use crate::config::GameFormat;
use crate::detect::*;
use anyhow::{anyhow, Context, Result};
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

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
