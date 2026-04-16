# Area and Module Resources

This family covers the GFF-backed resources that describe module-level and
area-level structure rather than one standalone gameplay object.

## Common Members of This Family

- `ARE` area static data
- `GIT` placed instance data
- `IFO` module metadata and top-level module settings
- `GIC` ancillary area companion metadata

## Per-Tag Chapters

- [ARE Area Static Data](./formats-are.md)
- [IFO Module Metadata](./formats-ifo.md)
- [GIT Area Instances](./formats-git.md)
- [GIC Area Companion Data](./formats-gic.md)

Adjacent non-GFF resources such as `SET`, `WOK`, `PWK`, and `DWK` participate in
the same runtime problem space, but they are not themselves GFF schema variants.

## Structural Split

The important split in NWN area/module data is:

```text
module / area identity and static configuration
!=
placed runtime-facing instances
```

Conceptually:

```text
IFO  -> module-level metadata, top-level configuration, global hooks
ARE  -> one area's static/environmental data
GIT  -> one area's placed instances and local instance geometry
GIC  -> companion area-side metadata
```

Another useful diagram:

```text
module
|
+-- IFO   module identity and defaults
|
+-- area A
|   +-- ARE  static area data
|   +-- GIT  placed objects and geometry
|   +-- GIC  companion metadata
|
+-- area B
    +-- ARE
    +-- GIT
    +-- GIC
```

## Current Typed Coverage

In the current repo:

- `GIT` has a dedicated lifted schema crate: `nwnrs-git`
- `ARE`, `IFO`, and `GIC` currently remain primarily documented/tagged GFF
  families rather than dedicated lifted schema crates

## Logical Edges

- Static area configuration and placed-instance state are different documents.
- A module-level resource such as `IFO` does not substitute for per-area
  `ARE`/`GIT` data.
- Placed-instance schemas are where transforms, polygon geometry, and blueprint
  references become concrete.
- The existence of multiple companion resource kinds for one gameplay concept is
  part of the engine architecture, not an accident of packaging.

## Why This Family Matters

Most install-backed tooling eventually needs to reason about areas and modules,
not only about isolated objects. That requires understanding the split between:

- authored templates
- static area metadata
- placed instance state
- module-level orchestration
