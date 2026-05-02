# nwnrs-types

Typed Neverwinter Nights `TGA` support.

## Scope

- parse typed TGA headers and payload sections
- decode supported images to top-left-origin RGBA8
- write typed TGA payloads back out
- encode RGBA8 input into authored uncompressed 32-bit TGA output

The parser preserves raw sections such as the image ID, color map bytes, image
data, trailing bytes, and optional TGA 2.0 footer.

## Public Surface

- `TGA_RES_TYPE`
- `TGA_HEADER_SIZE`
- `TgaError`
- `TgaResult`
- `TgaImageType`
- `TgaFooter`
- `TgaTexture`
- `read_tga`
- `write_tga`

## Core Model

`TgaTexture` preserves:

- header fields
- raw `image_id`
- raw `color_map_data`
- raw `image_data`
- `trailing_data`
- optional `footer`

That means decode-to-RGBA8 is a derived view over the stored representation,
not the authoritative representation itself.

## Binary Layout

Header size: `18` bytes.

```text
0x00  id_length                    u8
0x01  color_map_type               u8
0x02  image_type                   u8
0x03  color_map_first_entry_index  u16
0x05  color_map_length             u16
0x07  color_map_entry_size         u8
0x08  x_origin                     u16
0x0A  y_origin                     u16
0x0C  width                        u16
0x0E  height                       u16
0x10  pixel_depth                  u8
0x11  image_descriptor             u8
```

Payload structure:

```text
+----------------------+
| 18-byte header       |
+----------------------+
| image ID             | id_length bytes
+----------------------+
| color map            | optional
+----------------------+
| image data           | raw or RLE-packed
+----------------------+
| trailing data        | optional
+----------------------+
| footer               | optional 26-byte TGA 2.0 footer
+----------------------+
```

Footer semantics:

- extension area offset
- developer directory offset
- `TRUEVISION-XFILE.\0` signature

## Invariants

- the typed representation preserves header fields and raw payload sections
- decode operations normalize pixels to RGBA8 without discarding the typed
  source structure
- writes are produced from the typed texture state rather than from a lossy
  intermediate
- origin bits, storage mode, trailing bytes, and footer metadata remain part of
  the typed texture

## See also

- [`crate::dds`], the other primary NWN texture format
- [`crate::plt`], which stores recolorable palette-layer textures alongside
  standard TGA assets
- [`crate::txi`], which provides texture sidecar metadata that accompanies TGA
  assets

## Why This Crate Exists

Image formats often get crushed into "just decode to RGBA and move on." That is
useful for display, but it is not enough for archival fidelity, stable writes,
or format documentation. This crate keeps the actual stored structure visible.
