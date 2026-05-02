#![forbid(unsafe_code)]
#![doc = include_str!("README.md")]
mod io;
mod types;

pub use io::*;
pub use types::*;

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::compressedbuf::{
        Algorithm, AlgorithmHeader, CompressedBufError, CompressedBufPayload, CompressedBufResult,
        compress_bytes, compress_reader, compress_writer, decompress_bytes, decompress_reader,
        make_magic, read_payload_bytes, read_payload_reader, write_payload_bytes,
        write_payload_writer,
    };
}
