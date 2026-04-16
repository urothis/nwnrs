# TXI Sidecars

Docs:

- [crate docs](https://docs.rs/nwnrs-txi/latest/nwnrs_txi/)
- [README](https://github.com/urothis/nwnrs/blob/main/crates/formats/txi/README.md)
- [source](https://github.com/urothis/nwnrs/blob/main/crates/formats/txi/src/lib.rs)

`TXI` is line-oriented texture metadata. It is a sidecar format, but it is not
"just comments." It can materially affect how a texture is interpreted.

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

## Logical Edges

- The directive stream is authoritative when present.
- Typed convenience fields are derived views, not replacements.
- Continuation lines are semantically attached to the directive they extend.
- Directive order is preserved because rewrite stability and authored intent can
  depend on it.

## Why This Crate Exists

The common failure mode with sidecar text formats is over-normalization.
`TxiFile` refuses to pretend that a hand-selected set of recognized directives
fully captures the file. It keeps both:

- the preserved directive stream
- typed accessors for the high-value directives most tools care about
