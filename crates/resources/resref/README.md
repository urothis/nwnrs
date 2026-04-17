# nwnrs-resref

`nwnrs-resref` models Neverwinter Nights resource references as a typed pair:
a case-insensitive resource name together with a resource kind.

## Scope

- validate and parse resource references
- format resource references back to filenames or typed identifiers
- mediate between `name.ext` filenames and typed resource ids through
  `nwnrs-restype`

## Public Surface

### Constants and errors

- `RESREF_MAX_LENGTH`
- `ResRefError`
- `is_valid_resref_part1`

### Core types

- `ResRef`
- `ResolvedResRef`

### Important methods

- `ResRef::new`
- `ResRef::resolve`
- `ResRef::res_ref`
- `ResRef::res_type`
- `ResolvedResRef::new`
- `ResolvedResRef::try_from_filename`
- `ResolvedResRef::from_filename`
- `ResolvedResRef::base`
- `ResolvedResRef::res_ref`
- `ResolvedResRef::res_type`
- `ResolvedResRef::res_ext`
- `ResolvedResRef::to_file`

## Logical Edges

- `ResRef` preserves authored spelling for display, but equality, ordering, and
  hashing treat the name portion case-insensitively
- the resource kind participates in equality and hashing
- `ResolvedResRef` does not prove that a file exists; it proves only that the
  `(name, type)` pair has a known conventional extension
- the 16-byte name limit is part of the type contract

## Why This Crate Exists

This crate is the boundary between stringly filenames and typed resource
identity. `ResMan` and the container crates rely on it for stable lookup
semantics.

## See also

- [`nwnrs-restype`](https://docs.rs/nwnrs-restype), which supplies the
  extension/type mapping used by resolved resource references
- [`nwnrs-resman`](https://docs.rs/nwnrs-resman), which uses `ResRef` as its
  canonical lookup key
