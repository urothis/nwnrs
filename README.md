<h1 align="center">
  <img src="assets/logo/icon.svg" width="150" alt="nwnrs logo"/><br>
  nwnrs
</h1>
<div align="center">
  Rust tools and libraries for Neverwinter Nights files
</div>

## What Is This?

`nwnrs` is a workspace for reading, writing, inspecting, and converting NWN data.

It includes:

- Rust crates for formats like `GFF`, `2DA`, `TLK`, `SSF`, `ERF`, `KEY/BIF`, `MDL`, `TGA`, `DDS`, `PLT`, and `NWSync`
- a CLI for common workflows
- wasm bindings for browser and JS apps

If you just want one dependency, start with [`nwnrs`](./crates/meta/prelude/README.md).

## Quick Start


### CLI

Install:

```bash
cargo install --git https://github.com/urothis/nwnrs --bin nwnrs-cli
```

Use:

```bash
# inspect a file
nwnrs-cli inspect path/to/file.utc

# compile NWScript
nwnrs-cli compile --debug path/to/script.nss

# convert textures
nwnrs-cli convert input.png output.tga

# convert MDL between compiled and canonical ascii
nwnrs-cli convert path/to/model.mdl out/model_ascii.mdl
nwnrs-cli convert out/model_ascii.mdl rebuilt/model.mdl

# unpack and repack archives
nwnrs-cli unpack path/to/module.mod -d out/
nwnrs-cli pack out/ rebuilt.mod
```

More CLI details: [`cli/README.md`](./cli/README.md)

### Rust

```toml
[dependencies]
nwnrs = { git = "https://github.com/urothis/nwnrs" }
```

```rust
use nwnrs::{
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

### WebAssembly

```bash
wasm-pack build wasm --target bundler --out-dir pkg
```

The wasm package exposes helpers like:

- `read_gff_from_bytes`
- `write_gff_to_bytes`
- `read_twoda_from_bytes`
- `write_twoda_to_bytes`
- `read_mdl_from_bytes`
- `write_mdl_to_bytes`

More wasm details: [`wasm/README.md`](./wasm/README.md)

## Main Parts

- [`nwnrs`](./crates/meta/prelude/README.md): the simple umbrella crate
- [`nwnrs-resman`](./crates/resources/resman/README.md): shared resource loading
- [`nwnrs-install`](./crates/resources/install/README.md): find and open game installs
- [`nwnrs-nwscript`](./crates/language/nwscript/README.md): NWScript frontend and compiler
- [`nwnrs-mdl`](./crates/formats/mdl/README.md): MDL parsing and lowering

## Supported Work

- inspect NWN files
- parse and write common NWN formats
- compile NWScript to `NCS` and `NDB`
- convert textures between `png`, `jpg`, `tga`, `dds`, and `webp`
- load resources from installs, directories, archives, and manifests
- lower compiled MDL into canonical ASCII

## Important Note About MDL

Compiled `MDL` can be lowered to canonical ASCII today.

Rebuilding compiled `MDL` currently works for canonical ASCII produced by the library and CLI. It is not a general-purpose arbitrary ASCII-to-compiled compiler yet.

## Development

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
```

## License

[`GPL-3.0-only`](./LICENSE)
