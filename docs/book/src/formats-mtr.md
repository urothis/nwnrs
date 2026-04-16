# MTR Materials

Docs:

- [crate docs](https://docs.rs/nwnrs-mtr/latest/nwnrs_mtr/)
- [README](https://github.com/urothis/nwnrs/blob/main/crates/formats/mtr/README.md)
- [source](https://github.com/urothis/nwnrs/blob/main/crates/formats/mtr/src/lib.rs)

`MTR` is a text material descriptor. It is not a renderer implementation and it
is not equivalent to `TXI`.

## Public Surface

- `MTR_RES_TYPE`
- `MtrError`
- `MtrResult`
- `MtrParameter`
- `MtrMaterial`
- `read_mtr`
- `parse_mtr`
- `write_mtr`

## Core Model

- `MtrMaterial` preserves:
  - `render_hint`
  - `textures: BTreeMap<usize, String>`
  - `parameters: BTreeMap<String, MtrParameter>`
  - optional custom shader names for VS/GS/FS
- `MtrParameter` preserves:
  - `param_type`
  - numeric `values`

## Text Layout

The format is directive-like, one statement per line:

```text
customshaderVS my_vertex_shader
customshaderFS my_fragment_shader
renderhint NormalAndSpecMapped
texture0 my_diffuse
texture1 my_normal
parameter float Roughness 0.5
parameter float Tint 1.0 0.8 0.7
```

Conceptually:

```text
+----------------------+
| shader selectors     |
+----------------------+
| render hint          |
+----------------------+
| textureN bindings    |
+----------------------+
| parameter rows       |
+----------------------+
```

## Logical Edges

- Texture slots are explicit numeric bindings, not one bag of string properties.
- Named parameters are explicit typed rows, not anonymous vectors.
- The crate models the material descriptor, not runtime shading state.
- Deterministic serialization is intentionally simpler and more canonical than
  "preserve every line verbatim."

## Why This Crate Exists

`MTR` is where "text format" and "semantic descriptor" overlap. The important
thing is not line parsing by itself. The important thing is preserving the
actual modeled concepts:

- texture slot identity
- parameter name identity
- shader name bindings
- render-hint classification
