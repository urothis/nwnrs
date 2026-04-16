# GFF Schema Families

This section sits one layer above [Generic File Format (GFF)](./formats-gff.md).

The `GFF` chapter explains the container mechanics:

- header
- struct table
- field table
- label table
- field-data indirection
- list indirection

This section explains the major schema families that ride on top of that
container. These are not all implemented today as dedicated Rust crates, so the
goal here is architectural accuracy rather than pretending the repo already has
field-by-field typed coverage for every file tag.

## The Core Distinction

At this layer, a resource is no longer just "some GFF document." Its top-level
file tag determines what kind of object graph the engine expects.

Conceptually:

```text
GFF container
|
+-- file tag "UTC "  -> creature blueprint schema
+-- file tag "UTI "  -> item blueprint schema
+-- file tag "ARE "  -> area static data schema
+-- file tag "GIT "  -> placed instance schema
+-- file tag "DLG "  -> dialogue graph schema
+-- file tag "JRL "  -> journal schema
+-- ...
```

That means reverse engineering has two separate obligations:

1. parse `GFF` correctly
2. know what the top-level schema means for a given file tag

## Current Coverage Boundary

Today the repo has:

- first-class generic `GFF` support in `nwnrs-gff`
- first-class lifted `GIT` support in `nwnrs-git`
- broad registry coverage for many GFF-backed resource kinds via `nwnrs-restype`

For most other GFF-backed resource families, the codebase currently stops at the
container layer. These chapters document those schema classes so the book can be
broader than the current implementation without misrepresenting current code
coverage.

## Reading Order

- [Blueprint Resources](./formats-gff-blueprints.md)
- [Area and Module Resources](./formats-gff-areas.md)
- [Dialogue, Journal, and Meta Resources](./formats-gff-meta.md)

From there, drill into the per-tag chapters for the individual schema classes.
