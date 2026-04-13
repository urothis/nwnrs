# nwnrs-tlk

`nwnrs-tlk` reads, writes, and queries dialog-table (`TLK`) files.

## Scope

- parse standalone TLK tables into typed [`SingleTlk`] values
- support layered male/female lookup through [`Tlk`]
- preserve entry metadata such as sound references, flags, and stored text
  bytes when possible
- support lazy stream-backed reads with optional caching
- support explicit male/female chain writes through [`write_tlk_chain`]

The principal entry points are [`read_single_tlk`], [`write_single_tlk`],
[`write_tlk_chain`], [`SingleTlk`], and [`Tlk`].

## Example

```rust
use std::io::Cursor;

use nwnrs_resman::CachePolicy;
use nwnrs_tlk::{SingleTlk, TlkEntry, read_single_tlk, write_single_tlk};

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

## Invariants

- string references remain stable numeric indices into the table
- each [`TlkEntry`] preserves sound-reference and sound-length descriptor data
  when the stored raw representation is still consistent with the typed fields
- stream-backed tables do not renumber entries during lazy access
- layered [`Tlk`] lookup preserves chain precedence exactly as supplied

## Non-goals

- interpret gameplay-specific meaning for particular string references
- perform machine translation or localization workflow management
- replace higher-level installation logic that chooses which TLK layers to load

## See also

- [`nwnrs-localization`](https://docs.rs/nwnrs-localization), which defines
  `Language`, `Gender`, and `StrRef`
- [`nwnrs-install`](https://docs.rs/nwnrs-install), which selects language roots
  for install-backed resource loading
