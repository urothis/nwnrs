# Core Data Formats

This family covers the low-level data formats that a large portion of the rest
of the workspace depends on.

Dedicated chapters:

- [Generic File Format (GFF)](./formats-gff.md)
- [GFF Schema Families](./formats-gff-schemas.md)
- [2DA Tables](./formats-twoda.md)
- [Dialog Tables (TLK)](./formats-tlk.md)
- [SoundSets (SSF)](./formats-ssf.md)
- [EXO Wire Vocabulary](./formats-exo.md)

Why this family matters:

- `GFF` is the structural substrate for a large fraction of gameplay-facing
  resources.
- many engine resource kinds are schema variants over `GFF`, not separate
  container formats
- `2DA` and `TLK` are not complex in the "compiler" sense, but they are
  foundational because they anchor table-driven behavior and localized text.
- `SSF` is small but semantically positional.
- `EXO` defines shared compression markers that show up in container formats.

If you are trying to understand higher-level crates, start here before reading
resource composition, models, or install-backed lookup.
