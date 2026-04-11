# `nwnrs-tga`

Typed Neverwinter Nights `TGA` support.

## Scope

- parse typed TGA headers and payload sections
- decode supported images to top-left-origin RGBA8
- write typed TGA payloads back out
- encode RGBA8 input into authored uncompressed 32-bit TGA output

The parser preserves raw sections such as the image ID, color map bytes, image
data, trailing bytes, and optional TGA 2.0 footer.

## Invariants

- the typed representation preserves header fields and raw payload sections
- decode operations normalize pixels to RGBA8 without discarding the typed
  source structure
- writes are produced from the typed texture state rather than from a lossy
  intermediate

## Non-goals

- act as a general-purpose image-processing crate
- define higher-level material or asset-loading policy
