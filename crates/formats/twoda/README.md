# nwnrs-twoda

`nwnrs-twoda` reads and writes `2DA V2.0` tables.

## Scope

- parse ordered column names, row labels, cell values, and the table-wide
  default value
- preserve the typed table structure closely enough for stable editing
- write the typed representation back to NWN `2DA` text

For most consumers, the relevant entry points are [`read_twoda`],
[`write_twoda`], and [`TwoDa`].

## Example

```rust
use nwnrs_twoda::{TwoDa, read_twoda, write_twoda};

let mut table = TwoDa::new();
table.set_columns(vec!["Label".to_string(), "Value".to_string()])?;
table.replace_rows(
    vec![vec![Some("Row0".to_string()), Some("42".to_string())]],
    vec!["0".to_string()],
)?;

let mut bytes = Vec::new();
write_twoda(&mut bytes, &table, false)?;

let decoded = read_twoda(bytes.as_slice())?;
assert_eq!(decoded.cell_or(0, "Value", ""), "42");
# Ok::<(), nwnrs_twoda::TwoDaError>(())
```

## Public Surface

- `TwoDa`
- `Cell`
- `Row`
- `TWO_DA_HEADER`
- `TwoDaError`
- `TwoDaResult`
- `as_2da`
- `escape_field`
- `read_twoda`
- `write_twoda`

## Core Model

- `Cell = Option<String>`
  `None` means the authored cell was `****`, not the empty string
- `Row = Vec<Cell>`
- `TwoDa` preserves:
  - ordered column headers
  - ordered row labels
  - ordered rows
  - optional table-wide default value
  - original source layout metadata when parsed from disk

## Text Layout

The crate models the canonical `2DA V2.0` text form.

```text
2DA V2.0

DEFAULT: <optional default token>

<column0>  <column1>  <column2> ...
<row0>     <cell>     <cell>     ...
<row1>     <cell>     <cell>     ...
...
```

Conceptually:

```text
+-------------------+
| magic line        | "2DA V2.0"
+-------------------+
| default line?     | optional
+-------------------+
| header row        | ordered column names
+-------------------+
| data rows         | row label + cells
+-------------------+
```

Cell encoding rules:

- `****` means "no value"
- other tokens are stored as text
- quoted and escaped output is a serialization concern, not a semantic type
  system

## Invariants

- column order is preserved explicitly
- row order and row labels are preserved explicitly
- the table-wide default value remains part of the typed representation
- column lookup is case-insensitive, while stored column names retain authored
  case
- `None` means the authored cell was `****`, not the empty string

## Tricky Parts

- empty string and absent value are not the same thing
- numeric-looking cells remain strings until a higher layer interprets them
- table semantics are external; the crate intentionally does not know what a
  given `2DA` means
- the same physical table can be consumed positionally, by row label, or by
  case-insensitive column name

## Non-goals

- interpret the semantics of particular `2DA` tables
- normalize tables into a database-like relational model
- replace higher-level crates that add domain meaning on top of `2DA`

## See also

- [`nwnrs-resman`](https://docs.rs/nwnrs-resman), the resource layer through
  which `2DA` files are typically loaded by name
- [`nwnrs-install`](https://docs.rs/nwnrs-install), which assembles the
  install-backed resource view containing the base-game `2DA` tables

## Why This Crate Exists

`2DA` is a good example of a format that is textual but still deserves a real
typed model. What actually matters is:

- preserving authorial ordering
- preserving `****`
- preserving row identity
- preserving enough layout information that deterministic rewrites do not create
  unnecessary diffs
