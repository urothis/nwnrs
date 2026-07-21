# nwnrs-nwpkg

`nwnrs-nwpkg` defines the typed `nwproject.toml` and `nwpkg.lock` behavior
used by the workspace packaging tools.

It owns:

- the supported `nwproject` kind taxonomy
- serde-backed TOML manifest read/write behavior
- local, transitive `include` package dependencies resolved relative to the
  manifest that declares them
- `nwpkg.lock` read/write behavior with SHA-256 source snapshots
- repack optimization helpers such as exact original-file reuse
- project-wide nwnrs event collection and deterministic dispatcher generation

The crate depends on `nwnrs-types` for NWN-specific archive/resource vocabulary
such as `ResRef`, ERF versions, KEY/BIF versions, checksum helpers, and
compression algorithms.

An include library is an `nwproject` with `kind = "include"`. A consuming
project declares it by local path:

```toml
[dependencies]
nwnrs = { path = "../include/nwnrs" }
```

The resolver rejects missing or non-include packages, dependency cycles,
source roots outside their package, and case-insensitive `.nss` filename
collisions. Git dependency resolution is intentionally deferred; local path
dependencies do not require network access or a dependency lock entry.

For module projects, packing always generates and compiles
`_nwnrs_onload.nss` in memory. Functions marked with
`#[nwnrs::events(module_load)]` and native event phase handlers such as
`#[nwnrs::events(associate_add_before)]` are gathered across the module source
root.
Each handler must have the exact `void Handler(json event)` signature. The
project preprocessor passes every module source to the checked-in
`nwnrs::__build_event_dispatcher!` procedural macro from
`nwnrs_macros.nss`. That NSS macro discovers and validates registrations,
constructs repeated include and handler bindings, and uses `quote!` to return
the complete dispatcher token stream. Rust only enumerates source files,
materializes the returned virtual source, and compiles it. Handler source files
are included once, the current event is fetched and parsed once, and only the
handlers registered for its semantic name and phase receive the same immutable
JSON snapshot, in deterministic name order. With no handlers the generated
script is simply an empty `void main() {}`. The attribute and generated source
are compiler inputs only and never appear in NCS bytecode or the source tree.
