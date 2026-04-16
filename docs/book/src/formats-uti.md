# UTI Item Blueprints

Registry identity:

- extension: `uti`
- resource type: `2025`
- top-level GFF tag: `"UTI "`

`UTI` is the canonical item blueprint resource.

## Role

A `UTI` defines one authored item template. It describes the default
classification, appearance-facing configuration, localized naming/description,
and gameplay defaults for an item before that item is instantiated into
inventory, placed loot, or a store catalog.

## Conceptual Shape

```text
UTI root
|
+-- identity            tag, localized names, blueprint identity
+-- classification      base item type, category, rarity-like flags
+-- appearance          icon/model/texture-facing references
+-- mechanics           cost, stack/charge/use properties, item-specific flags
+-- properties          nested item-property lists
+-- scripts/metadata    optional behavior hooks and authored notes
```

## Current Code Coverage

- generic container support: `nwnrs-gff`
- resource identity: `nwnrs-restype`
- no dedicated lifted `UTI` schema crate yet

## Logical Edges

- `UTI` is an item template, not one runtime inventory slot instance.
- The most important nested structure is typically the item-property list. That
  is where simple "field bag" thinking breaks down quickly.
- Visual appearance for an item may fan out into many other formats, but the
  `UTI` remains the coordinating template rather than the renderer payload.

## Related Chapters

- [GFF](./formats-gff.md)
- [TXI Sidecars](./formats-txi.md)
- [MTR Materials](./formats-mtr.md)
- [TGA Textures](./formats-tga.md)
