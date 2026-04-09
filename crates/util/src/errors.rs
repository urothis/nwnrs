use std::{error::Error, fmt};

use serde::{Deserialize, Serialize};

/// An error returned when an expected condition is not met.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpectationError {
    message: String,
}

impl ExpectationError {
    /// Creates a new expectation error with the given message.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for ExpectationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for ExpectationError {}

/// An error returned when an encoding label cannot be resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownEncodingError {
    label: String,
}

impl UnknownEncodingError {
    /// Creates a new unknown-encoding error.
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
        }
    }
}

impl fmt::Display for UnknownEncodingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown encoding: {}", self.label)
    }
}

impl Error for UnknownEncodingError {}

/// An error returned when a text conversion fails for a configured encoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodingConversionError {
    encoding: String,
    operation: &'static str,
}

impl EncodingConversionError {
    pub(crate) fn new(encoding: impl Into<String>, operation: &'static str) -> Self {
        Self {
            encoding: encoding.into(),
            operation,
        }
    }
}

impl fmt::Display for EncodingConversionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "failed to {} with encoding {}",
            self.operation, self.encoding
        )
    }
}

impl Error for EncodingConversionError {}

/// An error returned when the native system encoding cannot be determined.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeEncodingError {
    message: String,
}

impl NativeEncodingError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for NativeEncodingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for NativeEncodingError {}
