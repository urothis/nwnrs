# nwnrs-types

`nwnrs-types` reads and writes `GFF V3.2`, the structured container format
underlying a large portion of NWN gameplay data.

## Scope

- parse typed GFF roots, structures, fields, and values
- preserve authored field order so stable editing remains possible
- write typed GFF documents back to binary form
- provide a compact typed vocabulary on which higher-level crates can build

The principal entry points are `read_gff_root`, `write_gff_root`, and
`GffRoot`.

## Example

```rust
use std::io::Cursor;

use nwnrs_types::gff::{GffRoot, GffValue, read_gff_root, write_gff_root};

let mut root = GffRoot::new("UTC ");
root.put_value("Tag", GffValue::CExoString("nw_chicken".to_string()))?;

let mut bytes = Cursor::new(Vec::new());
write_gff_root(&mut bytes, &root)?;
bytes.set_position(0);

let decoded = read_gff_root(&mut bytes)?;
assert_eq!(decoded.file_type, "UTC ");
assert_eq!(decoded.fields().len(), 1);
# Ok::<(), nwnrs_types::gff::GffError>(())
```

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
  source provenance
- `GffStruct` is an ordered labeled field map keyed by unique labels
- `GffField` separates field metadata from `GffValue`
- `GffValue` does not collapse field kinds into one lossy generic scalar type
- `GffCExoLocString` preserves both the top-level `str_ref` and the explicit
  localized override entries

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
- if a struct has `field_count == 1`, `data_or_offset` is the direct field
  index
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

## Invariants

- the order of fields inside each `GffStruct` is preserved explicitly
- the root `file_type` and `file_version` remain first-class typed fields
- each `GffValue` retains its declared GFF field kind
- writes are derived from the typed representation rather than from an
  unstructured map
- labels must be unique within a struct
- complex fields preserve raw payload bytes when that is needed for stable
  rewrites
- `merge_root_preserving_provenance` exists because naive merge logic tends to
  destroy stable ordering and untouched raw structure

## Coverage Boundary

This module owns two different but related layers:

- the raw `GFF V3.2` container vocabulary (`GffRoot`, `GffStruct`, `GffField`,
  and `GffValue`)
- the lifted `GIT` area-instance model exposed through `GitFile`

Many NWN resource kinds still live at the raw-GFF layer today. Formats such as
`UTC`, `UTI`, `UTP`, `UTD`, `UTM`, `UTE`, `UTS`, `UTT`, `UTW`, `ARE`, `IFO`,
`DLG`, `JRL`, `FAC`, `GUI`, and `BIC` are represented as `GffRoot` plus
domain-specific knowledge rather than as one dedicated lifted type per file
tag.

That boundary is intentional: the crate only lifts a schema into a dedicated
typed model when there is stable value in doing so beyond generic `GFF`
structure access.

## See also

- [`crate::erf`], which often carries `GFF` payloads in NWN archives
- [`crate::resman`], which resolves `GFF`-backed resources by typed resource
  identity

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
