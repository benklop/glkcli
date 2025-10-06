use anyhow::{Context, Result};
use clap::Parser;
use std::path::PathBuf;

mod launcher;
mod detect;
mod config;
mod ifdb;
mod storage;
mod tui;
mod network;
mod app;
mod ui;
mod utils;

use launcher::*;

fn setup_debug_logging() -> Result<()> {
    let log_dir = dirs::home_dir()
        .context("Could not determine home directory")?
        .join(".glkcli");
    
    std::fs::create_dir_all(&log_dir)
        .context("Failed to create log directory")?;
    
    let log_file = log_dir.join("debug.log");
    
    // Use fern for simpler file logging
    fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "[{}] {} - {}",
                chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC"),
                record.level(),
                message
            ))
        })
        .level(log::LevelFilter::Debug)
        .chain(std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_file)
            .context("Failed to open log file")?)
        .apply()
        .context("Failed to initialize logger")?;
    
    Ok(())
}

#[derive(Parser)]
#[command(name = "glkcli")]
#[command(about = "glkterm command-line launcher")]
#[command(version)]
struct Cli {
    /// Show detected game format without running
    #[arg(short, long)]
    format: bool,

    /// Show additional information
    #[arg(short, long)]
    verbose: bool,

    /// Enable debug logging to file
    #[arg(short, long)]
    debug: bool,

    /// Assume network is always online (skip connectivity checks)
    #[arg(long)]
    assume_online: bool,

    /// Launch without loading save file (future feature)
    #[arg(long)]
    no_save: bool,

    /// List available save files (future feature)
    #[arg(long)]
    list_saves: bool,

    /// Load specific save file (future feature)
    #[arg(long)]
    save: Option<String>,

    /// Game file to run (optional - if not provided, launches TUI browser)
    game_file: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Set up logging if debug is enabled
    if cli.debug {
        setup_debug_logging()?;
        log::info!("Debug logging enabled");
    }

    // If no game file provided, launch TUI browser
    if cli.game_file.is_none() {
        return tui::run_tui(cli.debug, cli.assume_online).await;
    }

    let game_file = cli.game_file.unwrap();

    if cli.list_saves {
        anyhow::bail!("--list-saves option not yet implemented");
    }

    if cli.save.is_some() {
        anyhow::bail!("--save option not yet implemented");
    }

    if cli.no_save && cli.verbose {
        println!("Note: --no-save option not yet implemented");
    }

    let launcher = Launcher::new()?;

    if cli.format {
        let format = launcher.detect_format(&game_file)?;
        println!("Detected format: {}", format.name());
        return Ok(());
    }

    if cli.verbose {
        println!("glkcli - glkterm command-line launcher");
        println!("Game file: {}", game_file.display());
    }

    launcher.detect_and_run(&game_file, cli.verbose)
        .context("Failed to run game")?;

    Ok(())
}
