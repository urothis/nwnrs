use std::{
    collections::HashMap,
    fmt,
    fs::File,
    io::{self, Cursor},
    path::Path,
};

use nwnrs_core::prelude::*;
use nwnrs_lru::prelude::*;
use nwnrs_resman::prelude::*;
use nwnrs_util::prelude::*;

/// Size of the fixed TLK header in bytes.
pub const HEADER_SIZE: u64 = 20;
/// Size of a single TLK entry descriptor in bytes.
pub const DATA_ELEMENT_SIZE: u64 = 40;

#[derive(Debug)]
/// Errors returned while reading, writing, or querying TLK data.
pub enum TlkError {
    /// An underlying IO operation failed.
    Io(io::Error),
    /// Resource-manager access failed.
    ResMan(ResManError),
    /// Text could not be converted using the configured NWN encoding.
    Encoding(EncodingConversionError),
    /// The TLK contents were otherwise invalid.
    Message(String),
}

impl TlkError {
    pub(crate) fn msg(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }
}

impl fmt::Display for TlkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::ResMan(error) => error.fmt(f),
            Self::Encoding(error) => error.fmt(f),
            Self::Message(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for TlkError {}

impl From<io::Error> for TlkError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<ResManError> for TlkError {
    fn from(value: ResManError) -> Self {
        Self::ResMan(value)
    }
}

impl From<EncodingConversionError> for TlkError {
    fn from(value: EncodingConversionError) -> Self {
        Self::Encoding(value)
    }
}

/// Result type for TLK operations.
pub type TlkResult<T> = Result<T, TlkError>;

#[derive(Debug, Clone, PartialEq)]
/// A single TLK entry.
pub struct TlkEntry {
    /// Localized text content.
    pub text:          String,
    /// Associated sound resource reference.
    pub sound_res_ref: String,
    /// Sound length in seconds.
    pub sound_length:  f32,
}

impl TlkEntry {
    /// Returns `true` when the entry contains either text or a sound reference.
    #[must_use] 
    pub fn has_value(&self) -> bool {
        !self.text.is_empty() || !self.sound_res_ref.is_empty()
    }
}

impl fmt::Display for TlkEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

/// A single-language TLK table.
///
/// Stream-backed instances read entries lazily and may cache decoded entries in
/// an internal weighted LRU.
pub struct SingleTlk {
    /// Language represented by the table.
    pub language: Language,
    pub(crate) static_entries: HashMap<StrRef, TlkEntry>,
    pub(crate) static_entries_highest: i32,
    pub(crate) stream: Option<SharedReadSeek>,
    pub(crate) io_start_pos: u64,
    pub(crate) io_entry_count: usize,
    pub(crate) io_entries_offset: u64,
    /// Whether lazy reads should populate the internal entry cache.
    pub use_cache: bool,
    pub(crate) io_cache: Option<WeightedLru<StrRef, TlkEntry>>,
}

impl fmt::Debug for SingleTlk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SingleTlk")
            .field("language", &self.language)
            .field("static_entries", &self.static_entries)
            .field("static_entries_highest", &self.static_entries_highest)
            .field("stream_backed", &self.stream.is_some())
            .field("io_start_pos", &self.io_start_pos)
            .field("io_entry_count", &self.io_entry_count)
            .field("io_entries_offset", &self.io_entries_offset)
            .field("use_cache", &self.use_cache)
            .field(
                "io_cache_entries",
                &self.io_cache.as_ref().map(WeightedLru::len).unwrap_or(0),
            )
            .finish()
    }
}

#[derive(Debug)]
/// A male/female TLK pair from one layer in a TLK chain.
pub struct TlkPair {
    /// Male table for the layer, when present.
    pub male:   Option<SingleTlk>,
    /// Female table for the layer, when present.
    pub female: Option<SingleTlk>,
}

#[derive(Debug, Default)]
/// Layered TLK lookup chain.
///
/// Queries walk the chain in order and return the first matching entry for the
/// requested gender.
pub struct Tlk {
    /// Ordered TLK layers from highest to lowest precedence.
    pub chain: Vec<TlkPair>,
}

impl SingleTlk {
    /// Creates an empty English TLK table.
    #[must_use] 
    pub fn new() -> Self {
        Self {
            language:               Language::English,
            static_entries:         HashMap::new(),
            static_entries_highest: -1,
            stream:                 None,
            io_start_pos:           0,
            io_entry_count:         0,
            io_entries_offset:      0,
            use_cache:              true,
            io_cache:               None,
        }
    }

    /// Opens a TLK file from disk.
    pub fn from_file(path: impl AsRef<Path>, use_cache: bool) -> TlkResult<Self> {
        let file = File::open(path.as_ref())?;
        crate::io::read_single_tlk(file, use_cache)
    }

    /// Reads a TLK payload from a [`Res`].
    pub fn from_res(res: &Res, use_cache: bool) -> TlkResult<Self> {
        let bytes = res.read_all(false)?;
        crate::io::read_single_tlk(Cursor::new(bytes), use_cache)
    }

    /// Returns the highest string reference known to this table.
    #[must_use] 
    pub fn highest(&self) -> i32 {
        let io_highest = i32::try_from(self.io_entry_count.saturating_sub(1)).unwrap_or(i32::MAX);
        io_highest.max(self.static_entries_highest)
    }

    /// Returns the entry for `str_ref`, if present.
    pub fn get(&mut self, str_ref: StrRef) -> TlkResult<Option<TlkEntry>> {
        if let Some(entry) = self.static_entries.get(&str_ref) {
            return Ok(Some(entry.clone()));
        }

        if usize::try_from(str_ref).unwrap_or(usize::MAX) >= self.io_entry_count {
            return Ok(None);
        }

        if self.use_cache
            && let Some(entry) = self
                .io_cache
                .as_mut()
                .and_then(|cache| cache.get(&str_ref).cloned())
        {
            return Ok(Some(entry));
        }

        if self.use_cache {
            let (weight, entry) = self.get_from_io(str_ref)?;
            if let Some(cache) = self.io_cache.as_mut() {
                cache.insert_weighted(str_ref, weight, entry.clone());
            }
            return Ok(Some(entry));
        }

        self.get_from_io(str_ref).map(|(_, entry)| Some(entry))
    }

    /// Replaces or inserts an entry at `str_ref`.
    pub fn set_entry(&mut self, str_ref: StrRef, entry: TlkEntry) {
        if let Some(cache) = self.io_cache.as_mut() {
            cache.remove(&str_ref);
        }
        self.static_entries.insert(str_ref, entry);
        self.static_entries_highest = self
            .static_entries_highest
            .max(i32::try_from(str_ref).unwrap_or(i32::MAX));
    }

    /// Convenience helper that sets only the text portion of an entry.
    pub fn set_text(&mut self, str_ref: StrRef, text: impl Into<String>) {
        self.set_entry(
            str_ref,
            TlkEntry {
                text:          text.into(),
                sound_res_ref: String::new(),
                sound_length:  0.0,
            },
        );
    }

    fn get_from_io(&self, str_ref: StrRef) -> TlkResult<(usize, TlkEntry)> {
        crate::io::get_from_io(self, str_ref)
    }
}

impl Default for SingleTlk {
    fn default() -> Self {
        Self::new()
    }
}

impl Tlk {
    /// Creates a TLK chain from explicit layers.
    #[must_use] 
    pub fn new(chain: Vec<TlkPair>) -> Self {
        Self {
            chain,
        }
    }

    /// Builds a TLK chain from resource pairs.
    pub fn from_res_pairs(
        chain: &[(Option<Res>, Option<Res>)],
        use_cache: bool,
    ) -> TlkResult<Self> {
        let mut pairs = Vec::with_capacity(chain.len());
        for (male, female) in chain {
            pairs.push(TlkPair {
                male:   male
                    .as_ref()
                    .map(|res| SingleTlk::from_res(res, use_cache))
                    .transpose()?,
                female: female
                    .as_ref()
                    .map(|res| SingleTlk::from_res(res, use_cache))
                    .transpose()?,
            });
        }
        Ok(Self::new(pairs))
    }

    /// Queries the chain for `str_ref` using the requested gender.
    pub fn get(&mut self, str_ref: StrRef, gender: Gender) -> TlkResult<Option<TlkEntry>> {
        for pair in &mut self.chain {
            let queried = match gender {
                Gender::Female => pair
                    .female
                    .as_mut()
                    .map(|tlk| tlk.get(str_ref))
                    .transpose()?,
                Gender::Male => pair.male.as_mut().map(|tlk| tlk.get(str_ref)).transpose()?,
            };
            if let Some(entry) = queried.flatten() {
                return Ok(Some(entry));
            }
        }
        Ok(None)
    }
}
