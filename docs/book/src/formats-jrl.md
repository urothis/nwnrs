# JRL Journal Data

Registry identity:

- extension: `jrl`
- resource type: `2056`
- top-level GFF tag: `"JRL "`

`JRL` is the journal or quest-log resource.

## Role

A `JRL` stores journal structure: categories, entries, and progression-facing
metadata that define how narrative or quest information is organized.

## Conceptual Shape

```text
JRL root
|
+-- journal metadata
+-- category list
+-- entry lists within categories
+-- localized text references
+-- progression/classification metadata
```

## Current Code Coverage

- generic container support: `nwnrs-gff`
- resource identity: `nwnrs-restype`
- no dedicated lifted `JRL` schema crate yet

## Logical Edges

- Journal semantics are catalog and progression oriented rather than spatial.
- Ordering often matters because journal displays and progression logic depend
  on explicit structure.
- Localized content is typically externalized to `TLK` or equivalent string
  systems even when the journal owns the progression graph.

## Related Chapters

- [GFF](./formats-gff.md)
- [Dialog Tables (TLK)](./formats-tlk.md)
