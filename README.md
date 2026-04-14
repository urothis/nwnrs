# nwnrs

![nwnrs logo](assets/logo/icon.svg)

Rust tools and libraries for Neverwinter Nights

## What Is This?

`nwnrs` is a workspace for reading, writing, inspecting, and converting NWN
data.

It includes:

- Rust crates for formats like `GFF`, `2DA`, `TLK`, `SSF`, `ERF`, `KEY/BIF`,
  `MDL`, `TGA`, `DDS`, `PLT`, and `NWSync`
- a CLI for operational workflows such as inspection, compilation, conversion,
  packing, and unpacking
- wasm bindings for browser and JavaScript applications

## Start Here

The canonical guided documentation now lives in the `nwnrs` umbrella crate
docs:

- [`nwnrs` rustdoc](https://docs.rs/nwnrs/latest/nwnrs/)
- [`crates/meta/prelude/README.md`](./crates/meta/prelude/README.md)

Operational and interface-specific docs live here:

- [`cli/README.md`](./cli/README.md)
- [`wasm/README.md`](./wasm/README.md)

## Quick Start

### CLI

Install:

```bash
cargo install --git https://github.com/urothis/nwn-rs --bin nwnrs-cli
```

Use:

```bash
nwnrs-cli inspect path/to/file.utc
nwnrs-cli compile --debug path/to/script.nss
nwnrs-cli convert input.png output.tga
nwnrs-cli convert path/to/model.mdl out/model_ascii.mdl
nwnrs-cli convert out/model_ascii.mdl rebuilt/model.mdl
nwnrs-cli convert path/to/model.mdl out/model.obj
nwnrs-cli unpack path/to/module.mod -d out/
nwnrs-cli pack out/ rebuilt.mod
```

### Rust

```toml
[dependencies]
nwnrs = { git = "https://github.com/urothis/nwn-rs" }
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

The wasm package exposes helpers such as `read_gff_from_bytes`,
`write_gff_to_bytes`, `read_twoda_from_bytes`, `write_twoda_to_bytes`,
`read_mdl_from_bytes`, and `write_mdl_to_bytes`.

## Main Parts

- [`nwnrs`](./crates/meta/prelude/README.md): umbrella crate and guided entry
  point
- [`nwnrs-resman`](./crates/resources/resman/README.md): shared resource lookup
- [`nwnrs-install`](./crates/resources/install/README.md): install discovery and
  conventional layered resource assembly
- [`nwnrs-nwscript`](./crates/language/nwscript/README.md): NWScript frontend
  and compiler
- [`nwnrs-mdl`](./crates/formats/mdl/README.md): MDL parsing, lowering,
  composition, and export

## Development

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
```

## License

[`GPL-3.0-only`](./LICENSE)
