# UTW Waypoint Blueprints

Registry identity:

- extension: `utw`
- resource type: `2058`
- top-level GFF tag: `"UTW "`

`UTW` is the canonical waypoint blueprint resource.

## Role

A `UTW` defines one authored waypoint template: symbolic identity, display
metadata, and default behavior before a concrete waypoint instance is placed
into an area.

## Conceptual Shape

```text
UTW root
|
+-- identity            tag, localized name, blueprint identity
+-- map/display         waypoint-facing presentation metadata
+-- scripts/state       hooks and local defaults
```

## Current Code Coverage

- generic container support: `nwnrs-gff`
- resource identity: `nwnrs-restype`
- no dedicated lifted `UTW` schema crate yet

## Logical Edges

- A waypoint is semantically light compared to many other blueprint classes,
  but it still benefits from a dedicated schema identity.
- Concrete placement still belongs in `GIT`.
- Waypoint identity is often more important than large amounts of nested state.

## Related Chapters

- [GFF](./formats-gff.md)
- [GIT Area Instances](./formats-git.md)
