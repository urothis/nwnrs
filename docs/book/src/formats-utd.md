# UTD Door Blueprints

Registry identity:

- extension: `utd`
- resource type: `2042`
- top-level GFF tag: `"UTD "`

`UTD` is the canonical door blueprint resource.

## Role

A `UTD` defines one authored door template. It captures default door behavior,
appearance-facing configuration, linkage semantics, and event hooks before a
specific instance is placed into an area.

## Conceptual Shape

```text
UTD root
|
+-- identity            tag, localized name, blueprint identity
+-- appearance          appearance/model state
+-- connection          transition/link metadata
+-- interaction         lock/open/trap state defaults
+-- scripts             event hooks
+-- miscellaneous       door-specific authored defaults
```

## Current Code Coverage

- generic container support: `nwnrs-gff`
- resource identity: `nwnrs-restype`
- no dedicated lifted `UTD` schema crate yet

## Logical Edges

- Door connection/transition semantics make `UTD` more than "a placeable with a
  different appearance."
- Placed door state in `GIT` adds transform and instance-local information that
  does not belong in the blueprint.
- Door-related navigation also interacts with non-GFF world resources such as
  walkmesh/pathing formats.

## Related Chapters

- [GFF](./formats-gff.md)
- [GIT Area Instances](./formats-git.md)
- [SET Tilesets](./formats-set.md)
