# SET Tilesets

Docs:

- [crate docs](https://docs.rs/nwnrs-set/latest/nwnrs_set/)
- [README](https://github.com/urothis/nwnrs/blob/main/crates/formats/set/README.md)
- [source](https://github.com/urothis/nwnrs/blob/main/crates/formats/set/src/lib.rs)

`SET` is the authored catalog for a tileset. It is not an instantiated scene.

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
  - explicit top/right/bottom/left crosser tags
- `SetTile`
  - model reference
  - walkmesh reference
  - terrain annotations
  - lighting/animation flags
  - tile-level visibility/pathing metadata

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

## Logical Edges

- Section identity is explicit and keyed by authored ids.
- Tile, group, terrain, crosser, and door metadata remain distinct structures.
- Optional values stay optional instead of being normalized to arbitrary
  defaults.
- Deterministic serialization rebuilds the catalog in ascending key order.

## Why This Crate Exists

`SET` is one of the clearest examples in the workspace of "catalog structure is
data." If you flatten it into one generic section map, you lose too much:

- explicit typed tile semantics
- tile-door relationship structure
- terrain and crosser taxonomy
- deterministic reconstruction of the authored tileset catalog
