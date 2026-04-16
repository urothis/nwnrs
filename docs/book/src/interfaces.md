# Interfaces

These are the layers most consumers touch directly.

- [`nwnrs`](./interfaces-prelude.md) is the umbrella crate
- [`nwnrs-cli`](./interfaces-cli.md) is the operational command-line interface
- [`nwnrs-wasm`](./interfaces-wasm.md) is the browser and JavaScript boundary

Each of these should be read after the lower layers, because they are meant to sit on top of the domain logic rather than replace it.
