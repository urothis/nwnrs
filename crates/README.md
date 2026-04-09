# Crates

This directory contains all the workspace crates that provide the core functionality for NWN resource handling.

## Workspace Map

| Crate | Role |
| --- | --- |
| `nwnrs-checksums` | SHA-1 and MD5 helpers used by archive and manifest formats. |
| `nwnrs-compressedbuf` | Reads and writes the EXO compressed-buffer wrapper used by several NWN formats. |
| `nwnrs-core` | Shared language, gender, and string-reference types. |
| `nwnrs-dds` | Parses, decodes, writes, and encodes NWN-specific `DDS` texture payloads. |
| `nwnrs-erf` | Reads and writes ERF-family archives: `ERF`, `MOD`, `HAK`, and `NWM`. |
| `nwnrs-exo` | EXO-level constants and compression markers shared by container formats. |
| `nwnrs-game` | Finds NWN installations and assembles a default layered resource manager. |
| `nwnrs-gff` | Reads and writes typed `GFF V3.2` documents. |
| `nwnrs-key` | Reads `KEY` indexes, opens `BIF` payloads, and writes KEY/BIF sets. |
| `nwnrs-lru` | Small weighted LRU cache used by higher-level crates. |
| `nwnrs-masterlist` | Async client for the Beamdog masterlist API. |
| `nwnrs-model` | Reads and writes raw Neverwinter Nights model (`MDL`) payloads. |
| `nwnrs-nwscript` | Pure Rust NWScript frontend and compiler pipeline, including preprocessing, parsing, semantic analysis, optimization, and `NCS`/`NDB` support. |
| `nwnrs-nwsync` | Reads and writes manifest files used by NWSync repositories. |
| `nwnrs-plt` | Parses and writes typed Neverwinter Nights palette texture (`PLT`) payloads. |
| `nwnrs-resdir` | Exposes a directory tree as a `ResContainer`. |
| `nwnrs-resfile` | Exposes a single on-disk file as a one-entry `ResContainer`. |
| `nwnrs-resman` | Core resource abstraction: `Res`, `ResContainer`, and `ResMan`. |
| `nwnrs-resmemfile` | Exposes an in-memory buffer as a one-entry `ResContainer`. |
| `nwnrs-resnwsync` | Opens NWSync repositories and exposes manifests as `ResContainer`s. |
| `nwnrs-resref` | Parses and formats NWN resource references. |
| `nwnrs-restype` | Registry of numeric NWN resource types and file extensions. |
| `nwnrs-ssf` | Reads and writes soundset (`SSF`) files. |
| `nwnrs-streamext` | Size-prefixed stream helpers used by binary codecs. |
| `nwnrs-tga` | Parses, decodes, and writes typed `TGA` image payloads. |
| `nwnrs-tlk` | Reads, writes, and queries dialog table (`TLK`) files. |
| `nwnrs-twoda` | Reads and writes `2DA V2.0` tables. |
| `nwnrs-util` | Shared encoding, endian, IO, and expectation helpers. |

## Texture Status

The workspace now treats NWN texture formats as separate first-class crates instead of a shared umbrella texture crate.

- `nwnrs-tga` owns typed TGA parsing, RGBA decode, exact writing, and RGBA encode for authored image conversion
- `nwnrs-dds` owns NWN-specific DDS parsing, DXT decode, exact writing, and RGBA encode to NWN `dxt1`/`dxt5`
- `nwnrs-plt` owns typed PLT parsing and writing, preserving the NWN header and per-pixel `value` plus `layer_id` structure

`nwnrs-plt` currently stops at typed file ownership. Palette resolution and rendered-color output remain a separate future layer.

## Architectural Model

The repository is intentionally split by responsibility rather than by application feature.

1. Identity and primitives — `nwnrs-core`, `nwnrs-restype`, `nwnrs-resref`, `nwnrs-checksums`, `nwnrs-util`, and `nwnrs-streamext` define the small reusable types that every higher layer depends on.
2. Resource backends — `nwnrs-erf`, `nwnrs-key`, `nwnrs-resdir`, `nwnrs-resfile`, `nwnrs-resmemfile`, and `nwnrs-resnwsync` translate specific storage layouts into a common container interface.
3. Format codecs — `nwnrs-gff`, `nwnrs-twoda`, `nwnrs-tlk`, `nwnrs-ssf`, `nwnrs-model`, `nwnrs-tga`, `nwnrs-dds`, `nwnrs-plt`, `nwnrs-nwsync`, and `nwnrs-compressedbuf` focus on decoding and encoding individual file formats.
4. Language tooling — `nwnrs-nwscript` provides the NWScript compiler stack, from source resolution and preprocessing through parsing, semantic analysis, optimization, and code generation.
5. Composition and tooling — `nwnrs-resman` resolves resources across multiple containers. `nwnrs-game` chooses a conventional load order for a real installation.

## Core Resource Model and Container Layering

- [`resman/src/types.rs`](./resman/src/types.rs)
- [`resman/src/manager.rs`](./resman/src/manager.rs)
- [`game/src/builder.rs`](./game/src/builder.rs)
- [`game/src/discovery.rs`](./game/src/discovery.rs)

## Format Implementations

- [`gff/src/io.rs`](./gff/src/io.rs)
- [`gff/src/types.rs`](./gff/src/types.rs)
- [`twoda/src/io.rs`](./twoda/src/io.rs)
- [`tlk/src/io.rs`](./tlk/src/io.rs)
- [`ssf/src/io.rs`](./ssf/src/io.rs)
- [`model/src/io.rs`](./model/src/io.rs)
- [`tga/src/lib.rs`](./tga/src/lib.rs)
- [`dds/src/lib.rs`](./dds/src/lib.rs)
- [`plt/src/lib.rs`](./plt/src/lib.rs)
- [`nwsync/src/io.rs`](./nwsync/src/io.rs)

## NWScript Compiler Stack

- [`nwscript/src/source.rs`](./nwscript/src/source.rs)
- [`nwscript/src/preprocess.rs`](./nwscript/src/preprocess.rs)
- [`nwscript/src/lexer.rs`](./nwscript/src/lexer.rs)
- [`nwscript/src/parser.rs`](./nwscript/src/parser.rs)
- [`nwscript/src/sema.rs`](./nwscript/src/sema.rs)
- [`nwscript/src/ir.rs`](./nwscript/src/ir.rs)
- [`nwscript/src/codegen.rs`](./nwscript/src/codegen.rs)
- [`nwscript/src/ncs.rs`](./nwscript/src/ncs.rs)
- [`nwscript/src/ndb.rs`](./nwscript/src/ndb.rs)

## Archive and Repository Containers

- [`erf/src/io.rs`](./erf/src/io.rs)
- [`key/src/io.rs`](./key/src/io.rs)
- [`resdir/src/read.rs`](./resdir/src/read.rs)
- [`resfile/src/read.rs`](./resfile/src/read.rs)
- [`resmemfile/src/read.rs`](./resmemfile/src/read.rs)
- [`resnwsync/src/io.rs`](./resnwsync/src/io.rs)

## External Service Reference

- Beamdog masterlist API base URL from [`masterlist/src/lib.rs`](./masterlist/src/lib.rs): <https://api.nwn.beamdog.net/v1>
