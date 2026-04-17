# nwnrs-exo

Shared EXO-level constants and enums.

## Scope

- define the small set of magic values and compression markers shared by
  EXO-backed container formats
- prevent those low-level constants from being redefined inconsistently across
  multiple crates

## Public Surface

- `ExoResFileCompressionType`
- `EXO_RES_FILE_COMPRESSED_BUF_MAGIC`

## Invariants

- each constant or enum value corresponds directly to a known EXO wire-level
  concept
- the crate exists for wire vocabulary, not for container parsing

## Non-goals

- parse full EXO-backed containers on its own
- provide a general binary-protocol abstraction

## See also

- [`nwnrs-compressedbuf`](https://docs.rs/nwnrs-compressedbuf), which uses
  these constants for compressed-buffer framing
- [`nwnrs-erf`](https://docs.rs/nwnrs-erf) and
  [`nwnrs-key`](https://docs.rs/nwnrs-key), which use these constants for `E1`
  compression metadata
