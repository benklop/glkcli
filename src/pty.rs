use anyhow::{Context, Result};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{Read, Write};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;
use std::time::Duration;

/// Special keystroke events that can be intercepted during game execution
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterceptedKey {
    /// F1: Save checkpoint and exit to menu
    SaveAndExit,
    /// F2: Save checkpoint and continue playing
    SaveAndContinue,
    /// F3: Quick reload from last checkpoint
    QuickReload,
    /// Escape: Prompt to save before exit
    ExitPrompt,
}

/// Handle to a running process in a PTY
pub struct PtyHandle {
    /// Process ID of the child process
    pub pid: i32,
    /// Channel to receive intercepted keystrokes
    pub keystroke_rx: Receiver<InterceptedKey>,
    /// Reader handle (for thread join)
    reader_thread: Option<thread::JoinHandle<()>>,
    /// Writer handle (for thread join)
    writer_thread: Option<thread::JoinHandle<()>>,
}

impl PtyHandle {
    /// Get the process ID as nix::Pid
    pub fn pid_as_nix(&self) -> Pid {
        Pid::from_raw(self.pid)
    }

    /// Wait for the process to exit
    pub fn wait(&mut self) -> Result<()> {
        // Join the I/O threads
        if let Some(handle) = self.reader_thread.take() {
            let _ = handle.join();
        }
        if let Some(handle) = self.writer_thread.take() {
            let _ = handle.join();
        }
        Ok(())
    }

    /// Send a signal to the process
    pub fn signal(&self, signal: Signal) -> Result<()> {
        kill(self.pid_as_nix(), signal)
            .with_context(|| format!("Failed to send signal {:?} to process {}", signal, self.pid))
    }
}

/// Spawn a command in a PTY and return a handle
///
/// The PTY will intercept special keys (F1, F2, F3, Escape) and send them
/// through the keystroke_rx channel. All other input/output is proxied
/// transparently between the terminal and the child process.
pub fn spawn_in_pty(
    command: &str,
    args: &[&str],
    working_dir: Option<&std::path::Path>,
) -> Result<PtyHandle> {
    let pty_system = native_pty_system();

    // Get current terminal size
    let size = crossterm::terminal::size().unwrap_or((80, 24));
    let pty_size = PtySize {
        rows: size.1,
        cols: size.0,
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

    // Spawn child process
    let child = pty_pair
        .slave
        .spawn_command(cmd)
        .context("Failed to spawn command in PTY")?;

    let pid = child
        .process_id()
        .context("Failed to get process ID")?
        .try_into()
        .context("Process ID overflow")?;

    log::debug!("Spawned process {} in PTY: {} {:?}", pid, command, args);

    // Create channel for intercepted keystrokes
    let (keystroke_tx, keystroke_rx) = channel();

    // Clone master for reading (PTY -> stdout)
    let mut reader = pty_pair
        .master
        .try_clone_reader()
        .context("Failed to clone PTY reader")?;

    // Spawn thread to forward PTY output to stdout
    let reader_thread = thread::spawn(move || {
        let mut buf = [0u8; 8192];
        let mut stdout = std::io::stdout();
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break, // EOF
                Ok(n) => {
                    if stdout.write_all(&buf[..n]).is_err() {
                        break;
                    }
                    let _ = stdout.flush();
                }
                Err(_) => break,
            }
        }
    });

    // Clone master for writing (stdin -> PTY)
    let mut writer = pty_pair
        .master
        .take_writer()
        .context("Failed to get PTY writer")?;

    // Spawn thread to forward stdin to PTY with key interception
    let writer_thread = thread::spawn(move || {
        if let Err(e) = forward_with_interception(&mut writer, keystroke_tx) {
            log::error!("Error in input forwarding: {}", e);
        }
    });

    Ok(PtyHandle {
        pid,
        keystroke_rx,
        reader_thread: Some(reader_thread),
        writer_thread: Some(writer_thread),
    })
}

/// Forward stdin to PTY writer, intercepting special keys
fn forward_with_interception(
    writer: &mut Box<dyn Write + Send>,
    keystroke_tx: Sender<InterceptedKey>,
) -> Result<()> {
    // Enable raw mode to capture key events
    crossterm::terminal::enable_raw_mode()?;

    loop {
        // Poll for events with timeout
        if crossterm::event::poll(Duration::from_millis(100))? {
            match crossterm::event::read()? {
                Event::Key(key_event) => {
                    // Check if this is an intercepted key
                    if let Some(intercepted) = check_intercepted_key(&key_event) {
                        log::debug!("Intercepted key: {:?}", intercepted);
                        // Send to main thread
                        if keystroke_tx.send(intercepted).is_err() {
                            // Main thread dropped receiver, exit
                            break;
                        }
                    } else {
                        // Forward to PTY
                        if let Err(_) = write_key_event(writer, &key_event) {
                            break;
                        }
                    }
                }
                Event::Resize(cols, rows) => {
                    // TODO: Handle terminal resize (send SIGWINCH or resize PTY)
                    log::debug!("Terminal resized: {}x{}", cols, rows);
                }
                _ => {}
            }
        }
    }

    crossterm::terminal::disable_raw_mode()?;
    Ok(())
}

/// Check if a key event should be intercepted
fn check_intercepted_key(key_event: &KeyEvent) -> Option<InterceptedKey> {
    match key_event.code {
        KeyCode::F(1) => Some(InterceptedKey::SaveAndExit),
        KeyCode::F(2) => Some(InterceptedKey::SaveAndContinue),
        KeyCode::F(3) => Some(InterceptedKey::QuickReload),
        KeyCode::Esc => Some(InterceptedKey::ExitPrompt),
        _ => None,
    }
}

/// Write a key event to the PTY
fn write_key_event(writer: &mut Box<dyn Write + Send>, key_event: &KeyEvent) -> Result<()> {
    // Convert crossterm key event to bytes
    let bytes = match key_event.code {
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
        KeyCode::F(n) => format!("\x1B[{}", n + 10).into_bytes(), // F1-F12
        _ => return Ok(()), // Ignore other keys
    };

    writer.write_all(&bytes)?;
    writer.flush()?;
    Ok(())
}
