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

## Public Surface

### Core aliases and constants

- `MEMORY_CACHE_THRESHOLD`
- `ReadSeek`
- `SharedReadSeek`
- `ResIoSpawner`

### Cache behavior

- `CachePolicy`

### Error and result vocabulary

- `ResManError`
- `ResManResult`

### Resource identity and provenance

- `ResOrigin`
- `new_res_origin`
- `shared_stream`

### Resource payload model

- `Res`

### Container abstraction

- `ResContainer`

### Manager

- `ResMan`

### Important `ResMan` operations

- `ResMan::new`
- `ResMan::contains`
- `ResMan::demand`
- `ResMan::contents`
- `ResMan::get_resolved`
- `ResMan::get`
- `ResMan::add`
- `ResMan::containers`
- `ResMan::remove`
- `ResMan::remove_at`
- `ResMan::cache`

## Logical Edges

- precedence order is front-to-back; newly added containers shadow older ones
- `contains` and `demand` can consult or bypass the manager cache according to
  `CachePolicy`
- `Res` is lazy and owns decompression metadata as part of the resource model
- small decoded payloads may be memoized inside `Res::read_all`
- `ResOrigin` is provenance for diagnostics, not identity
- the `ResContainer` trait is intentionally abstract so different storage forms
  can plug into the same lookup model

## Why This Crate Exists

This crate is the core of install-backed and archive-backed tooling. Without it,
every workflow would need to hard-code its own precedence policy across
directories, KEY/BIF sets, ERFs, and manifests.

## See also

- [`nwnrs-resdir`](https://docs.rs/nwnrs-resdir),
  [`nwnrs-resfile`](https://docs.rs/nwnrs-resfile),
  [`nwnrs-resmemfile`](https://docs.rs/nwnrs-resmemfile), and
  [`nwnrs-resnwsync`](https://docs.rs/nwnrs-resnwsync) for concrete container
  implementations
- [`nwnrs-install`](https://docs.rs/nwnrs-install), which assembles a
  conventional install-backed manager
