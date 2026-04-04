use std::{fmt, io};

use nwnrs_util::ExpectationError;

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
    pub(crate) fn from_u32(value: u32) -> Result<Self, CompressedBufError> {
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
