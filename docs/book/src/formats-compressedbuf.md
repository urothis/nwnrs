# Compressed Buffers

Docs:

- [crate docs](https://docs.rs/nwnrs-compressedbuf/latest/nwnrs_compressedbuf/)
- [README](https://github.com/urothis/nwnrs/blob/main/crates/formats/compressedbuf/README.md)
- [types.rs](https://github.com/urothis/nwnrs/blob/main/crates/formats/compressedbuf/src/types.rs)
- [io.rs](https://github.com/urothis/nwnrs/blob/main/crates/formats/compressedbuf/src/io.rs)

This crate models the EXO compressed-buffer wrapper as a first-class framed
payload.

## Public Surface

- `Algorithm`
- `AlgorithmHeader`
- `CompressedBufPayload`
- `CompressedBufError`
- `CompressedBufResult`
- `make_magic`
- `read_payload_bytes`
- `read_payload_reader`
- `write_payload_bytes`
- `write_payload_writer`
- `compress_bytes`
- `compress_reader`
- `compress_writer`
- `decompress_bytes`
- `decompress_reader`

## Core Model

- `Algorithm`
  - `None`
  - `Zlib`
  - `Zstd`
- `AlgorithmHeader`
  - algorithm-specific side fields
- `CompressedBufPayload`
  - `magic`
  - `header_version`
  - `algorithm`
  - `algorithm_header`
  - uncompressed `data`
  - optional `original_bytes`

## Binary Layout

Common prefix:

```text
0x00  magic             u32
0x04  header_version    u32
0x08  algorithm         u32
0x0C  uncompressed_len  u32
```

Then per algorithm:

```text
Algorithm::None
  payload bytes

Algorithm::Zlib
  zlib_header_version u32
  zlib-compressed bytes

Algorithm::Zstd
  zstd_header_version u32
  dictionary_marker   u32
  zstd-compressed bytes
```

Conceptually:

```text
+----------------------+
| common wrapper       |
+----------------------+
| algorithm header     | depends on algorithm
+----------------------+
| compressed payload   |
+----------------------+
```

## Logical Edges

- The wrapper is not the payload format.
- The declared uncompressed size is part of the format contract.
- The algorithm header must match the selected algorithm.
- `original_bytes` can be replayed when reparsing proves the typed payload is
  identical.

## Why This Crate Exists

Compression framing shows up inside other formats, but that does not mean it
should be reimplemented ad hoc inside every container parser. This crate keeps
the frame semantics isolated:

- magic
- version
- algorithm tag
- algorithm-specific header fields
- decompressed payload bytes
