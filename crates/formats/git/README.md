# nwnrs-git

Typed parser for Neverwinter Nights area instance (`GIT`) resources.

## Why This Crate Exists

`GFF` is a general-purpose container; `GIT` is domain-specific. The raw GFF
layer has no knowledge of placed instances, instance types, or area geometry.
This crate lifts raw GFF structs into typed Rust collections so area tooling
can work with creatures, doors, placeables, and waypoints directly instead of
navigating untyped field maps.

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

## See also

- [`nwnrs-gff`](https://docs.rs/nwnrs-gff), the underlying typed GFF container
  layer
- [`nwnrs-set`](https://docs.rs/nwnrs-set), which describes tileset structure
  rather than placed instances
