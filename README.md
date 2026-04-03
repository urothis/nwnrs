# `nwn_rs_types`

Rust workspace for reading, writing, inspecting, and composing Neverwinter Nights resource data.

This repository is organized as a layered toolkit:

- low-level binary and text codecs for NWN file formats such as `GFF`, `2DA`, `TLK`, `SSF`, `ERF`, `KEY/BIF`, and `NWSync`
- resource identity, type, checksum, encoding, and stream utilities
- container adapters that expose archives, directories, single files, in-memory buffers, and NWSync manifests through a shared resource-manager abstraction
- a high-level game crate for installation discovery and default resource loading
- a CLI for inspection, packing, unpacking, and selected NWSync workflows

## What This Workspace Does

The codebase is designed around one practical question: given an NWN installation or archive, how do you identify resources, open them reliably, and transform them into forms that are easier to inspect or rebuild?

At a high level:

- `nwn-cli` exposes the main workflows from the terminal
- `nwn-resref`, `nwn-restype`, and `nwn-core` define the shared identity vocabulary
- `nwn-resman` defines a common `Res`/`ResContainer` model and a layered `ResMan`
- container crates such as `nwn-erf`, `nwn-key`, `nwn-resdir`, `nwn-resfile`, `nwn-resmemfile`, and `nwn-resnwsync` project different storage backends into that shared model
- format crates such as `nwn-gff`, `nwn-gffjson`, `nwn-twoda`, `nwn-tlk`, `nwn-ssf`, and `nwn-nwsync` provide typed parsers and writers
- `nwn-game` composes those pieces into a default game-facing resource-loading stack

## Usage

Since this workspace is not published to [crates.io](https://crates.io), you can depend on individual crates using Git dependencies in your `Cargo.toml`:

```toml
[dependencies]
# You more than likely only need the prelude
nwn-prelude = { git = "https://github.com/urothis/nwn1ee.types.rs", rev = "main" }

# But if you wanna get fancy

# Core types and utilities
nwn-core = { git = "https://github.com/urothis/nwn1ee.types.rs", rev = "main" }
nwn-resref = { git = "https://github.com/urothis/nwn1ee.types.rs", rev = "main" }
nwn-restype = { git = "https://github.com/urothis/nwn1ee.types.rs", rev = "main" }

# Resource management
nwn-resman = { git = "https://github.com/urothis/nwn1ee.types.rs", rev = "main" }

# Format codecs
nwn-gff = { git = "https://github.com/urothis/nwn1ee.types.rs", rev = "main" }
nwn-twoda = { git = "https://github.com/urothis/nwn1ee.types.rs", rev = "main" }
nwn-tlk = { git = "https://github.com/urothis/nwn1ee.types.rs", rev = "main" }

# Container formats
nwn-erf = { git = "https://github.com/urothis/nwn1ee.types.rs", rev = "main" }
nwn-key = { git = "https://github.com/urothis/nwn1ee.types.rs", rev = "main" }

# Game integration
nwn-game = { git = "https://github.com/urothis/nwn1ee.types.rs", rev = "main" }
```

### CLI Usage

For command-line usage, you can install the CLI directly from the repository:

```bash
cargo install --git https://github.com/urothis/nwn1ee.types.rs --bin nwn-cli
```

Or run it directly:

```bash
cargo run --git https://github.com/urothis/nwn1ee.types.rs --bin nwn-cli -- --help
```

## Supported Workflows

The workspace already supports:

- inspecting ERF, KEY, GFF, 2DA, TLK, and SSF files from the CLI
- unpacking ERF archives and KEY/BIF sets into directory form
- converting GFF resources into JSON and packing them back into binary form
- writing 2DA text back into binary-compatible output
- opening NWSync repositories and printing manifest contents
- building a layered `ResMan` from game roots, override directories, ERFs, and NWSync manifests

## Sources

Repository structure and dependency graph:

- [`Cargo.toml`](./Cargo.toml)
- [CLI](./cli/README.md)
- [Crates](./crates/README.md)

## Contributing

### Development Tools

This repository uses several development tools with custom configurations:

- **Clippy**: Configured in [`clippy.toml`](clippy.toml) with strict linting rules including `forbid(unsafe_code)`
- **rustfmt**: Configured in [`rustfmt.toml`](rustfmt.toml) for consistent code formatting
- **cargo-deny**: Configured in [`deny.toml`](deny.toml) for dependency auditing and license checking

### Pull Request Labels

This repository uses automatic labeling for pull requests based on the files changed. Labels are applied automatically when you create or update a pull request.

Available labels include:

- **Component labels**: `cli`, `checksums`, `compressedbuf`, `core`, `erf`, `exo`, `game`, `gff`, `gffjson`, `key`, `lru`, `masterlist`, `nwsync`, `resdir`, `resfile`, `resman`, `resmemfile`, `resnwsync`, `resref`, `restype`, `ssf`, `streamext`, `tlk`, `twoda`, `util`
- **Category labels**: `documentation`, `ci/cd`, `dependencies`, `tests`, `config`, `breaking`

The labeling configuration is defined in [`.github/labeler.yml`](.github/labeler.yml) and is applied automatically by the [PR Validation workflow](.github/workflows/pr-validation.yml).
