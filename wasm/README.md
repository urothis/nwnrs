# nwnrs-wasm

`nwnrs-wasm` exposes a small WebAssembly boundary for Neverwinter Nights resource formats.

The model is intentionally simple:

1. Read raw file bytes into a plain JavaScript value.
2. Write that value back unchanged when you need exact byte preservation.
3. Apply edited writes only where the native Rust codec has provenance-backed editing.

In other words, the wasm crate is a thin ABI layer over the Rust format crates. Unchanged DTO roundtrips are byte-exact for every supported format, and edited DTO writes are only enabled where the native codec can preserve untouched representation details.

## Supported Formats

- `GFF` (`.gff`, `.bic`, `.dlg`, `.itp`, `.utc`, `.utd`, `.ute`, `.uti`, `.utm`, `.utp`, `.uts`, `.utt`, `.utw`)
- `2DA`
- `TLK`
- `SSF`
- `ERF`
- `MDL`

Each format has a pair of functions:

- `read_*_from_bytes(bytes) -> object`
- `write_*_to_bytes(value) -> Uint8Array`

The returned objects are DTOs serialized with `serde_wasm_bindgen`. In JavaScript terms, they behave like ordinary objects, arrays, strings, numbers, and byte arrays.

For every supported format, the read API also carries hidden provenance metadata in the top-level DTO. If you read bytes and write the DTO back unchanged, the original bytes are returned exactly.

Edited DTO writes are supported for `GFF`, `2DA`, `TLK`, `SSF`, `ERF`, and ASCII `MDL`. The wasm layer delegates preservation behavior to the native crates instead of maintaining a second set of format rules.

## Installation

## Building

This crate is not consumed directly with `cargo build`. The intended output is
the generated JavaScript package under `wasm/pkg`.

Prerequisites:

- the `wasm32-unknown-unknown` Rust target
- `wasm-pack`

Install them once:

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-pack
```

Build the package from the repository root:

```bash
wasm-pack build wasm --target bundler --out-dir pkg
```

That command writes the generated WebAssembly module, JavaScript loader, and
TypeScript declarations into [`wasm/pkg`](./pkg).

If you need a different JavaScript environment, change `--target` accordingly:

- `bundler` for Vite, Webpack, Rollup, and similar toolchains
- `web` for direct browser usage without a bundler
- `nodejs` for direct Node.js consumption

Rebuild `wasm/pkg` whenever the Rust bindings change.

If you are consuming the generated package directly:

```bash
npm install ./wasm/pkg
```

## Usage

```ts
import init, {
  read_gff_from_bytes,
  write_gff_to_bytes,
  read_mdl_from_bytes,
  write_mdl_to_bytes,
  read_twoda_from_bytes,
  write_twoda_to_bytes,
} from "nwnrs-wasm";

await init();

const gffBytes = await fetch("/fixture.utc").then((r) => r.arrayBuffer());
const gff = read_gff_from_bytes(new Uint8Array(gffBytes));

const encodedGff = write_gff_to_bytes(gff);

const mdlBytes = await fetch("/model.mdl").then((r) => r.arrayBuffer());
const mdl = read_mdl_from_bytes(new Uint8Array(mdlBytes));

const encodedMdl = write_mdl_to_bytes(mdl);

const twodaBytes = await fetch("/appearance.2da").then((r) => r.arrayBuffer());
const twoda = read_twoda_from_bytes(new Uint8Array(twodaBytes));

const encodedTwoDa = write_twoda_to_bytes(twoda);
```

## Format Notes

### GFF

GFF is exposed as a tree of structs and fields.

- A document is `{ file_type, file_version, root }`.
- A struct is `{ id, fields }`.
- A field is `{ label, value }`.
- A value is a tagged union: `{ kind, value }`.

This is deliberate. GFF fields are ordered, and the wasm DTO preserves that order instead of collapsing everything into a JavaScript object map.
Edited writes use the native GFF provenance-preserving merge workflow.

### 2DA

`2DA` is exposed as:

```ts
{
  default_value: string | null,
  columns: string[],
  rows: (string | null)[][],
  row_labels: string[]
}
```

The visible DTO is the semantic table plus row labels. The top-level DTO carries enough hidden provenance to preserve the exact original bytes when it is written back unchanged and to keep existing layout details stable on supported edits.

### TLK

TLK is exposed as:

```ts
{
  language_id: number,
  entries: ({
    text: string,
    raw_text?: Uint8Array,
    sound_res_ref: string,
    raw_sound_res_ref: Uint8Array,
    sound_length: number,
    sound_length_bits: number,
    flags: number,
    volume_variance: number,
    pitch_variance: number
  } | null)[]
}
```

The `entries` array is sparse by position. A `null` slot means that strref is absent.
Edited writes preserve untouched descriptor metadata through the native TLK provenance model.

### SSF

SSF is exposed as:

```ts
{
  entries: { raw_resref: Uint8Array, resref: string, strref: number }[]
}
```

Edited writes preserve untouched raw slot bytes through the native SSF provenance model.

### ERF

ERF is exposed as archive metadata plus ordered entries:

```ts
{
  file_type: string,
  file_version: "V1" | "E1",
  build_year: number,
  build_day: number,
  str_ref: number,
  oid: string | null,
  resource_list_padding: number,
  loc_strings: { id: number, text: string }[],
  entries: {
    filename: string,
    bytes: Uint8Array,
    compressed_buf_algorithm: "none" | "zlib" | "zstd" | null
  }[]
}
```

`read_erf_from_bytes(bytes, filename)` requires the source filename because ERF parsing uses it to resolve archive context. Edited writes preserve untouched archive layout metadata such as resource-list padding.

### MDL

MDL is exposed as:

```ts
{
  encoding: "ascii" | "compiled",
  text: string
}
```

ASCII source is returned as text directly. Compiled MDL is lowered to canonical ASCII text on read.

Unchanged writes preserve the exact original bytes for both encodings. Edited writes are supported when `encoding` is `"ascii"`. Edited writes for `"compiled"` are rejected for now instead of silently emitting lossy or incorrect rebuilt bytes.

## Error Model

All exported functions throw JavaScript errors represented as `Result<_, JsValue>` on the Rust side.

Practically, this means:

- malformed input bytes fail during `read_*_from_bytes`
- malformed DTO shapes fail during `write_*_to_bytes`
- edited DTOs for formats without wasm provenance support yet fail during `write_*_to_bytes`
- invalid enum-like values such as an unknown ERF compression algorithm fail with a descriptive message

## Design Intent

This crate does not try to make the Rust internals look like idiomatic handwritten TypeScript classes. It exposes stable data-transfer objects.

That tradeoff is intentional:

- the core crates remain transport-agnostic
- the wasm layer stays thin
- JavaScript gets a predictable `bytes <-> DTO` interface

If you need richer application semantics, build them on top of these DTOs instead of coupling them into the binary format layer.
