# nwnrs-checksums

`nwnrs-checksums` defines the digest primitives used throughout the workspace.

## Scope

- provide typed SHA-1 and MD5 wrappers
- expose parse and formatting routines for those digest types
- centralize digest handling so higher-level crates do not reimplement it

The principal entry points are [`secure_hash`], [`parse_secure_hash`], and
[`md5_digest`].

## Non-goals

- provide a general cryptography toolkit
- define repository or asset policy built on top of those digests
