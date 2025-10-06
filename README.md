# Rust GLK CLI Launcher

A memory-safe Rust launcher for glkterm based interactive fiction interpreters.

## Features

- Automatic detection of game file formats by header and extension

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
