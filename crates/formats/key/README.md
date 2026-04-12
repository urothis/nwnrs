# nwnrs-key

`nwnrs-key` reads and writes KEY/BIF resource sets, which form the canonical
indexed storage layout for base-game content.

## Scope

- parse KEY files and their BIF index tables
- expose the result as a typed [`KeyTable`]
- implement `nwnrs-resman` container behavior for KEY/BIF-backed content
- write KEY/BIF output from typed archive state

The main entry points are [`read_key_table`], [`read_key_table_from_file`], and
[`write_key_and_bif`].

## Invariants

- resource references remain typed rather than stringly indexed
- the mapping from KEY entries to BIF-backed payload locations remains explicit
- the same typed value may be inspected structurally and used as a resource
  container

## Non-goals

- choose install-layer precedence across multiple KEY tables
- hide the distinction between KEY indexing and BIF payload storage

## See also

- [`nwnrs-resman`](https://docs.rs/nwnrs-resman), which consumes `KeyTable` as a
  resource container
- [`nwnrs-install`](https://docs.rs/nwnrs-install), which uses KEY/BIF data to
  assemble a conventional install-backed resource stack
