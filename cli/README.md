# CLI

The command-line interface for inspecting, packing, unpacking, and managing NWN resources.

## Quick Start

Install from crates.io:

```bash
cargo install nwnrs-cli
```

Build or run the CLI from the workspace root:

```bash
cargo run -p nwnrs-cli -- compile --debug path/to/script.nss
cargo run -p nwnrs-cli -- inspect path/to/module.mod
cargo run -p nwnrs-cli -- unpack path/to/module.mod -d out/
cargo run -p nwnrs-cli -- pack out/ rebuilt.mod
cargo run -p nwnrs-cli -- nwsync print path/to/repository --manifest <sha1>
```

Useful patterns:

- compile `.nss` to `.ncs` using a sibling `nwscript.nss`, or override it with `--langspec`
- unpack a KEY/BIF set, preserve `resource.json`, and repack without losing archive ordering
- open an install with `nwnrs-install`, then query resources through `nwnrs-resman`

## CLI Behavior and Supported Commands

- [`main.rs`](./src/main.rs)
- [`args.rs`](./src/args.rs)
- [`inspect.rs`](./src/inspect.rs)
- [`compile.rs`](./src/compile.rs)
- [`pack.rs`](./src/pack.rs)
- [`unpack.rs`](./src/unpack.rs)
- [`nwsync.rs`](./src/nwsync.rs)
