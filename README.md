# Rust GLK CLI Launcher

A memory-safe Rust launcher for glkterm based interactive fiction interpreters.

## Features

- Automatic detection of game file formats by header and extension
- TUI browser for discovering and downloading games from IFDB
- Local game library management
- **CRIU-based checkpoint system for uniform save/restore across all game types**
- Playtime tracking per game
- Hotkey support during gameplay (F1/F2/F3)

## CRIU Checkpoints (New!)

This launcher now supports process-level checkpoints using CRIU (Checkpoint/Restore In Userspace). This provides uniform save/restore functionality across all interpreter types, with automatic playtime tracking.

### Requirements

- **Linux only** (CRIU does not support macOS or Windows)
- CRIU installed: `sudo apt install criu` (Debian/Ubuntu) or equivalent
- Proper capabilities configured (one of):
  - Run glkcli with sufficient privileges
  - Set capabilities: `sudo setcap cap_checkpoint_restore+eip $(readlink -f $(which criu))` (kernel 5.9+)
  - For older kernels: `sudo setcap cap_sys_admin,cap_sys_ptrace,cap_dac_override+eip $(readlink -f $(which criu))`
  - Note: Use `readlink -f` to resolve symlinks to the actual binary

### Usage During Gameplay

While playing any game:

- **F1**: Create checkpoint and exit to menu
- **F2**: Quick-save (checkpoint) and continue playing
- **F3**: Quick-reload from last checkpoint
- **Escape**: Exit prompt (future: will ask to save)

### Managing Checkpoints

From the TUI "My Games" tab:

1. Select a game with arrow keys
2. Press **c** to view checkpoints for that game
3. Use arrow keys to navigate checkpoints
4. Press **Enter** to load a checkpoint
5. Press **Esc** to close checkpoint browser

Each checkpoint displays:
- Checkpoint name
- Creation date/time
- Total playtime (HH:MM:SS format)

### How It Works

CRIU creates process-level snapshots of the running interpreter, including:
- All memory state
- Open file descriptors
- Terminal state
- Game progress

This means you can save at any point during gameplay, even if the game itself doesn't support saving.

### Limitations

- CRIU restore with PTY re-attachment is not yet fully implemented (F3 quick-reload has limitations)
- No in-game dialog overlays yet (F1/Escape just exit, no confirmation)
- Checkpoint files can be 5-50MB each depending on interpreter
- Requires Linux kernel with CRIU support

## Supported Game Formats

- Z-code (.z1-.z8, .dat) → bocfel
- Glulx (.ulx) → git  
- TADS (.gam, .t3) → tadsr
- Hugo (.hex) → hugo
- AGT (.agx, .d$$) → agility
- JACL (.jacl, .j2) → jacl
- Level 9 (.l9, .sna) → level9
- Magnetic Scrolls (.mag) → magnetic
- Alan 2 (.acd) → alan2
- Alan 3 (.a3c) → alan3
- Adrift (.taf) → scare
- Scott Adams (.saga) → scott
- Plus (.plus) → plus
- TaylorMade (.tay) → taylor
- AdvSys → advsys

## Usage

```bash
# Run a game (auto-detects format)
./glkcli mygame.z5

# Show detected format without running
./glkcli --format mygame.z5

# Verbose output
./glkcli --verbose adventure.ulx

# Show help
./glkcli --help
```

## Building

```bash
cargo build --release
```

The binary will be created at `target/release/glkcli`.

### Build Features

The following compile-time features can be enabled or disabled:

- **`network-check`** (enabled by default): Enables D-Bus based network connectivity checking via IWD or NetworkManager. Disable this for embedded systems without D-Bus support:

```bash
# Build without network checking
cargo build --release --no-default-features

# Or explicitly disable it
cargo build --release --features ""
```

When `network-check` is disabled, the app assumes network is always available.

### Runtime Options

- **`--assume-online`**: Skip network connectivity checks and assume online (useful if D-Bus checks are unreliable on your system)
- **`--debug`**: Enable debug logging to `~/.glkcli/glkcli.log`

## TUI Browser

Launch without a game file to enter the interactive TUI browser:

```bash
./glkcli
```

Features:
- Browse and search the IFDB (Interactive Fiction Database)
- Download games directly to `~/.glkcli/games/`
- Launch downloaded games
- Automatic ZIP extraction and IF file detection
- Network connectivity detection (hides online features when offline)
- Tab navigation between Browse, My Games, and Save Files

### Network Connectivity

The TUI automatically detects network connectivity using:
1. IWD via D-Bus (for systems using iwd)
2. NetworkManager via D-Bus (for systems using NetworkManager)

When offline:
- Browse tab is automatically hidden
- Search and download features are disabled
- Press 'r' to recheck connectivity
