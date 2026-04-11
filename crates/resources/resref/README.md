# nwnrs-resref

`nwnrs-resref` models Neverwinter Nights resource references as a typed pair:
a case-insensitive resource name together with a resource kind.

## Scope

- validate and parse resource references
- format resource references back to filenames or typed identifiers
- mediate between `name.ext` filenames and typed resource ids through
  `nwnrs-restype`

## Non-goals

- resolve resources from storage backends
- define file-format semantics for the referenced payloads

## See also

- [`nwnrs-restype`](https://docs.rs/nwnrs-restype), which supplies the
  extension/type mapping used by resolved resource references
- [`nwnrs-resman`](https://docs.rs/nwnrs-resman), which uses `ResRef` as its
  canonical lookup key
