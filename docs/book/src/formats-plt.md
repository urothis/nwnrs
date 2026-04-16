# PLT Layer Textures

Docs:

- [crate docs](https://docs.rs/nwnrs-plt/latest/nwnrs_plt/)
- [README](https://github.com/urothis/nwnrs/blob/main/crates/formats/plt/README.md)
- [source](https://github.com/urothis/nwnrs/blob/main/crates/formats/plt/src/lib.rs)

`PLT` is not a normal image format. It is a per-pixel layer/value map used to
support recolorable assets.

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
  metal, and tattoos.
- `PltPixel` stores:
  - `value`
  - `layer_id`
- `PltRenderSpec` is a convenience policy for turning the typed layer map into
  RGBA output.
- `PltTexture` preserves header fields, typed pixels, and trailing bytes.

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

## Logical Edges

- `PLT` is not a final-color bitmap.
- The `value` byte and `layer_id` byte both matter; one without the other is
  not sufficient to reconstruct the intended appearance pipeline.
- `PltRenderSpec` is intentionally not the canonical representation. It is one
  rendering policy over the stored typed data.

## Why This Crate Exists

If you flatten `PLT` into one rendered image too early, you destroy the whole
point of the format. The real stored information is:

- where recolorable regions are
- which layer each region belongs to
- the per-pixel source value used by the palette logic
