# nwnrs-types

`nwnrs-types` reads, writes, and queries dialog-table (`TLK`) files.

## Scope

- parse standalone TLK tables into typed `SingleTlk` values
- support layered male/female lookup through `Tlk`
- preserve entry metadata such as sound references, flags, and stored text
  bytes when possible
- support lazy stream-backed reads with optional caching
- support explicit male/female chain writes through `write_tlk_chain`

The principal entry points are `read_single_tlk`, `write_single_tlk`,
`write_tlk_chain`, `SingleTlk`, and `Tlk`.

## Example

```rust
use std::io::Cursor;

use nwnrs_types::resman::CachePolicy;
use nwnrs_types::tlk::{SingleTlk, TlkEntry, read_single_tlk, write_single_tlk};

let mut tlk = SingleTlk::new();
tlk.set_entry(0, TlkEntry::new("Hello there", "hello01", 1.25));

let mut bytes = Cursor::new(Vec::new());
write_single_tlk(&mut bytes, &mut tlk)?;
bytes.set_position(0);

let mut decoded = read_single_tlk(bytes, CachePolicy::Use)?;
let entry = decoded.get(0)?.expect("entry 0 should exist");
assert_eq!(entry.text, "Hello there");
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Public Surface

- `SingleTlk`
- `Tlk`
- `TlkEntry`
- `TlkPair`
- `TlkLayerWriteTarget`
- `TlkWriteStream`
- `HEADER_SIZE`
- `DATA_ELEMENT_SIZE`
- `TlkError`
- `TlkResult`
- `read_single_tlk`
- `write_single_tlk`
- `write_tlk_chain`

## Core Model

- `TlkEntry` preserves:
  - `text`
  - `raw_text`
  - `sound_res_ref`
  - `raw_sound_res_ref`
  - `sound_length`
  - `sound_length_bits`
  - flags
  - volume variance
  - pitch variance
- `SingleTlk` is one standalone table
- `Tlk` is the layered male/female lookup abstraction built from one or more
  `TlkPair` values

## Binary Layout

The crate models `TLK V3.0`.

Header:

```text
0x00  "TLK "
0x04  "V3.0"
0x08  language_id     u32
0x0C  entry_count     u32
0x10  string_offset   u32

total header size: 20 bytes
```

Entry descriptors follow immediately:

```text
+----------------------+
| TLK header           | 20 bytes
+----------------------+
| entry descriptor 0   | 40 bytes
+----------------------+
| entry descriptor 1   | 40 bytes
+----------------------+
| ...                  |
+----------------------+
| string blob          | variable
+----------------------+
```

Descriptor shape:

```text
flags                u32
sound_resref[16]     bytes
volume_variance      u32
pitch_variance       u32
string_offset        u32
string_length        u32
sound_length         f32
```

Conceptually, each entry descriptor names a slice inside the trailing string
blob.

## Invariants

- string references remain stable numeric indices into the table
- each `TlkEntry` preserves sound-reference and sound-length descriptor data
  when the stored raw representation is still consistent with the typed fields
- stream-backed tables do not renumber entries during lazy access
- layered `Tlk` lookup preserves chain precedence exactly as supplied
- `SingleTlk` is the file; `Tlk` is the higher-level layered lookup system

## Tricky Parts

- the stored string table is one large blob plus descriptor offsets, not one
  self-delimiting record per entry
- the male/female distinction is not a property of the physical `TLK` file
  format
- a typed entry and a rewrite-stable entry are related but not identical goals

## See also

- [`crate::localization`], which defines `Language`, `Gender`, and `StrRef`
- [`crate::install`], which selects language roots for install-backed resource
  loading

## Why This Crate Exists

There are two distinct technical problems here:

1. parse one `TLK` file correctly and preserve its descriptor semantics
2. expose the install-level layered lookup model that real consumers need

The crate does both, but it does not blur them together. `SingleTlk` is the
file. `Tlk` is the lookup system.
