use std::{error::Error, fmt};

use nwnrs_io::prelude::*;
use serde::{Deserialize, Serialize};

use crate::lookup_res_ext;

/// A numeric NWN resource type identifier.
///
/// Formatting consults the registered extension table when one exists, but the
/// typed value itself remains the numeric kind id used by NWN file formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ResType(pub u16);

/// Errors returned when registering a custom resource type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RegisterResTypeError {
    /// The extension violated a registry invariant.
    Expectation(ExpectationError),
    /// The extension contained unsupported characters.
    InvalidCharacters(String),
}

impl fmt::Display for RegisterResTypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Expectation(error) => error.fmt(f),
            Self::InvalidCharacters(ext) => {
                write!(f, "ResType {ext:?} contains invalid characters")
            }
        }
    }
}

impl Error for RegisterResTypeError {}

impl From<ExpectationError> for RegisterResTypeError {
    fn from(value: ExpectationError) -> Self {
        Self::Expectation(value)
    }
}

impl fmt::Display for ResType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(ext) = lookup_res_ext(*self) {
            f.write_str(&ext)
        } else {
            write!(f, "{}", self.0)
        }
    }
}
