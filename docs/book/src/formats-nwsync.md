# NWSync Manifests

Docs:

- [crate docs](https://docs.rs/nwnrs-nwsync/latest/nwnrs_nwsync/)
- [README](https://github.com/urothis/nwnrs/blob/main/crates/formats/nwsync/README.md)
- [types.rs](https://github.com/urothis/nwnrs/blob/main/crates/formats/nwsync/src/types.rs)
- [io.rs](https://github.com/urothis/nwnrs/blob/main/crates/formats/nwsync/src/io.rs)

This crate models standalone `NWSync` manifest files. Repository access lives
elsewhere.

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

## Logical Edges

- A manifest file is not the repository.
- Primary entries own hashes and sizes. Mapping entries alias primaries.
- Sorting and deduplication during write are part of the manifest's storage
  rules, not generic container policy.
- Resource references are normalized for manifest output, but raw 16-byte slots
  are still modeled on read.

## Why This Crate Exists

`NWSync` manifests are a file-format problem distinct from repository access.
That distinction matters because:

- the manifest defines hash/resource mappings
- the repository defines shard layout and retrieval policy

This crate owns the first problem. `nwnrs-resnwsync` owns the second.
