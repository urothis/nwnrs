# BIC Character Resources

Registry identity:

- extension: `bic`
- resource type: `2015`
- top-level GFF tag: `"BIC "`

`BIC` is the character-state resource.

## Role

A `BIC` stores one character's persisted state. It is related to creature data
but not equivalent to a creature blueprint.

## Conceptual Shape

```text
BIC root
|
+-- identity            player/character identity
+-- progression         levels, classes, feats, skills, progression state
+-- inventory/equipment owned item state
+-- appearance          character-facing presentation state
+-- locals/state        persisted character data
```

## Difference From `UTC`

- `UTC` is the blueprint template for a creature archetype
- `BIC` is persisted character state

They may overlap in broad subject matter, but they represent different points in
the lifecycle of an entity.

## Current Code Coverage

- generic container support: `nwnrs-gff`
- resource identity: `nwnrs-restype`
- no dedicated lifted `BIC` schema crate yet

## Logical Edges

- Persisted character state is not just "a creature template with more fields."
- Inventory and progression information are central, not incidental.
- Character state links outward to many other resource classes without replacing
  them.

## Related Chapters

- [UTC Creature Blueprints](./formats-utc.md)
- [UTI Item Blueprints](./formats-uti.md)
- [GFF](./formats-gff.md)
