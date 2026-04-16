# Stream Extensions

Docs:

- crate: `nwnrs-streamext`
- [crate docs](https://docs.rs/nwnrs-streamext/latest/nwnrs_streamext/)
- [README](https://github.com/urothis/nwnrs/blob/main/crates/foundation/streamext/README.md)
- [source](https://github.com/urothis/nwnrs/blob/main/crates/foundation/streamext/src/lib.rs)

## Scope

`nwnrs-streamext` contains helpers for size-prefixed and small framed binary structures. It lives above raw IO but below domain-specific codecs.

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

- This crate is for framed stream structure, not full parser semantics.
- `SizePrefix` makes the width and interpretation of a length field explicit instead of burying it in individual codecs.
- If a format uses a length-prefixed sequence, the framing behavior should generally come from here. If the format couples framing tightly to domain meaning, the higher crate should own it.

## Why This Crate Exists

Size-prefixed framing patterns recur across older binary formats. This crate keeps those patterns consistent without inflating `nwnrs-io` into a broader codec layer.
