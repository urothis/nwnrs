# `nwn_rs_types`

Rust workspace for reading, writing, inspecting, and composing Neverwinter Nights resource data.

This repository is organized as a layered toolkit:

- low-level binary and text codecs for NWN file formats such as `GFF`, `2DA`, `TLK`, `SSF`, `ERF`, `KEY/BIF`, and `NWSync`
- resource identity, type, checksum, encoding, and stream utilities
- container adapters that expose archives, directories, single files, in-memory buffers, and NWSync manifests through a shared resource-manager abstraction
- a high-level game crate for installation discovery and default resource loading
- a CLI for inspection, packing, unpacking, and selected NWSync workflows

## What This Workspace Does

The codebase is designed around one practical question: given an NWN installation or archive, how do you identify resources, open them reliably, and transform them into forms that are easier to inspect or rebuild?

At a high level:

- `nwn-cli` exposes the main workflows from the terminal
- `nwn-resref`, `nwn-restype`, and `nwn-core` define the shared identity vocabulary
- `nwn-resman` defines a common `Res`/`ResContainer` model and a layered `ResMan`
- container crates such as `nwn-erf`, `nwn-key`, `nwn-resdir`, `nwn-resfile`, `nwn-resmemfile`, and `nwn-resnwsync` project different storage backends into that shared model
- format crates such as `nwn-gff`, `nwn-gffjson`, `nwn-twoda`, `nwn-tlk`, `nwn-ssf`, and `nwn-nwsync` provide typed parsers and writers
- `nwn-game` composes those pieces into a default game-facing resource-loading stack

## Supported Workflows

The workspace already supports:

- inspecting ERF, KEY, GFF, 2DA, TLK, and SSF files from the CLI
- unpacking ERF archives and KEY/BIF sets into directory form
- converting GFF resources into JSON and packing them back into binary form
- writing 2DA text back into binary-compatible output
- opening NWSync repositories and printing manifest contents
- building a layered `ResMan` from game roots, override directories, ERFs, and NWSync manifests

## Sources

Repository structure and dependency graph:

- [`Cargo.toml`](./Cargo.toml)
- [CLI](./cli/README.md)
- [Crates](./crates/README.md)
