# GIC Area Companion Data

Registry identity:

- extension: `gic`
- resource type: `2046`
- top-level GFF tag: `"GIC "`

`GIC` is a companion area-side metadata resource.

## Role

`GIC` sits in the area/module family as a companion document rather than as the
primary static area definition or placed-instance payload. It exists because not
all area-related state belongs naturally inside `ARE` or `GIT`.

## Conceptual Shape

```text
GIC root
|
+-- area companion metadata
+-- auxiliary authored configuration
+-- references into neighboring area resources
```

## Current Code Coverage

- generic container support: `nwnrs-gff`
- resource identity: `nwnrs-restype`
- no dedicated lifted `GIC` schema crate yet

## Logical Edges

- The existence of a companion area document is itself important architectural
  information.
- Reverse engineering should resist the urge to flatten every area-related
  field into `ARE` just because that feels simpler.
- `GIC` is best understood relative to the rest of the area family rather than
  in isolation.

## Related Chapters

- [ARE Area Static Data](./formats-are.md)
- [GIT Area Instances](./formats-git.md)
- [IFO Module Metadata](./formats-ifo.md)
