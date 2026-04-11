# `nwnrs-dds`

Typed Neverwinter Nights `DDS` support.

## Scope

This crate owns the NWN-specific DDS workflow used by this repository:

- parsing the compact NWN DDS header
- splitting and validating mip chains
- decoding packed DXT data to top-left-origin RGBA8
- writing typed NWN DDS payloads back out
- encoding RGBA8 input into NWN `dxt1` or `dxt5`

This crate targets the Neverwinter Nights DDS payload format used by the game
and related tooling in this repo. It is not intended to be a general-purpose
desktop DDS container crate.

## Current Coverage

- NWN compact DDS header parsing
- typed mip-level ownership
- DXT1 decode
- DXT5 decode
- RGBA8 encode to NWN `dxt1`
- RGBA8 encode to NWN `dxt5`
- exact typed write support for NWN DDS payloads

## Main Types

- `DdsTexture`
- `DdsMipLevel`
- `DdsFormat`
- `NwnDdsHeader`
- `DdsError`

## Main Entry Points

- `read_dds`
- `read_dds_from_file`
- `read_dds_from_res`
- `write_dds`
- `DdsTexture::decode_rgba8`
- `DdsTexture::decode_mip_rgba8`
- `DdsTexture::encode_rgba8`

## Notes

This crate stays at the NWN texture-format layer. It does not handle Bevy
integration, material policy, or runtime asset resolution.
