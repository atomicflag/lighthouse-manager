# lighthouse-manager

Control SteamVR Lighthouse base stations (V1 & V2) via Bluetooth LE from the command line.

[![Rust](https://github.com/atomicflag/lighthouse-manager/actions/workflows/rust.yml/badge.svg)](https://github.com/atomicflag/lighthouse-manager/actions/workflows/rust.yml)
[![Crates.io](https://img.shields.io/crates/v/lighthouse-manager.svg)](https://crates.io/crates/lighthouse-manager)
[![Docs.rs](https://docs.rs/lighthouse-manager/badge.svg)](https://docs.rs/lighthouse-manager)
[![Minimum Rust Version: 1.85](https://img.shields.io/badge/Rust-1.85+-orange.svg)](https://www.rust-lang.org/tools/install)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](#license)

---

## Overview

**lighthouse-manager** is a Rust CLI tool that lets you discover, power on/off, and identify SteamVR Lighthouse base stations wirelessly via Bluetooth Low Energy. It scans for nearby devices using BLE advertising data, stores them in a local JSON database, and communicates with both **Lighthouse V1 (HTC BS-*)** and **V2 (LHB-*)** stations.

### Features

- 🔍 **Discover** — Scan the air for nearby Lighthouse base stations via BLE
- 📋 **List** — View known lighthouses (filter by managed status, JSON output)
- ⚡ **Power On/Off** — Turn all managed lighthouses on or off in parallel
- 💡 **Identify (blink)** — Make a V2 Lighthouse flash its LED by index
- 💾 **Persistent storage** — Database backed by a local JSON file (via XDG dirs)
- 🌳 **Structured logging** — `tracing` with verbosity levels (`-v`, `-vv`) and `RUST_LOG` support
- ✅ **Cross-platform** — Linux, macOS, and Windows (requires BLE adapter)

---

## Installation

### From crates.io (recommended)

```bash
cargo install lighthouse-manager
```

Then run `lighthouse-manager --help` to verify the installation.

### Prebuilt binaries

Download the latest release binary for your platform from [GitHub Releases](https://github.com/atomicflag/lighthouse-manager/releases).

### From source

```bash
git clone https://github.com/atomicflag/lighthouse-manager.git
cd lighthouse-manager
cargo build --release
./target/release/lighthouse-manager --help
```

---

## Usage

All commands require a working Bluetooth adapter. Run with `-v` / `-vv` for debug/trace logs, or set the `RUST_LOG` environment variable directly.

### Discover nearby Lighthouses

Scans for BLE-advertising Lighthouse base stations and saves them to the local database (newly discovered units are marked *unmanaged* by default):

```bash
# Default 10-second scan
lighthouse-manager discover

# Custom scan duration
lighthouse-manager discover -d 20
```

### List known Lighthouses

```bash
# Show all lighthouses in the database
lighthouse-manager list

# Show only managed lighthouses
lighthouse-manager list --managed

# Output as pretty JSON
lighthouse-manager list --json
```

### Power On / Off

```bash
# Power on all managed lighthouses simultaneously
lighthouse-manager power-on

# Sleep (power off) all managed lighthouses
lighthouse-manager power-off
```

### Identify a Lighthouse (V2 only)

Causes a V2 base station to flash its LED so you can locate it physically:

```bash
# Flash the lighthouse at index 0 in the database
lighthouse-manager identify 0

# Alias: same as above
lighthouse-manager blink 0
```

### Logging

Control verbosity globally with `-v` / `-vv`, or use `RUST_LOG`:

```bash
# Equivalent to -v
RUST_LOG=lighthouse_manager=debug lighthouse-manager list

# Trace-level logging for debugging BLE issues
RUST_LOG=lighthouse_manager=trace lighthouse-manager discover
```

<!-- start: CLI Help -->

<!-- end: CLI Help -->

---

## Architecture

```
src/
├── main.rs              # CLI entry point (clap Parser + subcommand dispatch)
├── bluetooth.rs         # Bluetooth adapter management via btleplug
├── lighthouse.rs        # Lighthouse device model & version detection
├── protocol.rs          # BLE GATT communication protocol layer
├── storage.rs           # Local JSON database (XDG-compliant paths)
└── commands/
    ├── discover.rs      # BLE scan → save discovered units
    ├── list.rs          # Database listing with filtering
    ├── power.rs         # Parallel power on/off via GATT writes
    └── identify.rs      # V2 LED flash via GATT characteristic
```

---

## Building from Source

### Prerequisites

- Rust 1.85+ (`rustup` recommended)
- A Bluetooth 4.0+ adapter (Linux: `bluez`; macOS / Windows: native BLE stack)

### Compile

```bash
# Development build (fast iteration)
cargo build

# Release build (optimized, smaller binary)
cargo build --release
```

---

## Testing

Run the full test suite:

```bash
cargo test
```

---

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

1. Fork the repository
2. Create your feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request
