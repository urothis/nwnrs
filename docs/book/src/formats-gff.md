# Generic File Format (GFF)

Docs:

- [crate docs](https://docs.rs/nwnrs-gff/latest/nwnrs_gff/)
- [README](https://github.com/urothis/nwnrs/blob/main/crates/formats/gff/README.md)
- [types.rs](https://github.com/urothis/nwnrs/blob/main/crates/formats/gff/src/types.rs)
- [io.rs](https://github.com/urothis/nwnrs/blob/main/crates/formats/gff/src/io.rs)

`GFF` is the typed structural substrate behind a large fraction of NWN data:
creatures, items, areas, stores, dialogs, and many more file families reduce
to "a typed tree over GFF."

## Public Surface

- `GffRoot`
- `GffStruct`
- `GffField`
- `GffFieldKind`
- `GffValue`
- `GffCExoLocString`
- `GffError`
- `GffResult`
- `read_gff_root`
- `write_gff_root`
- `merge_root_preserving_provenance`

## Core Model

- `GffRoot` carries the outer file tag, version, root struct, and optional
  source provenance.
- `GffStruct` is an ordered labeled field map keyed by unique labels.
- `GffField` separates field metadata from `GffValue`.
- `GffValue` does not collapse field kinds into one lossy generic scalar type.
  A `Dword`, `Int`, and `Float` are distinct even if all fit in 32 bits.
- `GffCExoLocString` preserves both the top-level `str_ref` and the explicit
  localized override entries.

## Binary Layout

The crate models `GFF V3.2`.

```text
0x00  file_type[4]          e.g. "UTC ", "ARE ", "GIT "
0x04  file_version[4]       "V3.2"
0x08  struct_offset         u32
0x0C  struct_count          u32
0x10  field_offset          u32
0x14  field_count           u32
0x18  label_offset          u32
0x1C  label_count           u32
0x20  field_data_offset     u32
0x24  field_data_size       u32
0x28  field_indices_offset  u32
0x2C  field_indices_size    u32
0x30  list_indices_offset   u32
0x34  list_indices_size     u32

total header size: 56 bytes
```

After the header:

```text
+----------------------+
| struct table         | struct_count * 12
+----------------------+
| field table          | field_count * 12
+----------------------+
| label table          | label_count * 16
+----------------------+
| field data blob      | variable
+----------------------+
| field index array    | i32[]
+----------------------+
| list index array     | i32[]
+----------------------+
```

Struct table entry:

```text
i32 id
i32 data_or_offset
i32 field_count
```

Field table entry:

```text
u32 field_kind
i32 label_index
i32 data_or_offset
```

Important indirections:

- if a struct has `field_count == 0`, it has no fields
- if a struct has `field_count == 1`, `data_or_offset` is the direct field index
- if a struct has `field_count > 1`, `data_or_offset` is a byte offset into the
  field-index array
- list fields point into the list-index array
- complex field kinds point into the field-data blob

## Field-Kind Semantics

Inline 32-bit payloads:

- `Byte`
- `Char`
- `Word`
- `Short`
- `Dword`
- `Int`
- `Float`

Out-of-line payloads in the field-data blob:

- `Dword64`
- `Int64`
- `Double`
- `CExoString`
- `ResRef`
- `CExoLocString`
- `Void`

Recursive payloads:

- `Struct`
- `List`

The practical point is that "GFF value" is not one uniform storage class.
Reconstruction requires honoring the original split between inline scalars,
out-of-line payloads, and recursive references.

## Logical Edges

- Field order is explicit and preserved. `GffStruct` is not an unordered map.
- Labels must be unique within a struct. Duplicate labels are rejected.
- The root struct is special. On write it must serialize as struct index `0`
  with id `-1`.
- Complex fields preserve raw payload bytes when that is needed for stable
  rewrites.
- `GffRoot` stores `source_bytes` and a `source_snapshot`. If the typed value is
  unchanged, writes can replay original bytes rather than normalize the file.
- `merge_root_preserving_provenance` exists because naïve merge logic tends to
  destroy stable ordering and untouched raw structure.

## Why This Crate Exists

`GFF` is one of the places where reverse engineering turns into systems design.
The difficult part is not only learning the table layout. It is deciding which
properties are structural enough to model:

- order
- typed field kind
- label identity
- recursive structure
- raw payload fidelity

This crate chooses to preserve all of those explicitly so higher layers can
lift `GFF` into domain types without pretending the underlying container is a
schema-free blob.
