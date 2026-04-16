# Text Encoding

Docs:

- crate: `nwnrs-encoding`
- [crate docs](https://docs.rs/nwnrs-encoding/latest/nwnrs_encoding/)
- [README](https://github.com/urothis/nwnrs/blob/main/crates/foundation/encoding/README.md)
- [source](https://github.com/urothis/nwnrs/blob/main/crates/foundation/encoding/src/lib.rs)

## Scope

`nwnrs-encoding` is the workspace policy boundary for text encoding. NWN data does not live in a Unicode-only world, so the conversion policy has to be explicit and centralized.

## Public Surface

### Error vocabulary

- `EncodingConversionError`
- `NativeEncodingError`
- `UnknownEncodingError`

These distinguish conversion failure from configuration or lookup failure.

### NWN-side conversion

- `to_nwnrs_encoding`
- `from_nwnrs_encoding`
- `get_nwnrs_encoding`
- `get_nwnrs_encoding_name`
- `set_nwnrs_encoding`

These define the encoding policy for bytes that should be interpreted as "NWN text" inside the workspace.

### Host-side conversion

- `to_native_encoding`
- `from_native_encoding`
- `detect_system_native_encoding`
- `get_native_encoding`
- `get_native_encoding_name`
- `set_native_encoding`
- `clear_native_encoding`

These are the host-facing operations for situations where the library needs to cross the boundary between NWN storage and the local machine's text environment.

## Logical Edges

- There are two distinct policies here: the encoding associated with NWN storage and the encoding associated with the local host environment. They should not be conflated.
- The explicit `set_*` and `clear_*` functions mean encoding policy is not purely compile-time. Callers can make process-local policy choices.
- Higher-level crates should not infer encoding rules independently. If a payload is text and its encoding is not structurally fixed by the format, this crate is the place where that policy belongs.
- This crate does not own localization semantics. It owns byte-to-string and string-to-byte conversion policy.

## Why This Crate Exists

Reverse engineered text handling tends to drift if every format crate decodes bytes on its own. This crate prevents that by forcing the policy question to have one answer.
