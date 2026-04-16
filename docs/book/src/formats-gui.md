# GUI UI Resources

Registry identity:

- extension: `gui`
- resource type: `2047`
- top-level GFF tag: `"GUI "`

`GUI` is a GFF-backed UI/resource-layout family.

## Role

A `GUI` resource stores structured user-interface data: screens, controls,
layout-ish metadata, and references needed by the UI subsystem.

## Conceptual Shape

```text
GUI root
|
+-- screen metadata
+-- control/widget list
+-- layout and presentation configuration
+-- resource references for UI assets
+-- behavior/configuration metadata
```

## Current Code Coverage

- generic container support: `nwnrs-gff`
- resource identity: `nwnrs-restype`
- no dedicated lifted `GUI` schema crate yet

## Logical Edges

- UI structure is graph/catalog data, not a world-space scene.
- Ordering and nesting are often semantically important.
- The schema may reference non-GFF assets such as textures or fonts, but it is
  not itself those payload formats.

## Related Chapters

- [GFF](./formats-gff.md)
- [TGA Textures](./formats-tga.md)
- [DDS Textures](./formats-dds.md)
