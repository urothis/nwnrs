# nwnrs-types

`nwnrs-types` defines the central resource-resolution model used by the rest
of the workspace.

## Scope

- model a single payload as `Res`
- model a source of payloads as `ResContainer`
- resolve multiple containers in precedence order through `ResMan`
- provide optional weighted caching for repeated lookups

This crate is intentionally abstract. The container crates supply concrete
backends; `nwnrs-types` supplies the common lookup algebra.

## Example

```rust
use nwnrs_types::resman::ResMan;

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

- `ResType`
- `ResRef`
- `ResolvedResRef`
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
- resource identity lives here: resource types, resource references, filename
  normalization, and the built-in registry are all part of the same lookup
  layer
- `Res` is lazy and owns decompression metadata as part of the resource model
- small decoded payloads may be memoized inside `Res::read_all`
- `ResOrigin` is provenance for diagnostics, not identity
- the `ResContainer` trait is intentionally abstract so different storage forms
  can plug into the same lookup model

## Resource Identity and Registry

This module owns the typed identity layer for resource lookup:

- `ResType` is the typed resource-kind id
- `ResRef` is a typed `(name, type)` reference
- `ResolvedResRef` is the normalized form commonly used by loaders and
  containers

It also owns the built-in resource-type registry and the extension mapping
functions used across the workspace:

- `lookup_res_type`
- `lookup_res_ext`
- `get_res_type`
- `get_res_ext`
- `register_custom_res_type`

There is no separate `restype` or `resref` crate anymore. Those concepts now
live here because they are inseparable from lookup and container behavior.

## Container Backends

The abstraction layer lives here, but several concrete backends also ship from
this module:

- directory-backed containers
- single-file containers
- in-memory byte containers
- shared-stream helpers

Higher modules such as `install`, `erf`, `key`, and `nwsync` build on the same
resource identity and precedence algebra rather than redefining it.

## Why This Crate Exists

This crate is the core of install-backed and archive-backed tooling. Without it,
every workflow would need to hard-code its own precedence policy across
directories, KEY/BIF sets, ERFs, and manifests.

## See also

- the built-in directory, single-file, and in-memory `ResContainer`
  implementations exposed directly by this module
- [`crate::install`], which assembles a conventional install-backed manager
- [`crate::erf`], [`crate::key`], and [`crate::nwsync`], which expose
  additional container-backed resource sources
