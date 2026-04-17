# nwnrs-ssf

Reader and writer for soundset (`SSF`) files.

## Scope

- parse fixed-layout SSF tables into typed slot entries
- preserve the association between each slot and its sound and string
  references
- write typed SSF data back to disk

## Public Surface

- `SsfRoot`
- `SsfEntry`
- `SsfError`
- `SsfResult`
- `read_ssf`
- `write_ssf`

## Core Model

- `SsfRoot` is an ordered vector of slots
- `SsfEntry` preserves:
  - `raw_resref`
  - decoded `resref`
  - `strref`

Each slot binds two different namespaces:

- a resource reference for audio
- a string reference for localized text

## Binary Layout

The crate models:

- magic: `"SSF "`
- version: `"V1.0"`
- fixed table offset: `40`
- fixed entry size: `20` bytes

Layout:

```text
0x00  "SSF "
0x04  "V1.0"
0x08  entry_count      u32
0x0C  table_offset     u32 == 40
0x10  padding          24 bytes of zero
0x28  entry_offsets    entry_count * u32
0x..  entry data       entry_count * 20 bytes
```

Entry payload:

```text
resref[16]
strref u32
```

Conceptually:

```text
+----------------------+
| fixed header         |
+----------------------+
| offset table         |
+----------------------+
| slot 0               |
+----------------------+
| slot 1               |
+----------------------+
| ...                  |
+----------------------+
```

## Invariants

- soundset slots remain positional and typed
- string references and resource references stay distinct fields
- slot position is semantic; reordering entries changes meaning
- raw resref bytes are preserved when only `strref` changes and the original
  encoded bytes still match the typed name

## Non-goals

- interpret gameplay meaning beyond the SSF slot structure itself
- resolve the referenced audio payloads or dialog strings

## Why This Crate Exists

`SSF` is a reminder that small file formats can still justify dedicated typed
models. The key fact is not "it is 20 bytes per entry." The key fact is that it
is a positional dispatch table whose meaning is destroyed if you flatten it into
"some list of audio references."
