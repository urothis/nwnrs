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

## Public Surface

- `TXI_RES_TYPE`
- `TxiError`
- `TxiResult`
- `TxiFile`
- `TxiDirective`
- `read_txi`
- `parse_txi`
- `build_txi_text`
- `write_txi`

## Core Model

- `TxiDirective` preserves:
  - directive `name`
  - inline `arguments`
  - `continuations`
- `TxiFile` preserves the directive stream and also exposes selected recognized
  directives as typed convenience fields such as:
  - `procedure_type`
  - `bump_map_texture`
  - `channel_scale`
  - `channel_translate`
  - `alpha_mean`

## Text Layout

Conceptually:

```text
directive arg0 arg1 ...
continuation
continuation

directive ...
directive ...
```

The parser behavior is:

- blank lines are ignored
- `#`, `//`, and `;` comment lines are ignored
- a line that begins a new directive creates a new `TxiDirective`
- a non-directive line attaches to the previous directive as a continuation

Example shape:

```text
channelscale 4 1.0 1.0
0.5
0.25

proceduretype water
alphamean 0.75
```

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

## Why This Crate Exists

The common failure mode with sidecar text formats is over-normalization.
`TxiFile` refuses to pretend that a hand-selected set of recognized directives
fully captures the file. It keeps both:

- the preserved directive stream
- typed accessors for the high-value directives most tools care about
