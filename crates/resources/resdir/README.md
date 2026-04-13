# nwnrs-resdir

Directory-backed `nwnrs-resman::ResContainer` implementation.

## Scope

- scan an on-disk directory tree for NWN-style resources
- resolve filenames into typed resource references
- expose the resulting directory as a `ResContainer`

## Non-goals

- define precedence policy across multiple directories
- parse the contents of the resolved resources
