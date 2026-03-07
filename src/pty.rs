//! PTY (Pseudo-Terminal) types and utilities
//!
//! This module defines common types used for PTY-based game execution.
//! The actual PTY implementation has been moved to `pty_embedded.rs`.

/// Special keystroke events that can be intercepted during game execution
///
/// These represent function key shortcuts that control game state:
/// - F1: Save checkpoint and exit to menu
/// - F2: Save checkpoint and continue playing
/// - F3: Quick reload from last checkpoint (with confirmation)
/// - Shift+F3: Quick reload without confirmation
/// - F4: Quit with confirmation prompt
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterceptedKey {
    /// F1: Save checkpoint and exit to menu
    SaveAndExit,
    /// F2: Save checkpoint and continue playing
    SaveAndContinue,
    /// F3: Quick reload from last checkpoint (with confirmation)
    QuickReload,
    /// Shift+F3: Quick reload without confirmation
    QuickReloadNoConfirm,
    /// F4: Quit with confirmation prompt
    QuitPrompt,
    /// Escape: Prompt to save before exit (legacy, not currently used)
    ExitPrompt,
}
