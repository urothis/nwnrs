# nwnrs-mtr

Typed parser and writer for Neverwinter Nights material (`MTR`) payloads.

## Scope

- parse text-based NWN material descriptors
- expose texture-layer bindings and shader-relevant settings through typed data
- write the typed material representation back to text

## Invariants

- authored material properties remain explicit typed fields
- the crate models NWN material descriptors, not a renderer-specific material
  object

## Non-goals

- resolve the final texture assets referenced by a material
- implement runtime shading or renderer integration
