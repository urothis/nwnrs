use std::{collections::HashMap, fmt, io};

use nwnrs_checksums::prelude::*;
use nwnrs_resref::prelude::*;

/// The default hash tree depth for NWSync manifests.
pub const HASH_TREE_DEPTH: u32 = 2;
/// The manifest version implemented by this crate.
pub const VERSION: u32 = 3;
/// The NWSync manifest magic bytes.
pub const MAGIC: &[u8; 4] = b"NSYM";

/// Errors returned while reading or writing manifests.
#[derive(Debug)]
pub enum ManifestError {
    /// I/O failed.
    Io(io::Error),
    /// Resource reference parsing failed.
    ResRef(ResRefError),
    /// SHA-1 parsing failed.
    ParseSecureHash(ParseSecureHashError),
    /// The manifest was invalid.
    Message(String),
}

impl ManifestError {
    pub(crate) fn msg(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::ResRef(error) => error.fmt(f),
            Self::ParseSecureHash(error) => error.fmt(f),
            Self::Message(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for ManifestError {}

impl From<io::Error> for ManifestError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<ResRefError> for ManifestError {
    fn from(value: ResRefError) -> Self {
        Self::ResRef(value)
    }
}

impl From<ParseSecureHashError> for ManifestError {
    fn from(value: ParseSecureHashError) -> Self {
        Self::ParseSecureHash(value)
    }
}

/// Result type for manifest operations.
pub type ManifestResult<T> = Result<T, ManifestError>;

/// A single manifest resource mapping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestEntry {
    /// The content hash stored for this entry.
    pub sha1:   SecureHash,
    /// The uncompressed payload size.
    pub size:   u32,
    /// The resource reference exposed by this entry.
    pub resref: ResRef,
}

impl ManifestEntry {
    /// Creates a new manifest entry.
    #[must_use]
    pub fn new(sha1: SecureHash, size: u32, resref: ResRef) -> Self {
        Self {
            sha1,
            size,
            resref,
        }
    }
}

impl fmt::Display for ManifestEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.sha1, self.resref)
    }
}

/// A parsed NWSync manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub(crate) version:         u32,
    pub(crate) hash_tree_depth: u32,
    /// The manifest entries in their stored order.
    pub entries:                Vec<ManifestEntry>,
}

impl Default for Manifest {
    fn default() -> Self {
        Self::new(HASH_TREE_DEPTH)
    }
}

impl Manifest {
    /// Creates a new empty manifest.
    #[must_use]
    pub fn new(hash_tree_depth: u32) -> Self {
        Self {
            version: VERSION,
            hash_tree_depth,
            entries: Vec::new(),
        }
    }

    /// Returns the stored manifest version.
    #[must_use]
    pub fn version(&self) -> u32 {
        self.version
    }

    /// Returns the configured hash tree depth.
    pub fn hash_tree_depth(&self) -> ManifestResult<usize> {
        match self.version {
            VERSION => Ok(self.hash_tree_depth as usize),
            2 => Ok(2),
            _ => Err(ManifestError::msg("Unsupported manifest version")),
        }
    }

    /// Returns the manifest hashing algorithm label.
    pub fn algorithm(&self) -> ManifestResult<&'static str> {
        if self.version == VERSION {
            Ok("SHA1")
        } else {
            Err(ManifestError::msg("Unsupported manifest version"))
        }
    }

    /// Returns the manifest entries.
    #[must_use]
    pub fn entries(&self) -> &[ManifestEntry] {
        &self.entries
    }

    /// Appends a manifest entry.
    pub fn add_entry(&mut self, entry: ManifestEntry) {
        self.entries.push(entry);
    }

    /// Returns the total size of all entries.
    #[must_use]
    pub fn total_size(&self) -> i64 {
        self.entries.iter().map(|entry| i64::from(entry.size)).sum()
    }

    /// Returns the deduplicated size keyed by hash.
    #[must_use]
    pub fn deduplicated_size(&self) -> i64 {
        let mut unique = HashMap::<SecureHash, u32>::new();
        for entry in &self.entries {
            unique.entry(entry.sha1).or_insert(entry.size);
        }
        unique.values().map(|size| i64::from(*size)).sum()
    }
}
