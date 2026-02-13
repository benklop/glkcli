//! Terminal User Interface module
//!
//! This module coordinates the TUI application's main event loop and business logic.
//! It brings together several specialized modules:
//!
//! - `app::state` - Core application state and data structures
//! - `ui` - All rendering logic for the terminal interface  
//! - `utils` - Utility functions like HTML entity decoding
//! - `ifdb` - IFDB API client for fetching game data
//! - `storage` - Local game storage and management
//! - `launcher` - Game interpreter detection and launching
//! - `network` - Network connectivity checking
//!
//! ## Architecture
//!
//! The TUI uses a clean separation of concerns:
//! - **State Management** (`app::state`): TuiApp struct holds all application state
//! - **Presentation** (`ui`): Rendering functions transform state into terminal UI
//! - **Business Logic** (this module): Operations like download, import, search
//! - **Input Handling** (this module): Keyboard event processing and routing
//!
//! ## Design Decisions
//!
//! Business operations and input handlers remain in this module because they:
//! 1. Require mutable access to nearly all TuiApp state fields
//! 2. Need terminal control (e.g., disable/enable raw mode for game launches)
//! 3. Coordinate between multiple subsystems (network, storage, IFDB API)
//! 4. Are inherently coupled to the application's state machine
//!
//! Further extraction would increase complexity without improving maintainability.

use anyhow::{Context, Result};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    Terminal,
};
use std::io;

use crate::app::state::{TuiApp, AppState, InputMode};
use crate::ifdb::{Game, SearchOptions};
use crate::storage::{LocalGame, SaveFile};

/// Run the TUI application
///
/// This is the main entry point for the terminal interface. It:
/// 1. Sets up the terminal in raw mode with alternate screen
/// 2. Creates and initializes the TuiApp
/// 3. Runs the event loop until the user quits
/// 4. Restores the terminal to its original state
///
/// # Arguments
///
/// * `debug` - Enable debug logging to ~/.glkcli/debug.log
/// * `assume_online` - Assume network is available (skip connectivity check)
pub async fn run_tui(debug: bool, assume_online: bool) -> Result<()> {
    // Check CRIU availability
    if let Err(e) = crate::criu::check_criu_available() {
        eprintln!("Warning: CRIU is not available or not properly configured.");
        eprintln!("{}", e);
        eprintln!("\nCheckpoint features will not be available.");
        eprintln!("You can still use glkcli without CRIU, but you won't be able to:");
        eprintln!("  - Create checkpoints with F1/F2 during gameplay");
        eprintln!("  - Quick-reload with F3");
        eprintln!("  - Track playtime accurately");
        eprintln!("\nPress Enter to continue or Ctrl+C to exit...");
        
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
    }
    
    // Setup terminal
    enable_raw_mode().context("Failed to enable raw mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)
        .context("Failed to setup terminal")?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("Failed to create terminal")?;

    // Create app
    let mut app = TuiApp::new(debug, assume_online).await?;
    
    // Load initial data
    app.refresh_downloaded_games().await?;
    
    // Only browse games if online
    if app.is_online {
        app.browse_popular_games().await?;
    } else {
        // Start on the downloaded games tab if offline
        app.current_tab = 1;
        app.set_status_message("Offline - showing downloaded games only".to_string());
    }
    
    // Run app
    let result = app.run(&mut terminal).await;

    // Restore terminal
    disable_raw_mode().context("Failed to disable raw mode")?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    ).context("Failed to restore terminal")?;
    terminal.show_cursor().context("Failed to show cursor")?;
    
    // Explicitly drop terminal to ensure proper cleanup
    drop(terminal);

    result
}

impl TuiApp {
    async fn run<B: ratatui::backend::Backend>(&mut self, terminal: &mut Terminal<B>) -> Result<()> {
        // Clear terminal on startup to ensure clean initial state
        terminal.clear()?;
        
        loop {
            // If we need a full redraw (e.g., after launching a game), clear the terminal
            if self.needs_redraw {
                terminal.clear()?;
                self.needs_redraw = false;
            }
            
            // Check if status message should be auto-cleared
            self.check_status_timeout();
            
            // Check if we need to load the next page
            if self.should_load_next_page {
                self.should_load_next_page = false;
                self.load_next_page().await?;
            }
            
            // Clear status message after 3 seconds
            if let Some(time) = self.status_message_time {
                if time.elapsed().as_secs() >= 3 {
                    self.status_message = None;
                    self.status_message_time = None;
                }
            }
            
            // Draw the UI - ratatui's double-buffering will handle efficient updates
            terminal.draw(|f| self.ui(f))?;

            // Poll for events with a timeout to allow status message clearing
            if event::poll(std::time::Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    match self.input_mode {
                        InputMode::Normal => {
                            if self.handle_normal_input(key.code).await? {
                                break;
                            }
                        }
                        InputMode::Searching => {
                            if self.handle_search_input(key.code).await? {
                                break;
                            }
                        }
                        InputMode::Confirmation => {
                            if self.handle_confirmation_input(key.code).await? {
                                break;
                            }
                        }
                        InputMode::ImportingFile => {
                            if self.handle_import_input(key.code).await? {
                                break;
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    async fn handle_normal_input(&mut self, key: KeyCode) -> Result<bool> {
        match key {
            KeyCode::Char('q') => return Ok(true),
            KeyCode::Tab => {
                // Exit dialogs when switching tabs
                if self.state == AppState::GameDetails || self.state == AppState::SaveFilesDialog {
                    self.state = AppState::Browse;
                    self.current_game_details = None;
                }
                
                // Calculate next tab, skipping Browse tab (0) if offline
                if self.is_online {
                    self.current_tab = (self.current_tab + 1) % 2; // Only 2 tabs now
                } else {
                    // When offline, only My Games tab (which is now tab 0 in offline mode)
                    self.current_tab = 0;
                }
                self.switch_tab().await?;
            }
            KeyCode::Char('s') => {
                if self.is_online {
                    self.input_mode = InputMode::Searching;
                    self.search_input.clear();
                } else {
                    self.set_status_message("Search unavailable - no network connection".to_string());
                }
            }
            KeyCode::Up => self.move_selection_up().await?,
            KeyCode::Down => self.move_selection_down().await?,
            KeyCode::Enter => self.handle_enter().await?,
            KeyCode::Char('d') => self.handle_download().await?,
            KeyCode::Char('i') => self.handle_import().await?,
            KeyCode::Char('x') => self.handle_delete().await?,
            KeyCode::Char('v') => self.handle_view_saves().await?,
            KeyCode::Char('c') => self.handle_view_checkpoints().await?,
            KeyCode::Char('r') => self.refresh_current_view().await?,
            KeyCode::Esc => self.handle_escape(),
            _ => {}
        }
        Ok(false)
    }

    async fn handle_search_input(&mut self, key: KeyCode) -> Result<bool> {
        match key {
            KeyCode::Enter => {
                self.input_mode = InputMode::Normal;
                self.perform_search().await?;
            }
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                self.search_input.clear();
            }
            KeyCode::Backspace => {
                self.search_input.pop();
            }
            KeyCode::Char(c) => {
                self.search_input.push(c);
            }
            _ => {}
        }
        Ok(false)
    }

    async fn handle_confirmation_input(&mut self, key: KeyCode) -> Result<bool> {
        match key {
            KeyCode::Char('y') | KeyCode::Enter => {
                self.input_mode = InputMode::Normal;
                // Handle confirmed action
                self.set_status_message("Action confirmed".to_string());
            }
            KeyCode::Char('n') | KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                self.set_status_message("Action cancelled".to_string());
            }
            _ => {}
        }
        Ok(false)
    }

    async fn move_selection_up(&mut self) -> Result<()> {
        // Handle game details view - navigate to previous game
        if self.state == AppState::GameDetails {
            match self.current_tab {
                0 if self.is_online => {
                    // Browse tab - move through search results
                    if let Some(i) = self.search_selection.selected() {
                        let new_i = if i == 0 {
                            self.search_results.len().saturating_sub(1)
                        } else {
                            i - 1
                        };
                        self.search_selection.select(Some(new_i));
                        
                        // Load the new game's details
                        if let Some(game) = self.search_results.get(new_i) {
                            let tuid = game.tuid.clone();
                            self.show_game_details(&tuid).await?;
                        }
                    }
                }
                _ => {
                    // My Games tab or offline mode - not implemented yet as those navigate to launch
                }
            }
            return Ok(());
        }
        
        // Handle save files dialog separately
        if self.state == AppState::SaveFilesDialog {
            let i = match self.save_selection.selected() {
                Some(i) => {
                    if i == 0 {
                        self.save_files.len().saturating_sub(1)
                    } else {
                        i - 1
                    }
                }
                None => 0,
            };
            self.save_selection.select(Some(i));
            return Ok(());
        }
        
        // Handle checkpoint browser separately
        if self.state == AppState::CheckpointBrowser {
            let i = match self.checkpoint_selection.selected() {
                Some(i) => {
                    if i == 0 {
                        self.checkpoints.len().saturating_sub(1)
                    } else {
                        i - 1
                    }
                }
                None => 0,
            };
            self.checkpoint_selection.select(Some(i));
            return Ok(());
        }
        
        match self.current_tab {
            0 => {
                if self.is_online {
                    // Browse tab
                    let i = match self.search_selection.selected() {
                        Some(i) => {
                            if i == 0 {
                                self.search_results.len().saturating_sub(1)
                            } else {
                                i - 1
                            }
                        }
                        None => 0,
                    };
                    self.search_selection.select(Some(i));
                } else {
                    // My Games when offline
                    let i = match self.downloaded_selection.selected() {
                        Some(i) => {
                            if i == 0 {
                                self.downloaded_games.len().saturating_sub(1)
                            } else {
                                i - 1
                            }
                        }
                        None => 0,
                    };
                    self.downloaded_selection.select(Some(i));
                }
            }
            1 => {
                // My Games tab
                let i = match self.downloaded_selection.selected() {
                    Some(i) => {
                        if i == 0 {
                            self.downloaded_games.len().saturating_sub(1)
                        } else {
                            i - 1
                        }
                    }
                    None => 0,
                };
                self.downloaded_selection.select(Some(i));
            }
            _ => {}
        }
        Ok(())
    }

    async fn move_selection_down(&mut self) -> Result<()> {
        // Handle game details view - navigate to next game
        if self.state == AppState::GameDetails {
            match self.current_tab {
                0 if self.is_online => {
                    // Browse tab - move through search results
                    if let Some(i) = self.search_selection.selected() {
                        let new_i = if i >= self.search_results.len().saturating_sub(1) {
                            0  // Wrap to beginning
                        } else {
                            i + 1
                        };
                        self.search_selection.select(Some(new_i));
                        
                        // Load the new game's details
                        if let Some(game) = self.search_results.get(new_i) {
                            let tuid = game.tuid.clone();
                            self.show_game_details(&tuid).await?;
                        }
                    }
                }
                _ => {
                    // My Games tab or offline mode - not implemented yet
                }
            }
            return Ok(());
        }
        
        // Handle save files dialog separately
        if self.state == AppState::SaveFilesDialog {
            let i = match self.save_selection.selected() {
                Some(i) => {
                    if i >= self.save_files.len().saturating_sub(1) {
                        0
                    } else {
                        i + 1
                    }
                }
                None => 0,
            };
            self.save_selection.select(Some(i));
            return Ok(());
        }
        
        // Handle checkpoint browser separately
        if self.state == AppState::CheckpointBrowser {
            let i = match self.checkpoint_selection.selected() {
                Some(i) => {
                    if i >= self.checkpoints.len().saturating_sub(1) {
                        0
                    } else {
                        i + 1
                    }
                }
                None => 0,
            };
            self.checkpoint_selection.select(Some(i));
            return Ok(());
        }
        
        match self.current_tab {
            0 => {
                if self.is_online {
                    // Browse tab
                    let i = match self.search_selection.selected() {
                        Some(i) => {
                            if i >= self.search_results.len().saturating_sub(1) {
                                // At the end of the list - trigger loading more if available
                                if self.has_more_search_results && !self.loading {
                                    self.should_load_next_page = true;
                                }
                                i // Stay at current position
                            } else {
                                i + 1
                            }
                        }
                        None => 0,
                    };
                    self.search_selection.select(Some(i));
                } else {
                    // My Games when offline
                    let i = match self.downloaded_selection.selected() {
                        Some(i) => {
                            if i >= self.downloaded_games.len().saturating_sub(1) {
                                0
                            } else {
                                i + 1
                            }
                        }
                        None => 0,
                    };
                    self.downloaded_selection.select(Some(i));
                }
            }
            1 => {
                // My Games tab
                let i = match self.downloaded_selection.selected() {
                    Some(i) => {
                        if i >= self.downloaded_games.len().saturating_sub(1) {
                            0
                        } else {
                            i + 1
                        }
                    }
                    None => 0,
                };
                self.downloaded_selection.select(Some(i));
            }
            _ => {}
        }
        Ok(())
    }

    async fn handle_enter(&mut self) -> Result<()> {
        // Handle checkpoint browser separately
        if self.state == AppState::CheckpointBrowser {
            if let Some(i) = self.checkpoint_selection.selected() {
                if let Some(checkpoint) = self.checkpoints.get(i).cloned() {
                    self.load_checkpoint(&checkpoint).await?;
                }
            }
            return Ok(());
        }
        
        // Handle save files dialog separately
        if self.state == AppState::SaveFilesDialog {
            if let Some(i) = self.save_selection.selected() {
                if let Some(save) = self.save_files.get(i).cloned() {
                    self.load_save_file(&save).await?;
                }
            }
            return Ok(());
        }
        
        match self.current_tab {
            0 => {
                if self.is_online {
                    // Browse games - show details or download
                    if let Some(i) = self.search_selection.selected() {
                        if let Some(game) = self.search_results.get(i) {
                            let tuid = game.tuid.clone();
                            self.show_game_details(&tuid).await?;
                        }
                    }
                } else {
                    // When offline, tab 0 is My Games
                    if let Some(i) = self.downloaded_selection.selected() {
                        if let Some(game) = self.downloaded_games.get(i).cloned() {
                            self.launch_game(&game).await?;
                        }
                    }
                }
            }
            1 => {
                // My Games tab - launch downloaded game
                if let Some(i) = self.downloaded_selection.selected() {
                    if let Some(game) = self.downloaded_games.get(i).cloned() {
                        self.launch_game(&game).await?;
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    async fn handle_download(&mut self) -> Result<()> {
        if !self.is_online {
            self.set_status_message("Download unavailable - no network connection".to_string());
            return Ok(());
        }
        
        if self.current_tab == 0 {
            if let Some(i) = self.search_selection.selected() {
                if let Some(game) = self.search_results.get(i).cloned() {
                    self.download_game(&game).await?;
                }
            }
        }
        Ok(())
    }

    async fn handle_delete(&mut self) -> Result<()> {
        if self.current_tab == 1 {
            if let Some(i) = self.downloaded_selection.selected() {
                if let Some(game) = self.downloaded_games.get(i) {
                    let tuid = game.tuid.clone();
                    let title = game.title.clone();
                    
                    match self.storage.remove_game(&tuid) {
                        Ok(_) => {
                            self.set_status_message(format!("Deleted '{}'", title));
                            self.refresh_downloaded_games().await?;
                        }
                        Err(e) => {
                            self.set_status_message(format!("Failed to delete game: {}", e));
                        }
                    }
                }
            }
        }
        Ok(())
    }

    async fn switch_tab(&mut self) -> Result<()> {
        match self.current_tab {
            0 => {
                if self.is_online {
                    // Browse tab - refresh search results if empty
                    if self.search_results.is_empty() {
                        self.browse_popular_games().await?;
                    }
                } else {
                    // When offline, tab 0 is My Games
                    self.refresh_downloaded_games().await?;
                }
            }
            1 => {
                // My Games tab (when online) or doesn't exist (when offline)
                if self.is_online {
                    self.refresh_downloaded_games().await?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    async fn handle_view_saves(&mut self) -> Result<()> {
        // Only works in My Games tab
        if self.current_tab == 1 || (self.current_tab == 0 && !self.is_online) {
            if let Some(i) = self.downloaded_selection.selected() {
                if let Some(game) = self.downloaded_games.get(i) {
                    let tuid = game.tuid.clone();
                    self.refresh_save_files(&tuid).await?;
                    self.state = AppState::SaveFilesDialog;
                } else {
                    self.set_status_message("No game selected".to_string());
                }
            } else {
                self.set_status_message("No game selected".to_string());
            }
        }
        Ok(())
    }

    async fn handle_view_checkpoints(&mut self) -> Result<()> {
        // Only works in My Games tab
        if self.current_tab == 1 || (self.current_tab == 0 && !self.is_online) {
            if let Some(i) = self.downloaded_selection.selected() {
                if let Some(game) = self.downloaded_games.get(i) {
                    let tuid = game.tuid.clone();
                    self.refresh_checkpoints(&tuid).await?;
                    self.state = AppState::CheckpointBrowser;
                } else {
                    self.set_status_message("No game selected".to_string());
                }
            } else {
                self.set_status_message("No game selected".to_string());
            }
        }
        Ok(())
    }

    async fn handle_import(&mut self) -> Result<()> {
        // Only allow import from game details view for commercial games
        if self.state == AppState::GameDetails {
            if let Some(details) = &self.current_game_details {
                // Check if game is already downloaded
                if let Some(ifdb) = &details.ifdb {
                    if self.storage.is_game_downloaded(&ifdb.tuid)? {
                        self.set_status_message("Game already in My Games - cannot import again".to_string());
                        return Ok(());
                    }
                }
                
                if details.is_commercial() {
                    // Get the TUID from the current game
                    if let Some(ifdb) = &details.ifdb {
                        self.import_game_tuid = Some(ifdb.tuid.clone());
                        self.import_file_path.clear();
                        self.input_mode = InputMode::ImportingFile;
                        self.set_status_message("Enter the path to the game file you purchased".to_string());
                    }
                } else {
                    self.set_status_message("This game is not commercial - use 'd' to download".to_string());
                }
            }
        }
        Ok(())
    }

    async fn handle_import_input(&mut self, key: KeyCode) -> Result<bool> {
        match key {
            KeyCode::Enter => {
                self.input_mode = InputMode::Normal;
                let file_path = self.import_file_path.trim().to_string();
                
                if file_path.is_empty() {
                    self.set_status_message("Import cancelled".to_string());
                    return Ok(false);
                }
                
                if let Some(tuid) = &self.import_game_tuid.clone() {
                    self.import_game_file(tuid, &file_path).await?;
                }
                
                self.import_file_path.clear();
                self.import_game_tuid = None;
            }
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                self.import_file_path.clear();
                self.import_game_tuid = None;
                self.set_status_message("Import cancelled".to_string());
            }
            KeyCode::Char(c) => {
                self.import_file_path.push(c);
            }
            KeyCode::Backspace => {
                self.import_file_path.pop();
            }
            _ => {}
        }
        Ok(false)
    }

    async fn import_game_file(&mut self, tuid: &str, file_path: &str) -> Result<()> {
        if self.debug {
            log::debug!("Importing game file from: {}", file_path);
        }

        // Expand home directory if path starts with ~
        let expanded_path = if file_path.starts_with("~/") {
            if let Some(home) = std::env::var_os("HOME") {
                std::path::PathBuf::from(home).join(&file_path[2..])
            } else {
                std::path::PathBuf::from(file_path)
            }
        } else {
            std::path::PathBuf::from(file_path)
        };

        // Check if file exists
        if !expanded_path.exists() {
            self.set_status_message(format!("File not found: {}", file_path));
            return Ok(());
        }

        // Check if it's a file
        if !expanded_path.is_file() {
            self.set_status_message(format!("Not a file: {}", file_path));
            return Ok(());
        }

        self.loading = true;
        self.set_status_message("Importing game file...".to_string());

        // Read the file
        match std::fs::read(&expanded_path) {
            Ok(bytes) => {
                if self.debug {
                    log::debug!("Read {} bytes from {}", bytes.len(), file_path);
                }

                // Get file extension
                let extension = expanded_path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .unwrap_or("dat");

                if self.debug {
                    log::debug!("File extension: {}", extension);
                }

                // We need to create a Game struct from the current game details
                if let Some(details) = &self.current_game_details {
                    // Create a minimal Game struct for storage
                    let game_title = details.bibliographic
                        .as_ref()
                        .and_then(|b| b.title.clone())
                        .unwrap_or_else(|| "Unknown".to_string());
                    
                    let game_author = details.bibliographic
                        .as_ref()
                        .and_then(|b| b.author.clone())
                        .unwrap_or_else(|| "Unknown".to_string());
                    
                    let link = details.ifdb
                        .as_ref()
                        .map(|i| i.link.clone())
                        .unwrap_or_else(|| format!("https://ifdb.org/viewgame?id={}", tuid));

                    let game = Game {
                        tuid: tuid.to_string(),
                        title: game_title.clone(),
                        author: game_author,
                        link,
                        has_cover_art: None,
                        devsys: None,
                        published: None,
                        average_rating: details.ifdb.as_ref().and_then(|i| i.average_rating),
                        num_ratings: None,
                        star_rating: details.ifdb.as_ref().and_then(|i| i.star_rating),
                        cover_art_link: details.ifdb.as_ref().and_then(|i| i.coverart.as_ref().map(|c| c.url.clone())),
                        play_time_in_minutes: details.ifdb.as_ref().and_then(|i| i.play_time_in_minutes),
                    };

                    // Store the file using add_game_with_cover (async version)
                    match self.storage.add_game_with_cover(&game, Some(details), &bytes, extension).await {
                        Ok(_) => {
                            self.set_status_message(format!("Game '{}' imported successfully!", game_title));
                            self.refresh_downloaded_games().await?;
                            
                            // Return to browse view
                            self.state = AppState::Browse;
                            self.current_game_details = None;
                        }
                        Err(e) => {
                            if self.debug {
                                log::error!("Failed to store imported game: {}", e);
                            }
                            self.set_status_message(format!("Failed to import game: {}", e));
                        }
                    }
                } else {
                    self.set_status_message("Game details not available".to_string());
                }
            }
            Err(e) => {
                if self.debug {
                    log::error!("Failed to read file {}: {}", file_path, e);
                }
                self.set_status_message(format!("Failed to read file: {}", e));
            }
        }

        self.loading = false;
        Ok(())
    }

    async fn perform_search(&mut self) -> Result<()> {
        if self.search_input.trim().is_empty() {
            self.browse_popular_games().await?;
        } else {
            self.loading = true;
            self.current_search_page = 1;
            self.set_status_message(format!("Searching for '{}'...", self.search_input));
            let options = SearchOptions::new(&self.search_input)
                .with_limit(50)
                .with_page(1)
                .with_glk_formats();  // Filter for playable formats
            
            match self.ifdb_client.search_games(&options).await {
                Ok(games) => {
                    self.has_more_search_results = games.len() >= 50;
                    self.search_results = games;
                    self.search_selection.select(if self.search_results.is_empty() {
                        None
                    } else {
                        Some(0)
                    });
                    self.set_status_message(format!("Found {} games", self.search_results.len()));
                }
                Err(e) => {
                    self.set_status_message(format!("Search failed: {}", e));
                }
            }
            self.loading = false;
        }
        Ok(())
    }

    async fn load_next_page(&mut self) -> Result<()> {
        if !self.has_more_search_results || self.loading {
            return Ok(());
        }
        
        self.loading = true;
        self.current_search_page += 1;
        self.set_status_message(format!("Loading page {}...", self.current_search_page));
        
        let options = SearchOptions::new(&self.search_input)
            .with_limit(50)
            .with_page(self.current_search_page)
            .with_glk_formats();
        
        match self.ifdb_client.search_games(&options).await {
            Ok(mut games) => {
                self.has_more_search_results = games.len() >= 50;
                let prev_len = self.search_results.len();
                self.search_results.append(&mut games);
                self.set_status_message(format!("Loaded {} more games (total: {})", 
                    self.search_results.len() - prev_len, 
                    self.search_results.len()));
            }
            Err(e) => {
                self.current_search_page -= 1; // Revert page on error
                self.set_status_message(format!("Failed to load more results: {}", e));
            }
        }
        
        self.loading = false;
        Ok(())
    }

    async fn browse_popular_games(&mut self) -> Result<()> {
        self.loading = true;
        self.current_search_page = 1;
        match self.ifdb_client.browse_games(Some("rating")).await {
            Ok(games) => {
                self.has_more_search_results = games.len() >= 50;
                self.search_results = games;
                self.search_selection.select(if self.search_results.is_empty() {
                    None
                } else {
                    Some(0)
                });
            }
            Err(e) => {
                self.set_status_message(format!("Failed to load games: {}", e));
            }
        }
        self.loading = false;
        Ok(())
    }

    async fn show_game_details(&mut self, tuid: &str) -> Result<()> {
        if self.debug {
            log::debug!("Fetching game details for TUID: {}", tuid);
        }
        
        self.loading = true;
        match self.ifdb_client.get_game_details(tuid).await {
            Ok(details) => {
                if self.debug {
                    log::debug!("Successfully fetched game details for {}: {:?}", tuid, details);
                }
                self.current_game_details = Some(details);
                self.state = AppState::GameDetails;
            }
            Err(e) => {
                if self.debug {
                    log::error!("Failed to load game details for {}: {}", tuid, e);
                }
                self.set_status_message(format!("Failed to load game details: {}", e));
            }
        }
        self.loading = false;
        Ok(())
    }

    async fn download_game(&mut self, game: &Game) -> Result<()> {
        if self.debug {
            log::debug!("Starting download for game: {} ({})", game.title, game.tuid);
        }

        // Check if already downloaded
        if self.storage.is_game_downloaded(&game.tuid)? {
            self.set_status_message("Game already downloaded".to_string());
            return Ok(());
        }

        self.loading = true;
        self.set_status_message("Downloading game...".to_string());

        // Get game details for download links
        if self.debug {
            log::debug!("Fetching game details for TUID: {}", game.tuid);
        }
        
        match self.ifdb_client.get_game_details(&game.tuid).await {
            Ok(details) => {
                if self.debug {
                    log::debug!("Successfully fetched game details: {:?}", details);
                }
                
                // Check if game is commercial
                if details.is_commercial() {
                    self.loading = false;
                    let msg = if let Some(url) = details.get_purchase_url() {
                        format!("This is a commercial game. Purchase at: {}", url)
                    } else {
                        "This is a commercial game and must be purchased separately".to_string()
                    };
                    self.set_status_message(msg);
                    return Ok(());
                }
                
                if let Some(ifdb_data) = &details.ifdb {
                    if let Some(downloads) = &ifdb_data.downloads {
                        if self.debug {
                            log::debug!("Found {} download links", downloads.links.len());
                            for (i, link) in downloads.links.iter().enumerate() {
                                log::debug!("Link {}: {} (is_game: {}, format: {:?})", 
                                    i, link.url, link.is_game, link.format);
                            }
                        }
                        
                        // Helper function to check if download link format is acceptable
                        let is_acceptable_format = |link: &&crate::ifdb::DownloadLink| {
                            if let Some(format) = &link.format {
                                let format_lower = format.to_lowercase();
                                // Exclude generic formats that might be platform-specific
                                !matches!(format_lower.as_str(), 
                                    "storyfile" | "hypertextgame" | "executable"
                                )
                            } else {
                                // If no format specified, we'll allow it (might be a direct download)
                                true
                            }
                        };
                        
                        // Find the best download link (prioritize game files with acceptable formats)
                        let download_link = downloads.links.iter()
                            .filter(is_acceptable_format)
                            .find(|link| link.is_game)
                            .or_else(|| downloads.links.iter().filter(is_acceptable_format).next());

                        if let Some(link) = download_link {
                            if self.debug {
                                log::debug!("Selected download link: {} (format: {:?})", link.url, link.format);
                            }
                            
                            self.set_status_message(format!("Downloading from: {}", link.url));
                            match self.ifdb_client.download_file(&link.url).await {
                                Ok(response) => {
                                    let bytes = response.bytes().await?;
                                    
                                    if self.debug {
                                        log::debug!("Downloaded {} bytes", bytes.len());
                                    }

                                    // Get file extension from URL
                                    let extension = std::path::Path::new(&link.url)
                                        .extension()
                                        .and_then(|ext| ext.to_str())
                                        .unwrap_or("dat");

                                    if self.debug {
                                        log::debug!("Using file extension from URL: {}", extension);
                                    }

                                    match self.storage.add_game_with_cover(game, Some(&details), &bytes, extension).await {
                                        Ok(_) => {
                                            self.set_status_message("Game downloaded successfully".to_string());
                                            self.refresh_downloaded_games().await?;
                                        }
                                        Err(e) => {
                                            self.set_status_message(format!("Failed to save game: {}", e));
                                        }
                                    }
                                }
                                Err(e) => {
                                    if self.debug {
                                        log::error!("Download failed: {}", e);
                                    }
                                    self.set_status_message(format!("Download failed: {}", e));
                                }
                            }
                        } else {
                            if self.debug {
                                log::warn!("No download links found in game details");
                            }
                            self.set_status_message("No download links found".to_string());
                        }
                    } else {
                        if self.debug {
                            log::warn!("No download section found in IFDB data");
                        }
                        self.set_status_message("No download section found".to_string());
                    }
                } else {
                    if self.debug {
                        log::warn!("No IFDB data found in game details");
                    }
                    self.set_status_message("No IFDB data found in game details".to_string());
                }
            }
            Err(e) => {
                if self.debug {
                    log::error!("Failed to get game details: {}", e);
                }
                self.set_status_message(format!("Failed to get game details: {}", e));
            }
        }

        self.loading = false;
        Ok(())
    }

    async fn launch_game(&mut self, game: &LocalGame) -> Result<()> {
        // Temporarily disable raw mode and restore terminal before launching game
        disable_raw_mode().context("Failed to disable raw mode")?;
        execute!(
            io::stdout(),
            LeaveAlternateScreen,
            DisableMouseCapture,
            crossterm::cursor::Show
        ).context("Failed to restore terminal")?;
        
        // Launch the game
        let launch_result = self.launcher.detect_and_run(&game.file_path, false);
        
        // Re-enable raw mode and alternate screen after game exits
        enable_raw_mode().context("Failed to re-enable raw mode")?;
        execute!(
            io::stdout(),
            EnterAlternateScreen,
            EnableMouseCapture,
            crossterm::cursor::Hide
        ).context("Failed to re-setup terminal")?;
        
        match launch_result {
            Ok(_) => {
                // Record that the game was played
                let _ = self.storage.record_game_played(&game.tuid);
                self.set_status_message("Game ended successfully".to_string());
                // Refresh the game list to update play count
                let _ = self.refresh_downloaded_games().await;
            }
            Err(e) => {
                self.set_status_message(format!("Failed to launch game: {}", e));
            }
        }
        
        // Mark that we need a full terminal redraw
        self.needs_redraw = true;
        
        Ok(())
    }

    async fn load_save_file(&mut self, save: &SaveFile) -> Result<()> {
        // Find the game associated with this save file
        if let Some(game) = self.downloaded_games.iter().find(|g| g.tuid == save.game_tuid).cloned() {
            // GLK interpreters automatically detect and can load save files from the game directory
            // So we just launch the game and the player can restore from the interpreter
            self.set_status_message(format!("Launching game to load: {}", save.save_name));
            self.launch_game(&game).await?;
        } else {
            self.set_status_message("Game not found for this save file".to_string());
        }
        Ok(())
    }

    async fn load_checkpoint(&mut self, checkpoint: &crate::storage::Checkpoint) -> Result<()> {
        // Find the game associated with this checkpoint
        if let Some(game) = self.downloaded_games.iter().find(|g| g.tuid == checkpoint.game_tuid).cloned() {
            self.set_status_message(format!("Restoring checkpoint: {}", checkpoint.name));
            
            // Temporarily disable raw mode and restore terminal before launching game
            disable_raw_mode().context("Failed to disable raw mode")?;
            execute!(
                io::stdout(),
                LeaveAlternateScreen,
                DisableMouseCapture,
                crossterm::cursor::Show
            ).context("Failed to restore terminal")?;
            
            // Detect game format
            let format = self.launcher.detect_format(&game.file_path)?;
            
            // Launch the game with checkpoint restoration
            let launch_result = self.launcher.run_game_with_checkpoints(
                &game.file_path,
                format,
                &game.tuid,
                &self.storage,
                Some(checkpoint.id.clone()),
            );
            
            // Re-enable raw mode and alternate screen after game exits
            enable_raw_mode().context("Failed to re-enable raw mode")?;
            execute!(
                io::stdout(),
                EnterAlternateScreen,
                EnableMouseCapture,
                crossterm::cursor::Hide
            ).context("Failed to re-setup terminal")?;
            
            match launch_result {
                Ok(_) => {
                    // Record that the game was played
                    let _ = self.storage.record_game_played(&game.tuid);
                    self.set_status_message("Game session ended".to_string());
                    // Refresh checkpoints to show any new ones created
                    let _ = self.refresh_checkpoints(&game.tuid).await;
                }
                Err(e) => {
                    self.set_status_message(format!("Failed to restore checkpoint: {}", e));
                }
            }
            
            // Mark that we need a full terminal redraw
            self.needs_redraw = true;
        } else {
            self.set_status_message("Game not found for this checkpoint".to_string());
        }
        Ok(())
    }

    async fn refresh_downloaded_games(&mut self) -> Result<()> {
        match self.storage.get_downloaded_games() {
            Ok(games) => {
                self.downloaded_games = games;
                self.downloaded_selection.select(if self.downloaded_games.is_empty() {
                    None
                } else {
                    Some(0)
                });
            }
            Err(e) => {
                self.set_status_message(format!("Failed to load downloaded games: {}", e));
            }
        }
        Ok(())
    }

    async fn refresh_save_files(&mut self, tuid: &str) -> Result<()> {
        match self.storage.discover_save_files(tuid) {
            Ok(saves) => {
                self.save_files = saves;
                self.save_selection.select(if self.save_files.is_empty() {
                    None
                } else {
                    Some(0)
                });
            }
            Err(e) => {
                self.set_status_message(format!("Failed to load save files: {}", e));
            }
        }
        Ok(())
    }

    async fn refresh_checkpoints(&mut self, tuid: &str) -> Result<()> {
        match self.storage.get_checkpoints(tuid) {
            Ok(checkpoints) => {
                self.checkpoints = checkpoints;
                self.checkpoint_selection.select(if self.checkpoints.is_empty() {
                    None
                } else {
                    Some(0)
                });
            }
            Err(e) => {
                self.set_status_message(format!("Failed to load checkpoints: {}", e));
            }
        }
        Ok(())
    }

    async fn refresh_current_view(&mut self) -> Result<()> {
        // Check network connectivity when refreshing
        let was_online = self.is_online;
        self.is_online = self.network.is_connected().await;
        
        if !was_online && self.is_online {
            // Just came online - show message and switch to browse tab
            self.set_status_message("Network connection restored!".to_string());
            self.current_tab = 0;
            self.browse_popular_games().await?;
            return Ok(());
        } else if was_online && !self.is_online {
            // Just went offline - show message and switch to downloaded games
            self.set_status_message("Network connection lost".to_string());
            self.current_tab = 1;
            return Ok(());
        }
        
        match self.current_tab {
            0 => {
                if self.is_online {
                    self.browse_popular_games().await?
                } else {
                    self.set_status_message("Cannot refresh - no network connection".to_string());
                }
            },
            1 => {
                if self.is_online {
                    self.refresh_downloaded_games().await?
                }
            },
            _ => {}
        }
        Ok(())
    }

    fn handle_escape(&mut self) {
        use ratatui::widgets::ListState;
        match self.state {
            AppState::GameDetails => {
                self.state = AppState::Browse;
                self.current_game_details = None;
            }
            AppState::SaveFilesDialog => {
                self.state = AppState::Browse;
                self.save_files.clear();
                self.save_selection = ListState::default();
            }
            AppState::CheckpointBrowser => {
                self.state = AppState::Browse;
                self.checkpoints.clear();
                self.checkpoint_selection = ListState::default();
            }
            _ => {
                self.status_message = None;
            }
        }
    }
}
