# Dialog Tables (TLK)

Docs:

- [crate docs](https://docs.rs/nwnrs-tlk/latest/nwnrs_tlk/)
- [README](https://github.com/urothis/nwnrs/blob/main/crates/formats/tlk/README.md)
- [types.rs](https://github.com/urothis/nwnrs/blob/main/crates/formats/tlk/src/types.rs)
- [io.rs](https://github.com/urothis/nwnrs/blob/main/crates/formats/tlk/src/io.rs)

`TLK` is the string table for the engine, but that undersells it. It is not
just text storage. Each entry also carries metadata for voice references,
playback properties, and raw descriptor fidelity.

## Public Surface

- `SingleTlk`
- `Tlk`
- `TlkEntry`
- `TlkPair`
- `TlkLayerWriteTarget`
- `TlkWriteStream`
- `HEADER_SIZE`
- `DATA_ELEMENT_SIZE`
- `TlkError`
- `TlkResult`
- `read_single_tlk`
- `write_single_tlk`
- `write_tlk_chain`

## Core Model

- `TlkEntry` preserves:
  - `text`
  - `raw_text`
  - `sound_res_ref`
  - `raw_sound_res_ref`
  - `sound_length`
  - `sound_length_bits`
  - flags
  - volume variance
  - pitch variance
- `SingleTlk` is one standalone table.
- `Tlk` is the layered male/female lookup abstraction built from one or more
  `TlkPair` values.

## Binary Layout

The crate models `TLK V3.0`.

Header:

```text
0x00  "TLK "
0x04  "V3.0"
0x08  language_id     u32
0x0C  entry_count     u32
0x10  string_offset   u32

total header size: 20 bytes
```

Entry descriptors follow immediately:

```text
+----------------------+
| TLK header           | 20 bytes
+----------------------+
| entry descriptor 0   | 40 bytes
+----------------------+
| entry descriptor 1   | 40 bytes
+----------------------+
| ...                  |
+----------------------+
| string blob          | variable
+----------------------+
```

Descriptor shape:

```text
flags                u32
sound_resref[16]     bytes
volume_variance      u32
pitch_variance       u32
string_offset        u32
string_length        u32
sound_length         f32
```

Conceptually, each entry descriptor names a slice inside the trailing string
blob.

## Logical Edges

- String references are stable numeric indices. They are not content hashes or
  symbolic names.
- Raw sound-resref bytes are preserved when they still agree with the typed
  value.
- `sound_length_bits` exist because floating-point metadata is not always
  something you want to rewrite from scratch.
- Stream-backed `SingleTlk` avoids forcing the entire string blob into memory.
- `Tlk` layering preserves precedence exactly as given; it does not invent a
  new merge policy.

## Tricky Parts

- The stored string table is one large blob plus descriptor offsets, not one
  self-delimiting record per entry.
- The male/female distinction is not a property of the physical `TLK` file
  format. It is a higher-level lookup abstraction over paired tables.
- A typed entry and a rewrite-stable entry are related but not identical goals.
  This crate tries to preserve descriptor fidelity where possible.

## Why This Crate Exists

There are two distinct technical problems here:

1. parse one `TLK` file correctly and preserve its descriptor semantics
2. expose the install-level layered lookup model that real consumers need

The crate does both, but it does not blur them together. `SingleTlk` is the
file. `Tlk` is the lookup system.
