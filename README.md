# `nwn-rs`

Rust workspace for reading, writing, inspecting, and composing Neverwinter Nights resource data.

This repository is organized as a layered toolkit:

- low-level binary and text codecs for NWN file formats such as `GFF`, `2DA`, `TLK`, `SSF`, `MDL`, `TGA`, NWN `DDS`, typed palette texture payloads (`PLT`), `ERF`, `KEY/BIF`, and `NWSync`
- resource identity, type, checksum, encoding, and stream utilities
- container adapters that expose archives, directories, single files, in-memory buffers, and NWSync manifests through a shared resource-manager abstraction
- a high-level install crate for installation discovery and default resource loading
- a root `nwnrs-bevy` crate for loading static NWN `mdl` assets into Bevy `0.18.1`
- a CLI for inspection, packing, unpacking, selected NWSync workflows, and NWScript compilation

## What This Workspace Does

The codebase is designed around one practical question: given an NWN installation or archive, how do you identify resources, open them reliably, and transform them into forms that are easier to inspect or rebuild?

At a high level:

- `nwnrs-cli` exposes the current main workflows
- `nwnrs-resref`, `nwnrs-restype`, and `nwnrs-localization` define the shared identity vocabulary
- `nwnrs-resman` defines a common `Res`/`ResContainer` model and a layered `ResMan`
- container crates such as `nwnrs-erf`, `nwnrs-key`, `nwnrs-resdir`, `nwnrs-resfile`, `nwnrs-resmemfile`, and `nwnrs-resnwsync` project different storage backends into that shared model
- format crates such as `nwnrs-gff`, `nwnrs-twoda`, `nwnrs-tlk`, `nwnrs-ssf`, `nwnrs-mdl`, `nwnrs-tga`, `nwnrs-dds`, `nwnrs-plt`, and `nwnrs-nwsync` provide typed parsers and writers, with the texture crates now split cleanly by on-disk format
- `nwnrs-nwscript` provides the NWScript frontend and compiler pipeline: source loading, preprocessing, lexing, parsing, semantic analysis, optimization, and `NCS`/`NDB` emission
- `nwnrs-install` composes those pieces into a default install-facing resource-loading stack
- `nwnrs-bevy` is the first Bevy-facing integration layer, currently scoped to static `mdl` loading plus NWN `dds`/`tga` texture decode for Bevy `Image` assets

## Crate Map

The publishable crate tree is grouped by responsibility.

### Foundation

- [`nwnrs-checksums`](./crates/foundation/checksums/README.md)
- [`nwnrs-encoding`](./crates/foundation/encoding/README.md)
- [`nwnrs-io`](./crates/foundation/io/README.md)
- [`nwnrs-localization`](./crates/foundation/localization/README.md)
- [`nwnrs-lru`](./crates/foundation/lru/README.md)
- [`nwnrs-streamext`](./crates/foundation/streamext/README.md)

### Formats

- [`nwnrs-compressedbuf`](./crates/formats/compressedbuf/README.md)
- [`nwnrs-dds`](./crates/formats/dds/README.md)
- [`nwnrs-erf`](./crates/formats/erf/README.md)
- [`nwnrs-exo`](./crates/formats/exo/README.md)
- [`nwnrs-gff`](./crates/formats/gff/README.md)
- [`nwnrs-git`](./crates/formats/git/README.md)
- [`nwnrs-key`](./crates/formats/key/README.md)
- [`nwnrs-mdl`](./crates/formats/mdl/README.md)
- [`nwnrs-mtr`](./crates/formats/mtr/README.md)
- [`nwnrs-nwsync`](./crates/formats/nwsync/README.md)
- [`nwnrs-plt`](./crates/formats/plt/README.md)
- [`nwnrs-set`](./crates/formats/set/README.md)
- [`nwnrs-ssf`](./crates/formats/ssf/README.md)
- [`nwnrs-tga`](./crates/formats/tga/README.md)
- [`nwnrs-tlk`](./crates/formats/tlk/README.md)
- [`nwnrs-twoda`](./crates/formats/twoda/README.md)
- [`nwnrs-txi`](./crates/formats/txi/README.md)

### Resources

- [`nwnrs-install`](./crates/resources/install/README.md)
- [`nwnrs-resdir`](./crates/resources/resdir/README.md)
- [`nwnrs-resfile`](./crates/resources/resfile/README.md)
- [`nwnrs-resman`](./crates/resources/resman/README.md)
- [`nwnrs-resmemfile`](./crates/resources/resmemfile/README.md)
- [`nwnrs-resnwsync`](./crates/resources/resnwsync/README.md)
- [`nwnrs-resref`](./crates/resources/resref/README.md)
- [`nwnrs-restype`](./crates/resources/restype/README.md)

### Language

- [`nwnrs-nwscript`](./crates/language/nwscript/README.md)

### Meta

- [`nwnrs`](./crates/meta/prelude/README.md)
- [`nwnrs-masterlist`](./crates/meta/masterlist/README.md)

## Choosing a Crate

- Use [`nwnrs`](./crates/meta/prelude/README.md) if you want one umbrella dependency with stable root modules such as `nwnrs::gff` and `nwnrs::resman`.
- Use [`nwnrs-resman`](./crates/resources/resman/README.md) if you are composing resources from directories, archives, and manifests behind one retrieval model.
- Use [`nwnrs-install`](./crates/resources/install/README.md) if you want install discovery and a default Neverwinter Nights resource-loading stack.
- Use the format crates directly when you only need a codec, for example [`nwnrs-gff`](./crates/formats/gff/README.md), [`nwnrs-twoda`](./crates/formats/twoda/README.md), or [`nwnrs-tlk`](./crates/formats/tlk/README.md).

## Usage

Since this workspace is not published to [crates.io](https://crates.io), you can depend on individual crates using Git dependencies in your `Cargo.toml`:

```toml
# You more than likely only need the prelude
[dependencies]
nwnrs = { git = "https://github.com/urothis/nwn-rs" }
```

```rust
use nwnrs::{gff, localization};

let language = localization::resolve_language("en")?;
let root = gff::GffRoot::new("UTC ");
# let _ = (language, root);
# Ok::<(), Box<dyn std::error::Error>>(())
```

Or depend on individual crates directly:

```toml
# Core types and utilities
nwnrs-localization = { git = "https://github.com/urothis/nwn-rs" }
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

# Install integration
nwnrs-install = { git = "https://github.com/urothis/nwn-rs" }
```

### Pinning Git Dependencies

If you want reproducible builds, pin the repository explicitly instead of tracking the moving default branch.

Pin to a commit:

```toml
[dependencies]
nwnrs = { git = "https://github.com/urothis/nwn-rs", rev = "<commit-sha>" }
```

Pin to a tag:

```toml
[dependencies]
nwnrs = { git = "https://github.com/urothis/nwn-rs", tag = "<tag>" }
```

Track a branch deliberately:

```toml
[dependencies]
nwnrs = { git = "https://github.com/urothis/nwn-rs", branch = "main" }
```

The same pattern works for individual crates such as `nwnrs-gff`, `nwnrs-resman`, or `nwnrs-install`.

### Updating a Pinned Dependency

There are two sane workflows:

1. Change the `rev`, `tag`, or `branch` in `Cargo.toml`, then run:

```bash
cargo update -p nwnrs
```

2. If you already know the exact commit you want, update the lockfile directly:

```bash
cargo update -p nwnrs --precise <commit-sha>
```

If you depend on individual crates instead of `nwnrs`, replace `nwnrs` in the command with the specific package name, for example:

```bash
cargo update -p nwnrs-gff
```

### CLI Usage

For command-line usage, you can install the CLI directly from the repository:

```bash
cargo install --git https://github.com/urothis/nwn-rs --bin nwnrs-cli
```

You can pin the install the same way:

```bash
cargo install --git https://github.com/urothis/nwn-rs --rev <commit-sha> --bin nwnrs-cli
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
- loading static ASCII `mdl` models into Bevy `0.18.1` meshes, materials, and images through `nwnrs-bevy`
- compiling NWScript `.nss` files to `.ncs` and optional `.ndb`
- parsing and semantically analyzing NWScript source through the `nwnrs-nwscript` crate
- unpacking ERF archives and KEY/BIF sets into directory form
- opening NWSync repositories and printing manifest contents
- building a layered `ResMan` from game roots, override directories, ERFs, and NWSync manifests
- treating `plt` as typed file ownership only for now; final color rendering and game-accurate material mapping are still future work
- keeping Bevy phase 1 intentionally narrow: static meshes plus NWN `dds`/`tga` textures only, with animation, skinning, and `plt` rendering deferred

## Contributing

### Development Tools

This repository uses several development tools with custom configurations:

- **Clippy**: Configured in [`clippy.toml`](clippy.toml) with strict linting rules including
- **rustfmt**: Configured in [`rustfmt.toml`](rustfmt.toml) for consistent code formatting
- **cargo-deny**: Configured in [`deny.toml`](deny.toml) for dependency auditing and license checking
