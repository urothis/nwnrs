# nwnrs-key

`nwnrs-key` reads and writes KEY/BIF resource sets, which form the canonical
indexed storage layout for base-game content.

## Scope

- parse KEY files and their BIF index tables
- expose the result as a typed [`KeyTable`]
- implement `nwnrs-resman` container behavior for KEY/BIF-backed content
- write KEY/BIF output from typed archive state

The main entry points are [`read_key_table`], [`read_key_table_from_file`], and
[`write_key_and_bif`].

## Public Surface

- `KeyTable`
- `KeyBifEntry`
- `KeyBifContents`
- `KeyBifVersion`
- `VariableResource`
- `BifResolver`
- `ResId`
- `KeyError`
- `KeyResult`
- `read_key_table`
- `read_key_table_from_file`
- `write_key_and_bif`
- `write_key_table_archive`

## Core Model

- `ResId` is a packed `u32`
  - upper bits identify the owning BIF
  - lower bits identify the variable resource within that BIF
- `KeyTable` preserves:
  - version
  - label
  - build year and day
  - BIF handles
  - resource-to-id lookup
  - optional OID metadata
- `VariableResource` preserves:
  - packed id
  - I/O offset
  - stored size
  - compression type
  - uncompressed size

## KEY Layout

Header size: `64` bytes.

Known KEY versions:

- `V1`
- `E1`

Header shape:

```text
0x00  "KEY "
0x04  version
0x08  bif_count              u32
0x0C  key_count              u32
0x10  offset_to_file_table   u32
0x14  offset_to_key_table    u32
0x18  build_year             u32
0x1C  build_day              u32
0x20  remaining header / OID area
```

Body:

```text
+----------------------+
| KEY header           |
+----------------------+
| file table           | one row per BIF
+----------------------+
| filename strings     |
+----------------------+
| key table            | one row per resource
+----------------------+
```

## BIF Layout

Known BIF magic:

- `"BIFF"`

Known BIF versions:

- `V1`
- `E1`

Conceptually:

```text
+----------------------+
| BIF header           |
+----------------------+
| variable table       |
+----------------------+
| payload 0            |
+----------------------+
| payload 1            |
+----------------------+
| ...                  |
+----------------------+
```

Each variable-resource row records:

- packed resource id
- byte offset
- stored size
- resource type
- optional `E1` compression metadata

## Invariants

- resource references remain typed rather than stringly indexed
- the mapping from KEY entries to BIF-backed payload locations remains explicit
- the same typed value may be inspected structurally and used as a
  `ResContainer`
- KEY indexing and BIF payload storage are separate concepts
- `BifResolver` exists because the KEY file references BIFs by filename and the
  actual stream-opening policy belongs to the caller

## See also

- [`nwnrs-resman`](https://docs.rs/nwnrs-resman), which consumes `KeyTable` as a
  resource container
- [`nwnrs-install`](https://docs.rs/nwnrs-install), which uses KEY/BIF data to
  assemble a conventional install-backed resource stack

## Why This Crate Exists

If you only think in terms of "resource name to bytes," the KEY/BIF design
looks more complicated than it needs to be. If you think in terms of indexed
archival storage, it becomes clear:

- KEY is the index
- BIF is the payload store
- `ResId` is the join key

The crate makes that split explicit rather than hiding it behind opaque lookup
machinery.
