# Formats

The format layer is where reverse-engineered wire layouts become typed Rust
representations.

Read this part in two passes:

1. Use the family pages to orient yourself around the major problem domains.
2. Use the dedicated per-format chapters for concrete layouts, exported types,
   fidelity boundaries, and awkward edge cases.

The families are:

- [Core Data Formats](./formats-core.md)
- [Textures and Materials](./formats-textures.md)
- [Models and World Data](./formats-models.md)
- [Archives, Compression, and Sync](./formats-archives.md)

Most of the subtlety in this workspace is not just "how do I parse the bytes?"
but "what invariants are actually stable enough to model?" The dedicated format
chapters try to make those boundaries explicit.
