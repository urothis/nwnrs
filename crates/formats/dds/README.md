# `nwnrs-dds`

Typed Neverwinter Nights `DDS` support.

## Scope

- parse the NWN compact DDS header
- split and validate mip chains
- decode packed DXT data to top-left-origin RGBA8
- encode RGBA8 input into NWN `dxt1` or `dxt5`
- write typed NWN DDS payloads back out

The main entry points are [`read_dds`], [`write_dds`], and [`DdsTexture`].

## Invariants

- the typed representation preserves image dimensions, format, and mip ordering
- decode operations normalize image data to RGBA8 without mutating the stored
  compressed payload
- write operations emit NWN DDS payloads from the typed texture state rather
  than from ad hoc byte manipulation

## Non-goals

- act as a general-purpose desktop DDS crate
- define engine-level material or asset-loading policy
