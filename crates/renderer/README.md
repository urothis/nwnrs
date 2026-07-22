# nwnrs-renderer

Shared, renderer-neutral scene assembly for nwnrs tools.

This crate owns the game-aware work required before a frontend can draw an NWN
resource:

- model, walkmesh, blueprint, and area scene assembly
- layered resource resolution and dependency provenance
- material, texture, TXI, MTR, and SHD resolution
- animation, attachment, collision, and environment data preparation
- packed GPU-buffer construction

It deliberately does not depend on VS Code, the DOM, WebGL, or a desktop window
system. Frontends consume the same scene documents and transport packets.

