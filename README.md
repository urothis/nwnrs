# `nwn-rs`

Rust workspace for reading, writing, inspecting, and composing Neverwinter Nights resource data.

This repository is organized as a layered toolkit:

- low-level binary and text codecs for NWN file formats such as `GFF`, `2DA`, `TLK`, `SSF`, `ERF`, `KEY/BIF`, and `NWSync`
- resource identity, type, checksum, encoding, and stream utilities
- container adapters that expose archives, directories, single files, in-memory buffers, and NWSync manifests through a shared resource-manager abstraction
- a high-level game crate for installation discovery and default resource loading
- a CLI for inspection, packing, unpacking, selected NWSync workflows, and NWScript compilation

## What This Workspace Does

The codebase is designed around one practical question: given an NWN installation or archive, how do you identify resources, open them reliably, and transform them into forms that are easier to inspect or rebuild?

At a high level:

- `nwnrs-cli` exposes the current main workflows
- `nwnrs-resref`, `nwnrs-restype`, and `nwnrs-core` define the shared identity vocabulary
- `nwnrs-resman` defines a common `Res`/`ResContainer` model and a layered `ResMan`
- container crates such as `nwnrs-erf`, `nwnrs-key`, `nwnrs-resdir`, `nwnrs-resfile`, `nwnrs-resmemfile`, and `nwnrs-resnwsync` project different storage backends into that shared model
- format crates such as `nwnrs-gff`, `nwnrs-twoda`, `nwnrs-tlk`, `nwnrs-ssf`, and `nwnrs-nwsync` provide typed parsers and writers
- `nwnrs-game` composes those pieces into a default game-facing resource-loading stack

## Usage

Since this workspace is not published to [crates.io](https://crates.io), you can depend on individual crates using Git dependencies in your `Cargo.toml`:

```toml
# You more than likely only need the prelude
[dependencies]
nwnrs = { git = "https://github.com/urothis/nwn-rs" }
```

```rust
use nwnrs::prelude::*;
```

Or depend on individual crates directly:

```toml
# Core types and utilities
nwnrs-core = { git = "https://github.com/urothis/nwn-rs" }
nwnrs-resref = { git = "https://github.com/urothis/nwn-rs" }
nwnrs-restype = { git = "https://github.com/urothis/nwn-rs" }

# Resource management
nwnrs-resman = { git = "https://github.com/urothis/nwn-rs" }

# Format codecs
nwnrs-gff = { git = "https://github.com/urothis/nwn-rs" }
nwnrs-twoda = { git = "https://github.com/urothis/nwn-rs" }
nwnrs-tlk = { git = "https://github.com/urothis/nwn-rs" }

# Container formats
nwnrs-erf = { git = "https://github.com/urothis/nwn-rs" }
nwnrs-key = { git = "https://github.com/urothis/nwn-rs" }

# Game integration
nwnrs-game = { git = "https://github.com/urothis/nwn-rs" }
```

### CLI Usage

For command-line usage, you can install the CLI directly from the repository:

```bash
cargo install --git https://github.com/urothis/nwn-rs --bin nwnrs-cli
```

Or run it directly:

```bash
cargo run --git https://github.com/urothis/nwn-rs --bin nwnrs-cli -- --help
```

Compile one NWScript source file:

```bash
cargo run -p nwnrs-cli -- compile --debug path/to/script.nss
```

## Supported Workflows

The workspace supports:

- inspecting ERF, KEY, GFF, 2DA, TLK, and SSF files
- compiling NWScript `.nss` files to `.ncs` and optional `.ndb`
- unpacking ERF archives and KEY/BIF sets into directory form
- opening NWSync repositories and printing manifest contents
- building a layered `ResMan` from game roots, override directories, ERFs, and NWSync manifests

## Contributing

### Development Tools

This repository uses several development tools with custom configurations:

- **Clippy**: Configured in [`clippy.toml`](clippy.toml) with strict linting rules including
- **rustfmt**: Configured in [`rustfmt.toml`](rustfmt.toml) for consistent code formatting
- **cargo-deny**: Configured in [`deny.toml`](deny.toml) for dependency auditing and license checking
