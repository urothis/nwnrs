use std::{
    fmt,
    io::{self, Read, Seek, SeekFrom},
    sync::{Arc, Mutex, MutexGuard},
    time::SystemTime,
};

use nwnrs_checksums::prelude::*;
use nwnrs_compressedbuf::prelude::*;
use nwnrs_exo::prelude::*;
use nwnrs_resref::prelude::*;
use tracing::instrument;

/// Maximum payload size that [`Res::read_all`] will retain in the per-resource
/// cache.
pub const MEMORY_CACHE_THRESHOLD: usize = 1024 * 1024;

/// Convenience trait alias for readable, seekable streams.
pub trait ReadSeek: Read + Seek {}
impl<T: Read + Seek> ReadSeek for T {}

/// Shared stream handle used by stream-backed [`Res`] values.
pub type SharedReadSeek = Arc<Mutex<Box<dyn ReadSeek + Send>>>;
/// Factory for creating a fresh readable, seekable stream on demand.
pub type ResIoSpawner =
    Arc<dyn Fn() -> io::Result<Box<dyn ReadSeek + Send>> + Send + Sync + 'static>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// Cache behavior for resource reads and cache-aware loaders.
pub enum CachePolicy {
    /// Use and populate any cache involved in the operation.
    Use,
    /// Bypass and do not populate any cache involved in the operation.
    Bypass,
}

impl CachePolicy {
    /// Returns `true` when caches should be consulted and populated.
    #[must_use]
    pub const fn uses_cache(self) -> bool {
        matches!(self, Self::Use)
    }
}

#[derive(Debug)]
/// Errors returned by resource-container and resource-manager operations.
pub enum ResManError {
    /// An underlying IO operation failed.
    Io(io::Error),
    /// A compressed EXO payload could not be decoded.
    CompressedBuf(CompressedBufError),
    /// The requested resource could not be resolved or interpreted.
    Message(String),
}

impl ResManError {
    pub(crate) fn msg(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }
}

impl fmt::Display for ResManError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::CompressedBuf(error) => error.fmt(f),
            Self::Message(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for ResManError {}

impl From<io::Error> for ResManError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<CompressedBufError> for ResManError {
    fn from(value: CompressedBufError) -> Self {
        Self::CompressedBuf(value)
    }
}

/// Result type for resource-manager operations.
pub type ResManResult<T> = Result<T, ResManError>;

#[derive(Debug, Clone, PartialEq, Eq)]
/// Human-readable origin information for a [`Res`].
///
/// The origin is used for error messages and debug output rather than for
/// identity.
pub struct ResOrigin {
    container: String,
    label:     String,
}

impl ResOrigin {
    /// Creates a new origin description.
    pub fn new(container: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            container: container.into(),
            label:     label.into(),
        }
    }

    /// Returns the high-level container name.
    #[must_use]
    pub fn container(&self) -> &str {
        &self.container
    }

    /// Returns the container-local label for the resource.
    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }
}

impl fmt::Display for ResOrigin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.label.is_empty() {
            f.write_str(&self.container)
        } else {
            write!(f, "{}({})", self.container, self.label)
        }
    }
}

pub(crate) enum ResBacking {
    Shared(SharedReadSeek),
    Spawned(ResIoSpawner),
}

pub(crate) struct ResMutableState {
    pub cached: bool,
    pub cache:  Vec<u8>,
    pub sha1:   SecureHash,
}

pub(crate) struct ResInner {
    pub mtime:                    SystemTime,
    pub io_offset:                u64,
    pub io_size:                  i64,
    pub resref:                   ResRef,
    pub compression:              ExoResFileCompressionType,
    pub compressed_buf_algorithm: Option<Algorithm>,
    pub uncompressed_size:        usize,
    pub origin:                   ResOrigin,
    pub backing:                  ResBacking,
    pub state:                    Mutex<ResMutableState>,
}

#[derive(Clone)]
/// A lazily readable NWN resource payload.
///
/// A `Res` remembers where a payload lives, how large it is, whether it must be
/// decompressed, and how to reopen or share the underlying stream. Cloning a
/// `Res` is cheap.
pub struct Res {
    pub(crate) inner: Arc<ResInner>,
}

impl fmt::Debug for Res {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Res")
            .field("resref", &self.resref())
            .field("origin", &self.origin())
            .field("io_offset", &self.io_offset())
            .field("io_size", &self.io_size())
            .field("compression", &self.compression_algorithm())
            .finish()
    }
}

impl fmt::Display for Res {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.resref(), self.origin())
    }
}

impl Res {
    #[allow(clippy::too_many_arguments)]
    /// Creates a resource backed by a shared stream handle.
    pub fn new_with_stream(
        origin: ResOrigin,
        resref: ResRef,
        mtime: SystemTime,
        io: SharedReadSeek,
        io_size: i64,
        io_offset: u64,
        compression: ExoResFileCompressionType,
        compressed_buf_algorithm: Option<Algorithm>,
        uncompressed_size: usize,
        sha1: SecureHash,
    ) -> Self {
        Self::new(
            origin,
            resref,
            mtime,
            ResBacking::Shared(io),
            io_size,
            io_offset,
            compression,
            compressed_buf_algorithm,
            uncompressed_size,
            sha1,
        )
    }

    #[allow(clippy::too_many_arguments)]
    /// Creates a resource backed by a stream factory.
    ///
    /// This is useful when a caller wants each read to operate on a fresh
    /// stream instead of a shared locked handle.
    pub fn new_with_spawner(
        origin: ResOrigin,
        resref: ResRef,
        mtime: SystemTime,
        io_spawner: ResIoSpawner,
        io_size: i64,
        io_offset: u64,
        compression: ExoResFileCompressionType,
        compressed_buf_algorithm: Option<Algorithm>,
        uncompressed_size: usize,
        sha1: SecureHash,
    ) -> Self {
        Self::new(
            origin,
            resref,
            mtime,
            ResBacking::Spawned(io_spawner),
            io_size,
            io_offset,
            compression,
            compressed_buf_algorithm,
            uncompressed_size,
            sha1,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn new(
        origin: ResOrigin,
        resref: ResRef,
        mtime: SystemTime,
        backing: ResBacking,
        io_size: i64,
        io_offset: u64,
        compression: ExoResFileCompressionType,
        compressed_buf_algorithm: Option<Algorithm>,
        uncompressed_size: usize,
        sha1: SecureHash,
    ) -> Self {
        let effective_uncompressed =
            if compression == ExoResFileCompressionType::None && uncompressed_size == 0 {
                usize::try_from(io_size.max(0)).unwrap_or(usize::MAX)
            } else {
                uncompressed_size
            };

        Self {
            inner: Arc::new(ResInner {
                mtime,
                io_offset,
                io_size,
                resref,
                compression,
                compressed_buf_algorithm,
                uncompressed_size: effective_uncompressed,
                origin,
                backing,
                state: Mutex::new(ResMutableState {
                    cached: false,
                    cache: Vec::new(),
                    sha1,
                }),
            }),
        }
    }

    /// Returns the resource reference identifying this payload.
    #[must_use]
    pub fn resref(&self) -> ResRef {
        self.inner.resref.clone()
    }

    /// Returns the modification time recorded for this resource.
    #[must_use]
    pub fn mtime(&self) -> SystemTime {
        self.inner.mtime
    }

    /// Returns the byte offset of the stored payload inside the backing stream.
    #[must_use]
    pub fn io_offset(&self) -> u64 {
        self.inner.io_offset
    }

    /// Returns the stored payload size in bytes.
    ///
    /// A negative value indicates that the payload should be read until
    /// end-of-stream.
    #[must_use]
    pub fn io_size(&self) -> i64 {
        self.inner.io_size
    }

    /// Returns whether the decoded payload is currently cached in memory.
    #[must_use]
    pub fn cached(&self) -> bool {
        self.lock_state().map(|state| state.cached).unwrap_or(false)
    }

    /// Returns the expected size after decompression.
    #[must_use]
    pub fn uncompressed_size(&self) -> usize {
        self.inner.uncompressed_size
    }

    /// Returns the EXO compression marker for this payload.
    #[must_use]
    pub fn compression_algorithm(&self) -> ExoResFileCompressionType {
        self.inner.compression
    }

    /// Returns the compressed-buffer algorithm stored in an ERF payload, when
    /// known.
    #[must_use]
    pub fn compressed_buf_algorithm(&self) -> Option<Algorithm> {
        self.inner.compressed_buf_algorithm
    }

    /// Returns the descriptive origin for this payload.
    #[must_use]
    pub fn origin(&self) -> ResOrigin {
        self.inner.origin.clone()
    }

    /// Returns `true` when this resource is backed by a shared stream handle.
    #[must_use]
    pub fn io_owned(&self) -> bool {
        matches!(self.inner.backing, ResBacking::Shared(_))
    }

    /// Seeks the underlying stream to the start of this payload.
    ///
    /// This is mainly useful for callers performing manual reads with
    /// [`with_stream`](Self::with_stream).
    #[instrument(level = "debug", skip_all, err, fields(resref = %self.inner.resref))]
    pub fn seek(&self) -> ResManResult<()> {
        self.with_stream(|stream| {
            stream.seek(SeekFrom::Start(self.inner.io_offset))?;
            Ok(())
        })
    }

    /// Reads the full payload, decompressing it when required.
    ///
    /// When [`CachePolicy::Use`] is selected, small decoded payloads are
    /// retained in memory.
    #[instrument(level = "debug", skip_all, err, fields(resref = %self.inner.resref, cache_policy = ?cache_policy))]
    pub fn read_all(&self, cache_policy: CachePolicy) -> ResManResult<Vec<u8>> {
        if cache_policy.uses_cache() {
            let state = self.lock_state()?;
            if state.cached {
                return Ok(state.cache.clone());
            }
        }

        let raw = self.read_raw()?;
        let data = match self.inner.compression {
            ExoResFileCompressionType::None => raw,
            ExoResFileCompressionType::CompressedBuf => {
                decompress_bytes(&raw, EXO_RES_FILE_COMPRESSED_BUF_MAGIC)?
            }
        };

        if cache_policy.uses_cache() && data.len() < MEMORY_CACHE_THRESHOLD {
            let mut state = self.lock_state()?;
            state.cached = true;
            state.cache.clone_from(&data);
        }

        Ok(data)
    }

    /// Returns the SHA-1 digest for the decoded payload.
    ///
    /// If the digest was not provided by the container, it is computed lazily
    /// and cached.
    #[instrument(level = "debug", skip_all, err, fields(resref = %self.inner.resref))]
    pub fn sha1(&self) -> ResManResult<SecureHash> {
        {
            let state = self.lock_state()?;
            if state.sha1 != EMPTY_SECURE_HASH {
                return Ok(state.sha1);
            }
        }

        let digest = secure_hash(self.read_all(CachePolicy::Use)?);
        let mut state = self.lock_state()?;
        state.sha1 = digest;
        Ok(digest)
    }

    /// Runs `op` against the underlying stream.
    ///
    /// Shared-stream resources lock the stream for the duration of the
    /// callback. Spawned resources create a fresh stream for the call.
    #[instrument(level = "debug", skip_all, err, fields(resref = %self.inner.resref))]
    pub fn with_stream<T, F>(&self, op: F) -> ResManResult<T>
    where
        F: FnOnce(&mut dyn ReadSeek) -> ResManResult<T>,
    {
        match &self.inner.backing {
            ResBacking::Shared(stream) => {
                let mut stream = stream.lock().map_err(|error| {
                    ResManError::msg(format!("shared res stream lock poisoned: {error}"))
                })?;
                op(stream.as_mut())
            }
            ResBacking::Spawned(spawner) => {
                let mut stream = spawner()?;
                op(stream.as_mut())
            }
        }
    }

    fn read_raw(&self) -> ResManResult<Vec<u8>> {
        self.with_stream(|stream| {
            stream.seek(SeekFrom::Start(self.inner.io_offset))?;
            if self.inner.io_size < 0 {
                let mut data = Vec::new();
                stream.read_to_end(&mut data)?;
                Ok(data)
            } else {
                let data_len = usize::try_from(self.inner.io_size).map_err(|error| {
                    ResManError::msg(format!("resource size out of range: {error}"))
                })?;
                let mut data = vec![0_u8; data_len];
                stream.read_exact(&mut data)?;
                Ok(data)
            }
        })
    }

    fn lock_state(&self) -> ResManResult<MutexGuard<'_, ResMutableState>> {
        self.inner
            .state
            .lock()
            .map_err(|error| ResManError::msg(format!("res state lock poisoned: {error}")))
    }
}

/// Trait implemented by all resource containers in the workspace.
pub trait ResContainer: fmt::Display + Send + Sync {
    /// Returns whether the container can resolve `rr`.
    fn contains(&self, rr: &ResRef) -> bool;
    /// Returns the resource identified by `rr` or an error when it is absent.
    fn demand(&self, rr: &ResRef) -> ResManResult<Res>;
    /// Returns the number of resources exposed by the container.
    fn count(&self) -> usize;
    /// Returns every resource reference exposed by the container.
    fn contents(&self) -> Vec<ResRef>;
}

/// Convenience constructor for [`ResOrigin`].
pub fn new_res_origin(container: impl Into<String>, label: impl Into<String>) -> ResOrigin {
    ResOrigin::new(container, label)
}

/// Wraps a stream in the shared-handle type used by stream-backed resources.
pub fn shared_stream<T>(stream: T) -> SharedReadSeek
where
    T: ReadSeek + Send + 'static,
{
    Arc::new(Mutex::new(Box::new(stream)))
}
