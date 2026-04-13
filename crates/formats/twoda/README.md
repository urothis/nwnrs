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

## Invariants

- column order is preserved explicitly
- row order and row labels are preserved explicitly
- the table-wide default value remains part of the typed representation
- column lookup is case-insensitive, while stored column names retain authored
  case

## Non-goals

- interpret the semantics of particular `2DA` tables
- normalize tables into a database-like relational model
- replace higher-level crates that add domain meaning on top of `2DA`
