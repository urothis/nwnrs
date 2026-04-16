# ARE Area Static Data

Registry identity:

- extension: `are`
- resource type: `2012`
- top-level GFF tag: `"ARE "`

`ARE` is the static area-definition document.

## Role

An `ARE` captures the non-instance side of one area: environmental defaults,
area-level metadata, and static authored settings that belong to the area as a
whole rather than to one placed object.

## Conceptual Shape

```text
ARE root
|
+-- identity            area name and area-level metadata
+-- environment         lighting/ambient/music-like defaults
+-- area policy         flags and authored configuration
+-- references          links to related area resources
```

## Why It Is Separate From `GIT`

- `ARE` holds area-static configuration
- `GIT` holds placed instances and local geometry

That split is one of the central structural decisions in NWN data layout.

## Current Code Coverage

- generic container support: `nwnrs-gff`
- resource identity: `nwnrs-restype`
- no dedicated lifted `ARE` schema crate yet

## Logical Edges

- Area-level ambient/audio/environment settings do not belong inside each placed
  object.
- Static area metadata and instance placement evolve on different axes and
  therefore deserve separate documents.
- Nearby non-GFF resources such as tilesets and walkmeshes participate in area
  realization, but they are not encoded inside the `ARE` schema itself.

## Related Chapters

- [GIT Area Instances](./formats-git.md)
- [IFO Module Metadata](./formats-ifo.md)
- [SET Tilesets](./formats-set.md)
