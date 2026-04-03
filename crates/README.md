# Crates

This directory contains all the workspace crates that provide the core functionality for NWN resource handling.

## Workspace Map

| Crate | Role |
| --- | --- |
| `nwn-checksums` | SHA-1 and MD5 helpers used by archive and manifest formats. |
| `nwn-compressedbuf` | Reads and writes the EXO compressed-buffer wrapper used by several NWN formats. |
| `nwn-core` | Shared language, gender, and string-reference types. |
| `nwn-erf` | Reads and writes ERF-family archives: `ERF`, `MOD`, `HAK`, and `NWM`. |
| `nwn-exo` | EXO-level constants and compression markers shared by container formats. |
| `nwn-game` | Finds NWN installations and assembles a default layered resource manager. |
| `nwn-gff` | Reads and writes typed `GFF V3.2` documents. |
| `nwn-gffjson` | Converts `GFF` documents to and from a stable JSON representation. |
| `nwn-key` | Reads `KEY` indexes, opens `BIF` payloads, and writes KEY/BIF sets. |
| `nwn-lru` | Small weighted LRU cache used by higher-level crates. |
| `nwn-masterlist` | Async client for the Beamdog masterlist API. |
| `nwn-nwsync` | Reads and writes manifest files used by NWSync repositories. |
| `nwn-resdir` | Exposes a directory tree as a `ResContainer`. |
| `nwn-resfile` | Exposes a single on-disk file as a one-entry `ResContainer`. |
| `nwn-resman` | Core resource abstraction: `Res`, `ResContainer`, and `ResMan`. |
| `nwn-resmemfile` | Exposes an in-memory buffer as a one-entry `ResContainer`. |
| `nwn-resnwsync` | Opens NWSync repositories and exposes manifests as `ResContainer`s. |
| `nwn-resref` | Parses and formats NWN resource references. |
| `nwn-restype` | Registry of numeric NWN resource types and file extensions. |
| `nwn-ssf` | Reads and writes soundset (`SSF`) files. |
| `nwn-streamext` | Size-prefixed stream helpers used by binary codecs. |
| `nwn-tlk` | Reads, writes, and queries dialog table (`TLK`) files. |
| `nwn-twoda` | Reads and writes `2DA V2.0` tables. |
| `nwn-util` | Shared encoding, endian, IO, and expectation helpers. |

## Architectural Model

The repository is intentionally split by responsibility rather than by application feature.

1. Identity and primitives

`nwn-core`, `nwn-restype`, `nwn-resref`, `nwn-checksums`, `nwn-util`, and `nwn-streamext` define the small reusable types that every higher layer depends on.

2. Resource backends

`nwn-erf`, `nwn-key`, `nwn-resdir`, `nwn-resfile`, `nwn-resmemfile`, and `nwn-resnwsync` translate specific storage layouts into a common container interface.

3. Format codecs

`nwn-gff`, `nwn-gffjson`, `nwn-twoda`, `nwn-tlk`, `nwn-ssf`, `nwn-nwsync`, and `nwn-compressedbuf` focus on decoding and encoding individual file formats.

4. Composition and tooling

`nwn-resman` resolves resources across multiple containers. `nwn-game` chooses a conventional load order for a real installation.

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