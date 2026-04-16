# WebAssembly Interface

Docs:

- crate: `nwnrs-wasm`
- [README](https://github.com/urothis/nwnrs/blob/main/wasm/README.md)
- [source](https://github.com/urothis/nwnrs/blob/main/wasm/src/lib.rs)

## Scope

`nwnrs-wasm` is the JS/browser boundary over selected Rust codecs. It is intentionally thin.

## Public DTO Families

### GFF

- `GffRootDto`
- `GffStructDto`
- `GffFieldDto`
- `GffValueDto`
- `GffLocStringDto`
- `GffLocStringEntryDto`
- `read_gff_from_bytes`
- `write_gff_to_bytes`

### `2DA`

- `TwoDaDto`
- `read_twoda_from_bytes`
- `write_twoda_to_bytes`

### TLK

- `SingleTlkDto`
- `TlkEntryDto`
- `read_tlk_from_bytes`
- `write_tlk_to_bytes`

### SSF

- `SsfRootDto`
- `SsfEntryDto`
- `read_ssf_from_bytes`
- `write_ssf_to_bytes`

### ERF

- `ErfDto`
- `ErfEntryDto`
- `ErfLocStringDto`
- `ErfVersionDto`
- `CompressedBufAlgorithmDto`
- `read_erf_from_bytes`
- `write_erf_to_bytes`

### MDL

- `MdlDto`
- `MdlEncodingDto`
- `read_mdl_from_bytes`
- `write_mdl_to_bytes`

### Shared provenance metadata

- `LosslessDtoMetadata`

## Logical Edges

- The wasm layer is not a second implementation of the format rules.
- DTO writes are only enabled where the underlying Rust crate can defend the edit semantics.
- Lossless metadata exists to preserve exact original bytes when a DTO is read and then written back unchanged.

## Why This Interface Exists

It exposes proven native codec behavior to browser and JS consumers without forking the semantics into another implementation language.
