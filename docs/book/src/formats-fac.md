# FAC Faction Data

Registry identity:

- extension: `fac`
- resource type: `2038`
- top-level GFF tag: `"FAC "`

`FAC` is the faction-definition resource.

## Role

A `FAC` stores faction taxonomy and relationship metadata. Its purpose is not
visual representation but social/alignment structure within the module's data
model.

## Conceptual Shape

```text
FAC root
|
+-- faction list
+-- faction metadata
+-- inter-faction relationship data
+-- module-facing defaults or classification
```

## Current Code Coverage

- generic container support: `nwnrs-gff`
- resource identity: `nwnrs-restype`
- no dedicated lifted `FAC` schema crate yet

## Logical Edges

- Faction relationships are relational data, not one isolated object record.
- This schema is closer to a policy/configuration graph than to a world object.
- Consumer code should not confuse faction membership references with the
  faction-definition catalog itself.

## Related Chapters

- [UTC Creature Blueprints](./formats-utc.md)
- [GFF](./formats-gff.md)
