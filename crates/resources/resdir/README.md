# nwnrs-resdir

Directory-backed `nwnrs-resman::ResContainer` implementation.

## Why This Crate Exists

Override directories are a first-class NWN concept. Without a
`ResContainer`-backed directory implementation, tools that use `nwnrs-resman`
could not include loose files alongside archive-backed resources. This crate
bridges the gap so `nwnrs-install` and user tooling can add override directories
to a layered resource manager without special-casing them.

## Scope

- scan an on-disk directory tree for NWN-style resources
- resolve filenames into typed resource references
- expose the resulting directory as a `ResContainer`

## See also

- [`nwnrs-resman`](https://docs.rs/nwnrs-resman), which defines the
  `ResContainer` abstraction this crate implements
- [`nwnrs-install`](https://docs.rs/nwnrs-install), which adds directory
  containers to the layered resource manager
