# nwnrs-resfile

Single-file `nwnrs-resman::ResContainer` implementation.

## Scope

- wrap one on-disk file as a single resource entry
- expose that entry through the same `ResContainer` abstraction used elsewhere

## Non-goals

- infer complex directory or archive structure from a lone file
- parse the payload contents directly
