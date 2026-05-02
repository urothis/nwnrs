# nwnrs-types

Typed support for `NWSync` manifests and repository-backed resource access.

## Scope

- parse standalone `NWSync` manifests into typed entries
- write typed manifests back to disk
- model manifest hashes, sizes, and resource-reference mappings directly
- expose repository-backed payload access and manifest loading for layered
  resource workflows

Start with `read_manifest`, `read_manifest_file`, `write_manifest`, and
`write_manifest_file`.

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
- repository support is exposed through:
  - `ResNWSyncRepository`
  - `ResNWSyncManifestContainer`
  - `open_resnwsync_repository`
  - `new_resnwsync_manifest`

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

Repository path derivation for payload data uses the hash-tree depth:

```text
data/sha1/aa/bb/<full_sha1>
```

for depth `2`.

## Invariants

- manifest membership is represented as typed resource-reference and digest
  mappings
- primary entries own hashes and sizes; mapping entries alias primaries
- the manifest file format and the repository shard layout are separate but
  related concerns
- sorting and deduplication during write are part of the manifest's storage
  rules, not generic container policy

## Manifest Versus Repository

This module owns both halves of the `NWSync` story:

- the manifest file format, which maps resources to content hashes
- the repository storage layer, which maps those hashes to payload bytes

That split matters operationally:

- manifests answer "which resource names point at which payloads?"
- repositories answer "where do I load the payload for this hash?"

The module keeps those roles separate in the type system so callers can work
with manifest data, repository data, or both without conflating them.

## See also

- [`crate::resman`], which consumes repository-backed manifest containers as
  resource containers
- [`crate::checksums`], which defines the typed SHA-1 digest vocabulary used by
  manifests and shard payloads

## Why This Crate Exists

`NWSync` needs both typed manifest structure and typed repository access. This
module keeps them separate enough to reason about, but close enough that users
can build archive and install workflows without another crate boundary.
