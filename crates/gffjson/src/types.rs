use nwn_gff::GffError;
use nwn_util::ExpectationError;
use std::error::Error;
use std::fmt;

pub(crate) const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Errors returned while converting between GFF and JSON.
#[derive(Debug)]
pub enum GffJsonError {
    /// GFF parsing or validation failed.
    Gff(GffError),
    /// JSON parsing or serialization failed.
    Json(serde_json::Error),
    /// An expected shape or value was missing.
    Expectation(ExpectationError),
    /// The input data was invalid.
    Message(String),
}

impl GffJsonError {
    pub(crate) fn msg(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }
}

impl fmt::Display for GffJsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Gff(error) => error.fmt(f),
            Self::Json(error) => error.fmt(f),
            Self::Expectation(error) => error.fmt(f),
            Self::Message(message) => f.write_str(message),
        }
    }
}

impl Error for GffJsonError {}

impl From<GffError> for GffJsonError {
    fn from(value: GffError) -> Self {
        Self::Gff(value)
    }
}

impl From<serde_json::Error> for GffJsonError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

impl From<ExpectationError> for GffJsonError {
    fn from(value: ExpectationError) -> Self {
        Self::Expectation(value)
    }
}

/// Result type for GFF JSON operations.
pub type GffJsonResult<T> = Result<T, GffJsonError>;
