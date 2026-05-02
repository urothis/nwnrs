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
