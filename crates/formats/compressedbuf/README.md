# nwnrs-compressedbuf

Reader and writer for the EXO compressed-buffer wrapper.

## Scope

- parse the wrapper header, compression algorithm tag, and declared output size
- decompress wrapped payloads from byte slices or generic readers
- compress payloads back into the same wrapper format

The main entry points are [`read_payload_bytes`], [`read_payload_reader`],
[`write_payload_bytes`], and [`write_payload_writer`].

## Invariants

- the wrapper magic, algorithm tag, and declared uncompressed size remain
  explicit typed fields
- compression and decompression operate on the framed payload, not on an
  inferred container format

## Non-goals

- infer the semantic type of the wrapped payload
- replace higher-level crates that parse the decompressed content
