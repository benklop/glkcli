use anyhow::{Context, Result};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use ratatui::{
    backend::Backend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame, Terminal,
};
use std::io::{Read, Write};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{channel, Sender},
    Arc, Mutex,
};
use std::thread;
use std::time::{Duration, Instant};

use crate::pty::InterceptedKey;

/// Set CLOEXEC on all open file descriptors except stdin/stdout/stderr
/// This prevents child processes from inheriting file descriptors like D-Bus sockets
fn set_cloexec_on_fds() {
    // Set CLOEXEC on all FDs >= 3, keeping only stdin (0), stdout (1), stderr (2)
    // This is safer than closing FDs because it doesn't affect the parent process,
    // only the child processes that will be spawned later
    close_fds::set_fds_cloexec(3, &[]);
    
    log::debug!("Set CLOEXEC on all file descriptors >= 3");
}


/// Embedded PTY terminal with status bar
pub struct EmbeddedPty {
    /// VT100 parser/screen
    parser: Arc<Mutex<vt100::Parser>>,
    /// PTY pair
    pty_pair: portable_pty::PtyPair,
    /// Child process
    child: Option<Box<dyn portable_pty::Child + Send>>,
    /// Process ID
    pid: i32,
    /// Reader thread handle
    reader_thread: Option<thread::JoinHandle<()>>,
    /// Writer thread handle  
    writer_thread: Option<thread::JoinHandle<()>>,
    /// Channel to send input to writer thread
    input_tx: Sender<Vec<u8>>,
    /// Flag to signal threads to stop
    should_stop: Arc<AtomicBool>,
    /// Start time for elapsed time display
    start_time: Instant,
    /// Game/command name for display
    game_name: String,
    /// Whether we're showing quit confirmation
    quit_confirmation: bool,
    /// Whether we're showing load confirmation
    load_confirmation: bool,
}

impl EmbeddedPty {
    /// Create and spawn a new embedded PTY
    pub fn spawn(
        command: &str,
        args: &[&str],
        working_dir: Option<&std::path::Path>,
        game_name: String,
    ) -> Result<Self> {
        // Set CLOEXEC on all existing file descriptors BEFORE creating the PTY
        // This prevents children from inheriting D-Bus sockets and other FDs,
        // but allows the PTY library to manage its own FDs properly
        set_cloexec_on_fds();
        
        let pty_system = native_pty_system();

        // Get current terminal size - reserve 3 lines for status bar
        let size = crossterm::terminal::size().unwrap_or((80, 24));
        let rows = size.1.saturating_sub(3).max(10); // Reserve 3 lines for status
        let cols = size.0;

        let pty_size = PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        };

        // Create PTY pair
        let pty_pair = pty_system
            .openpty(pty_size)
            .context("Failed to create PTY")?;

        // Build command
        let mut cmd = CommandBuilder::new(command);
        for arg in args {
            cmd.arg(arg);
        }
        if let Some(dir) = working_dir {
            cmd.cwd(dir);
        }

        log::debug!(
            "Spawning embedded PTY: command='{}' args={:?} cwd={:?}",
            command,
            args,
            working_dir
        );

        // Spawn child process
        let child = pty_pair.slave.spawn_command(cmd).with_context(|| {
            format!(
                "Failed to spawn command in PTY: '{}' with args {:?}",
                command, args
            )
        })?;

        let pid = child
            .process_id()
            .context("Failed to get process ID")?
            .try_into()
            .context("Process ID overflow")?;

        log::debug!("Spawned process {} in embedded PTY", pid);

        // Create VT100 parser with the PTY size
        let parser = Arc::new(Mutex::new(vt100::Parser::new(rows, cols, 0)));
        let parser_clone = Arc::clone(&parser);

        // Clone master for reading (PTY -> parser)
        let mut reader = pty_pair
            .master
            .try_clone_reader()
            .context("Failed to clone PTY reader")?;

        let should_stop = Arc::new(AtomicBool::new(false));
        let should_stop_reader = Arc::clone(&should_stop);

        // Spawn thread to read PTY output and feed to parser
        let reader_thread = thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                if should_stop_reader.load(Ordering::Relaxed) {
                    log::debug!("Reader thread stopping");
                    break;
                }

                match reader.read(&mut buf) {
                    Ok(0) => {
                        log::debug!("PTY reader got EOF");
                        break;
                    }
                    Ok(n) => {
                        // Feed data to VT100 parser
                        if let Ok(mut parser) = parser_clone.lock() {
                            parser.process(&buf[..n]);
                        }
                    }
                    Err(e) => {
                        log::debug!("PTY reader error: {}", e);
                        break;
                    }
                }
            }
            log::debug!("PTY reader thread exiting");
        });

        // Clone master for writing (input -> PTY)
        let mut writer = pty_pair
            .master
            .take_writer()
            .context("Failed to get PTY writer")?;

        let should_stop_writer = Arc::clone(&should_stop);
        
        // Create channel for sending input to writer thread
        let (input_tx, input_rx) = channel::<Vec<u8>>();

        // Spawn thread to handle writing to PTY
        let writer_thread = thread::spawn(move || {
            log::debug!("PTY writer thread starting");
            loop {
                if should_stop_writer.load(Ordering::Relaxed) {
                    log::debug!("Writer thread stopping");
                    break;
                }
                
                // Wait for input with timeout so we can check should_stop
                match input_rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(data) => {
                        if let Err(e) = writer.write_all(&data) {
                            log::error!("Failed to write to PTY: {}", e);
                            break;
                        }
                        if let Err(e) = writer.flush() {
                            log::error!("Failed to flush PTY writer: {}", e);
                            break;
                        }
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        // Continue waiting
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                        log::debug!("Input channel disconnected");
                        break;
                    }
                }
            }
            log::debug!("PTY writer thread exiting");
        });

        Ok(EmbeddedPty {
            parser,
            pty_pair,
            child: Some(child),
            pid,
            reader_thread: Some(reader_thread),
            writer_thread: Some(writer_thread),
            input_tx,
            should_stop,
            start_time: Instant::now(),
            game_name,
            quit_confirmation: false,
            load_confirmation: false,
        })
    }

    /// Check if the process is still running
    pub fn is_running(&mut self) -> bool {
        if let Some(child) = &mut self.child {
            match child.try_wait() {
                Ok(Some(_)) => false,
                Ok(None) => true,
                Err(_) => false,
            }
        } else {
            false
        }
    }

    /// Get process ID
    pub fn pid(&self) -> i32 {
        self.pid
    }

    /// Send signal to process
    pub fn signal(&self, signal: Signal) -> Result<()> {
        kill(Pid::from_raw(self.pid), signal)
            .with_context(|| format!("Failed to send signal {:?} to process {}", signal, self.pid))
    }

    /// Write input to the PTY
    pub fn write_input(&mut self, data: &[u8]) -> Result<()> {
        self.input_tx
            .send(data.to_vec())
            .context("Failed to send input to writer thread")?;
        Ok(())
    }

    /// Handle a key event and write to PTY
    pub fn handle_key(&mut self, key_event: &KeyEvent) -> Result<Option<InterceptedKey>> {
        // If we're in quit confirmation mode, handle Y/N
        if self.quit_confirmation {
            match key_event.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    // User confirmed quit
                    self.quit_confirmation = false;
                    return Ok(Some(InterceptedKey::QuitPrompt));
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    // User cancelled quit
                    self.quit_confirmation = false;
                    return Ok(None);
                }
                _ => {
                    // Ignore other keys during confirmation
                    return Ok(None);
                }
            }
        }

        // If we're in load confirmation mode, handle Y/N
        if self.load_confirmation {
            match key_event.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    // User confirmed load
                    self.load_confirmation = false;
                    return Ok(Some(InterceptedKey::QuickReload));
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    // User cancelled load
                    self.load_confirmation = false;
                    return Ok(None);
                }
                _ => {
                    // Ignore other keys during confirmation
                    return Ok(None);
                }
            }
        }

        // Check if this is an intercepted key
        if let Some(intercepted) = check_intercepted_key(key_event) {
            // Handle F4 specially - show confirmation instead of returning immediately
            if intercepted == InterceptedKey::QuitPrompt {
                self.quit_confirmation = true;
                return Ok(None); // Don't return QuitPrompt yet, just show confirmation
            }
            // Handle F3 specially - show confirmation instead of returning immediately
            if intercepted == InterceptedKey::QuickReload {
                self.load_confirmation = true;
                return Ok(None); // Don't return QuickReload yet, just show confirmation
            }
            // Shift+F3 bypasses confirmation
            return Ok(Some(intercepted));
        }

        // Convert key to bytes and write to PTY
        let bytes = key_event_to_bytes(key_event);
        if !bytes.is_empty() {
            self.write_input(&bytes)?;
        }

        Ok(None)
    }

    /// Show quit confirmation prompt
    pub fn show_quit_confirmation(&mut self) {
        self.quit_confirmation = true;
    }

    /// Check if quit confirmation is active
    pub fn is_quit_confirmation_active(&self) -> bool {
        self.quit_confirmation
    }

    /// Show load confirmation prompt
    pub fn show_load_confirmation(&mut self) {
        self.load_confirmation = true;
    }

    /// Check if load confirmation is active
    pub fn is_load_confirmation_active(&self) -> bool {
        self.load_confirmation
    }

    /// Render the embedded terminal with status bar
    pub fn render(&self, frame: &mut Frame) {
        let size = frame.size();

        // Split the screen: terminal area + status bar (3 lines)
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(10),      // Terminal display
                Constraint::Length(3),    // Status bar
            ])
            .split(size);

        // Render terminal content
        self.render_terminal(frame, chunks[0]);

        // Render status bar
        self.render_status_bar(frame, chunks[1]);
    }

    /// Render the terminal content area
    fn render_terminal(&self, frame: &mut Frame, area: Rect) {
        if let Ok(parser) = self.parser.lock() {
            let screen = parser.screen();
            
            // Get the screen contents as formatted text
            let contents = screen.contents();
            
            // Split into lines for rendering
            let lines: Vec<Line> = contents
                .lines()
                .map(|line| Line::from(line.to_string()))
                .collect();
            
            let paragraph = Paragraph::new(lines)
                .block(Block::default().borders(Borders::NONE));
            
            frame.render_widget(paragraph, area);
        }
    }

    /// Render the status bar
    fn render_status_bar(&self, frame: &mut Frame, area: Rect) {
        if self.quit_confirmation {
            // Show quit confirmation prompt
            let confirm_line = Line::from(vec![
                Span::styled(
                    " ⚠ Quit Game? ",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" "),
                Span::styled(
                    "Y",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                ),
                Span::raw("es / "),
                Span::styled(
                    "N",
                    Style::default()
                        .fg(Color::Red)
                        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                ),
                Span::raw("o"),
            ]);

            let paragraph = Paragraph::new(vec![Line::raw(""), confirm_line])
                .block(
                    Block::default()
                        .borders(Borders::TOP)
                        .border_style(Style::default().fg(Color::Yellow)),
                )
                .alignment(Alignment::Center);

            frame.render_widget(paragraph, area);
        } else if self.load_confirmation {
            // Show load confirmation prompt
            let confirm_line = Line::from(vec![
                Span::styled(
                    " ⚠ Load Checkpoint? This will lose unsaved progress! ",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" "),
                Span::styled(
                    "Y",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                ),
                Span::raw("es / "),
                Span::styled(
                    "N",
                    Style::default()
                        .fg(Color::Red)
                        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                ),
                Span::raw("o"),
            ]);

            let paragraph = Paragraph::new(vec![Line::raw(""), confirm_line])
                .block(
                    Block::default()
                        .borders(Borders::TOP)
                        .border_style(Style::default().fg(Color::Yellow)),
                )
                .alignment(Alignment::Center);

            frame.render_widget(paragraph, area);
        } else {
            // Show normal status bar
            let elapsed = self.start_time.elapsed();
            let hours = elapsed.as_secs() / 3600;
            let minutes = (elapsed.as_secs() % 3600) / 60;
            let seconds = elapsed.as_secs() % 60;

            let time_str = if hours > 0 {
                format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
            } else {
                format!("{:02}:{:02}", minutes, seconds)
            };

            let status_line = Line::from(vec![
                Span::styled(
                    format!(" {} ", self.game_name),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" │ "),
                Span::styled(
                    format!("⏱ {}", time_str),
                    Style::default().fg(Color::Yellow),
                ),
                Span::raw(" │ "),
                Span::styled("F1", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::raw(" Save+Exit │ "),
                Span::styled("F2", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::raw(" Save │ "),
                Span::styled("F3", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::raw(" Load │ "),
                Span::styled("F4", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                Span::raw(" Quit"),
            ]);

            let paragraph = Paragraph::new(vec![Line::raw(""), status_line])
                .block(
                    Block::default()
                        .borders(Borders::TOP)
                        .border_style(Style::default().fg(Color::DarkGray)),
                )
                .alignment(Alignment::Left);

            frame.render_widget(paragraph, area);
        }
    }

    /// Resize the terminal
    pub fn resize(&mut self, cols: u16, rows: u16) -> Result<()> {
        // Reserve space for status bar
        let term_rows = rows.saturating_sub(3).max(10);
        
        if let Ok(mut parser) = self.parser.lock() {
            *parser = vt100::Parser::new(term_rows, cols, 0);
        }
        
        let pty_size = PtySize {
            rows: term_rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        };
        
        self.pty_pair
            .master
            .resize(pty_size)
            .context("Failed to resize PTY")?;
        
        Ok(())
    }

    /// Wait for the process to exit
    pub fn wait(&mut self) -> Result<()> {
        self.cleanup()
    }

    /// Cleanup resources
    fn cleanup(&mut self) -> Result<()> {
        log::debug!("Cleaning up embedded PTY for PID {}", self.pid);

        // Signal threads to stop
        self.should_stop.store(true, Ordering::Relaxed);

        // Wait for child process
        if let Some(mut child) = self.child.take() {
            log::debug!("Waiting for child process {} to exit", self.pid);
            match child.wait() {
                Ok(status) => {
                    log::debug!("Child process {} exited with status: {:?}", self.pid, status);
                }
                Err(e) => {
                    log::warn!("Error waiting for child process {}: {}", self.pid, e);
                }
            }
        }

        // Join threads
        if let Some(handle) = self.writer_thread.take() {
            let _ = handle.join();
        }
        if let Some(handle) = self.reader_thread.take() {
            let _ = handle.join();
        }

        Ok(())
    }
}

impl Drop for EmbeddedPty {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

/// Convert VT100 color to ratatui color
fn vt_color_to_ratatui(color: vt100::Color) -> Color {
    match color {
        vt100::Color::Default => Color::Reset,
        vt100::Color::Idx(0) => Color::Black,
        vt100::Color::Idx(1) => Color::Red,
        vt100::Color::Idx(2) => Color::Green,
        vt100::Color::Idx(3) => Color::Yellow,
        vt100::Color::Idx(4) => Color::Blue,
        vt100::Color::Idx(5) => Color::Magenta,
        vt100::Color::Idx(6) => Color::Cyan,
        vt100::Color::Idx(7) => Color::Gray,
        vt100::Color::Idx(8) => Color::DarkGray,
        vt100::Color::Idx(9) => Color::LightRed,
        vt100::Color::Idx(10) => Color::LightGreen,
        vt100::Color::Idx(11) => Color::LightYellow,
        vt100::Color::Idx(12) => Color::LightBlue,
        vt100::Color::Idx(13) => Color::LightMagenta,
        vt100::Color::Idx(14) => Color::LightCyan,
        vt100::Color::Idx(15) => Color::White,
        vt100::Color::Idx(n) => Color::Indexed(n),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

/// Check if a key event should be intercepted
fn check_intercepted_key(key_event: &KeyEvent) -> Option<InterceptedKey> {
    match key_event.code {
        KeyCode::F(1) => Some(InterceptedKey::SaveAndExit),
        KeyCode::F(2) => Some(InterceptedKey::SaveAndContinue),
        KeyCode::F(3) => {
            if key_event.modifiers.contains(KeyModifiers::SHIFT) {
                Some(InterceptedKey::QuickReloadNoConfirm)
            } else {
                Some(InterceptedKey::QuickReload)
            }
        }
        KeyCode::F(4) => Some(InterceptedKey::QuitPrompt),
        // ESC is not intercepted - let games handle it themselves
        _ => None,
    }
}

/// Convert a key event to bytes for PTY input
fn key_event_to_bytes(key_event: &KeyEvent) -> Vec<u8> {
    match key_event.code {
        KeyCode::Char(c) => {
            if key_event.modifiers.contains(KeyModifiers::CONTROL) {
                // Control characters
                if c.is_ascii_alphabetic() {
                    let ctrl_char = (c.to_ascii_uppercase() as u8 - b'A' + 1) as char;
                    vec![ctrl_char as u8]
                } else {
                    vec![c as u8]
                }
            } else if key_event.modifiers.contains(KeyModifiers::ALT) {
                // Alt key sends ESC prefix
                vec![0x1B, c as u8]
            } else {
                c.to_string().into_bytes()
            }
        }
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Backspace => vec![0x7F],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::Up => b"\x1B[A".to_vec(),
        KeyCode::Down => b"\x1B[B".to_vec(),
        KeyCode::Right => b"\x1B[C".to_vec(),
        KeyCode::Left => b"\x1B[D".to_vec(),
        KeyCode::Home => b"\x1BOH".to_vec(),
        KeyCode::End => b"\x1BOF".to_vec(),
        KeyCode::PageUp => b"\x1B[5~".to_vec(),
        KeyCode::PageDown => b"\x1B[6~".to_vec(),
        KeyCode::Delete => b"\x1B[3~".to_vec(),
        KeyCode::Insert => b"\x1B[2~".to_vec(),
        _ => vec![],
    }
}

/// Run an embedded PTY in a ratatui terminal
pub fn run_embedded_pty<B: Backend>(
    terminal: &mut Terminal<B>,
    mut pty: EmbeddedPty,
) -> Result<()> {
    loop {
        // Draw the current state
        terminal.draw(|f| pty.render(f))?;

        // Poll for events
        if crossterm::event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = crossterm::event::read()? {
                // Handle key input
                match pty.handle_key(&key)? {
                    Some(InterceptedKey::SaveAndExit) => {
                        log::info!("Save and exit requested");
                        // TODO: Implement checkpoint save
                        break;
                    }
                    Some(InterceptedKey::SaveAndContinue) => {
                        log::info!("Save and continue requested");
                        // TODO: Implement checkpoint save
                    }
                    Some(InterceptedKey::QuickReload) => {
                        log::info!("Quick reload requested");
                        // TODO: Implement checkpoint reload
                    }
                    Some(InterceptedKey::QuickReloadNoConfirm) => {
                        log::info!("Quick reload without confirmation requested");
                        // TODO: Implement checkpoint reload
                    }
                    Some(InterceptedKey::QuitPrompt) => {
                        log::info!("Quit confirmed");
                        break;
                    }
                    Some(InterceptedKey::ExitPrompt) => {
                        log::info!("Exit prompt requested");
                        // TODO: Show exit confirmation
                        break;
                    }
                    None => {
                        // Key was forwarded to PTY
                    }
                }
            } else if let Event::Resize(cols, rows) = crossterm::event::read()? {
                pty.resize(cols, rows)?;
            }
        }

        // Check if process has exited
        if !pty.is_running() {
            log::info!("Process has exited");
            break;
        }
    }

    pty.wait()?;
    Ok(())
}
