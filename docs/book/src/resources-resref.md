# Resource References

Docs:

- crate: `nwnrs-resref`
- [crate docs](https://docs.rs/nwnrs-resref/latest/nwnrs_resref/)
- [README](https://github.com/urothis/nwnrs/blob/main/crates/resources/resref/README.md)
- [source](https://github.com/urothis/nwnrs/blob/main/crates/resources/resref/src/lib.rs)

## Scope

`nwnrs-resref` defines the canonical lookup key used by the rest of the workspace: a case-insensitive resource name paired with a typed resource kind.

## Public Surface

### Constants and errors

- `RESREF_MAX_LENGTH`
- `ResRefError`
- `is_valid_resref_part1`

### Core types

- `ResRef`
- `ResolvedResRef`

### Important methods

#### `ResRef`

- `ResRef::new`
- `ResRef::resolve`
- `ResRef::res_ref`
- `ResRef::res_type`

#### `ResolvedResRef`

- `ResolvedResRef::new`
- `ResolvedResRef::try_from_filename`
- `ResolvedResRef::from_filename`
- `ResolvedResRef::base`
- `ResolvedResRef::res_ref`
- `ResolvedResRef::res_type`
- `ResolvedResRef::res_ext`
- `ResolvedResRef::to_file`

## Logical Edges

- `ResRef` preserves authored spelling for display, but equality, ordering, and hashing treat the name portion case-insensitively.
- The resource kind participates in equality and hashing. `foo.utc` and `foo.utp` are not the same reference.
- `ResolvedResRef` does not prove that a file exists. It proves only that the `(name, type)` pair has a known conventional extension.
- `try_from_filename` normalizes through the registry and validation logic rather than accepting arbitrary `name.ext` strings.
- The 16-byte name limit is part of the type contract.

## Why This Crate Exists

This crate is the boundary between stringly filenames and typed resource identity. `ResMan` and the container crates rely on it for stable lookup semantics.
