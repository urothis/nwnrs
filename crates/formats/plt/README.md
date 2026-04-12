# `nwnrs-plt`

Typed Neverwinter Nights `PLT` support.

## Scope

- parse the fixed PLT header
- expose per-pixel `value` and `layer_id` pairs through typed data
- preserve the typed header fields and pixel payload
- write PLT data back out through the typed representation

It also exposes the known palette layers as [`PltLayer`].

## Invariants

- the texture is represented as typed pixels rather than a precomposited image
- palette layer ids remain explicit instead of being collapsed into final colors
- writes are derived from the typed texture state

## Non-goals

- resolve final colors from game palette tables
- render PLT data into a final material or image by itself
