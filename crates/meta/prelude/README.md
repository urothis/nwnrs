# nwnrs

`nwnrs` is the umbrella crate for the public workspace surface.

## Scope

- re-export the public crates through root modules such as [`gff`], [`twoda`],
  [`resman`], and [`install`]
- provide a convenience [`prelude`] module for callers that prefer one import
  boundary
- keep the top-level API aligned with the workspace crate boundaries rather than
  introducing a second abstraction layer

## Example

```rust
use nwnrs::{
    gff::{GffRoot, GffValue},
    twoda::TwoDa,
};

let mut root = GffRoot::new("UTC ");
root.put_value("Tag", GffValue::CExoString("nw_chicken".to_string()))?;

let mut table = TwoDa::new();
table.set_columns(vec!["Label".to_string()])?;
table.replace_rows(
    vec![vec![Some("Chicken".to_string())]],
    vec!["0".to_string()],
)?;

assert_eq!(root.file_type, "UTC ");
assert_eq!(table.cell_or(0, "Label", ""), "Chicken");
# Ok::<(), Box<dyn std::error::Error>>(())
```
