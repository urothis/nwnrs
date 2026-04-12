use std::{fmt, io};

use nwnrs_resman::prelude::*;
use nwnrs_resref::prelude::*;

/// Errors returned while building an in-memory resource container.
#[derive(Debug)]
pub enum ResMemFileError {
    /// Stream setup failed.
    Io(io::Error),
    /// Resource manager setup failed.
    ResMan(ResManError),
}

impl fmt::Display for ResMemFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::ResMan(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for ResMemFileError {}

impl From<io::Error> for ResMemFileError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<ResManError> for ResMemFileError {
    fn from(value: ResManError) -> Self {
        Self::ResMan(value)
    }
}

/// An in-memory resource exposed as a one-entry resource container.
#[derive(Debug, Clone)]
pub struct ResMemFile {
    pub(crate) label: String,
    pub(crate) entry: Res,
    pub(crate) len:   usize,
}

impl ResMemFile {
    /// Returns the display label used for this container.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Returns the byte length of the stored payload.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns whether the stored payload is empty.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Returns the contained resource entry.
    pub fn res(&self) -> Res {
        self.entry.clone()
    }
}

impl fmt::Display for ResMemFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ResMemFile:{}", self.label)
    }
}

impl ResContainer for ResMemFile {
    fn contains(&self, rr: &ResRef) -> bool {
        self.entry.resref() == *rr
    }

    fn demand(&self, rr: &ResRef) -> ResManResult<Res> {
        if self.contains(rr) {
            Ok(self.entry.clone())
        } else {
            Err(ResManError::Message(format!("not found: {rr}")))
        }
    }

    fn count(&self) -> usize {
        1
    }

    fn contents(&self) -> Vec<ResRef> {
        vec![self.entry.resref()]
    }
}

/// Result type for in-memory resource operations.
pub type ResMemFileResult<T> = Result<T, ResMemFileError>;
