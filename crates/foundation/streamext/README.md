# nwnrs-streamext

Stream helpers for size-prefixed binary formats.

## Scope

- read and write compact little-endian length-prefixed values
- provide small generic helpers for stream-oriented binary framing
- keep size-prefix handling out of higher-level format crates

## Non-goals

- replace the broader binary-read helpers in `nwnrs-io`
- provide a general binary codec framework
