# EXO Wire Vocabulary

Docs:

- [crate docs](https://docs.rs/nwnrs-exo/latest/nwnrs_exo/)
- [README](https://github.com/urothis/nwnrs/blob/main/crates/formats/exo/README.md)
- [types.rs](https://github.com/urothis/nwnrs/blob/main/crates/formats/exo/src/types.rs)

`nwnrs-exo` is intentionally tiny. It is not a container parser. It is the
shared vocabulary for EXO-level compression markers used by container formats.

## Public Surface

- `EXO_RES_FILE_COMPRESSED_BUF_MAGIC`
- `ExoResFileCompressionType`

## Wire-Level Concepts

Known compression markers:

- `0` => `None`
- `1` => `CompressedBuf`

Those values are consumed by higher-level formats such as:

- `ERF E1`
- `BIF E1`

## Logical Edges

- This crate exists to keep low-level EXO constants from being redefined
  inconsistently in multiple archive implementations.
- It does not parse a standalone file format.
- It does not define a general binary protocol abstraction.

## Why This Crate Exists

Reverse-engineering projects tend to accumulate "small constants" in whatever
crate happens to need them first. That gets sloppy fast. This crate isolates the
shared EXO-level vocabulary so archive crates can depend on one canonical source
of truth.
