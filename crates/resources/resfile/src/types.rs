use std::{
    fmt, io,
    path::{Path, PathBuf},
};

use nwnrs_resman::prelude::*;
use nwnrs_resref::prelude::*;

/// Errors returned while reading a single-file resource container.
#[derive(Debug)]
pub enum ResFileError {
    /// Filesystem access failed.
    Io(io::Error),
    /// Resource manager setup failed.
    ResMan(ResManError),
    /// Resource reference parsing failed.
    ResRef(nwnrs_resref::ResRefError),
    /// The input path was invalid.
    Message(String),
}

impl ResFileError {
    pub(crate) fn msg(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }
}

impl fmt::Display for ResFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::ResMan(error) => error.fmt(f),
            Self::ResRef(error) => error.fmt(f),
            Self::Message(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for ResFileError {}

impl From<io::Error> for ResFileError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<ResManError> for ResFileError {
    fn from(value: ResManError) -> Self {
        Self::ResMan(value)
    }
}

impl From<nwnrs_resref::ResRefError> for ResFileError {
    fn from(value: nwnrs_resref::ResRefError) -> Self {
        Self::ResRef(value)
    }
}

/// A single file exposed as a one-entry resource container.
#[derive(Debug, Clone)]
pub struct ResFile {
    pub(crate) path:  PathBuf,
    pub(crate) label: String,
    pub(crate) entry: Res,
}

impl ResFile {
    /// Returns the underlying file path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the display label used for this container.
    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Returns the contained resource entry.
    #[must_use]
    pub fn res(&self) -> Res {
        self.entry.clone()
    }
}

impl fmt::Display for ResFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ResFile:{}", self.label)
    }
}

impl ResContainer for ResFile {
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

/// Result type for single-file resource operations.
pub type ResFileResult<T> = Result<T, ResFileError>;
