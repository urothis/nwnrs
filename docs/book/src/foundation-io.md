# Binary IO

Docs:

- crate: `nwnrs-io`
- [crate docs](https://docs.rs/nwnrs-io/latest/nwnrs_io/)
- [README](https://github.com/urothis/nwnrs/blob/main/crates/foundation/io/README.md)
- [source](https://github.com/urothis/nwnrs/blob/main/crates/foundation/io/src/lib.rs)

## Scope

`nwnrs-io` is the small invariant-enforcing binary substrate used by the format crates. It is deliberately narrow. It exists to keep exact-read semantics, endian conversion, and small parser assertions out of the domain crates.

## Public Surface

### Error and assertion vocabulary

- `ExpectationError`
- `expect`

These are the core "this invariant must hold" primitives. They are used where malformed input should be reported as a domain error rather than as a panic.

### Binary read helpers

- `read_bytes_or_err`
- `read_fixed_count_seq`
- `read_str_or_err`
- `map_with_index`

These helpers are the main answer to short reads and fixed-count structural reads. They keep the crate users from open-coding "read exactly N bytes, then reinterpret" logic in every format parser.

### Endian conversion

- `SwappableEndian`
- `swap_endian`

This is intentionally explicit. The crate does not try to act like a serialization framework. It gives codecs a narrow tool for endian-sensitive field handling.

## Logical Edges

- Exact-read semantics are part of the crate contract. If the input is too short, the caller gets a typed error rather than partial data or implicit zero-filling.
- `expect` is not a parser convenience. It is how format-level invariants are surfaced without losing context.
- `read_fixed_count_seq` is for homogeneous counted structures. If the format semantics are irregular, the higher-level crate should own that logic directly.
- The crate is intentionally below domain semantics. If a parser needs to know what a field means, that behavior does not belong here.

## Why This Crate Exists

Without `nwnrs-io`, every codec would grow its own slightly different interpretation of:

- what counts as EOF versus malformed structure
- how fixed-count reads report failure
- how endian-aware conversions are expressed

This crate is the normalization point for those decisions.
