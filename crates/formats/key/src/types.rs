use std::{
    collections::hash_map::RandomState,
    fmt, io,
    sync::{Arc, Mutex},
    time::SystemTime,
};

use indexmap::IndexMap;
use nwnrs_checksums::prelude::*;
use nwnrs_compressedbuf::prelude::*;
use nwnrs_exo::prelude::*;
use nwnrs_resman::prelude::*;
use nwnrs_resref::prelude::*;

pub(crate) const HEADER_SIZE: u64 = 64;

/// Packed KEY resource id.
///
/// The upper bits identify the owning BIF and the lower bits identify the
/// variable resource within that BIF.
pub type ResId = u32;
/// Callback used to open a referenced BIF by filename.
pub type BifResolver =
    Arc<dyn Fn(&str) -> io::Result<Option<SharedReadSeek>> + Send + Sync + 'static>;

#[derive(Debug)]
/// Errors returned while reading or writing KEY/BIF data.
pub enum KeyError {
    /// An underlying IO operation failed.
    Io(io::Error),
    /// Resource manager setup failed while constructing entries.
    ResMan(ResManError),
    /// A resource reference or filename could not be interpreted.
    ResRef(ResRefError),
    /// A compressed payload could not be decoded or encoded.
    Compression(CompressedBufError),
    /// The KEY or BIF contents were otherwise invalid.
    Message(String),
}

impl KeyError {
    pub(crate) fn msg(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }
}

impl fmt::Display for KeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::ResMan(error) => error.fmt(f),
            Self::ResRef(error) => error.fmt(f),
            Self::Compression(error) => error.fmt(f),
            Self::Message(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for KeyError {}

impl From<io::Error> for KeyError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<ResManError> for KeyError {
    fn from(value: ResManError) -> Self {
        Self::ResMan(value)
    }
}

impl From<ResRefError> for KeyError {
    fn from(value: ResRefError) -> Self {
        Self::ResRef(value)
    }
}

impl From<CompressedBufError> for KeyError {
    fn from(value: CompressedBufError) -> Self {
        Self::Compression(value)
    }
}

/// Result type for KEY/BIF operations.
pub type KeyResult<T> = Result<T, KeyError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Supported KEY/BIF versions.
pub enum KeyBifVersion {
    /// Legacy KEY/BIF layout.
    V1,
    /// Enhanced-edition KEY/BIF layout with optional compression metadata and
    /// OID support.
    E1,
}

#[derive(Debug, Clone)]
/// Metadata for a variable resource entry stored inside a BIF.
pub struct VariableResource {
    pub(crate) id:                ResId,
    pub(crate) io_offset:         u64,
    pub(crate) io_size:           usize,
    pub(crate) compression_type:  ExoResFileCompressionType,
    pub(crate) uncompressed_size: usize,
}

impl VariableResource {
    /// Returns the packed KEY resource id for this entry.
    #[must_use]
    pub fn id(&self) -> ResId {
        self.id
    }

    /// Returns the byte offset of the payload inside the BIF stream.
    #[must_use]
    pub fn io_offset(&self) -> u64 {
        self.io_offset
    }

    /// Returns the stored payload size on disk.
    #[must_use]
    pub fn io_size(&self) -> usize {
        self.io_size
    }

    /// Returns the compression marker stored for the payload.
    #[must_use]
    pub fn compression_type(&self) -> ExoResFileCompressionType {
        self.compression_type
    }

    /// Returns the expected size after decompression.
    #[must_use]
    pub fn uncompressed_size(&self) -> usize {
        self.uncompressed_size
    }
}

pub(crate) struct LoadedBif {
    pub stream:             SharedReadSeek,
    pub file_type:          String,
    pub file_version:       KeyBifVersion,
    pub variable_resources: IndexMap<ResId, VariableResource, RandomState>,
    pub oid:                Option<String>,
    pub raw_oid:            Option<String>,
}

impl fmt::Debug for LoadedBif {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LoadedBif")
            .field("file_type", &self.file_type)
            .field("file_version", &self.file_version)
            .field("variable_resources", &self.variable_resources.len())
            .field("oid", &self.oid)
            .finish()
    }
}

pub(crate) struct BifHandle {
    pub filename:          String,
    pub resolver_filename: String,
    pub expected_version:  KeyBifVersion,
    pub expected_oid:      Option<String>,
    pub drives:            u16,
    pub resolver:          BifResolver,
    pub loaded:            Mutex<Option<Arc<LoadedBif>>>,
}

impl fmt::Debug for BifHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BifHandle")
            .field("filename", &self.filename)
            .field("expected_version", &self.expected_version)
            .field("expected_oid", &self.expected_oid)
            .finish()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct KeyEntry {
    pub res_id: ResId,
    pub sha1:   SecureHash,
}

/// Decoded contents of a KEY file together with lazy BIF resolvers.
///
/// The table implements [`nwnrs_resman::ResContainer`], so it can be placed
/// directly inside a layered [`nwnrs_resman::ResMan`].
/// A decoded KEY table together with its referenced BIF handles.
///
/// The table preserves the KEY-level lookup structure explicitly: typed
/// resource references map to KEY entries, which in turn identify one BIF and
/// one variable resource id. The same typed value also implements
/// [`nwnrs_resman::ResContainer`] so callers may use it directly in layered
/// resource resolution.
pub struct KeyTable {
    pub(crate) version:          KeyBifVersion,
    pub(crate) label:            String,
    pub(crate) build_year:       u32,
    pub(crate) build_day:        u32,
    pub(crate) bifs:             Vec<BifHandle>,
    pub(crate) resref_id_lookup: IndexMap<ResRef, KeyEntry, RandomState>,
    pub(crate) oid:              Option<String>,
    pub(crate) raw_oid:          Option<String>,
}

#[derive(Debug, Clone)]
/// The resources stored in a single BIF referenced by a [`KeyTable`].
pub struct KeyBifContents {
    /// The BIF filename as recorded by the KEY file.
    pub filename:  String,
    /// The resources stored in that BIF, in table order.
    pub resources: Vec<ResRef>,
}

impl fmt::Debug for KeyTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KeyTable")
            .field("version", &self.version)
            .field("label", &self.label)
            .field("build_year", &self.build_year)
            .field("build_day", &self.build_day)
            .field("oid", &self.oid)
            .field(
                "bifs",
                &self
                    .bifs
                    .iter()
                    .map(|bif| bif.filename.clone())
                    .collect::<Vec<_>>(),
            )
            .field("entry_count", &self.resref_id_lookup.len())
            .finish()
    }
}

impl fmt::Display for KeyTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "KeyTable:{}", self.label)
    }
}

impl ResContainer for KeyTable {
    fn contains(&self, rr: &ResRef) -> bool {
        self.resref_id_lookup.contains_key(rr)
    }

    fn demand(&self, rr: &ResRef) -> ResManResult<Res> {
        let entry = self
            .resref_id_lookup
            .get(rr)
            .ok_or_else(|| ResManError::Message(format!("not found: {rr}")))?;
        let bif_idx = (entry.res_id >> 20) as usize;
        let variable_id = entry.res_id & 0x000f_ffff;
        let bif = self.bifs.get(bif_idx).ok_or_else(|| {
            ResManError::Message(format!("invalid bif index for {rr}: {bif_idx}"))
        })?;
        let loaded = bif
            .load()
            .map_err(|error| ResManError::Message(error.to_string()))?;
        let variable = loaded.variable_resources.get(&variable_id).ok_or_else(|| {
            ResManError::Message(format!(
                "keytable references non-existent id: {}",
                entry.res_id
            ))
        })?;

        Ok(Res::new_with_stream(
            new_res_origin(
                format!("KeyTable:{}", self.label),
                format!("id={} in {}", entry.res_id, bif.filename),
            ),
            rr.clone(),
            SystemTime::UNIX_EPOCH,
            loaded.stream.clone(),
            variable.io_size as i64,
            variable.io_offset,
            variable.compression_type,
            None,
            variable.uncompressed_size,
            entry.sha1,
        ))
    }

    fn count(&self) -> usize {
        self.resref_id_lookup.len()
    }

    fn contents(&self) -> Vec<ResRef> {
        self.resref_id_lookup.keys().cloned().collect()
    }
}

impl KeyTable {
    /// Returns the KEY/BIF version expected by this table.
    #[must_use]
    pub fn version(&self) -> KeyBifVersion {
        self.version
    }

    /// Returns the build year stored in the KEY header.
    #[must_use]
    pub fn build_year(&self) -> u32 {
        self.build_year
    }

    /// Returns the build day stored in the KEY header.
    #[must_use]
    pub fn build_day(&self) -> u32 {
        self.build_day
    }

    /// Returns the enhanced-edition OID when present.
    #[must_use]
    pub fn oid(&self) -> Option<&str> {
        self.oid.as_deref()
    }

    /// Returns the raw enhanced-edition OID bytes as stored in the KEY header.
    #[must_use]
    pub fn raw_oid(&self) -> Option<&str> {
        self.raw_oid.as_deref()
    }

    /// Returns the referenced BIF filenames in table order.
    #[must_use]
    pub fn bifs(&self) -> Vec<String> {
        self.bifs.iter().map(|bif| bif.filename.clone()).collect()
    }

    /// Returns the resources grouped by BIF.
    ///
    /// Calling this may lazily open referenced BIF files through the configured
    /// resolver.
    pub fn bif_contents(&self) -> KeyResult<Vec<KeyBifContents>> {
        let mut by_bif = Vec::with_capacity(self.bifs.len());

        for (bif_idx, bif) in self.bifs.iter().enumerate() {
            let loaded = bif.load()?;
            let mut resources = Vec::with_capacity(loaded.variable_resources.len());

            for local_id in loaded.variable_resources.keys() {
                let full_id = (u32::try_from(bif_idx)
                    .map_err(|_error| KeyError::msg("bif index exceeds 32-bit range"))?
                    << 20)
                    | *local_id;
                let rr = self
                    .resref_id_lookup
                    .iter()
                    .find_map(|(rr, entry)| (entry.res_id == full_id).then(|| rr.clone()))
                    .ok_or_else(|| {
                        KeyError::msg(format!(
                            "bif {} references unknown variable resource id {}",
                            bif.filename, full_id
                        ))
                    })?;
                resources.push(rr);
            }

            by_bif.push(KeyBifContents {
                filename: bif.filename.clone(),
                resources,
            });
        }

        Ok(by_bif)
    }
}

impl BifHandle {
    pub(crate) fn load(&self) -> KeyResult<Arc<LoadedBif>> {
        {
            let loaded = self
                .loaded
                .lock()
                .map_err(|error| KeyError::msg(format!("bif lock poisoned: {error}")))?;
            if let Some(loaded) = loaded.as_ref() {
                return Ok(loaded.clone());
            }
        }

        let stream = (self.resolver)(&self.resolver_filename)?.ok_or_else(|| {
            KeyError::msg(format!(
                "key file referenced file {} but cannot open",
                self.filename
            ))
        })?;
        let loaded = Arc::new(crate::io::read_bif(
            stream.clone(),
            &self.filename,
            self.expected_version,
            self.expected_oid.as_deref(),
        )?);
        *self
            .loaded
            .lock()
            .map_err(|error| KeyError::msg(format!("bif lock poisoned: {error}")))? =
            Some(loaded.clone());
        Ok(loaded)
    }
}

#[derive(Debug, Clone)]
/// Specification for a BIF to be written by [`crate::write_key_and_bif`].
pub struct KeyBifEntry {
    /// Optional directory component to prepend to the emitted BIF path.
    pub directory:         String,
    /// Basename of the emitted BIF, without the `.bif` suffix.
    pub name:              String,
    /// Exact filename/path spelling to emit into the KEY file and on disk.
    pub recorded_filename: Option<String>,
    /// Raw file-table drive flags.
    pub drives:            u16,
    /// Exact 24-byte BIF OID to emit for E1 outputs.
    pub bif_oid:           Option<String>,
    /// Resource entries that should be written into the BIF.
    pub entries:           Vec<ResRef>,
}

pub(crate) type WriteBifResult = (usize, Vec<(ResRef, SecureHash)>);
