# nwnrs-types

`nwnrs-types` defines the digest primitives used throughout the workspace.

## Scope

- provide typed SHA-1, SHA-256, and MD5 wrappers
- expose parse and formatting routines for those digest types
- centralize digest handling so higher-level crates do not reimplement it

The principal entry points are `sha1_digest`, `parse_sha1_digest`,
`sha256_digest`, and `md5_digest`.

## Public Surface

### Digest types

- `Sha1Digest`
- `Sha256Digest`
- `Md5Digest`
- `ParseSha1DigestError`

### Constants

- `SHA1_HEX_LEN`
- `SHA256_HEX_LEN`
- `EMPTY_SHA1_DIGEST`

### Operations

- `sha1_digest`
- `parse_sha1_digest`
- `sha256_digest`
- `md5_digest`

## Logical Edges

- `Sha1Digest` is the typed SHA-1 boundary used by the resource and sync layers
- `parse_sha1_digest` accepts the hex representation and normalizes it into the
  typed digest value
- the crate is about typed handling and formatting of digests; it is not a
  general cryptography layer and does not define trust or policy
- `EMPTY_SHA1_DIGEST` exists as a concrete sentinel where a digest slot is
  structurally required even when no meaningful hash is known

## See also

- [`crate::nwsync`], which uses SHA-1 digests for manifests and repository
  payload identity

## Why This Crate Exists

This crate prevents digest handling from degrading into ad hoc strings and byte
arrays in higher layers, especially in `ResMan`, `NWSync`, and archive-related
code.
