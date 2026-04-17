# nwnrs-resmemfile

In-memory `nwnrs-resman::ResContainer` implementation.

## Why This Crate Exists

Downloaded or synthetically generated payloads need to enter the resource
lookup chain without touching the filesystem. Without an in-memory container,
callers would need to write bytes to a temporary file just to create a
`ResFile`. This crate lets any byte buffer participate in a `ResMan` lookup
chain directly.

## Scope

- wrap a byte buffer as a single resource entry
- expose synthetic or downloaded payloads through the same container interface
  as on-disk resources

## See also

- [`nwnrs-resman`](https://docs.rs/nwnrs-resman), which defines the
  `ResContainer` abstraction this crate implements
- [`nwnrs-resfile`](https://docs.rs/nwnrs-resfile), the on-disk equivalent for
  single-file resources
