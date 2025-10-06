use anyhow::{Context, Result};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, List, ListItem, ListState, Paragraph, Tabs, Wrap,
    },
    Frame, Terminal,
};
use std::io;
use std::time::SystemTime;

use crate::ifdb::{IfdbClient, Game, GameDetails, SearchOptions};
use crate::storage::{GameStorage, LocalGame, SaveFile};
use crate::launcher::Launcher;
use crate::network::NetworkChecker;

/// Main TUI application
pub struct TuiApp {
    /// Current application state
    state: AppState,
    /// IFDB API client
    ifdb_client: IfdbClient,
    /// Local game storage
    storage: GameStorage,
    /// Game launcher
    launcher: Launcher,
    /// Network connectivity checker
    network: NetworkChecker,
    /// Whether network is available
    is_online: bool,
    /// Debug mode enabled
    debug: bool,
    /// Current tab index
    current_tab: usize,
    /// Search input state
    search_input: String,
    /// Search results
    search_results: Vec<Game>,
    /// Selected game in search results
    search_selection: ListState,
    /// Current page for search results
    current_search_page: u32,
    /// Whether there are more search results to load
    has_more_search_results: bool,
    /// Flag to trigger loading next page
    should_load_next_page: bool,
    /// Downloaded games
    downloaded_games: Vec<LocalGame>,
    /// Selected downloaded game
    downloaded_selection: ListState,
    /// Save files for current game
    save_files: Vec<SaveFile>,
    /// Selected save file
    save_selection: ListState,
    /// Current game details being viewed
    current_game_details: Option<GameDetails>,
    /// Loading state
    loading: bool,
    /// Status message
    status_message: Option<String>,
    /// Time when status message was set (for auto-clearing)
    status_message_time: Option<std::time::Instant>,
    /// Input mode
    input_mode: InputMode,
    /// Flag to indicate terminal needs full redraw
    needs_redraw: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
enum AppState {
    Browse,
    GameDetails,
    SaveFilesDialog,
    Download,
    DownloadedGames,
    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
enum InputMode {
    Normal,
    Searching,
    Confirmation,
}

/// Run the TUI application
pub async fn run_tui(debug: bool, assume_online: bool) -> Result<()> {
    // Setup terminal
    enable_raw_mode().context("Failed to enable raw mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)
        .context("Failed to setup terminal")?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("Failed to create terminal")?;

    // Create app
    let mut app = TuiApp::new(debug, assume_online).await?;
    
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

    result
}

impl TuiApp {
    pub async fn new(debug: bool, assume_online: bool) -> Result<Self> {
        let ifdb_client = IfdbClient::new()?;
        let storage = GameStorage::new()?;
        let launcher = Launcher::new()?;
        let network = NetworkChecker::new(debug, assume_online);

        // Check network connectivity
        let is_online = network.is_connected().await;
        
        if debug {
            log::info!("Network connectivity: {}", if is_online { "online" } else { "offline" });
        }

        let mut app = TuiApp {
            state: AppState::Browse,
            ifdb_client,
            storage,
            launcher,
            network,
            is_online,
            debug,
            current_tab: 0,
            search_input: String::new(),
            search_results: Vec::new(),
            search_selection: ListState::default(),
            current_search_page: 1,
            has_more_search_results: true,
            should_load_next_page: false,
            downloaded_games: Vec::new(),
            downloaded_selection: ListState::default(),
            save_files: Vec::new(),
            save_selection: ListState::default(),
            current_game_details: None,
            loading: false,
            status_message: None,
            status_message_time: None,
            input_mode: InputMode::Normal,
            needs_redraw: false,
        };

        // Load initial data
        app.refresh_downloaded_games().await?;
        
        // Only browse games if online
        if is_online {
            app.browse_popular_games().await?;
        } else {
            // Start on the downloaded games tab if offline
            app.current_tab = 1;
            app.set_status_message("Offline - showing downloaded games only".to_string());
        }

        Ok(app)
    }

    async fn run<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> Result<()> {
        loop {
            // If we need a full redraw (e.g., after launching a game), clear and reset the terminal
            if self.needs_redraw {
                terminal.clear()?;
                self.needs_redraw = false;
            }
            
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
            KeyCode::Up => self.move_selection_up(),
            KeyCode::Down => self.move_selection_down(),
            KeyCode::Enter => self.handle_enter().await?,
            KeyCode::Char('d') => self.handle_download().await?,
            KeyCode::Char('x') => self.handle_delete().await?,
            KeyCode::Char('v') => self.handle_view_saves().await?,
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

    fn move_selection_up(&mut self) {
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
            return;
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
    }

    fn move_selection_down(&mut self) {
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
            return;
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
    }

    async fn handle_enter(&mut self) -> Result<()> {
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
        let mut stdout = io::stdout();
        execute!(
            stdout,
            LeaveAlternateScreen,
            DisableMouseCapture,
            crossterm::cursor::Show
        ).context("Failed to restore terminal")?;
        
        // Launch the game
        let launch_result = self.launcher.detect_and_run(&game.file_path, false);
        
        // Re-enable raw mode and alternate screen after game exits
        enable_raw_mode().context("Failed to re-enable raw mode")?;
        execute!(
            stdout,
            EnterAlternateScreen,
            EnableMouseCapture,
            crossterm::cursor::Hide
        ).context("Failed to re-setup terminal")?;
        
        match launch_result {
            Ok(_) => {
                // Record that the game was played
                let _ = self.storage.record_game_played(&game.tuid);
                self.set_status_message("Game ended successfully".to_string());
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

    fn set_status_message(&mut self, message: String) {
        self.status_message = Some(message);
        self.status_message_time = Some(std::time::Instant::now());
    }

    fn handle_escape(&mut self) {
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
            _ => {
                self.status_message = None;
            }
        }
    }

    fn ui(&mut self, f: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Tabs
                Constraint::Min(0),    // Main content
                Constraint::Length(3), // Status bar
            ])
            .split(f.size());

        self.render_tabs(f, chunks[0]);
        self.render_main_content(f, chunks[1]);
        self.render_status_bar(f, chunks[2]);
    }

    fn render_tabs(&self, f: &mut Frame, area: Rect) {
        let network_status = if self.is_online { "●" } else { "○" };
        let title = format!("glkcli - IFDB Browser {}", network_status);
        
        let titles = if self.is_online {
            vec!["Browse Games", "My Games"]
        } else {
            vec!["My Games"]
        };
        
        let current_tab = if self.is_online {
            self.current_tab
        } else {
            // When offline, only one tab (My Games), always show it selected
            0
        };
        
        let tabs = Tabs::new(titles)
            .block(Block::default().borders(Borders::ALL).title(title))
            .style(Style::default().fg(Color::Gray))
            .highlight_style(Style::default().fg(Color::Yellow))
            .select(current_tab);
        f.render_widget(tabs, area);
    }

    fn render_main_content(&mut self, f: &mut Frame, area: Rect) {
        match self.state {
            AppState::GameDetails => self.render_game_details(f, area),
            AppState::SaveFilesDialog => self.render_saves_dialog(f, area),
            _ => {
                match self.current_tab {
                    0 => {
                        if self.is_online {
                            self.render_browse_tab(f, area)
                        } else {
                            self.render_downloaded_tab(f, area)
                        }
                    },
                    1 => self.render_downloaded_tab(f, area),
                    _ => {}
                }
            }
        }
    }

    fn render_browse_tab(&mut self, f: &mut Frame, area: Rect) {
        if !self.is_online {
            // Show offline message
            let offline_msg = Paragraph::new("Network connection unavailable.\n\nBrowse and search features are disabled.\n\nPress Tab to view your downloaded games.")
                .block(Block::default()
                    .borders(Borders::ALL)
                    .title("Browse Games - Offline"))
                .wrap(Wrap { trim: true });
            f.render_widget(offline_msg, area);
            return;
        }
        
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Search input
                Constraint::Min(0),    // Results
            ])
            .split(area);

        // Search input
        let search_style = if self.input_mode == InputMode::Searching {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };

        let search_input = Paragraph::new(self.search_input.as_str())
            .style(search_style)
            .block(Block::default()
                .borders(Borders::ALL)
                .title("Search (Press 's' to search, Enter to execute)"));
        f.render_widget(search_input, chunks[0]);

        // Results list
        let items: Vec<ListItem> = self.search_results
            .iter()
            .map(|game| {
                let rating = game.star_rating
                    .map(|r| format!(" [{:.1}★]", r))
                    .unwrap_or_default();
                
                ListItem::new(format!("{} - {}{}", game.title, game.author, rating))
            })
            .collect();

        let list = List::new(items)
            .block(Block::default()
                .borders(Borders::ALL)
                .title("Games (Enter: Details, 'd': Download)"))
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
            .highlight_symbol("> ");

        f.render_stateful_widget(list, chunks[1], &mut self.search_selection);
    }

    fn render_downloaded_tab(&mut self, f: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = self.downloaded_games
            .iter()
            .map(|game| {
                let play_info = if game.play_count > 0 {
                    format!(" (played {} times)", game.play_count)
                } else {
                    " (not played)".to_string()
                };
                
                ListItem::new(format!("{} - {}{}", game.title, game.author, play_info))
            })
            .collect();

        let list = List::new(items)
            .block(Block::default()
                .borders(Borders::ALL)
                .title("Downloaded Games (Enter: Launch | v: View Saves | x: Delete)"))
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
            .highlight_symbol("> ");

        f.render_stateful_widget(list, area, &mut self.downloaded_selection);
    }

    fn render_saves_dialog(&mut self, f: &mut Frame, area: Rect) {
        if self.save_files.is_empty() {
            // Show helpful message when no saves
            let msg = "No save files found for this game.\n\nPlay the game to create save files.\n\nPress Esc to close.";
            
            let paragraph = Paragraph::new(msg)
                .block(Block::default()
                    .borders(Borders::ALL)
                    .title("Save Files"))
                .wrap(Wrap { trim: true });
            f.render_widget(paragraph, area);
            return;
        }
        
        let items: Vec<ListItem> = self.save_files
            .iter()
            .map(|save| {
                let date = save.save_date
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .ok()
                    .and_then(|d| {
                        chrono::DateTime::from_timestamp(d.as_secs() as i64, 0)
                    })
                    .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                    .unwrap_or_else(|| "Unknown date".to_string());
                    
                ListItem::new(format!("{} - {} ({} bytes)", 
                    save.save_name, date, save.file_size))
            })
            .collect();

        let title = if let Some(i) = self.downloaded_selection.selected() {
            if let Some(game) = self.downloaded_games.get(i) {
                format!("Save Files for: {} (Esc: Close)", game.title)
            } else {
                "Save Files (Esc: Close)".to_string()
            }
        } else {
            "Save Files (Esc: Close)".to_string()
        };

        let list = List::new(items)
            .block(Block::default()
                .borders(Borders::ALL)
                .title(title))
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
            .highlight_symbol("> ");

        f.render_stateful_widget(list, area, &mut self.save_selection);
    }

    fn render_game_details(&self, f: &mut Frame, area: Rect) {
        if let Some(details) = &self.current_game_details {
            let mut text = Vec::new();
            
            if let Some(biblio) = &details.bibliographic {
                if let Some(title) = &biblio.title {
                    text.push(Line::from(vec![
                        Span::styled("Title: ", Style::default().add_modifier(Modifier::BOLD)),
                        Span::from(title.clone()),
                    ]));
                }
                
                if let Some(author) = &biblio.author {
                    text.push(Line::from(vec![
                        Span::styled("Author: ", Style::default().add_modifier(Modifier::BOLD)),
                        Span::from(author.clone()),
                    ]));
                }
                
                if let Some(desc) = &biblio.description {
                    text.push(Line::from(""));
                    text.push(Line::from(vec![
                        Span::styled("Description:", Style::default().add_modifier(Modifier::BOLD)),
                    ]));
                    text.push(Line::from(desc.clone()));
                }
            }

            let paragraph = Paragraph::new(text)
                .block(Block::default()
                    .borders(Borders::ALL)
                    .title("Game Details (Esc: Back, 'd': Download)"))
                .wrap(Wrap { trim: true });

            f.render_widget(paragraph, area);
        }
    }

    fn render_status_bar(&self, f: &mut Frame, area: Rect) {
        let status_text = if self.loading {
            "Loading...".to_string()
        } else if let Some(msg) = &self.status_message {
            msg.clone()
        } else {
            match self.input_mode {
                InputMode::Searching => "Search mode - Type to search, Enter to execute, Esc to cancel".to_string(),
                InputMode::Confirmation => "Confirm action? (y/n)".to_string(),
                InputMode::Normal => {
                    // Context-aware status based on current tab
                    let base = "q: Quit | Tab: Switch";
                    match self.state {
                        AppState::GameDetails => format!("{} | d: Download | Esc: Back", base),
                        _ => {
                            match self.current_tab {
                                0 => format!("{} | s: Search | d: Download | r: Refresh", base),
                                1 => format!("{} | x: Delete | r: Refresh", base),
                                2 => format!("{} | r: Refresh", base),
                                _ => base.to_string(),
                            }
                        }
                    }
                }
            }
        };

        let status = Paragraph::new(status_text)
            .block(Block::default().borders(Borders::ALL));
        f.render_widget(status, area);
    }
}