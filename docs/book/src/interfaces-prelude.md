# Umbrella Crate

Docs:

- crate: `nwnrs`
- [crate docs](https://docs.rs/nwnrs/latest/nwnrs/)
- [README](https://github.com/urothis/nwnrs/blob/main/crates/meta/prelude/README.md)
- [source](https://github.com/urothis/nwnrs/blob/main/crates/meta/prelude/src/lib.rs)

## Scope

`nwnrs` is the umbrella crate. It is a re-export surface, not a second abstraction layer.

## Public Surface

### Root modules

- `checksums`
- `compressedbuf`
- `dds`
- `encoding`
- `erf`
- `exo`
- `gff`
- `git`
- `io`
- `key`
- `localization`
- `lru`
- `masterlist`
- `mdl`
- `mtr`
- `nwscript`
- `nwsync`
- `plt`
- `resdir`
- `resfile`
- `resman`
- `resmemfile`
- `resnwsync`
- `resref`
- `restype`
- `set`
- `ssf`
- `streamext`
- `tga`
- `tlk`
- `twoda`
- `txi`
- `install` on non-wasm targets

### Convenience namespace

- `prelude`

## Logical Edges

- The root modules mirror workspace crate boundaries intentionally.
- The umbrella crate is about import ergonomics, not about hiding the underlying architecture.
- If a consumer wants explicit imports, they should prefer root modules over the wildcard `prelude`.

## Why This Crate Exists

Most downstream users need one stable import boundary over the workspace. This crate provides that without flattening away the actual subsystem structure.
