//! UI rendering module
//!
//! This module contains all the rendering logic for the TUI application.
//! It's responsible for drawing the interface elements: tabs, game lists,
//! game details, status bars, and dialogs.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Tabs, Wrap},
    Frame,
};
use std::time::SystemTime;

use crate::app::state::{TuiApp, AppState, InputMode};
use crate::utils::decode_html_entities;
use crate::border_style::get_border_type;

/// Helper function to create a block with appropriate border type for the terminal
fn create_block() -> Block<'static> {
    Block::default().border_type(get_border_type())
}

/// UI rendering implementation for TuiApp
impl TuiApp {
    /// Render the main UI layout
    pub(crate) fn ui(&mut self, f: &mut Frame) {
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

    /// Render the tab bar at the top
    pub(crate) fn render_tabs(&self, f: &mut Frame, area: Rect) {
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
            .block(create_block()
                .borders(Borders::ALL)
                .title(title))
            .style(Style::default().fg(Color::Gray))
            .highlight_style(Style::default().fg(Color::Yellow))
            .select(current_tab);
        f.render_widget(tabs, area);
    }

    /// Render the main content area based on current state
    pub(crate) fn render_main_content(&mut self, f: &mut Frame, area: Rect) {
        match self.state {
            AppState::GameDetails => self.render_game_details(f, area),
            AppState::SaveFilesDialog => self.render_saves_dialog(f, area),
            AppState::CheckpointBrowser => self.render_checkpoints_dialog(f, area),
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

    /// Render the browse/search tab
    pub(crate) fn render_browse_tab(&mut self, f: &mut Frame, area: Rect) {
        if !self.is_online {
            // Show offline message
            let offline_msg = Paragraph::new("Network connection unavailable.\n\nBrowse and search features are disabled.\n\nPress Tab to view your downloaded games.")
                .block(create_block()
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
            .block(create_block()
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
            .block(create_block()
                .borders(Borders::ALL)
                .title("Games (Enter: Details, 'd': Download)"))
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
            .highlight_symbol("> ");

        f.render_stateful_widget(list, chunks[1], &mut self.search_selection);
    }

    /// Render the downloaded games tab
    pub(crate) fn render_downloaded_tab(&mut self, f: &mut Frame, area: Rect) {
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
            .block(create_block()
                .borders(Borders::ALL)
                .title("Downloaded Games (Enter: Launch | c: Checkpoints | v: View Saves | x: Delete)"))
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
            .highlight_symbol("> ");

        f.render_stateful_widget(list, area, &mut self.downloaded_selection);
    }

    /// Render the save files dialog
    pub(crate) fn render_saves_dialog(&mut self, f: &mut Frame, area: Rect) {
        if self.save_files.is_empty() {
            // Show helpful message when no saves
            let msg = "No save files found for this game.\n\nPlay the game to create save files.\n\nPress Esc to close.";
            
            let paragraph = Paragraph::new(msg)
                .block(create_block()
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
            .block(create_block()
                .borders(Borders::ALL)
                .title(title))
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
            .highlight_symbol("> ");

        f.render_stateful_widget(list, area, &mut self.save_selection);
    }

    pub(crate) fn render_checkpoints_dialog(&mut self, f: &mut Frame, area: Rect) {
        if self.checkpoints.is_empty() {
            // Show helpful message when no checkpoints
            let msg = "No checkpoints found for this game.\n\n\
                      While playing:\n\
                      • Press F1 to save and exit\n\
                      • Press F2 to quick-save and continue\n\
                      • Press F3 to quick-reload last checkpoint\n\n\
                      Press Esc to close.";
            
            let paragraph = Paragraph::new(msg)
                .block(create_block()
                    .borders(Borders::ALL)
                    .title("Checkpoints"))
                .wrap(Wrap { trim: true });
            f.render_widget(paragraph, area);
            return;
        }
        
        let items: Vec<ListItem> = self.checkpoints
            .iter()
            .map(|checkpoint| {
                let date = checkpoint.created_at
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .ok()
                    .and_then(|d| {
                        chrono::DateTime::from_timestamp(d.as_secs() as i64, 0)
                    })
                    .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                    .unwrap_or_else(|| "Unknown date".to_string());
                
                // Format playtime as HH:MM:SS
                let hours = checkpoint.playtime_seconds / 3600;
                let minutes = (checkpoint.playtime_seconds % 3600) / 60;
                let seconds = checkpoint.playtime_seconds % 60;
                let playtime = format!("{}:{:02}:{:02}", hours, minutes, seconds);
                    
                ListItem::new(format!("{} - {} | Playtime: {}", 
                    checkpoint.name, date, playtime))
            })
            .collect();

        let title = if let Some(i) = self.downloaded_selection.selected() {
            if let Some(game) = self.downloaded_games.get(i) {
                format!("Checkpoints for: {} (Enter: Load, Esc: Close)", game.title)
            } else {
                "Checkpoints (Enter: Load, Esc: Close)".to_string()
            }
        } else {
            "Checkpoints (Enter: Load, Esc: Close)".to_string()
        };

        let list = List::new(items)
            .block(create_block()
                .borders(Borders::ALL)
                .title(title))
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
            .highlight_symbol("> ");

        f.render_stateful_widget(list, area, &mut self.checkpoint_selection);
    }

    /// Render the game details view
    pub(crate) fn render_game_details(&self, f: &mut Frame, area: Rect) {
        if let Some(details) = &self.current_game_details {
            // If in import mode, show import input at the top
            let (details_area, import_input_area) = if self.input_mode == InputMode::ImportingFile {
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(3), // Import input
                        Constraint::Min(0),    // Game details
                    ])
                    .split(area);
                (chunks[1], Some(chunks[0]))
            } else {
                (area, None)
            };

            let mut text = Vec::new();
            
            // Check if this is a commercial game
            let is_commercial = details.is_commercial();
            
            // Check if game is already downloaded
            let tuid = details.ifdb.as_ref().map(|i| i.tuid.as_str()).unwrap_or("");
            let is_downloaded = self.storage.is_game_downloaded(tuid).unwrap_or(false);
            
            if let Some(biblio) = &details.bibliographic {
                if let Some(title) = &biblio.title {
                    text.push(Line::from(vec![
                        Span::styled("Title: ", Style::default().add_modifier(Modifier::BOLD)),
                        Span::from(decode_html_entities(title)),
                    ]));
                }
                
                if let Some(author) = &biblio.author {
                    text.push(Line::from(vec![
                        Span::styled("Author: ", Style::default().add_modifier(Modifier::BOLD)),
                        Span::from(decode_html_entities(author)),
                    ]));
                }
                
                // Show download status
                if is_downloaded {
                    text.push(Line::from(""));
                    text.push(Line::from(vec![
                        Span::styled("✓ ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                        Span::styled("Already in My Games", 
                            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                    ]));
                }
                
                // Show commercial status prominently
                if is_commercial {
                    text.push(Line::from(""));
                    text.push(Line::from(vec![
                        Span::styled("⚠ COMMERCIAL GAME ", 
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::BOLD)),
                        Span::styled("(Must be purchased separately)", 
                            Style::default().fg(Color::Yellow)),
                    ]));
                    
                    if let Some(url) = details.get_purchase_url() {
                        text.push(Line::from(vec![
                            Span::styled("Purchase: ", Style::default().add_modifier(Modifier::BOLD)),
                            Span::from(url),
                        ]));
                    }
                    
                    if !is_downloaded {
                        text.push(Line::from(""));
                        text.push(Line::from(vec![
                            Span::styled("→ ", Style::default().fg(Color::Green)),
                            Span::from("After purchasing, press 'i' to import the game file"),
                        ]));
                    }
                }
                
                if let Some(desc) = &biblio.description {
                    text.push(Line::from(""));
                    text.push(Line::from(vec![
                        Span::styled("Description:", Style::default().add_modifier(Modifier::BOLD)),
                    ]));
                    // Decode HTML entities in the description
                    let decoded_desc = decode_html_entities(desc);
                    text.push(Line::from(decoded_desc));
                }
            }

            let title = if is_downloaded {
                "Game Details (Esc: Back) - Already Downloaded"
            } else if is_commercial {
                "Game Details (i: Import | Esc: Back)"
            } else {
                "Game Details (Esc: Back, 'd': Download)"
            };

            let paragraph = Paragraph::new(text)
                .block(create_block()
                    .borders(Borders::ALL)
                    .title(title))
                .wrap(Wrap { trim: true });

            f.render_widget(paragraph, details_area);

            // Render import input if active
            if let Some(input_area) = import_input_area {
                let import_style = Style::default().fg(Color::Green);
                let import_input = Paragraph::new(self.import_file_path.as_str())
                    .style(import_style)
                    .block(create_block()
                        .borders(Borders::ALL)
                        .title("Enter file path (supports ~/ for home directory)"));
                f.render_widget(import_input, input_area);
            }
        }
    }

    /// Render the status bar at the bottom
    pub(crate) fn render_status_bar(&self, f: &mut Frame, area: Rect) {
        let status_text = if self.loading {
            "Loading...".to_string()
        } else if let Some(msg) = &self.status_message {
            msg.clone()
        } else {
            match self.input_mode {
                InputMode::Searching => "Search mode - Type to search, Enter to execute, Esc to cancel".to_string(),
                InputMode::Confirmation => "Confirm action? (y/n)".to_string(),
                InputMode::ImportingFile => "Import mode - Enter file path, Enter to confirm, Esc to cancel".to_string(),
                InputMode::Normal => {
                    // Context-aware status based on current tab
                    let base = "q: Quit | Tab: Switch";
                    match self.state {
                        AppState::GameDetails => {
                            // Check if game is already downloaded
                            let tuid = self.current_game_details
                                .as_ref()
                                .and_then(|d| d.ifdb.as_ref())
                                .map(|i| i.tuid.as_str())
                                .unwrap_or("");
                            let is_downloaded = self.storage.is_game_downloaded(tuid).unwrap_or(false);
                            
                            if is_downloaded {
                                format!("{} | ↑↓: Navigate | Esc: Back", base)
                            } else {
                                // Check if current game is commercial
                                let is_commercial = self.current_game_details
                                    .as_ref()
                                    .map(|d| d.is_commercial())
                                    .unwrap_or(false);
                                
                                if is_commercial {
                                    format!("{} | ↑↓: Navigate | i: Import | Esc: Back", base)
                                } else {
                                    format!("{} | ↑↓: Navigate | d: Download | Esc: Back", base)
                                }
                            }
                        }
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
            .block(create_block().borders(Borders::ALL));
        f.render_widget(status, area);
    }
}
