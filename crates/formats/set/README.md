# nwnrs-set

Typed parser for Neverwinter Nights tileset (`SET`) payloads.

## Scope

- parse the INI-like tileset structure into typed sections
- build deterministic `SET` text from the typed representation
- write typed tilesets back to a stream
- model tiles, terrain tags, crosser tags, groups, grass settings, and tile
  door metadata explicitly
- expose the authored tileset catalog without coupling it to a renderer

The primary entry points are [`read_set`], [`build_set_text`], [`write_set`],
and [`SetFile`].

## Invariants

- section identity is preserved explicitly through typed collections keyed by
  their authored ids
- tile, group, terrain, and door metadata remain distinct rather than being
  merged into one generic map
- optional values remain optional rather than being normalized to arbitrary
  defaults
- deterministic serialization rebuilds the modeled section structure in
  ascending key order, including synthesized catalog count sections

## Non-goals

- instantiate tile models or runtime area scenes
- infer missing tileset semantics beyond what the source data expresses
