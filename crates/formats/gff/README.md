# nwnrs-gff

`nwnrs-gff` reads and writes `GFF V3.2`, the structured container format
underlying a large portion of NWN gameplay data.

## Scope

- parse typed GFF roots, structures, fields, and values
- preserve authored field order so stable editing remains possible
- write typed GFF documents back to binary form
- provide a compact typed vocabulary on which higher-level crates can build

The principal entry points are [`read_gff_root`], [`write_gff_root`], and
[`GffRoot`].

## Example

```rust
use std::io::Cursor;

use nwnrs_gff::{GffRoot, GffValue, read_gff_root, write_gff_root};

let mut root = GffRoot::new("UTC ");
root.put_value("Tag", GffValue::CExoString("nw_chicken".to_string()))?;

let mut bytes = Cursor::new(Vec::new());
write_gff_root(&mut bytes, &root)?;
bytes.set_position(0);

let decoded = read_gff_root(&mut bytes)?;
assert_eq!(decoded.file_type, "UTC ");
assert_eq!(decoded.fields().len(), 1);
# Ok::<(), nwnrs_gff::GffError>(())
```

## Invariants

- the order of fields inside each [`GffStruct`] is preserved explicitly
- the root `file_type` and `file_version` remain first-class typed fields
- each [`GffValue`] retains its declared GFF field kind
- writes are derived from the typed representation rather than from an
  unstructured map

## Non-goals

- interpret gameplay meaning from raw GFF fields
- hide unknown higher-level schema behind premature domain types
- provide a generic, non-NWN object database format

## See also

- [`nwnrs-git`](https://docs.rs/nwnrs-git), which layers typed area-instance
  semantics over raw GFF data
- [`nwnrs-erf`](https://docs.rs/nwnrs-erf), which often carries GFF payloads in
  NWN archives
