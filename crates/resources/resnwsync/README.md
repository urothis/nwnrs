# nwnrs-resnwsync

Access to `NWSync` repositories as resource containers.

## Why This Crate Exists

`NWSync` repositories are SQLite-backed shard stores, not a format that `ResMan`
can consume directly. Without this crate, every tool that wants to query a
`NWSync` repository would need to open `SQLite` and implement shard resolution
itself. This crate wraps that complexity and exposes individual manifests as
standard `ResContainer` values that slot into any `ResMan` lookup chain.

## Scope

- open the SQLite-backed `NWSync` repository layout
- map manifest hashes to shard payloads
- expose individual manifests as `nwnrs-resman::ResContainer` values

Use [`open_nwsync`] to open a repository and [`new_resnwsync_manifest`] to
materialize a specific manifest as a container.

## Non-goals

- define the `NWSync` manifest file format itself
- act as a general-purpose network sync client

## See also

- [`nwnrs-nwsync`](https://docs.rs/nwnrs-nwsync), which defines the manifest
  file format
- [`nwnrs-resman`](https://docs.rs/nwnrs-resman), which consumes these
  manifests as layered resource containers
