# nwnrs-nwpkg

`nwnrs-nwpkg` defines the typed `nwproject.toml` and `nwproject.lock` behavior
used by the workspace packaging tools.

It owns:

- the supported `nwproject` kind taxonomy
- serde-backed TOML manifest read/write behavior
- JSON lockfile read/write behavior
- repack optimization helpers such as source snapshots and original-file reuse

The crate depends on `nwnrs-types` for NWN-specific archive/resource vocabulary
such as `ResRef`, ERF versions, KEY/BIF versions, checksum helpers, and
compression algorithms.
