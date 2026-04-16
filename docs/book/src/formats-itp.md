# ITP Tool Palettes

Registry identity:

- extension: `itp`
- resource type: `2030`
- top-level GFF tag: `"ITP "`

`ITP` is the tool-palette or palette-structure resource family.

## Role

An `ITP` stores editor-facing catalog structure: grouped placeable objects,
tools, or palette entries used by tooling rather than by runtime spatial
simulation directly.

## Conceptual Shape

```text
ITP root
|
+-- palette metadata
+-- category/group hierarchy
+-- entry list
+-- references to blueprint/resource kinds
```

## Current Code Coverage

- generic container support: `nwnrs-gff`
- resource identity: `nwnrs-restype`
- no dedicated lifted `ITP` schema crate yet

## Logical Edges

- `ITP` is tooling/catalog data, not one gameplay object.
- It is closely related to blueprint resource families because palette entries
  usually point at those templates.
- Hierarchy and ordering are usually part of the meaning.

## Related Chapters

- [UTC Creature Blueprints](./formats-utc.md)
- [UTI Item Blueprints](./formats-uti.md)
- [UTP Placeable Blueprints](./formats-utp.md)
- [GFF](./formats-gff.md)
