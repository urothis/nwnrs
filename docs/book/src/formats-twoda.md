# 2DA Tables

Docs:

- [crate docs](https://docs.rs/nwnrs-twoda/latest/nwnrs_twoda/)
- [README](https://github.com/urothis/nwnrs/blob/main/crates/formats/twoda/README.md)
- [types.rs](https://github.com/urothis/nwnrs/blob/main/crates/formats/twoda/src/types.rs)
- [io.rs](https://github.com/urothis/nwnrs/blob/main/crates/formats/twoda/src/io.rs)

`2DA` is simple enough to underestimate. In practice it is one of the most
important control surfaces in the game because a large amount of behavior is
table-driven.

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
  `None` means the authored cell was `****`, not the empty string.
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
- quoted/escaped output is a serialization concern, not a semantic type system

## Logical Edges

- Column order is first-class. This is not a relational table engine.
- Row labels are first-class. They are not derived from row position.
- Column lookup is case-insensitive, but authored case is preserved.
- The default value is part of the typed model because omitting it changes
  lookup semantics.
- Stable editing requires preserving layout choices rather than rebuilding
  everything from a normalized matrix.

## Tricky Parts

- Empty string and absent value are not the same thing.
- Numeric-looking cells remain strings until a higher layer interprets them.
- Table semantics are external. The crate intentionally does not know what a
  given `2DA` means.
- The same physical table can be consumed positionally, by row label, or by
  case-insensitive column name. Those are distinct access paths over one stored
  artifact.

## Why This Crate Exists

`2DA` is a good example of a format that is "textual" but still deserves a
real typed model. The reverse-engineering mistake here would be to parse it
into a bag of strings and call the problem solved. What actually matters is:

- preserving authorial ordering
- preserving `****`
- preserving row identity
- preserving enough layout information that deterministic rewrites do not
  accidentally create unnecessary diffs
