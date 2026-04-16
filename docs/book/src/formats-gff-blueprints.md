# Blueprint Resources

These are the canonical template-style `GFF` resources that describe authored
game objects before placement or runtime instantiation.

## Common Members of This Family

- `UTC` creature blueprint
- `UTI` item blueprint
- `UTP` placeable blueprint
- `UTD` door blueprint
- `UTM` merchant/store blueprint
- `UTE` encounter blueprint
- `UTS` sound blueprint
- `UTT` trigger blueprint
- `UTW` waypoint blueprint

The registry also includes nearby related tags such as `UTG`, but the workspace
does not currently expose a dedicated typed schema layer for them.

## Per-Tag Chapters

Object and actor blueprints:

- [UTC Creature Blueprints](./formats-utc.md)
- [UTI Item Blueprints](./formats-uti.md)
- [UTP Placeable Blueprints](./formats-utp.md)
- [UTD Door Blueprints](./formats-utd.md)
- [UTM Store Blueprints](./formats-utm.md)

Encounter and interaction blueprints:

- [UTE Encounter Blueprints](./formats-ute.md)
- [UTS Sound Blueprints](./formats-uts.md)
- [UTT Trigger Blueprints](./formats-utt.md)
- [UTW Waypoint Blueprints](./formats-utw.md)

## Shared Structural Pattern

At a high level, blueprint resources tend to follow this shape:

```text
GFF root tagged as one blueprint kind
|
+-- identity fields        tag, resref links, localized name
+-- appearance/model refs  appearance ids, model/material hooks, portraits
+-- gameplay data          stats, flags, faction/classification, scripts
+-- inventory/equipment    optional embedded lists or references
+-- locals/vars            optional per-object authored state
+-- script hooks           event-entry script names
```

That is not one exact field list. It is the recurring architectural pattern.

## The Main Semantics

- A blueprint is a template, not a placed instance.
- A blueprint usually owns object-level default state.
- A placed instance layer such as `GIT` may refer back to a blueprint by
  `template_resref`.
- Embedded substructures are still usually ordinary `GFF` structs/lists rather
  than a different container type.

## Logical Edges

- Blueprint identity is not placement. Position, orientation, and polygon
  geometry belong elsewhere.
- A blueprint can still contain rich nested state such as inventories,
  descriptions, and scripts, so "template" does not mean "small."
- The same engine concept may exist in both blueprint and instantiated form.
  Reverse engineering should not conflate the two.
- Script hooks are structurally just resource references or strings until a
  higher layer interprets them as event bindings.

## Why This Family Matters

If the repo ever grows dedicated schema crates for common gameplay objects, this
is likely where several of them will land first. Blueprint schemas are the most
frequently reused GFF-backed object definitions in the ecosystem.
