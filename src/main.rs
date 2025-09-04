use anyhow::{Context, Result};
use clap::Parser;
use std::path::PathBuf;

mod launcher;
mod detect;
mod config;

use launcher::*;

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

    /// Launch without loading save file (future feature)
    #[arg(long)]
    no_save: bool,

    /// List available save files (future feature)
    #[arg(long)]
    list_saves: bool,

    /// Load specific save file (future feature)
    #[arg(long)]
    save: Option<String>,

    /// Game file to run
    game_file: PathBuf,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

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
        let format = launcher.detect_format(&cli.game_file)?;
        println!("Detected format: {}", format.name());
        return Ok(());
    }

    if cli.verbose {
        println!("glkcli - glkterm command-line launcher");
        println!("Game file: {}", cli.game_file.display());
    }

    launcher.detect_and_run(&cli.game_file, cli.verbose)
        .context("Failed to run game")?;

    Ok(())
}
