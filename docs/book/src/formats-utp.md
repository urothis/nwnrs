# UTP Placeable Blueprints

Registry identity:

- extension: `utp`
- resource type: `2044`
- top-level GFF tag: `"UTP "`

`UTP` is the canonical placeable blueprint resource.

## Role

A `UTP` defines one authored placeable template: a world object that can be
placed into an area and later instantiated as a specific placeable entry.

## Conceptual Shape

```text
UTP root
|
+-- identity            tag, localized name, blueprint identity
+-- appearance          appearance/model references
+-- interaction flags   usability, lock/open/trap-like behavior
+-- scripts             event hooks
+-- inventory/contents  optional nested owned resources
+-- state defaults      authored object defaults
```

## Current Code Coverage

- generic container support: `nwnrs-gff`
- resource identity: `nwnrs-restype`
- no dedicated lifted `UTP` schema crate yet

## Logical Edges

- A placeable blueprint is not a placed placeable instance. Placement belongs
  in `GIT`.
- Many placeables are containers, so the schema often mixes world-object
  identity with nested inventory semantics.
- A placeable can participate in pathing, trap, lock, and script systems
  simultaneously, making it a multi-subsystem object.

## Related Chapters

- [GFF](./formats-gff.md)
- [GIT Area Instances](./formats-git.md)
- [SET Tilesets](./formats-set.md)
