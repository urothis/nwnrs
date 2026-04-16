# IFO Module Metadata

Registry identity:

- extension: `ifo`
- resource type: `2014`
- top-level GFF tag: `"IFO "`

`IFO` is the module-level metadata document.

## Role

An `IFO` carries top-level module identity and defaults. It is the resource that
orients the module as a whole rather than any one area or any one gameplay
object.

## Conceptual Shape

```text
IFO root
|
+-- module identity     name, description, top-level metadata
+-- configuration       module-wide defaults and policy
+-- entry references    start locations / starting-area style links
+-- scripts             module-scope hooks
+-- metadata            authored module information
```

## Current Code Coverage

- generic container support: `nwnrs-gff`
- resource identity: `nwnrs-restype`
- no dedicated lifted `IFO` schema crate yet

## Logical Edges

- Module-scope configuration does not substitute for area-scope `ARE` data.
- Entry/start references create relationships across other resource kinds rather
  than replacing them.
- `IFO` is orchestration metadata, not a physical scene description.

## Related Chapters

- [ARE Area Static Data](./formats-are.md)
- [GIT Area Instances](./formats-git.md)
- [ERF Archives](./formats-erf.md)
