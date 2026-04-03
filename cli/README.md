# CLI

The command-line interface for inspecting, packing, unpacking, and managing NWN resources.

## Quick Start

Build or run the CLI from the workspace root:

```bash
cargo run -p nwn-cli -- inspect path/to/module.mod
cargo run -p nwn-cli -- unpack path/to/module.mod -d out/
cargo run -p nwn-cli -- pack out/ rebuilt.mod
cargo run -p nwn-cli -- nwsync print path/to/repository --manifest <sha1>
```

Useful patterns:

- unpack a GFF-family resource to JSON, edit it, then pack it back
- unpack a KEY/BIF set, preserve `resource.json`, and repack without losing archive ordering
- open a game install with `nwn-game`, then query resources through `nwn-resman`

## CLI Behavior and Supported Commands

- [`main.rs`](./src/main.rs)
- [`args.rs`](./src/args.rs)
- [`inspect.rs`](./src/inspect.rs)
- [`pack.rs`](./src/pack.rs)
- [`unpack.rs`](./src/unpack.rs)
- [`nwsync.rs`](./src/nwsync.rs)