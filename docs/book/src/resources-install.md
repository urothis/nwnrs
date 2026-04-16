# Install Discovery

Docs:

- crate: `nwnrs-install`
- [crate docs](https://docs.rs/nwnrs-install/latest/nwnrs_install/)
- [README](https://github.com/urothis/nwnrs/blob/main/crates/resources/install/README.md)
- [source](https://github.com/urothis/nwnrs/blob/main/crates/resources/install/src/lib.rs)

## Scope

`nwnrs-install` is the orchestration layer that turns platform heuristics and NWN install conventions into a ready-to-query `ResMan`.

## Public Surface

### Constants and result vocabulary

- `DEFAULT_KEYFILES`
- `GFF_EXTENSIONS`
- `InstallError`
- `InstallResult`

### Discovery operations

- `find_nwnrs_root`
- `find_user_root`
- `resolve_language_root`

### Assembly operation

- `new_default_resman`

## Logical Edges

- Discovery order is explicit. For user roots: explicit override, `NWN_HOME`, then platform defaults. For install roots: explicit override, `NWN_ROOT`, Steam heuristics, then Beamdog heuristics.
- The crate is deterministic. It does not "search randomly until something looks plausible."
- `resolve_language_root` accepts both long-form names and known aliases, but it does not guess beyond the alias table.
- A missing `databuild.txt` on an otherwise plausible install root is treated as a warning rather than as a hard failure. That is a practical concession to development layouts.
- `new_default_resman` is where install semantics become actual layered lookup: language roots, KEY/BIF data, overrides, ERFs, and optional NWSync manifests.

## Why This Crate Exists

Every operational tool that works against a real install needs this knowledge. The point of the crate is to keep those heuristics and assembly rules in one place instead of scattering them across CLI commands and higher-level consumers.
