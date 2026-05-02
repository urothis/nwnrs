# nwnrs-types

`nwnrs-types` is the workspace policy boundary for text encoding.

## Why This Crate Exists

NWN stores text in a non-UTF-8 encoding that varies by platform and language.
Without a central policy boundary, every format crate would embed its own
encoding assumptions and diverge silently. This crate makes the workspace
encoding policy explicit and gives all format crates a single place to
transcode NWN byte storage to and from Rust `String` values.

## Scope

- define the default NWN text encoding
- detect host-native encoding when needed
- expose conversion routines between NWN byte storage and Rust `String` values
- make encoding policy explicit instead of scattering it across format crates

The central operations are `to_nwnrs_encoding`, `from_nwnrs_encoding`,
`to_native_encoding`, and `from_native_encoding`.

## Public Surface

- `get_nwnrs_encoding`
- `get_nwnrs_encoding_name`
- `set_nwnrs_encoding`
- `from_nwnrs_encoding`
- `to_nwnrs_encoding`
- `get_native_encoding`
- `get_native_encoding_name`
- `detect_system_native_encoding`
- `set_native_encoding`
- `from_native_encoding`
- `to_native_encoding`
- `UnknownEncodingError`
- `EncodingConversionError`

## Non-goals

- provide a complete transcoding framework for arbitrary encodings
- own higher-level localization semantics

## See also

- [`crate::localization`], which defines the language and string-reference
  vocabulary built on top of this encoding layer
