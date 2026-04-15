# nwnrs-plt

Typed Neverwinter Nights `PLT` support.

## Scope

- parse the fixed PLT header
- expose per-pixel `value` and `layer_id` pairs through typed data
- preserve the typed header fields and pixel payload
- write PLT data back out through the typed representation

It also exposes the known palette layers as [`PltLayer`].

## Public Surface

- `PLT_RES_TYPE`
- `PLT_SIGNATURE`
- `PLT_HEADER_SIZE`
- `PltLayer`
- `PltPixel`
- `PltRenderSpec`
- `PltTexture`
- `PltError`
- `PltResult`
- `read_plt`
- `write_plt`

## Core Model

- `PltLayer` names known palette layers such as skin, hair, cloth, leather,
  metal, and tattoos
- `PltPixel` stores:
  - `value`
  - `layer_id`
- `PltRenderSpec` is a convenience policy for turning the typed layer map into
  RGBA output
- `PltTexture` preserves header fields, typed pixels, and trailing bytes

## Binary Layout

Header size: `24` bytes.

```text
0x00  file_type     [4]   typically "PLT "
0x04  file_version  [4]   typically "V1  "
0x08  unused1       [4]
0x0C  unused2       [4]
0x10  width         u32
0x14  height        u32
```

Then:

```text
+----------------------+
| 24-byte header       |
+----------------------+
| pixel payload        | width * height * 2 bytes
+----------------------+
| trailing data        | optional
+----------------------+
```

Each pixel contributes two bytes conceptually:

```text
value
layer_id
```

## Invariants

- the texture is represented as typed pixels rather than a precomposited image
- palette layer ids remain explicit instead of being collapsed into final colors
- writes are derived from the typed texture state
- `PLT` is not a final-color bitmap
- `PltRenderSpec` is a convenience policy over stored typed data, not the
  canonical representation

## Non-goals

- resolve final colors from game palette tables
- render PLT data into a final material or image by itself

## Why This Crate Exists

If you flatten `PLT` into one rendered image too early, you destroy the whole
point of the format. The real stored information is:

- where recolorable regions are
- which layer each region belongs to
- the per-pixel source value used by the palette logic
