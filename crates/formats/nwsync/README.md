# nwnrs-nwsync

Reader and writer for `NWSync` manifest files.

## Scope

- parse standalone `NWSync` manifests into typed entries
- write typed manifests back to disk
- model manifest hashes, sizes, and resource-reference mappings directly

Repository access and shard lookup live in `nwnrs-resnwsync`.

Start with [`read_manifest`], [`read_manifest_file`], [`write_manifest`], and
[`write_manifest_file`].

## Public Surface

- `MAGIC`
- `VERSION`
- `HASH_TREE_DEPTH`
- `Manifest`
- `ManifestEntry`
- `ManifestEntrySource`
- `ManifestError`
- `ManifestResult`
- `path_for_entry`
- `read_manifest`
- `read_manifest_file`
- `write_manifest`
- `write_manifest_file`

## Core Model

- `ManifestEntry` preserves:
  - `sha1`
  - `size`
  - `resref`
  - `raw_resref`
  - `source`
- `ManifestEntrySource`
  - `Primary`
  - `Mapping { target }`
- `Manifest` preserves:
  - manifest version
  - hash-tree depth
  - ordered entries

## Binary Layout

Magic: `"NSYM"`

Header:

```text
0x00  magic          [4] == "NSYM"
0x04  version        u32
0x08  entry_count    u32   primary entries
0x0C  mapping_count  u32   alias entries
```

Body:

```text
+----------------------+
| manifest header      |
+----------------------+
| primary entry table  |
+----------------------+
| mapping table        |
+----------------------+
```

Primary entry row:

```text
sha1[20]
size          u32
raw_resref[16]
res_type      u16
```

Mapping row:

```text
target_primary_index  u32
raw_resref[16]
res_type              u16
```

Repository path derivation for payload data is separate and uses the hash-tree
depth:

```text
data/sha1/aa/bb/<full_sha1>
```

for depth `2`.

## Invariants

- manifest membership is represented as typed resource-reference and digest
  mappings
- primary entries own hashes and sizes; mapping entries alias primaries
- manifest parsing stays at the file-format layer rather than assuming a
  particular repository layout
- sorting and deduplication during write are part of the manifest's storage
  rules, not generic container policy

## Non-goals

- open or manage an on-disk `NWSync` repository
- fetch shard payloads or enforce repository precedence

## See also

- [`nwnrs-resnwsync`](https://docs.rs/nwnrs-resnwsync), which opens the
  repository layout and exposes manifests as resource containers

## Why This Crate Exists

`NWSync` manifests are a file-format problem distinct from repository access.
That distinction matters because:

- the manifest defines hash and resource mappings
- the repository defines shard layout and retrieval policy

This crate owns the first problem. `nwnrs-resnwsync` owns the second.
