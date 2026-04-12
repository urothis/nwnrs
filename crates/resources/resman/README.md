# nwnrs-resman

`nwnrs-resman` defines the central resource-resolution model used by the rest
of the workspace.

## Scope

- model a single payload as [`Res`]
- model a source of payloads as [`ResContainer`]
- resolve multiple containers in precedence order through [`ResMan`]
- provide optional weighted caching for repeated lookups

This crate is intentionally abstract. The container crates supply concrete
backends; `nwnrs-resman` supplies the common lookup algebra.

## Example

```rust
use nwnrs_resman::ResMan;

let resman = ResMan::new(64);
assert!(resman.contents().is_empty());
```

## Non-goals

- parse NWN file formats
- prescribe one on-disk storage layout
- replace container-specific crates such as `nwnrs-erf`, `nwnrs-key`, or
  `nwnrs-resdir`

## See also

- [`nwnrs-resdir`](https://docs.rs/nwnrs-resdir),
  [`nwnrs-resfile`](https://docs.rs/nwnrs-resfile),
  [`nwnrs-resmemfile`](https://docs.rs/nwnrs-resmemfile), and
  [`nwnrs-resnwsync`](https://docs.rs/nwnrs-resnwsync) for concrete container
  implementations
- [`nwnrs-install`](https://docs.rs/nwnrs-install), which assembles a
  conventional install-backed manager
