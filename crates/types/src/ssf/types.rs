use std::{fmt, io};

use nwnrs_types::localization::prelude::*;
use serde::{Deserialize, Serialize};

pub(crate) const HEADER_MAGIC: &str = "SSF ";
pub(crate) const HEADER_VERSION: &str = "V1.0";
pub(crate) const TABLE_OFFSET: u32 = 40;
pub(crate) const ENTRY_DATA_SIZE: usize = 20;

#[derive(Debug)]
/// Errors returned while reading or writing SSF data.
pub enum SsfError {
    /// An underlying IO operation failed.
    Io(io::Error),
    /// The SSF contents were otherwise invalid.
    Message(String),
}

impl fmt::Display for SsfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::Message(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for SsfError {}

impl From<io::Error> for SsfError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

/// Result type for SSF operations.
pub type SsfResult<T> = Result<T, SsfError>;

/// A single soundset slot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SsfEntry {
    /// The raw 16-byte resref slot as stored on disk.
    pub raw_resref: [u8; 16],
    /// The sound resource reference stored for the slot.
    pub resref:     String,
    /// The localized string reference associated with the slot.
    pub strref:     StrRef,
}

impl SsfEntry {
    /// Creates a canonical SSF slot with a zero-padded resref encoding.
    pub fn new(resref: impl Into<String>, strref: StrRef) -> Self {
        let resref = resref.into();
        let mut raw_resref = [0_u8; 16];
        let bytes = resref.as_bytes();
        let count = bytes.len().min(raw_resref.len());
        if let (Some(dst), Some(src)) = (raw_resref.get_mut(..count), bytes.get(..count)) {
            dst.copy_from_slice(src);
        }
        Self {
            raw_resref,
            resref,
            strref,
        }
    }

    pub(crate) fn stored_resref_bytes(&self) -> io::Result<[u8; 16]> {
        if decode_resref(&self.raw_resref) == self.resref {
            return Ok(self.raw_resref);
        }

        if self.resref.len() > 16 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("resref {:?} exceeds 16 bytes", self.resref),
            ));
        }

        let mut padded = [0_u8; 16];
        let bytes = self.resref.as_bytes();
        if let Some(prefix) = padded.get_mut(..bytes.len()) {
            prefix.copy_from_slice(bytes);
        }
        Ok(padded)
    }
}

/// The decoded contents of an `SSF` file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SsfRoot {
    /// The ordered soundset entries in the file.
    pub entries: Vec<SsfEntry>,
}

impl SsfRoot {
    /// Creates an empty `SSF` document.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

pub(crate) fn decode_resref(raw: &[u8]) -> String {
    let end = raw.iter().position(|byte| *byte == 0).unwrap_or(raw.len());
    String::from_utf8_lossy(raw.get(..end).unwrap_or(&[])).to_string()
}
