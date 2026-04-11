# nwnrs-io

`nwnrs-io` contains the small generic primitives that would otherwise be
duplicated across binary codecs.

## Scope

- exact-read helpers for binary parsing
- byte-order conversion helpers
- simple invariant-checking errors and assertions shared by format crates

The most important items are [`read_bytes_or_err`], [`read_fixed_count_seq`],
[`swap_endian`], and [`ExpectationError`].

## Non-goals

- act as a full serialization framework
- replace format-specific IO code where domain structure matters
