# `nwn-rs`

Rust workspace for reading, writing, inspecting, and composing Neverwinter Nights resource data.

This repository is organized as a layered toolkit:

- low-level binary and text codecs for NWN file formats such as `GFF`, `2DA`, `TLK`, `SSF`, `MDL`, `TGA`, NWN `DDS`, typed palette texture payloads (`PLT`), `ERF`, `KEY/BIF`, and `NWSync`
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
- format crates such as `nwnrs-gff`, `nwnrs-twoda`, `nwnrs-tlk`, `nwnrs-ssf`, `nwnrs-mdl`, `nwnrs-tga`, `nwnrs-dds`, `nwnrs-plt`, and `nwnrs-nwsync` provide typed parsers and writers, with the texture crates now split cleanly by on-disk format
- `nwnrs-nwscript` provides the NWScript frontend and compiler pipeline: source loading, preprocessing, lexing, parsing, semantic analysis, optimization, and `NCS`/`NDB` emission
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

Convert images between supported texture formats:

```bash
cargo run -p nwnrs-cli -- convert input.png output.tga
cargo run -p nwnrs-cli -- convert input.jpg output.dds --dds-format dxt1
cargo run -p nwnrs-cli -- convert ashlw_066.dds output.webp
```

Inspect the dedicated texture formats directly:

```bash
cargo run -p nwnrs-cli -- inspect amp01_g06.tga
cargo run -p nwnrs-cli -- inspect ashlw_066.dds
cargo run -p nwnrs-cli -- inspect cloak_001.plt
```

## Supported Workflows

The workspace supports:

- inspecting ERF, KEY, GFF, 2DA, TLK, SSF, MDL, and texture files
- parsing, decoding, writing, and RGBA-encoding NWN `tga` textures through `nwnrs-tga`
- parsing, decoding, writing, and RGBA-encoding NWN `dds` textures through `nwnrs-dds`
- parsing and writing typed `plt` palette textures through `nwnrs-plt`, including explicit per-pixel `value` and `layer_id` data plus known layer mappings
- converting image inputs (`png`, `jpg`, `tga`, `dds`) into `tga`, NWN `dds`, or `webp`
- compiling NWScript `.nss` files to `.ncs` and optional `.ndb`
- parsing and semantically analyzing NWScript source through the `nwnrs-nwscript` crate
- unpacking ERF archives and KEY/BIF sets into directory form
- opening NWSync repositories and printing manifest contents
- building a layered `ResMan` from game roots, override directories, ERFs, and NWSync manifests
- treating `plt` as typed file ownership only for now; final color rendering and game-accurate material mapping are still future work

## Contributing

### Development Tools

This repository uses several development tools with custom configurations:

- **Clippy**: Configured in [`clippy.toml`](clippy.toml) with strict linting rules including
- **rustfmt**: Configured in [`rustfmt.toml`](rustfmt.toml) for consistent code formatting
- **cargo-deny**: Configured in [`deny.toml`](deny.toml) for dependency auditing and license checking
