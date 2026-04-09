# `nwnrs-tga`

Typed Neverwinter Nights `TGA` support.

## Scope

This crate owns the NWN-facing `TGA` workflow:

- parsing typed TGA headers and payload sections
- decoding supported images to top-left-origin RGBA8
- writing typed TGA payloads back out
- encoding RGBA8 input into authored uncompressed 32-bit TGA output

The parser preserves raw sections such as:

- image ID
- color map bytes
- image data
- trailing bytes
- optional TGA 2.0 footer

## Current Coverage

- uncompressed truecolor TGA
- RLE truecolor TGA
- grayscale decode paths used by the current implementation
- exact round-trip writing for typed payload ownership

## Main Types

- `TgaTexture`
- `TgaImageType`
- `TgaFooter`
- `TgaError`

## Main Entry Points

- `read_tga`
- `read_tga_from_file`
- `read_tga_from_res`
- `write_tga`
- `TgaTexture::decode_rgba8`
- `TgaTexture::encode_rgba8`

## Notes

This crate is format-focused. It does not depend on Bevy and does not try to
solve higher-level material or asset-loading concerns.
