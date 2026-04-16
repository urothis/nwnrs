# UTE Encounter Blueprints

Registry identity:

- extension: `ute`
- resource type: `2040`
- top-level GFF tag: `"UTE "`

`UTE` is the canonical encounter blueprint resource.

## Role

A `UTE` defines the authored encounter template: what can spawn, under what
rules, and with what encounter-level defaults. A placed encounter volume in
`GIT` then supplies the concrete geometry and instance placement.

## Conceptual Shape

```text
UTE root
|
+-- identity            tag, name, blueprint identity
+-- spawn policy        counts, limits, reset-like behavior
+-- roster              nested spawnable creature references
+-- scripts             encounter hooks
+-- metadata            authored encounter defaults
```

## Current Code Coverage

- generic container support: `nwnrs-gff`
- resource identity: `nwnrs-restype`
- no dedicated lifted `UTE` schema crate yet

## Logical Edges

- Encounter geometry belongs in `GIT`, not `UTE`.
- The roster structure is usually the key semantic payload.
- Encounter blueprints are one of the clearest examples of template-vs-instance
  separation in the resource system.

## Related Chapters

- [GFF](./formats-gff.md)
- [GIT Area Instances](./formats-git.md)
- [UTC Creature Blueprints](./formats-utc.md)
