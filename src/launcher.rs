use crate::config::GameFormat;
use crate::detect::*;
use anyhow::{anyhow, Context, Result};
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct Launcher {
    // Could hold configuration or state in the future
}

impl Launcher {
    pub fn new() -> Result<Self> {
        Ok(Launcher {})
    }

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
        // First check if it exists in the current build directory (for development)
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

        // Check PATH
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
