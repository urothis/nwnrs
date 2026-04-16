# Checksums

Docs:

- crate: `nwnrs-checksums`
- [crate docs](https://docs.rs/nwnrs-checksums/latest/nwnrs_checksums/)
- [README](https://github.com/urothis/nwnrs/blob/main/crates/foundation/checksums/README.md)
- [source](https://github.com/urothis/nwnrs/blob/main/crates/foundation/checksums/src/lib.rs)

## Scope

`nwnrs-checksums` provides typed digest vocabulary for the parts of the workspace that need content identity rather than just raw bytes.

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

- `SecureHash` is the typed SHA-1 boundary used by the resource and sync layers.
- `parse_secure_hash` accepts the hex representation and normalizes it into the typed digest value.
- The crate is about typed handling and formatting of digests. It is not a general cryptography layer and does not define trust or policy.
- `EMPTY_SECURE_HASH` exists as a concrete sentinel value in places where a digest slot is structurally required even when no meaningful hash is known.

## Why This Crate Exists

This crate prevents digest handling from degrading into ad hoc strings and byte arrays in the higher layers, especially in `ResMan`, `NWSync`, and archive-related code.
