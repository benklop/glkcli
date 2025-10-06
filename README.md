# Rust GLK CLI Launcher

A memory-safe Rust launcher for glkterm bsased interactive fiction interpreters.

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
cargo run -- mygame.z5

# Show detected format without running
cargo run -- --format mygame.z5

# Verbose output
cargo run -- --verbose adventure.ulx

# Show help
cargo run -- --help
```

## Building

```bash
cd launcher_rust
cargo build --release
```

The binary will be created at `target/release/glkcli`.
