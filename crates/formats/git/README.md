# nwnrs-git

Typed parser for Neverwinter Nights area instance (`GIT`) resources.

## Scope

- parse `GIT` payloads into typed instance collections such as creatures,
  doors, placeables, triggers, sounds, and waypoints
- preserve the original raw `GFF` structures alongside the typed view
- expose geometry and transform data in forms suitable for higher-level tools
- rebuild and write typed `GIT` payloads back to `GFF`

The principal entry points are [`read_git`], [`build_git_root`], [`write_git`],
and [`GitFile`].

## Invariants

- authored instance order is preserved within each typed collection
- raw top-level and per-instance `GFF` data remain available through the typed
  model
- geometry points and transforms are represented explicitly rather than being
  folded into ad hoc tuples
- rebuilding a `GIT` payload preserves unknown per-entry raw fields while
  rewriting the typed fields owned by this crate

## Non-goals

- resolve blueprints, models, or runtime resources
- interpret all gameplay semantics attached to raw or not-yet-typed fields

## Why This Crate Exists

Area instance data is deeply nested GFF. Without this crate, every tool that
wanted to read creature or placeable positions would reimplement the same
field-path traversal against raw GFF nodes. This crate fixes the schema once
and exposes it as typed Rust values.

## See also

- [`nwnrs-gff`](https://docs.rs/nwnrs-gff), the underlying typed GFF container
  layer
- [`nwnrs-set`](https://docs.rs/nwnrs-set), which describes tileset structure
  rather than placed instances
