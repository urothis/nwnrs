# nwnrs-resdir

Directory-backed `nwnrs-resman::ResContainer` implementation.

## Scope

- scan an on-disk directory tree for NWN-style resources
- resolve filenames into typed resource references
- expose the resulting directory as a `ResContainer`

## Non-goals

- define precedence policy across multiple directories
- parse the contents of the resolved resources

## See also

- [`nwnrs-resman`](https://docs.rs/nwnrs-resman), which defines the
  `ResContainer` abstraction this crate implements
- [`nwnrs-install`](https://docs.rs/nwnrs-install), which adds directory
  containers to the layered resource manager
