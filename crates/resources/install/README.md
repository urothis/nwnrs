# nwnrs-install

`nwnrs-install` is the installation-facing orchestration layer of the
workspace.

## Scope

- locate a Neverwinter Nights installation and user directory
- resolve the conventional language-root and KEY/BIF layout
- build a ready-to-query [`nwnrs_resman::ResMan`] from the discovered install
- add optional override directories, ERFs, and `NWSync` manifests to that
  layered resource view

Discovery is ordered and deterministic: explicit overrides win, then built-in
platform heuristics are consulted in a fixed order.

The primary entry points are [`find_nwnrs_root`], [`find_user_root`], and
[`new_default_resman`].

## Discovery Overrides

- `NWN_ROOT` overrides install-root discovery for [`find_nwnrs_root`]
- `NWN_HOME` overrides user-directory discovery for [`find_user_root`]

Both are lower priority than explicit function arguments such as `--root` and
`--userdirectory`, but higher priority than Steam, Beamdog, or platform-default
heuristics.

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

## Example

```rust,no_run
use std::path::PathBuf;

use nwnrs_install::{find_nwnrs_root, find_user_root, new_default_resman};
use nwnrs_resnwsync::ManifestSha1;

let root = find_nwnrs_root("")?;
let user = find_user_root("")?;

let keys: Vec<String> = Vec::new();
let additional_erfs: Vec<PathBuf> = Vec::new();
let additional_dirs: Vec<PathBuf> = Vec::new();
let additional_manifests: Vec<ManifestSha1> = Vec::new();

let _resman = new_default_resman(
    &root,
    &user,
    "english",
    256,
    true,
    true,
    &keys,
    &additional_erfs,
    &additional_dirs,
    &additional_manifests,
)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Logical Edges

- discovery order is explicit: for user roots, explicit override, `NWN_HOME`,
  then platform defaults; for install roots, explicit override, `NWN_ROOT`,
  Steam heuristics, then Beamdog heuristics
- the crate is deterministic; it does not search randomly until something looks
  plausible
- `resolve_language_root` accepts both long-form names and known aliases, but
  does not guess beyond the alias table
- a missing `databuild.txt` on an otherwise plausible install root is treated
  as a warning rather than a hard failure
- `new_default_resman` is where install semantics become actual layered lookup

## Non-goals

- parse individual resource formats directly
- define alternative game-install layouts unrelated to NWN conventions
- replace `nwnrs-resman` as the general resource-resolution abstraction

## Internal Structure

- `discovery`: installation, user-directory, and language-root discovery
- `builder`: construction of the conventional layered `ResMan`
- `keyload`: loading KEY/BIF resources into that manager
- `types`: shared error and platform vocabulary

## See also

- [`nwnrs-resman`](https://docs.rs/nwnrs-resman), the underlying layered
  resource manager
- [`nwnrs-key`](https://docs.rs/nwnrs-key), [`nwnrs-erf`](https://docs.rs/nwnrs-erf),
  and [`nwnrs-resnwsync`](https://docs.rs/nwnrs-resnwsync), which provide the
  concrete container backends assembled here
