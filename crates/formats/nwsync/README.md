# nwnrs-nwsync

Reader and writer for NWSync manifest files.

## Scope

- parse standalone NWSync manifests into typed entries
- write typed manifests back to disk
- model manifest hashes, sizes, and resource-reference mappings directly

Repository access and shard lookup live in `nwnrs-resnwsync`.

Start with [`read_manifest`], [`read_manifest_file`], [`write_manifest`], and
[`write_manifest_file`].

## Invariants

- manifest membership is represented as typed resource-reference and digest
  mappings
- manifest parsing stays at the file-format layer rather than assuming a
  particular repository layout

## Non-goals

- open or manage an on-disk NWSync repository
- fetch shard payloads or enforce repository precedence

## See also

- [`nwnrs-resnwsync`](https://docs.rs/nwnrs-resnwsync), which opens the
  repository layout and exposes manifests as resource containers
