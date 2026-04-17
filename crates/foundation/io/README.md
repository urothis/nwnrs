# nwnrs-io

`nwnrs-io` contains the small generic primitives that would otherwise be
duplicated across binary codecs.

## Scope

- exact-read helpers for binary parsing
- byte-order conversion helpers
- simple invariant-checking errors and assertions shared by format crates

The most important items are [`read_bytes_or_err`], [`read_fixed_count_seq`],
[`swap_endian`], and [`ExpectationError`].

## Public Surface

### Error and assertion vocabulary

- `ExpectationError`
- `expect`

### Binary read helpers

- `read_bytes_or_err`
- `read_fixed_count_seq`
- `read_str_or_err`
- `map_with_index`

### Endian conversion

- `SwappableEndian`
- `swap_endian`

## Logical Edges

- exact-read semantics are part of the crate contract
- `expect` is not a parser convenience; it is how format-level invariants are
  surfaced without losing context
- `read_fixed_count_seq` is for homogeneous counted structures; irregular
  semantics should stay in higher-level crates
- if a parser needs to know what a field means, that behavior does not belong
  here

## Why This Crate Exists

Without `nwnrs-io`, every codec would grow its own slightly different
interpretation of short reads, fixed-count structure handling, and endian-aware
conversions.
