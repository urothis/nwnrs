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
