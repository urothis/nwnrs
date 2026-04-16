# Dialogue, Journal, and Meta Resources

This family covers GFF-backed resources whose main semantics are graph,
campaign-state, UI, or tooling metadata rather than one physical object in the
world.

## Common Members of This Family

- `DLG` dialogue resources
- `JRL` journal resources
- `FAC` faction resources
- `GUI` GUI layout/configuration resources
- `ITP` tool palette / palette-structure resources
- `BIC` character resources

## Per-Tag Chapters

- [DLG Dialogue Graphs](./formats-dlg.md)
- [JRL Journal Data](./formats-jrl.md)
- [FAC Faction Data](./formats-fac.md)
- [GUI UI Resources](./formats-gui.md)
- [ITP Tool Palettes](./formats-itp.md)
- [BIC Character Resources](./formats-bic.md)

These tags all share the property that their meaning is mostly in higher-level
graph or catalog structure rather than in a spatial scene.

## Structural Pattern

Conceptually:

```text
GFF root
|
+-- graph or catalog metadata
+-- ordered and/or keyed lists
+-- localized strings
+-- references into scripts, portraits, models, or resource ids
+-- state classification fields
```

Typical schema roles:

- `DLG`
  dialogue graph with nodes, links, conditions, and actions
- `JRL`
  journal categories and entries
- `FAC`
  faction definitions and inter-faction relationships
- `GUI`
  structured UI description data
- `ITP`
  palette/catalog structure used by tooling/editor workflows
- `BIC`
  character-state payload rather than one module-local blueprint

## Logical Edges

- These resources are often list-heavy and graph-heavy rather than geometry-
  heavy.
- Ordering often matters because graph/campaign semantics are not the same as
  an unordered field bag.
- Localized strings and resource references coexist, but they refer to
  different namespaces and should not be conflated.
- `BIC` is character-state data, not simply a renamed creature blueprint.

## Current Coverage Boundary

The workspace currently documents and recognizes these resource kinds via the
registry and generic `GFF` support, but it does not yet expose dedicated lifted
schema crates for most of them.

That means the accurate statement today is:

- container mechanics are implemented
- type identity is implemented
- dedicated schema lifting is selective rather than universal

## Why This Family Matters

These are the places where "reverse engineering the file format" quickly turns
into "reverse engineering the engine's state model." Their complexity is often
not low-level binary complexity but semantic complexity:

- graph traversal
- campaign progression state
- UI configuration structure
- editor-oriented catalogs
