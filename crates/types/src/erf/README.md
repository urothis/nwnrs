# nwnrs-types

`nwnrs-types` reads and writes the ERF-family archive formats used by
Neverwinter Nights, including `ERF`, `MOD`, `HAK`, and `NWM`.

## Scope

- parse typed ERF archives and their resource tables
- expose archive contents as an `Erf` value
- implement `crate::resman` container behavior for archive-backed resolution
- write typed archive data back to disk

The principal entry points are `read_erf`, `read_erf_from_file`,
`read_erf_shared`, and `write_erf`.

## Public Surface

- `Erf`
- `ErfVersion`
- `ErfWriteOptions`
- `ErfError`
- `ErfResult`
- `read_erf`
- `read_erf_from_file`
- `read_erf_shared`
- `write_erf`
- `write_erf_archive`
- `write_erf_with_options`

## Core Model

`Erf` preserves:

- outer archive type and version
- archive filename
- build year and day
- top-level `str_ref`
- localized strings
- ordered entries
- optional enhanced-edition `oid`
- preserved padding between key and resource lists

The same typed value also implements `ResContainer`.

## Binary Layout

Header size: `160` bytes.

Known outer file types:

- `ERF `
- `MOD `
- `HAK `
- `NWM `

Known versions:

- `V1`
- `E1`

Header shape:

```text
file_type                [4]
file_version             [4]
loc_str_count            i32
loc_string_size          i32
entry_count              i32
offset_to_loc_str        i32
offset_to_key_list       i32
offset_to_resource_list  i32
build_year               i32
build_day                i32
str_ref                  i32
reserved or OID area     remaining header bytes
```

Archive body:

```text
+----------------------+
| 160-byte header      |
+----------------------+
| localized strings    |
+----------------------+
| key list             |
+----------------------+
| resource list        |
+----------------------+
| resource data area   |
+----------------------+
```

Entry-table sizes differ by version:

- key entry
  - `V1`: 24 bytes
  - `E1`: 44 bytes
- resource entry
  - `V1`: 8 bytes
  - `E1`: 16 bytes

`E1` adds optional compression metadata and archive OID support.

## Invariants

- resource references and archive membership are represented explicitly
- archive semantics are preserved independently of the container filename
- the same typed archive value can be inspected structurally and used as a
  `ResContainer`
- stored entry order and resource-list padding are preserved on write
- `E1` per-entry compression metadata is physical-storage metadata, not content
  semantics

## See also

- [`crate::resman`], which layers multiple containers in precedence order
- [`crate::key`], which models the KEY/BIF storage family

## Why This Crate Exists

`ERF` is both:

- a physical archive format
- a logical resource container

This crate models both sides explicitly without conflating them with global
lookup policy, which belongs in `crate::resman`.
