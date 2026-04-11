# `nwnrs-plt`

Typed Neverwinter Nights `PLT` support.

## Scope

This crate owns typed file-format handling for NWN palette textures:

- parsing the fixed PLT header
- exposing per-pixel `value` and `layer_id` entries
- preserving the typed header fields and pixel payload
- writing PLT data back out exactly through the typed representation

It also exposes the known material layer ids as `PltLayer`.

## Current Coverage

- typed PLT header parsing
- typed pixel parsing as `PltPixel { value, layer_id }`
- known layer mapping helpers through `PltLayer`
- exact typed write support

## Main Types

- `PltTexture`
- `PltPixel`
- `PltLayer`
- `PltError`

## Main Entry Points

- `read_plt`
- `read_plt_from_file`
- `read_plt_from_res`
- `write_plt`
- `PltTexture::pixel_at`

## Notes

This crate intentionally stops at typed file ownership. It does not yet try to:

- resolve final colors
- apply game palette tables
- render PLT data to a finished material/image output

Those concerns belong in a later rendering or conversion layer.
