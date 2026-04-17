# `nwnrs-dds`

Typed Neverwinter Nights `DDS` support.

## Scope

- parse the NWN compact DDS header
- split and validate mip chains
- decode packed DXT data to top-left-origin RGBA8
- encode RGBA8 input into NWN `dxt1` or `dxt5`
- write typed NWN DDS payloads back out

The main entry points are [`read_dds`], [`write_dds`], and [`DdsTexture`].

## Public Surface

- `DDS_RES_TYPE`
- `NWN_DDS_HEADER_SIZE`
- `DdsError`
- `DdsResult`
- `DdsFormat`
- `NwnDdsHeader`
- `DdsMipLevel`
- `DdsTexture`
- `read_dds`
- `write_dds`

## Core Model

- `DdsFormat` currently distinguishes `Dxt1` and `Dxt5`
- `NwnDdsHeader` preserves:
  - `width`
  - `height`
  - `channels`
  - `linear_size`
  - `alpha_mean`
- `DdsTexture` preserves top-level dimensions, packed format, and ordered mip
  levels

## Binary Layout

NWN header size: `20` bytes.

```text
0x00  width        u32
0x04  height       u32
0x08  channels     u32   (3 => DXT1, 4 => DXT5)
0x0C  linear_size  u32
0x10  alpha_mean   f32
```

After the header, the file stores a packed mip chain:

```text
+----------------------+
| NWN DDS header       | 20 bytes
+----------------------+
| mip level 0 blocks   |
+----------------------+
| mip level 1 blocks   |
+----------------------+
| mip level 2 blocks   |
+----------------------+
| ...                  |
+----------------------+
```

Each mip level is stored as packed DXT blocks:

- `DXT1`: 8 bytes per 4x4 block
- `DXT5`: 16 bytes per 4x4 block

## Invariants

- the typed representation preserves image dimensions, format, and mip ordering
- decode operations normalize image data to RGBA8 without mutating the stored
  compressed payload
- write operations emit NWN DDS payloads from the typed texture state rather
  than from ad hoc byte manipulation
- this is not treated as generic desktop DDS; the NWN compact header is
  first-class

## Non-goals

- act as a general-purpose desktop DDS crate
- define engine-level material or asset-loading policy

## Why This Crate Exists

There is a difference between "I can display this texture" and "I can model the
engine's stored texture representation." This crate is about the latter:

- compact header fidelity
- mip-chain fidelity
- packed block preservation
- deterministic rewrite from typed texture state
