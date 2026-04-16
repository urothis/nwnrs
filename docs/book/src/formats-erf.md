# ERF Archives

Docs:

- [crate docs](https://docs.rs/nwnrs-erf/latest/nwnrs_erf/)
- [README](https://github.com/urothis/nwnrs/blob/main/crates/formats/erf/README.md)
- [types.rs](https://github.com/urothis/nwnrs/blob/main/crates/formats/erf/src/types.rs)
- [io.rs](https://github.com/urothis/nwnrs/blob/main/crates/formats/erf/src/io.rs)

`ERF` is the archive family for `ERF`, `MOD`, `HAK`, and `NWM`.

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
- build year/day
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

## Logical Edges

- Archive membership is not the same thing as resource precedence.
- The outer filename is not the archive's semantic identity.
- Stored entry order is preserved.
- `E1` per-entry compression metadata is physical-storage metadata, not content
  semantics.
- Resource-list padding is preserved on write because stable reconstruction
  sometimes requires keeping seemingly-unimportant layout details.

## Why This Crate Exists

`ERF` is both:

- a physical archive format
- a logical resource container

This crate models both sides explicitly without conflating them with global
lookup policy, which belongs in `nwnrs-resman`.
