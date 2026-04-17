# nwnrs-encoding

`nwnrs-encoding` is the workspace policy boundary for text encoding.

## Scope

- define the default NWN text encoding
- detect host-native encoding when needed
- expose conversion routines between NWN byte storage and Rust `String` values
- make encoding policy explicit instead of scattering it across format crates

The central operations are [`to_nwnrs_encoding`], [`from_nwnrs_encoding`],
[`to_native_encoding`], and [`from_native_encoding`].

## Public Surface

### Error vocabulary

- `EncodingConversionError`
- `NativeEncodingError`
- `UnknownEncodingError`

### NWN-side conversion

- `to_nwnrs_encoding`
- `from_nwnrs_encoding`
- `get_nwnrs_encoding`
- `get_nwnrs_encoding_name`
- `set_nwnrs_encoding`

### Host-side conversion

- `to_native_encoding`
- `from_native_encoding`
- `detect_system_native_encoding`
- `get_native_encoding`
- `get_native_encoding_name`
- `set_native_encoding`
- `clear_native_encoding`

## Logical Edges

- there are two distinct policies here: the encoding associated with NWN
  storage and the encoding associated with the local host environment
- the explicit `set_*` and `clear_*` functions mean encoding policy is not
  purely compile-time
- higher-level crates should not infer encoding rules independently
- this crate does not own localization semantics; it owns byte-to-string and
  string-to-byte conversion policy

## Why This Crate Exists

Reverse-engineered text handling tends to drift if every format crate decodes
bytes on its own. This crate prevents that by forcing the policy question to
have one answer.
