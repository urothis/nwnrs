# nwnrs-mtr

Typed parser and writer for Neverwinter Nights material (`MTR`) payloads.

## Scope

- parse text-based NWN material descriptors
- expose texture-layer bindings and shader-relevant settings through typed data
- write the typed material representation back to text

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
  - optional custom shader names for VS, GS, and FS
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

## Invariants

- authored material properties remain explicit typed fields
- texture slots are explicit numeric bindings, not one bag of string properties
- named parameters are explicit typed rows, not anonymous vectors
- the crate models the material descriptor, not a renderer-specific material
  object

## Non-goals

- resolve the final texture assets referenced by a material
- implement runtime shading or renderer integration

## See also

- [`nwnrs-mdl`](https://docs.rs/nwnrs-mdl), which references `MTR` descriptors
  for model material resolution
- [`nwnrs-txi`](https://docs.rs/nwnrs-txi), which provides texture sidecar
  metadata often used alongside material descriptors

## Why This Crate Exists

`MTR` is where text format and semantic descriptor overlap. The important thing
is preserving the actual modeled concepts:

- texture slot identity
- parameter name identity
- shader name bindings
- render-hint classification
