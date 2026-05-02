# nwnrs

![nwnrs logo](assets/logo/icon.svg)

[![License](https://img.shields.io/badge/license-GPL--3.0--only-blue.svg)](https://github.com/urothis/nwnrs#license)
[![Crates.io](https://img.shields.io/crates/v/nwnrs.svg)](https://crates.io/crates/nwnrs)
[![Downloads](https://img.shields.io/crates/d/nwnrs.svg)](https://crates.io/crates/nwnrs)
[![Docs](https://docs.rs/nwnrs/badge.svg)](https://docs.rs/nwnrs/latest/nwnrs/)
[![CI](https://github.com/urothis/nwnrs/workflows/CI/badge.svg)](https://github.com/urothis/nwnrs/actions)
[![Discord](https://img.shields.io/discord/721439329079263235.svg?label=&logo=discord&logoColor=ffffff&color=7389D8&labelColor=6A7EC2)](https://discord.gg/GGneSqUYHU)

Rust tools and libraries for Neverwinter Nights

## Why This Exists

`nwnrs` is a workspace for reading, writing, inspecting, and converting NWN
data.

It includes:

- Rust crates for formats like `GFF`, `2DA`, `TLK`, `SSF`, `ERF`, `KEY/BIF`,
  `MDL`, `TGA`, `DDS`, `PLT`, and `NWSync`
- a CLI for operational workflows such as inspection, NWScript packing/compilation, conversion,
  packing, and unpacking

## Start Here

The canonical guided documentation now lives in the `nwnrs-types` crate docs:

- [`nwnrs-types` rustdoc](https://docs.rs/nwnrs-types/latest/nwnrs_types/)
- [`crates/types/README.md`](./crates/types/README.md)

Operational and interface-specific docs live here:

- [`crates/nwnrs/README.md`](./crates/nwnrs/README.md)

## Quick Start

### CLI

Install:

```bash
cargo install --git https://github.com/urothis/nwnrs --bin nwnrs
```

Use:

```bash
nwnrs new --kind utc my_creature
nwnrs inspect path/to/file.utc
nwnrs pack --debug path/to/script.nss out/script.ncs
nwnrs convert input.png output.tga
nwnrs convert path/to/model.mdl out/model_ascii.mdl
nwnrs convert out/model_ascii.mdl rebuilt/model.mdl
nwnrs convert path/to/model.mdl out/model.obj
nwnrs unpack path/to/module.mod -d out/
nwnrs pack out/ rebuilt.mod
```

### Rust

```toml
[dependencies]
nwnrs-types = { git = "https://github.com/urothis/nwnrs" }
```

```rust
use nwnrs_types::{
    gff::{GffRoot, GffValue},
    twoda::TwoDa,
};

let mut root = GffRoot::new("UTC ");
root.put_value("Tag", GffValue::CExoString("nw_chicken".to_string()))?;

let mut table = TwoDa::new();
table.set_columns(vec!["Label".to_string()])?;

# let _ = (root, table);
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Main Parts

- [`nwnrs-types`](./crates/types/README.md): umbrella crate and guided entry
  point
- [`nwnrs-types::resman`](./crates/types/src/resman/README.md): shared resource
  lookup
- [`nwnrs-types::install`](./crates/types/src/install/README.md): install
  discovery and conventional layered resource assembly
- [`nwnrs`](./crates/nwnrs/README.md): command-line inspection, conversion,
  packing, and unpacking workflows
- [`nwnrs-nwscript`](./crates/nwscript/README.md): NWScript frontend
  and compiler
- [`nwnrs-types::mdl`](./crates/types/src/mdl/README.md): MDL parsing,
  lowering, composition, and export

## Development

Install Rust with [rustup](https://rustup.rs/).

This workspace pins its compiler toolchain through
[`rust-toolchain.toml`](./rust-toolchain.toml), so a normal `cargo` invocation
will automatically use the expected nightly once it is installed.

From the repository root, the main validation commands are:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

## License

[`GPL-3.0-only`](./LICENSE)
