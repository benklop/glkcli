//! Application state management
//!
//! This module contains the core application state structure and enums
//! that define the different states and modes the application can be in.

use ratatui::widgets::ListState;

use crate::ifdb::{IfdbClient, Game, GameDetails};
use crate::storage::{GameStorage, LocalGame, SaveFile, Checkpoint};
use crate::launcher::Launcher;
use crate::network::NetworkChecker;

/// Main TUI application state
pub struct TuiApp {
    /// Current application state
    pub(crate) state: AppState,
    /// IFDB API client
    pub(crate) ifdb_client: IfdbClient,
    /// Local game storage
    pub(crate) storage: GameStorage,
    /// Game launcher
    pub(crate) launcher: Launcher,
    /// Network connectivity checker
    pub(crate) network: NetworkChecker,
    /// Whether network is available
    pub(crate) is_online: bool,
    /// Debug mode enabled
    pub(crate) debug: bool,
    /// Current tab index
    pub(crate) current_tab: usize,
    /// Search input state
    pub(crate) search_input: String,
    /// Search results
    pub(crate) search_results: Vec<Game>,
    /// Selected game in search results
    pub(crate) search_selection: ListState,
    /// Current page for search results
    pub(crate) current_search_page: u32,
    /// Whether there are more search results to load
    pub(crate) has_more_search_results: bool,
    /// Flag to trigger loading next page
    pub(crate) should_load_next_page: bool,
    /// Downloaded games
    pub(crate) downloaded_games: Vec<LocalGame>,
    /// Selected downloaded game
    pub(crate) downloaded_selection: ListState,
    /// Save files for current game
    pub(crate) save_files: Vec<SaveFile>,
    /// Selected save file
    pub(crate) save_selection: ListState,
    /// Checkpoints for current game
    pub(crate) checkpoints: Vec<Checkpoint>,
    /// Selected checkpoint
    pub(crate) checkpoint_selection: ListState,
    /// Current game details being viewed
    pub(crate) current_game_details: Option<GameDetails>,
    /// Loading state
    pub(crate) loading: bool,
    /// Status message
    pub(crate) status_message: Option<String>,
    /// Time when status message was set (for auto-clearing)
    pub(crate) status_message_time: Option<std::time::Instant>,
    /// Input mode
    pub(crate) input_mode: InputMode,
    /// File path being entered for import
    pub(crate) import_file_path: String,
    /// TUID of game being imported (for commercial games)
    pub(crate) import_game_tuid: Option<String>,
    /// Flag to indicate terminal needs full redraw
    pub(crate) needs_redraw: bool,
}

/// Application state - which view/screen is currently active
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
pub enum AppState {
    /// Browsing/searching for games in IFDB
    Browse,
    /// Viewing details of a specific game
    GameDetails,
    /// Showing save files dialog
    SaveFilesDialog,
    /// Showing checkpoints browser
    CheckpointBrowser,
    /// Downloading a game (transition state)
    Download,
    /// Viewing downloaded games
    DownloadedGames,
    /// Settings screen (future use)
    Settings,
}

/// Input mode - what type of input the app is currently accepting
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
pub enum InputMode {
    /// Normal navigation mode
    Normal,
    /// Entering search query
    Searching,
    /// Awaiting confirmation (e.g., for deletion)
    Confirmation,
    /// Entering file path for game import
    ImportingFile,
}

impl TuiApp {
    /// Create a new TuiApp instance
    pub async fn new(debug: bool, assume_online: bool) -> anyhow::Result<Self> {
        let ifdb_client = IfdbClient::new()?;
        let storage = GameStorage::new()?;
        let launcher = Launcher::new()?;
        let network = NetworkChecker::new(debug, assume_online);

        // Check network connectivity
        let is_online = network.is_connected().await;
        
        if debug {
            log::info!("Network connectivity: {}", if is_online { "online" } else { "offline" });
        }

        Ok(TuiApp {
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
            checkpoints: Vec::new(),
            checkpoint_selection: ListState::default(),
            current_game_details: None,
            loading: false,
            status_message: None,
            status_message_time: None,
            input_mode: InputMode::Normal,
            import_file_path: String::new(),
            import_game_tuid: None,
            needs_redraw: false,
        })
    }

    /// Set a status message that will be displayed to the user
    pub(crate) fn set_status_message(&mut self, message: String) {
        self.status_message = Some(message);
        self.status_message_time = Some(std::time::Instant::now());
    }

    /// Clear the status message
    pub(crate) fn clear_status_message(&mut self) {
        self.status_message = None;
        self.status_message_time = None;
    }

    /// Check if status message should be auto-cleared (after 5 seconds)
    pub(crate) fn check_status_timeout(&mut self) {
        if let Some(time) = self.status_message_time {
            if time.elapsed().as_secs() >= 5 {
                self.clear_status_message();
            }
        }
    }
}
