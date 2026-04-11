# nwnrs-ssf

Reader and writer for soundset (`SSF`) files.

## Scope

- parse fixed-layout SSF tables into typed slot entries
- preserve the association between each slot and its sound/string references
- write typed SSF data back to disk

## Invariants

- soundset slots remain positional and typed
- string references and resource references stay distinct fields

## Non-goals

- interpret gameplay meaning beyond the SSF slot structure itself
- resolve the referenced audio payloads or dialog strings
