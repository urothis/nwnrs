# nwnrs-localization

`nwnrs-localization` defines the small vocabulary that recurs across TLK, GFF,
SSF, and installation-facing resource loading.

## Scope

- represent NWN language identifiers
- represent dialog string references
- represent the male/female selector used by TLK lookup
- keep those foundational concepts consistent across the workspace

The relevant entry points are [`Language`], [`StrRef`], and
[`resolve_language`].

## Public Surface

### Core types

- `StrRef`
- `Language`
- `Gender`

### Constants and parsing

- `BAD_STRREF`
- `ParseLanguageError`
- `resolve_language`

### Important `Language` operations

- `Language::id`
- `Language::short_code`
- `Language::from_id`
- `FromStr for Language`

## Logical Edges

- `BAD_STRREF` is the sentinel for "no string" and must be treated as such by
  higher layers
- `Language` is an NWN-specific vocabulary, not a general i18n abstraction
- `Gender` is here because TLK lookup has male/female layering semantics
- `resolve_language` and `FromStr` form the normalization boundary between user
  input, install directory naming, and the typed language enum

## Why This Crate Exists

Without a single localization vocabulary, every crate that touched `TLK` or
language roots would reinterpret the same concepts independently.
