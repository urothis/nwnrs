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

- provide a complete transcoding framework for arbitrary encodings
- own higher-level localization semantics

## See also

- [`nwnrs-localization`](https://docs.rs/nwnrs-localization), which defines
  the language and string-reference vocabulary built on top of this encoding
  layer
