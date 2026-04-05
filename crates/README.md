# Crates

This directory contains all the workspace crates that provide the core functionality for NWN resource handling.

## Workspace Map

| Crate | Role |
| --- | --- |
| `nwnrs-checksums` | SHA-1 and MD5 helpers used by archive and manifest formats. |
| `nwnrs-compressedbuf` | Reads and writes the EXO compressed-buffer wrapper used by several NWN formats. |
| `nwnrs-core` | Shared language, gender, and string-reference types. |
| `nwnrs-erf` | Reads and writes ERF-family archives: `ERF`, `MOD`, `HAK`, and `NWM`. |
| `nwnrs-exo` | EXO-level constants and compression markers shared by container formats. |
| `nwnrs-game` | Finds NWN installations and assembles a default layered resource manager. |
| `nwnrs-gff` | Reads and writes typed `GFF V3.2` documents. |
| `nwnrs-gffjson` | Converts `GFF` documents to and from a stable JSON representation. |
| `nwnrs-key` | Reads `KEY` indexes, opens `BIF` payloads, and writes KEY/BIF sets. |
| `nwnrs-lru` | Small weighted LRU cache used by higher-level crates. |
| `nwnrs-masterlist` | Async client for the Beamdog masterlist API. |
| `nwnrs-nwsync` | Reads and writes manifest files used by NWSync repositories. |
| `nwnrs-resdir` | Exposes a directory tree as a `ResContainer`. |
| `nwnrs-resfile` | Exposes a single on-disk file as a one-entry `ResContainer`. |
| `nwnrs-resman` | Core resource abstraction: `Res`, `ResContainer`, and `ResMan`. |
| `nwnrs-resmemfile` | Exposes an in-memory buffer as a one-entry `ResContainer`. |
| `nwnrs-resnwsync` | Opens NWSync repositories and exposes manifests as `ResContainer`s. |
| `nwnrs-resref` | Parses and formats NWN resource references. |
| `nwnrs-restype` | Registry of numeric NWN resource types and file extensions. |
| `nwnrs-ssf` | Reads and writes soundset (`SSF`) files. |
| `nwnrs-streamext` | Size-prefixed stream helpers used by binary codecs. |
| `nwnrs-tlk` | Reads, writes, and queries dialog table (`TLK`) files. |
| `nwnrs-twoda` | Reads and writes `2DA V2.0` tables. |
| `nwnrs-util` | Shared encoding, endian, IO, and expectation helpers. |

## Architectural Model

The repository is intentionally split by responsibility rather than by application feature.

1. Identity and primitives — `nwnrs-core`, `nwnrs-restype`, `nwnrs-resref`, `nwnrs-checksums`, `nwnrs-util`, and `nwnrs-streamext` define the small reusable types that every higher layer depends on.
2. Resource backends — `nwnrs-erf`, `nwnrs-key`, `nwnrs-resdir`, `nwnrs-resfile`, `nwnrs-resmemfile`, and `nwnrs-resnwsync` translate specific storage layouts into a common container interface.
3. Format codecs — `nwnrs-gff`, `nwnrs-gffjson`, `nwnrs-twoda`, `nwnrs-tlk`, `nwnrs-ssf`, `nwnrs-nwsync`, and `nwnrs-compressedbuf` focus on decoding and encoding individual file formats.
4. Composition and tooling — `nwnrs-resman` resolves resources across multiple containers. `nwnrs-game` chooses a conventional load order for a real installation.

## Core Resource Model and Container Layering

- [`resman/src/types.rs`](./resman/src/types.rs)
- [`resman/src/manager.rs`](./resman/src/manager.rs)
- [`game/src/builder.rs`](./game/src/builder.rs)
- [`game/src/discovery.rs`](./game/src/discovery.rs)

## Format Implementations

- [`gff/src/io.rs`](./gff/src/io.rs)
- [`gff/src/types.rs`](./gff/src/types.rs)
- [`gffjson/src/encode.rs`](./gffjson/src/encode.rs)
- [`gffjson/src/decode.rs`](./gffjson/src/decode.rs)
- [`twoda/src/io.rs`](./twoda/src/io.rs)
- [`tlk/src/io.rs`](./tlk/src/io.rs)
- [`ssf/src/io.rs`](./ssf/src/io.rs)
- [`nwsync/src/io.rs`](./nwsync/src/io.rs)

## Archive and Repository Containers

- [`erf/src/io.rs`](./erf/src/io.rs)
- [`key/src/io.rs`](./key/src/io.rs)
- [`resdir/src/read.rs`](./resdir/src/read.rs)
- [`resfile/src/read.rs`](./resfile/src/read.rs)
- [`resmemfile/src/read.rs`](./resmemfile/src/read.rs)
- [`resnwsync/src/io.rs`](./resnwsync/src/io.rs)

## External Service Reference

- Beamdog masterlist API base URL from [`masterlist/src/lib.rs`](./masterlist/src/lib.rs): <https://api.nwn.beamdog.net/v1>
