# nwnrs-streamext

Stream helpers for size-prefixed binary formats.

## Scope

- read and write compact little-endian length-prefixed values
- provide small generic helpers for stream-oriented binary framing
- keep size-prefix handling out of higher-level format crates

## Public Surface

### Framing type

- `SizePrefix`

### Read helpers

- `read_array`
- `read_bytes`
- `read_fixed_count_seq`
- `read_fixed_value`
- `read_size_prefixed_bytes`
- `read_size_prefixed_seq`
- `read_size_prefixed_string`
- `read_string`

### Write helpers

- `write_size_prefixed_bytes`
- `write_size_prefixed_seq`
- `write_size_prefixed_string`

## Logical Edges

- this crate is for framed stream structure, not full parser semantics
- `SizePrefix` makes the width and interpretation of a length field explicit
- if a format couples framing tightly to domain meaning, the higher crate should
  own it

## See also

- [`nwnrs-io`](https://docs.rs/nwnrs-io), which provides the broader set of
  binary-read helpers this crate extends

## Why This Crate Exists

Size-prefixed framing patterns recur across older binary formats. This crate
keeps those patterns consistent without inflating `nwnrs-io` into a broader
codec layer.
