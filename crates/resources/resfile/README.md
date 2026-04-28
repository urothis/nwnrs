# nwnrs-resfile

Single-file `nwnrs-resman::ResContainer` implementation.

## Why This Crate Exists

Tools occasionally need to inject a single file into a `ResMan` lookup chain —
for example, a lone `NWScript` standard library or a standalone blueprint. Without
a single-file `ResContainer`, callers would need a temporary directory or a
custom container type. This crate provides the minimal wrapper so any file can
be surfaced through the standard resource interface.

## Scope

- wrap one on-disk file as a single resource entry
- expose that entry through the same `ResContainer` abstraction used elsewhere

## See also

- [`nwnrs-resman`](https://docs.rs/nwnrs-resman), which defines the
  `ResContainer` abstraction this crate implements
- [`nwnrs-resdir`](https://docs.rs/nwnrs-resdir), which provides the
  directory-backed equivalent for scanning multiple files
