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

## Public Surface

- `SET_RES_TYPE`
- `SetError`
- `SetResult`
- `SetFile`
- `SetGeneral`
- `SetGrass`
- `SetNamedType`
- `SetPrimaryRule`
- `SetTile`
- `SetTileCorner`
- `SetTileEdges`
- `SetTileDoor`
- `SetGroup`
- `read_set`
- `parse_set`
- `build_set_text`
- `write_set`

## Core Model

`SetFile` preserves distinct keyed collections for:

- `general`
- optional `grass`
- `terrains`
- `crossers`
- `primary_rules`
- `tiles`
- `tile_doors`
- `groups`

Important typed pieces:

- `SetTileCorner`
  - terrain tag
  - height step
- `SetTileEdges`
  - explicit top, right, bottom, and left crosser tags
- `SetTile`
  - model reference
  - walkmesh reference
  - terrain annotations
  - lighting and animation flags
  - tile-level visibility and pathing metadata

## Text Layout

`SET` is INI-like and section-oriented.

```text
[GENERAL]
...

[GRASS]
...

[TERRAIN0]
...

[CROSSER0]
...

[PRIMARY RULE0]
...

[TILE0]
...

[TILE0DOOR0]
...

[GROUP0]
...
```

Conceptually:

```text
+----------------------+
| global metadata      |
+----------------------+
| optional grass block |
+----------------------+
| terrain catalog      |
+----------------------+
| crosser catalog      |
+----------------------+
| rule catalog         |
+----------------------+
| tile catalog         |
+----------------------+
| tile-door metadata   |
+----------------------+
| groups               |
+----------------------+
```

## Invariants

- section identity is preserved explicitly through typed collections keyed by
  their authored ids
- tile, group, terrain, crosser, and door metadata remain distinct rather than
  being merged into one generic map
- optional values remain optional rather than being normalized to arbitrary
  defaults
- deterministic serialization rebuilds the modeled section structure in
  ascending key order

## Non-goals

- instantiate tile models or runtime area scenes
- infer missing tileset semantics beyond what the source data expresses

## Why This Crate Exists

`SET` is one of the clearest examples in the workspace of "catalog structure is
data." If you flatten it into one generic section map, you lose too much:

- explicit typed tile semantics
- tile-door relationship structure
- terrain and crosser taxonomy
- deterministic reconstruction of the authored tileset catalog
