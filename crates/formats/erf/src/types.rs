use std::{
    collections::{BTreeMap, hash_map::RandomState},
    fmt, io,
    time::SystemTime,
};

use indexmap::IndexMap;
use nwnrs_compressedbuf::prelude::*;
use nwnrs_encoding::prelude::*;
use nwnrs_io::prelude::*;
use nwnrs_resman::prelude::*;
use nwnrs_resref::prelude::*;

pub(crate) const HEADER_SIZE: u64 = 160;
pub(crate) const VALID_ERF_TYPES: [&str; 4] = ["NWM ", "MOD ", "ERF ", "HAK "];

#[derive(Debug)]
/// Errors returned while reading or writing ERF-family archives.
pub enum ErfError {
    /// An underlying IO operation failed.
    Io(io::Error),
    /// Resource manager setup failed while constructing archive-backed [`Res`]
    /// entries.
    ResMan(ResManError),
    /// A resource reference inside the archive was invalid.
    ResRef(ResRefError),
    /// A compressed payload could not be decoded or encoded.
    Compression(CompressedBufError),
    /// A format invariant was violated.
    Expectation(ExpectationError),
    /// Text could not be converted using the configured NWN encoding.
    Encoding(EncodingConversionError),
    /// The archive contents were otherwise invalid.
    Message(String),
}

impl ErfError {
    pub(crate) fn msg(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }
}

impl fmt::Display for ErfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::ResMan(error) => error.fmt(f),
            Self::ResRef(error) => error.fmt(f),
            Self::Compression(error) => error.fmt(f),
            Self::Expectation(error) => error.fmt(f),
            Self::Encoding(error) => error.fmt(f),
            Self::Message(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for ErfError {}

impl From<io::Error> for ErfError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<ResManError> for ErfError {
    fn from(value: ResManError) -> Self {
        Self::ResMan(value)
    }
}

impl From<ResRefError> for ErfError {
    fn from(value: ResRefError) -> Self {
        Self::ResRef(value)
    }
}

impl From<CompressedBufError> for ErfError {
    fn from(value: CompressedBufError) -> Self {
        Self::Compression(value)
    }
}

impl From<ExpectationError> for ErfError {
    fn from(value: ExpectationError) -> Self {
        Self::Expectation(value)
    }
}

impl From<EncodingConversionError> for ErfError {
    fn from(value: EncodingConversionError) -> Self {
        Self::Encoding(value)
    }
}

/// Result type for ERF operations.
pub type ErfResult<T> = Result<T, ErfError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Supported ERF-family versions.
pub enum ErfVersion {
    /// Legacy archive layout without per-entry compression metadata.
    V1,
    /// Enhanced-edition layout with optional per-entry compression and an
    /// archive OID.
    E1,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
/// Optional layout controls for ERF-family archive writes.
pub struct ErfWriteOptions {
    /// Explicit padding to preserve between the key list and resource list.
    pub resource_list_padding: u64,
}

#[derive(Debug, Clone)]
/// A decoded ERF-family archive.
///
/// The entry map preserves archive order and the archive itself implements
/// [`nwnrs_resman::ResContainer`] for use with [`nwnrs_resman::ResMan`].
///
/// The typed value preserves archive identity, stored entry order, and archive
/// header metadata without requiring callers to choose between inspection and
/// container-style lookup.
pub struct Erf {
    /// The archive modification time when known.
    pub mtime: SystemTime,
    /// The four-byte archive type tag such as `ERF `, `MOD `, `HAK `, or `NWM
    /// `.
    pub file_type: String,
    /// The archive version.
    pub file_version: ErfVersion,
    pub(crate) filename: String,
    /// Build year stored in the archive header.
    pub build_year: i32,
    /// Build day stored in the archive header.
    pub build_day: i32,
    /// Localized string reference stored in the archive header.
    pub str_ref: i32,
    pub(crate) loc_strings: BTreeMap<i32, String>,
    pub(crate) entries: IndexMap<ResRef, Res, RandomState>,
    pub(crate) oid: Option<String>,
    pub(crate) resource_list_padding: u64,
}

impl Erf {
    /// Returns the display filename associated with this archive.
    pub fn filename(&self) -> &str {
        &self.filename
    }

    /// Returns the localized strings stored in the archive header.
    pub fn loc_strings(&self) -> &BTreeMap<i32, String> {
        &self.loc_strings
    }

    /// Returns the archive entries in stored order.
    pub fn entries(&self) -> &IndexMap<ResRef, Res, RandomState> {
        &self.entries
    }

    /// Returns the enhanced-edition archive OID when present.
    pub fn oid(&self) -> Option<&str> {
        self.oid.as_deref()
    }

    /// Returns the preserved padding between the key list and resource list.
    pub fn resource_list_padding(&self) -> u64 {
        self.resource_list_padding
    }
}

impl fmt::Display for Erf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Erf:{}", self.filename)
    }
}

impl ResContainer for Erf {
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

#[derive(Debug)]
pub(crate) struct ErfResMeta {
    pub offset:            u64,
    pub disk_size:         usize,
    pub uncompressed_size: usize,
    pub compression:       nwnrs_exo::ExoResFileCompressionType,
}
