# DLG Dialogue Graphs

Registry identity:

- extension: `dlg`
- resource type: `2029`
- top-level GFF tag: `"DLG "`

`DLG` is the dialogue graph resource.

## Role

A `DLG` stores conversational structure: nodes, links, branching conditions,
actions, and localized text references. Its primary semantics are graph
semantics, not physical object semantics.

## Conceptual Shape

```text
DLG root
|
+-- dialogue metadata
+-- node list           entries, replies, or comparable graph nodes
+-- link structure      outgoing transitions
+-- conditions/actions  script-facing logic hooks
+-- text references     localized content bindings
```

## Current Code Coverage

- generic container support: `nwnrs-gff`
- resource identity: `nwnrs-restype`
- no dedicated lifted `DLG` schema crate yet

## Logical Edges

- Ordering and linkage are core semantics, not incidental serialization detail.
- A dialogue graph is not well represented as one flat object record.
- Text payloads often live in `TLK`, so the dialogue schema is partially a graph
  over external localized references.

## Related Chapters

- [GFF](./formats-gff.md)
- [Dialog Tables (TLK)](./formats-tlk.md)
- [NWScript Compiler](./language-nwscript.md)
