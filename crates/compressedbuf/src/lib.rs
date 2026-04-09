#![forbid(unsafe_code)]
//! Reader and writer for the EXO compressed-buffer wrapper.
//!
//! Several NWN formats store payloads behind a small framing format that
//! records a magic value, the compression algorithm, and the expected
//! uncompressed size. This crate knows how to decode and encode that wrapper
//! for both in-memory buffers and generic streams.
//!
//! The main entry points are [`read_payload_bytes`], [`read_payload_reader`],
//! [`write_payload_bytes`], and [`write_payload_writer`].
mod io;
mod types;

pub use io::*;
pub use types::*;

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::{
        Algorithm, AlgorithmHeader, CompressedBufError, CompressedBufPayload,
        CompressedBufResult, compress_bytes, compress_reader, compress_writer, decompress_bytes,
        decompress_reader, make_magic, read_payload_bytes, read_payload_reader,
        write_payload_bytes, write_payload_writer,
    };
}
