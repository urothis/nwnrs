# UTT Trigger Blueprints

Registry identity:

- extension: `utt`
- resource type: `2032`
- top-level GFF tag: `"UTT "`

`UTT` is the canonical trigger blueprint resource.

## Role

A `UTT` defines one authored trigger template: default trigger behavior, event
bindings, and trigger-side state before a specific trigger volume is placed into
an area.

## Conceptual Shape

```text
UTT root
|
+-- identity            tag, localized/display metadata
+-- trigger policy      cursor/interaction/trap/transition-like defaults
+-- scripts             event hooks
+-- authored state      default trigger properties
```

## Current Code Coverage

- generic container support: `nwnrs-gff`
- resource identity: `nwnrs-restype`
- no dedicated lifted `UTT` schema crate yet

## Logical Edges

- Trigger polygon geometry belongs in `GIT`, not `UTT`.
- Trigger schemas are behavior-heavy rather than model-heavy.
- Transition logic, script hooks, and local state tend to matter more than
  visual appearance.

## Related Chapters

- [GFF](./formats-gff.md)
- [GIT Area Instances](./formats-git.md)
- [UTD Door Blueprints](./formats-utd.md)
