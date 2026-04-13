# nwnrs-txi

Typed parser for Neverwinter Nights texture info (`TXI`) resources.

## Scope

- parse line-oriented TXI directives into typed directive records
- build deterministic TXI text from the typed representation
- write typed TXI payloads back to a stream
- expose selected high-value directives through dedicated typed fields
- preserve directive ordering and continuation lines
- support optional sidecar lookup by texture name through `ResMan`

The primary entry points are [`read_txi`], [`build_txi_text`], [`write_txi`],
[`TxiFile::optional_from_resman`], and [`TxiFile`].

## Invariants

- directives remain available in source order through [`TxiFile::directives`]
- continuation lines stay attached to the directive they extend
- typed convenience fields are derived views over the parsed directives rather
  than replacements for them
- serialization treats [`TxiFile::directives`] as authoritative when present
  and only synthesizes directives from typed fields when the directive stream is
  empty

## Non-goals

- resolve texture assets or renderer policy implied by TXI settings
- normalize every directive into a fully semantic material representation
