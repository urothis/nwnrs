# Localization

Docs:

- crate: `nwnrs-localization`
- [crate docs](https://docs.rs/nwnrs-localization/latest/nwnrs_localization/)
- [README](https://github.com/urothis/nwnrs/blob/main/crates/foundation/localization/README.md)
- [source](https://github.com/urothis/nwnrs/blob/main/crates/foundation/localization/src/lib.rs)

## Scope

`nwnrs-localization` contains the small typed vocabulary that recurs across TLK, GFF localized strings, SSF, and installation-facing language selection.

## Public Surface

### Core types

- `StrRef`
- `Language`
- `Gender`

`StrRef` is a type alias for the numeric TLK index space. `Language` is the NWN language id vocabulary. `Gender` is the selector used by layered TLK lookup.

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

- `BAD_STRREF` is not just another number. It is the sentinel for "no string" and must be treated as such by higher layers.
- `Language` is an NWN-specific vocabulary, not a general i18n abstraction.
- `Gender` is here because TLK lookup has male/female layering semantics; it is not intended as a broader identity model.
- `resolve_language` and `FromStr` form the normalization boundary between user input, install directory naming, and the typed language enum.

## Why This Crate Exists

Without a single localization vocabulary, every crate that touched `TLK` or language roots would reinterpret the same concepts independently. This crate keeps the identity model small and stable.
