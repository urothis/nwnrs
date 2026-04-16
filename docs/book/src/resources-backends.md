# Concrete Resource Backends

These crates are the concrete storage forms that implement the abstract `ResContainer` model.

## `nwnrs-resdir`

Docs:

- [crate docs](https://docs.rs/nwnrs-resdir/latest/nwnrs_resdir/)
- [README](https://github.com/urothis/nwnrs/blob/main/crates/resources/resdir/README.md)

Public surface:

- `ResDir`
- `ResDirError`
- `ResDirResult`
- `read_resdir`

Logical edges:

- This crate maps a directory tree into typed resource references.
- It does not define precedence between multiple directories. That is a `ResMan` concern.

## `nwnrs-resfile`

Docs:

- [crate docs](https://docs.rs/nwnrs-resfile/latest/nwnrs_resfile/)
- [README](https://github.com/urothis/nwnrs/blob/main/crates/resources/resfile/README.md)

Public surface:

- `ResFile`
- `ResFileError`
- `ResFileResult`
- `read_resfile`
- `read_resfile_as`

Logical edges:

- This crate wraps one file as one resource entry.
- It is the minimal bridge from "I have this one file" into the `ResContainer` world.

## `nwnrs-resmemfile`

Docs:

- [crate docs](https://docs.rs/nwnrs-resmemfile/latest/nwnrs_resmemfile/)
- [README](https://github.com/urothis/nwnrs/blob/main/crates/resources/resmemfile/README.md)

Public surface:

- `ResMemFile`
- `ResMemFileError`
- `ResMemFileResult`
- `read_resmemfile`
- `read_resmemfile_arc`

Logical edges:

- This crate exists for synthetic, downloaded, or otherwise non-filesystem-backed payloads.
- It lets higher layers treat in-memory content like any other resource container.

## `nwnrs-resnwsync`

Docs:

- [crate docs](https://docs.rs/nwnrs-resnwsync/latest/nwnrs_resnwsync/)
- [README](https://github.com/urothis/nwnrs/blob/main/crates/resources/resnwsync/README.md)

Public surface:

- `ManifestSha1`
- `ResRefSha1`
- `NWSync`
- `ResNWSyncManifest`
- `ResNWSyncError`
- `ResNWSyncResult`
- `NWSYNC_COMPRESSED_BUF_MAGIC_STR`
- `new_resnwsync_manifest`
- `nwsync_compressed_buf_magic`
- `open_nwsync`
- `open_or_create_nwsync`

Logical edges:

- This crate is the repository-layout side of `NWSync`, not the manifest file-format side.
- It maps manifests and shard storage into resource-container semantics.
- `ManifestSha1` and `ResRefSha1` make repository identity explicit at the type level.

## Why These Crates Exist

The point of the backend layer is to let the rest of the workspace care about lookup semantics instead of storage-specific mechanics. Once a backend can behave like a `ResContainer`, higher layers can compose it with the rest of the system.
