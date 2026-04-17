# nwnrs-checksums

`nwnrs-checksums` defines the digest primitives used throughout the workspace.

## Scope

- provide typed SHA-1 and MD5 wrappers
- expose parse and formatting routines for those digest types
- centralize digest handling so higher-level crates do not reimplement it

The principal entry points are [`secure_hash`], [`parse_secure_hash`], and
[`md5_digest`].

## Public Surface

### Digest types

- `SecureHash`
- `Md5Digest`
- `ParseSecureHashError`

### Constants

- `SECURE_HASH_HEX_LEN`
- `EMPTY_SECURE_HASH`

### Operations

- `secure_hash`
- `parse_secure_hash`
- `md5_digest`

## Logical Edges

- `SecureHash` is the typed SHA-1 boundary used by the resource and sync layers
- `parse_secure_hash` accepts the hex representation and normalizes it into the
  typed digest value
- the crate is about typed handling and formatting of digests; it is not a
  general cryptography layer and does not define trust or policy
- `EMPTY_SECURE_HASH` exists as a concrete sentinel where a digest slot is
  structurally required even when no meaningful hash is known

## Non-goals

- provide a general cryptography toolkit
- define repository or asset policy built on top of those digests

## See also

- [`nwnrs-nwsync`](https://docs.rs/nwnrs-nwsync), which uses SHA-1 digests to
  identify manifest entries
- [`nwnrs-resnwsync`](https://docs.rs/nwnrs-resnwsync), which maps manifest
  hashes to shard payloads in the repository layout

## Why This Crate Exists

This crate prevents digest handling from degrading into ad hoc strings and byte
arrays in higher layers, especially in `ResMan`, `NWSync`, and archive-related
code.
