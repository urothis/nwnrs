# Scene System

`nwnrs_types::scene` provides shared, renderer-neutral scene assembly for
nwnrs tools.

This module owns the game-aware work required before a frontend can draw or
inspect an NWN resource:

- model, walkmesh, blueprint, and area scene assembly
- layered resource resolution and dependency provenance
- material, texture, TXI, MTR, and SHD resolution
- animation, attachment, collision, and environment data preparation
- lazy, type-aware area-object inspection with lossless GFF source layers,
  blueprint provenance, typed resource links, and cached 2DA lookups
- packed GPU-buffer construction

It deliberately does not depend on VS Code, the DOM, WebGL, or a desktop
window system. Frontends consume the same scene documents and transport
packets. Enable the `scene` feature when using `nwnrs-types` without its
default features.
