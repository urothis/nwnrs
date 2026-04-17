# nwnrs-compressedbuf

Reader and writer for the EXO compressed-buffer wrapper.

## Scope

- parse the wrapper header, compression algorithm tag, and declared uncompressed output size
- decompress wrapped payloads from byte slices or generic readers
- compress payloads back into the same wrapper format

The main entry points are [`read_payload_bytes`], [`read_payload_reader`],
[`write_payload_bytes`], and [`write_payload_writer`].

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

## Invariants

- the wrapper magic, algorithm tag, and declared uncompressed size remain
  explicit typed fields
- compression and decompression operate on the framed payload, not on an
  inferred container format
- the algorithm header must match the selected algorithm
- `original_bytes` can be replayed when reparsing proves the typed payload is
  identical

## Non-goals

- infer the semantic type of the wrapped payload
- replace higher-level crates that parse the decompressed content

## See also

- [`nwnrs-erf`](https://docs.rs/nwnrs-erf), which uses this wrapper for `E1`
  per-entry compression
- [`nwnrs-key`](https://docs.rs/nwnrs-key), which uses this wrapper for `E1`
  BIF payload compression
- [`nwnrs-exo`](https://docs.rs/nwnrs-exo), which defines the shared EXO-level
  magic and algorithm constants

## Why This Crate Exists

Compression framing shows up inside other formats, but that does not mean it
should be reimplemented ad hoc inside every container parser. This crate keeps
the frame semantics isolated:

- magic
- version
- algorithm tag
- algorithm-specific header fields
- decompressed payload bytes
