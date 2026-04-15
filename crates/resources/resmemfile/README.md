# nwnrs-resmemfile

In-memory `nwnrs-resman::ResContainer` implementation.

## Scope

- wrap a byte buffer as a single resource entry
- expose synthetic or downloaded payloads through the same container interface
  as on-disk resources

## See also

- [`nwnrs-resman`](https://docs.rs/nwnrs-resman), which defines the
  `ResContainer` abstraction this crate implements
- [`nwnrs-resfile`](https://docs.rs/nwnrs-resfile), the on-disk equivalent for
  single-file resources
