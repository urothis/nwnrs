# Archives, Compression, and Sync

This family covers framed payloads, archive containers, and distribution
metadata.

Dedicated chapters:

- [Compressed Buffers](./formats-compressedbuf.md)
- [ERF Archives](./formats-erf.md)
- [KEY and BIF](./formats-key.md)
- [NWSync Manifests](./formats-nwsync.md)

This is the family where "resource identity" and "physical storage" diverge
most sharply:

- compressed-buffer framing is not the same thing as the payload format
- archive membership is not the same thing as resource precedence
- KEY indexing is not the same thing as BIF storage
- an `NWSync` manifest is not the same thing as an `NWSync` repository

Keeping those separations clear is what makes the rest of the stack tractable.
