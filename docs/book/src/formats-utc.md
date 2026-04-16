# UTC Creature Blueprints

Registry identity:

- extension: `utc`
- resource type: `2027`
- top-level GFF tag: `"UTC "`

`UTC` is the canonical creature blueprint resource.

## Role

A `UTC` describes one authored creature template before placement. It is the
object that later gets instantiated into an area, often through a `GIT`
reference or other spawning mechanism.

## Conceptual Shape

```text
UTC root
|
+-- identity            tag, name, template identity
+-- classification      race, class, faction, challenge-style metadata
+-- appearance          appearance rows, portrait/model hooks, animation-facing ids
+-- stats               attributes, saves, combat-facing defaults
+-- scripts             event hooks
+-- inventory/equipment nested lists and owned items
+-- locals/state        authored default object state
```

## Why It Is Separate From `GIT`

- `UTC` is the template
- `GIT` is the placed instance

A `GIT` creature entry may point at a `template_resref`, but it also owns
placement-specific state such as transform and local overrides. Those are not
the same layer.

## Current Code Coverage

In the current workspace:

- generic container support exists through `nwnrs-gff`
- resource-type identity exists through `nwnrs-restype`
- install/resource lookup exists through `nwnrs-resman`
- there is not yet a dedicated lifted `UTC` schema crate

## Logical Edges

- A creature blueprint is not character-save state. That is a different schema
  problem, closer to `BIC`.
- A creature blueprint can embed owned substructures such as inventory, so
  "template" does not imply small or flat.
- Appearance-related fields usually reference other data layers such as `2DA`,
  `MDL`, `PLT`, `TXI`, `MTR`, and `TLK`; the `UTC` itself does not subsume those
  formats.

## Related Chapters

- [GFF](./formats-gff.md)
- [GIT Area Instances](./formats-git.md)
- [BIC Character Resources](./formats-bic.md)
- [MDL Models](./formats-mdl.md)
