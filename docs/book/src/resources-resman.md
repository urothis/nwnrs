# Resource Manager

Docs:

- crate: `nwnrs-resman`
- [crate docs](https://docs.rs/nwnrs-resman/latest/nwnrs_resman/)
- [README](https://github.com/urothis/nwnrs/blob/main/crates/resources/resman/README.md)
- [source](https://github.com/urothis/nwnrs/blob/main/crates/resources/resman/src/lib.rs)

## Scope

`nwnrs-resman` is the common lookup algebra for the workspace. It is the point where resource identity, storage backends, lazy IO, optional decompression, and precedence order come together.

## Public Surface

### Core aliases and constants

- `MEMORY_CACHE_THRESHOLD`
- `ReadSeek`
- `SharedReadSeek`
- `ResIoSpawner`

### Cache behavior

- `CachePolicy`

### Error/result vocabulary

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

## Important `ResMan` operations

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

- Precedence order is front-to-back. Newly added containers shadow older ones.
- `contains` and `demand` can consult or bypass the manager cache according to `CachePolicy`.
- `Res` is lazy. It does not mean "the bytes are already loaded"; it means "the manager knows how to reopen or share the underlying stream and decode the payload when asked."
- `Res` also owns decompression metadata. Compression state is part of the resource model, not an external concern.
- Small decoded payloads may be memoized inside `Res::read_all`; the threshold is exposed as `MEMORY_CACHE_THRESHOLD`.
- `ResOrigin` is provenance for diagnostics, not identity.
- The `ResContainer` trait is intentionally abstract: different storage forms plug into the same lookup model as long as they can answer "contains", "demand", and "contents".

## Why This Crate Exists

This crate is the core of install-backed and archive-backed tooling. Without it, every workflow would need to hard-code its own precedence policy across directories, KEY/BIF sets, ERFs, and manifests.
