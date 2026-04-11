# nwnrs-erf

`nwnrs-erf` reads and writes the ERF-family archive formats used by
Neverwinter Nights, including `ERF`, `MOD`, `HAK`, and `NWM`.

## Scope

- parse typed ERF archives and their resource tables
- expose archive contents as an [`Erf`] value
- implement `nwnrs-resman` container behavior for archive-backed resolution
- write typed archive data back to disk

The principal entry points are [`read_erf`], [`read_erf_from_file`],
[`read_erf_shared`], and [`write_erf`].

## Invariants

- resource references and archive membership are represented explicitly
- archive semantics are preserved independently of the container filename
- the same typed archive value can be inspected structurally and used as a
  `ResContainer`

## Non-goals

- define resource precedence policy across multiple archives
- replace `nwnrs-resman` as the general lookup layer

## See also

- [`nwnrs-resman`](https://docs.rs/nwnrs-resman), which layers multiple
  containers in precedence order
- [`nwnrs-key`](https://docs.rs/nwnrs-key), which models the KEY/BIF storage
  family
