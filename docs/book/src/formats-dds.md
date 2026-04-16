# DDS Textures

Docs:

- [crate docs](https://docs.rs/nwnrs-dds/latest/nwnrs_dds/)
- [README](https://github.com/urothis/nwnrs/blob/main/crates/formats/dds/README.md)
- [source](https://github.com/urothis/nwnrs/blob/main/crates/formats/dds/src/lib.rs)

This crate models the compact NWN `DDS` layout, not the general desktop DDS
ecosystem.

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

- `DdsFormat` currently distinguishes `Dxt1` and `Dxt5`.
- `NwnDdsHeader` preserves:
  - `width`
  - `height`
  - `channels`
  - `linear_size`
  - `alpha_mean`
- `DdsTexture` preserves top-level dimensions, packed format, and ordered mip
  levels.

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

## Logical Edges

- This is not treated as generic DDS. The NWN compact header is first-class.
- `channels` is a file-level marker, not just redundant metadata.
- Mip ordering is preserved exactly.
- Decode-to-RGBA8 is a convenience operation over packed blocks, not the stored
  truth.

## Why This Crate Exists

There is a difference between "I can display this texture" and "I can model the
engine's stored texture representation." This crate is about the latter:

- compact header fidelity
- mip-chain fidelity
- packed block preservation
- deterministic rewrite from typed texture state
