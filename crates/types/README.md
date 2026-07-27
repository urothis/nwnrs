# nwnrs-types

`nwnrs-types` is the monolithic typed Neverwinter Nights library crate in this
workspace. It exposes the resource system, format parsers and writers, and
texture/model tooling under one public surface.

This crate is for consumers who want one dependency and stable module entry
points such as `nwnrs_types::gff` and `nwnrs_types::resman`.

## What This Crate Contains

`nwnrs-types` groups its public modules by problem domain:

- foundations: [`crate::io`], [`crate::encoding`], [`crate::localization`],
  [`crate::checksums`], [`crate::lru`], and [`crate::streamext`]
- resource system: [`crate::resman`], [`crate::erf`], [`crate::key`], and
  [`crate::nwsync`]
- file formats: [`crate::gff`], [`crate::twoda`], [`crate::tlk`],
  [`crate::ssf`], [`crate::tga`], [`crate::dds`], [`crate::plt`],
  [`crate::txi`], [`crate::mtr`], [`crate::mdl`], and [`crate::set`]
- cross-format tooling: [`crate::scene`] for resource-aware scene assembly,
  inspection, dependency provenance, and packed frontend transport
- service/API surface: [`crate::masterlist`]

The important architectural rule is that the root page is the entry map, while
the module pages hold the detailed semantics and examples for each subsystem.

## How to Navigate the Crate

### Foundations

- [`crate::io`]: exact binary reads, invariant helpers, and endian conversion
- [`crate::encoding`]: NWN text-encoding policy and byte/string conversion
- [`crate::localization`]: `Language`, `Gender`, `StrRef`, and normalization
- [`crate::checksums`]: typed SHA-1 and MD5 handling
- [`crate::lru`]: weighted cache primitive used by higher layers
- [`crate::streamext`]: small framing helpers for size-prefixed streams

### Resource System

- [`crate::resman`]: resource identity, resource-type registry, containers, and
  precedence-based lookup
- [`crate::erf`]: ERF-family archives including `ERF`, `MOD`, `HAK`, and `NWM`
- [`crate::key`]: KEY/BIF indexed archive storage
- [`crate::nwsync`]: `NWSync` manifests and repository-backed resource access

### File Formats

- [`crate::gff`]: `GFF V3.2` plus lifted `GIT` area-instance support
- [`crate::twoda`]: `2DA V2.0` tables
- [`crate::tlk`]: dialog tables and layered male/female lookup
- [`crate::ssf`]: soundset tables
- [`crate::tga`], [`crate::dds`], [`crate::plt`], [`crate::txi`], and
  [`crate::mtr`]: texture, sidecar, and material formats
- [`crate::mdl`]: model parsing, transformation, lowering, and composition
- [`crate::set`]: typed tileset catalogs

### Service/API Surface

- [`crate::masterlist`]: typed async client for the Beamdog masterlist API

### Cross-format Tooling

- [`crate::scene`]: reusable model, blueprint, area, and module scene assembly;
  authored-object inspection; dependency provenance; and compact frontend
  packets

## Start Here If…

- you want to read or write `GFF`-backed gameplay data:
  start with [`crate::gff`]
- you want to load resources from an install, overrides, KEY/BIF sets, ERFs, or
  `NWSync`:
  start with [`crate::resman`] and [`crate::install`]
- you want to inspect or rewrite archives:
  start with [`crate::erf`], [`crate::key`], and [`crate::nwsync`]
- you want to work with models, materials, or textures:
  start with [`crate::mdl`], [`crate::mtr`], [`crate::txi`], [`crate::tga`],
  [`crate::dds`], and [`crate::plt`]
- you want to assemble complete areas, inspect effective object data, or feed a
  rendering frontend:
  start with [`crate::scene`]
- you want to compile or analyze `NWScript`:
  use the sibling `nwnrs-nwscript` crate

Two coverage notes matter:

- resource type and resource reference vocabulary live in [`crate::resman`]
  rather than in separate `restype` or `resref` crates
- lifted `GIT` support lives in [`crate::gff::GitFile`] rather than in a
  separate `git` crate

## Example

```rust
use nwnrs_types::{
    gff::{GffRoot, GffValue},
    twoda::TwoDa,
};

let mut root = GffRoot::new("UTC ");
root.put_value("Tag", GffValue::CExoString("nw_chicken".to_string()))?;

let mut table = TwoDa::new();
table.set_columns(vec!["Label".to_string()])?;
table.replace_rows(
    vec![vec![Some("Chicken".to_string())]],
    vec!["0".to_string()],
)?;

assert_eq!(root.file_type, "UTC ");
assert_eq!(table.cell_or(0, "Label", ""), "Chicken");
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Documentation Policy

- the root README owns crate identity, navigation, and entry-point guidance
- module docs own detailed behavior, invariants, and format structure
- format-specific reference material should live with the owning module, not in
  the root overview
- prefer explicit module imports such as `nwnrs_types::gff` over `prelude::*`
  when documenting public usage

## Convenience Namespace

[`crate::prelude`] re-exports the public modules for callers that prefer a
single import boundary. The recommended documentation style for this crate is
still explicit module paths because they make capability discovery easier on
docs.rs.
