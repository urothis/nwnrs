# Textures and Materials

This family covers raw texture payloads, palette-oriented texture encodings,
and sidecar material metadata.

Dedicated chapters:

- [TGA Textures](./formats-tga.md)
- [DDS Textures](./formats-dds.md)
- [PLT Layer Textures](./formats-plt.md)
- [TXI Sidecars](./formats-txi.md)
- [MTR Materials](./formats-mtr.md)

The important split here is between:

- canonical stored payloads such as `TGA`, `DDS`, and `PLT`
- sidecar or descriptor layers such as `TXI` and `MTR`
- derived views such as "decode to RGBA8" or "render PLT with a palette"

Those are not interchangeable representations. This part of the workspace keeps
that distinction explicit.
