# Rust GLK CLI Launcher

A memory-safe Rust rewrite of the glkcli launcher for glkterm.

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

## Installation

To replace the C version in your build:

```bash
# Build the Rust version
cd launcher_rust
cargo build --release

# Copy to build directory (adjust path as needed)
cp target/release/glkcli ../build/

# Or install system-wide
cargo install --path .
```

## Advantages over C Version

1. **Memory Safety**: No risk of double-free, use-after-free, or memory leaks
2. **Error Handling**: Robust error handling with context
3. **Maintainability**: Cleaner, more readable code
4. **Dependencies**: Minimal dependencies, statically linked
5. **Performance**: Should be comparable or faster than C version

## Testing

You can test the Rust version side-by-side with the C version:

```bash
# Test C version
./build/glkcli mygame.z5

# Test Rust version  
cd launcher_rust && cargo run -- ../mygame.z5
```

Both should behave identically, but the Rust version eliminates memory management bugs.
