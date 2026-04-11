use std::{fmt, io};

use nwnrs_util::prelude::*;

pub(crate) const VERSION: u32 = 3;
pub(crate) const ZLIB_VERSION: u32 = 1;
pub(crate) const ZSTD_VERSION: u32 = 1;

/// Compression algorithms supported by the compressed buffer format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum Algorithm {
    /// Stores the payload without compression.
    None = 0,
    /// Stores the payload compressed with zlib.
    Zlib = 1,
    /// Stores the payload compressed with Zstandard.
    Zstd = 2,
}

impl Algorithm {
    /// Converts a raw numeric marker into a compression algorithm.
    pub fn from_u32(value: u32) -> Result<Self, CompressedBufError> {
        Ok(match value {
            0 => Self::None,
            1 => Self::Zlib,
            2 => Self::Zstd,
            _ => {
                return Err(CompressedBufError::msg(format!(
                    "unsupported compression algorithm: {value}"
                )));
            }
        })
    }
}

/// Errors returned while reading or writing compressed buffer payloads.
#[derive(Debug)]
pub enum CompressedBufError {
    /// An underlying IO error occurred.
    Io(io::Error),
    /// A format invariant was violated.
    Expectation(ExpectationError),
    /// The payload could not be interpreted.
    Message(String),
}

impl CompressedBufError {
    pub(crate) fn msg(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }
}

impl fmt::Display for CompressedBufError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::Expectation(error) => error.fmt(f),
            Self::Message(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for CompressedBufError {}

impl From<io::Error> for CompressedBufError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<ExpectationError> for CompressedBufError {
    fn from(value: ExpectationError) -> Self {
        Self::Expectation(value)
    }
}

/// A result alias for compressed buffer operations.
pub type CompressedBufResult<T> = Result<T, CompressedBufError>;

/// Additional header fields that depend on the compression algorithm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AlgorithmHeader {
    /// No algorithm-specific header fields.
    None,
    /// Zlib payload header version.
    Zlib {
        /// The stored zlib wrapper version.
        version: u32,
    },
    /// Zstd payload header version and dictionary marker.
    Zstd {
        /// The stored zstd wrapper version.
        version:    u32,
        /// The stored zstd dictionary marker.
        dictionary: u32,
    },
}

impl AlgorithmHeader {
    pub(crate) fn canonical_for(algorithm: Algorithm) -> Self {
        match algorithm {
            Algorithm::None => Self::None,
            Algorithm::Zlib => Self::Zlib {
                version: ZLIB_VERSION,
            },
            Algorithm::Zstd => Self::Zstd {
                version:    ZSTD_VERSION,
                dictionary: 0,
            },
        }
    }
}

/// A provenance-rich compressed-buffer payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompressedBufPayload {
    /// Four-byte magic encoded in the wrapper header.
    pub magic:            u32,
    /// Wrapper header version.
    pub header_version:   u32,
    /// Compression algorithm.
    pub algorithm:        Algorithm,
    /// Algorithm-specific header fields.
    pub algorithm_header: AlgorithmHeader,
    /// Uncompressed payload bytes.
    pub data:             Vec<u8>,
    /// Original encoded bytes when this payload came from `read_payload_*`.
    pub original_bytes:   Option<Vec<u8>>,
}

impl CompressedBufPayload {
    /// Creates a canonical new payload with no preserved source bytes.
    pub fn new(magic: u32, algorithm: Algorithm, data: Vec<u8>) -> Self {
        Self {
            magic,
            header_version: VERSION,
            algorithm,
            algorithm_header: AlgorithmHeader::canonical_for(algorithm),
            data,
            original_bytes: None,
        }
    }
}
