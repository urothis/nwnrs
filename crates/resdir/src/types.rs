use indexmap::IndexMap;
use nwn_resman::{Res, ResContainer, ResManError, ResManResult};
use nwn_resref::ResRef;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

/// Errors returned while reading a resource directory.
#[derive(Debug)]
pub enum ResDirError {
    /// Filesystem access failed.
    Io(io::Error),
    /// Resource manager setup failed.
    ResMan(ResManError),
    /// Resource reference parsing failed.
    ResRef(nwn_resref::ResRefError),
    /// The directory contents were invalid.
    Message(String),
}

impl ResDirError {
    pub(crate) fn msg(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }
}

impl fmt::Display for ResDirError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::ResMan(error) => error.fmt(f),
            Self::ResRef(error) => error.fmt(f),
            Self::Message(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for ResDirError {}

impl From<io::Error> for ResDirError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<ResManError> for ResDirError {
    fn from(value: ResManError) -> Self {
        Self::ResMan(value)
    }
}

impl From<nwn_resref::ResRefError> for ResDirError {
    fn from(value: nwn_resref::ResRefError) -> Self {
        Self::ResRef(value)
    }
}

/// A directory-backed resource container.
#[derive(Debug, Clone)]
pub struct ResDir {
    pub(crate) root: PathBuf,
    pub(crate) label: String,
    pub(crate) entries: IndexMap<ResRef, Res>,
}

impl ResDir {
    /// Returns the scanned root directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Returns the display label used for this container.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Returns the indexed resource entries.
    pub fn entries(&self) -> &IndexMap<ResRef, Res> {
        &self.entries
    }
}

impl fmt::Display for ResDir {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ResDir:{}", self.label)
    }
}

impl ResContainer for ResDir {
    fn contains(&self, rr: &ResRef) -> bool {
        self.entries.contains_key(rr)
    }

    fn demand(&self, rr: &ResRef) -> ResManResult<Res> {
        self.entries
            .get(rr)
            .cloned()
            .ok_or_else(|| ResManError::Message(format!("not found: {rr}")))
    }

    fn count(&self) -> usize {
        self.entries.len()
    }

    fn contents(&self) -> Vec<ResRef> {
        self.entries.keys().cloned().collect()
    }
}

/// Result type for resource directory operations.
pub type ResDirResult<T> = Result<T, ResDirError>;
