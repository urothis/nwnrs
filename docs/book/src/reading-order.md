# Reading Order

If you are new to the workspace, start here:

1. [Foundation](./foundation.md)
2. [Resource Identity and Resolution](./resources.md)
3. [Formats](./formats.md)
4. [Language and Compiler](./language.md)
5. [Interfaces](./interfaces.md)

That is the dependency order.

If you only care about one problem area:

- format parsing and writing: jump to [Formats](./formats.md)
- install-backed resource loading: jump to [Resource Identity and Resolution](./resources.md)
- NWScript compilation: jump to [Language and Compiler](./language.md)
- the public umbrella API: jump to [`nwnrs`](./interfaces-prelude.md)
- the operational entry points: jump to [`nwnrs-cli`](./interfaces-cli.md) and [`nwnrs-wasm`](./interfaces-wasm.md)

One rule matters throughout this codebase: different layers promise different fidelity. Some layers preserve binary or textual structure. Some normalize. Some compose. Some export. If two types look similar but live in different crates, assume the difference is intentional.
