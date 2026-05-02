use std::{
    collections::HashMap,
    fmt,
    fs::File,
    io::{self, Cursor},
    path::Path,
};

use nwnrs_types::{
    encoding::prelude::*, localization::prelude::*, lru::prelude::*, resman::prelude::*,
};

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
///
/// The type stores both the interpreted fields and enough raw descriptor data
/// to preserve the authored representation when the typed fields still match
/// that original encoding.
pub struct TlkEntry {
    /// Localized text content.
    pub text:              String,
    /// Original encoded text bytes when this entry was read from disk.
    pub raw_text:          Option<Vec<u8>>,
    /// Associated sound resource reference.
    pub sound_res_ref:     String,
    /// Raw 16-byte sound resource slot.
    pub raw_sound_res_ref: [u8; 16],
    /// Sound length in seconds.
    pub sound_length:      f32,
    /// Raw IEEE-754 bits for the stored sound length field.
    pub sound_length_bits: u32,
    /// Raw TLK entry flags.
    pub flags:             i32,
    /// Stored volume variance field.
    pub volume_variance:   i32,
    /// Stored pitch variance field.
    pub pitch_variance:    i32,
}

impl TlkEntry {
    /// Creates a canonical TLK entry with no preserved descriptor provenance.
    ///
    /// This constructor is appropriate for new authored data. Entries read from
    /// disk may preserve additional raw descriptor state so they can be written
    /// back without unnecessary normalization.
    ///
    /// # Examples
    ///
    /// ```
    /// let entry = nwnrs_types::tlk::TlkEntry::new("Hello", "gui_open", 1.5);
    /// assert_eq!(entry.text, "Hello");
    /// assert!(entry.has_value());
    /// ```
    pub fn new(
        text: impl Into<String>,
        sound_res_ref: impl Into<String>,
        sound_length: f32,
    ) -> Self {
        let text = text.into();
        let sound_res_ref = sound_res_ref.into();
        let flags = Self::canonical_flags_from_parts(&text, &sound_res_ref);
        let mut raw_sound_res_ref = [0_u8; 16];
        let bytes = sound_res_ref.as_bytes();
        let count = bytes.len().min(raw_sound_res_ref.len());
        if let (Some(dst), Some(src)) = (raw_sound_res_ref.get_mut(..count), bytes.get(..count)) {
            dst.copy_from_slice(src);
        }
        Self {
            text,
            raw_text: None,
            sound_res_ref,
            raw_sound_res_ref,
            sound_length,
            sound_length_bits: sound_length.to_bits(),
            flags,
            volume_variance: 0,
            pitch_variance: 0,
        }
    }

    /// Returns `true` when the entry contains either text or a sound reference.
    #[must_use]
    pub fn has_value(&self) -> bool {
        !self.text.is_empty()
            || !self.sound_res_ref.is_empty()
            || self.flags != 0
            || self.volume_variance != 0
            || self.pitch_variance != 0
            || self.sound_length_bits != 0
    }

    pub(crate) fn canonical_flags(&self) -> i32 {
        Self::canonical_flags_from_parts(&self.text, &self.sound_res_ref)
    }

    fn canonical_flags_from_parts(text: &str, sound_res_ref: &str) -> i32 {
        let mut flags = 0;
        if !text.is_empty() {
            flags |= 0x1;
        }
        if !sound_res_ref.is_empty() {
            flags |= 0x6;
        }
        flags
    }

    pub(crate) fn stored_flags(&self) -> i32 {
        let raw_sound = decode_sound_res_ref(&self.raw_sound_res_ref);
        let raw_text_matches = self
            .raw_text
            .as_ref()
            .and_then(|bytes| from_nwnrs_encoding(bytes).ok())
            .is_some_and(|decoded| decoded == self.text);
        let raw_sound_matches = raw_sound == self.sound_res_ref;
        let raw_length_matches =
            f32::from_bits(self.sound_length_bits).to_bits() == self.sound_length.to_bits();

        if raw_text_matches
            && raw_sound_matches
            && raw_length_matches
            && self.volume_variance == 0
            && self.pitch_variance == 0
        {
            self.flags
        } else {
            self.canonical_flags()
        }
    }

    pub(crate) fn stored_sound_res_ref_bytes(&self) -> TlkResult<[u8; 16]> {
        if decode_sound_res_ref(&self.raw_sound_res_ref) == self.sound_res_ref {
            return Ok(self.raw_sound_res_ref);
        }

        if self.sound_res_ref.len() > 16 {
            return Err(TlkError::msg(format!(
                "sound resref {:?} exceeds 16 bytes",
                self.sound_res_ref
            )));
        }

        let mut raw = [0_u8; 16];
        let bytes = self.sound_res_ref.as_bytes();
        if let Some(prefix) = raw.get_mut(..bytes.len()) {
            prefix.copy_from_slice(bytes);
        }
        Ok(raw)
    }

    pub(crate) fn stored_text_bytes(&self) -> TlkResult<Vec<u8>> {
        if let Some(raw_text) = &self.raw_text
            && from_nwnrs_encoding(raw_text)? == self.text
        {
            return Ok(raw_text.clone());
        }

        Ok(to_nwnrs_encoding(&self.text)?)
    }

    pub(crate) fn stored_sound_length_bits(&self) -> u32 {
        if f32::from_bits(self.sound_length_bits).to_bits() == self.sound_length.to_bits() {
            self.sound_length_bits
        } else {
            self.sound_length.to_bits()
        }
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
    pub(crate) source_bytes: Option<Vec<u8>>,
    pub(crate) source_language: Option<Language>,
    /// Cache behavior for resource reads and lazy entry lookups.
    pub cache_policy: CachePolicy,
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
            .field("cache_policy", &self.cache_policy)
            .field(
                "io_cache_entries",
                &self.io_cache.as_ref().map_or(0, WeightedLru::len),
            )
            .finish_non_exhaustive()
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
    ///
    /// Use [`set_entry`](Self::set_entry) or [`set_text`](Self::set_text) to
    /// populate authored content after construction.
    ///
    /// # Examples
    ///
    /// ```
    /// let mut tlk = nwnrs_types::tlk::SingleTlk::new();
    /// tlk.set_text(42, "Hello");
    /// let entry = tlk.get(42)?.unwrap();
    /// assert_eq!(entry.text, "Hello");
    /// # Ok::<(), nwnrs_types::tlk::TlkError>(())
    /// ```
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
            source_bytes:           None,
            source_language:        None,
            cache_policy:           CachePolicy::Use,
            io_cache:               None,
        }
    }

    /// Opens a TLK file from disk.
    ///
    /// # Errors
    ///
    /// Returns [`TlkError`] if the file cannot be opened or parsed.
    pub fn from_file(path: impl AsRef<Path>, cache_policy: CachePolicy) -> TlkResult<Self> {
        let file = File::open(path.as_ref())?;
        crate::tlk::io::read_single_tlk(file, cache_policy)
    }

    /// Reads a TLK payload from a [`Res`].
    ///
    /// # Errors
    ///
    /// Returns [`TlkError`] if the resource bytes cannot be parsed as a TLK
    /// table.
    pub fn from_res(res: &Res, cache_policy: CachePolicy) -> TlkResult<Self> {
        let bytes = res.read_all(cache_policy)?;
        crate::tlk::io::read_single_tlk(Cursor::new(bytes), cache_policy)
    }

    /// Returns the highest string reference known to this table.
    #[must_use]
    pub fn highest(&self) -> i32 {
        let io_highest = i32::try_from(self.io_entry_count.saturating_sub(1)).unwrap_or(i32::MAX);
        io_highest.max(self.static_entries_highest)
    }

    /// Returns the entry for `str_ref`, if present.
    ///
    /// # Errors
    ///
    /// Returns [`TlkError`] if the underlying IO read fails.
    pub fn get(&mut self, str_ref: StrRef) -> TlkResult<Option<TlkEntry>> {
        if let Some(entry) = self.static_entries.get(&str_ref) {
            return Ok(Some(entry.clone()));
        }

        if usize::try_from(str_ref).unwrap_or(usize::MAX) >= self.io_entry_count {
            return Ok(None);
        }

        if self.cache_policy.uses_cache()
            && let Some(entry) = self
                .io_cache
                .as_mut()
                .and_then(|cache| cache.get(&str_ref).cloned())
        {
            return Ok(Some(entry));
        }

        if self.cache_policy.uses_cache() {
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
        self.set_entry(str_ref, TlkEntry::new(text, String::new(), 0.0));
    }

    fn get_from_io(&self, str_ref: StrRef) -> TlkResult<(usize, TlkEntry)> {
        crate::tlk::io::get_from_io(self, str_ref)
    }
}

impl Default for SingleTlk {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) fn decode_sound_res_ref(bytes: &[u8]) -> String {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    String::from_utf8_lossy(bytes.get(..end).unwrap_or(&[]))
        .trim_matches(|ch: char| ch == '\u{00c0}' || ch.is_ascii_whitespace())
        .to_string()
}

impl Tlk {
    /// Creates a TLK chain from explicit layers.
    ///
    /// # Examples
    ///
    /// ```
    /// let mut lower = nwnrs_types::tlk::SingleTlk::new();
    /// lower.set_text(7, "lower");
    /// let mut upper = nwnrs_types::tlk::SingleTlk::new();
    /// upper.set_text(7, "upper");
    ///
    /// let mut tlk = nwnrs_types::tlk::Tlk::new(vec![
    ///     nwnrs_types::tlk::TlkPair {
    ///         male: Some(upper),
    ///         female: None,
    ///     },
    ///     nwnrs_types::tlk::TlkPair {
    ///         male: Some(lower),
    ///         female: None,
    ///     },
    /// ]);
    ///
    /// let entry = tlk.get(7, nwnrs_types::localization::Gender::Male)?.unwrap();
    /// assert_eq!(entry.text, "upper");
    /// # Ok::<(), nwnrs_types::tlk::TlkError>(())
    /// ```
    #[must_use]
    pub fn new(chain: Vec<TlkPair>) -> Self {
        Self {
            chain,
        }
    }

    /// Builds a TLK chain from resource pairs.
    ///
    /// # Errors
    ///
    /// Returns [`TlkError`] if any resource pair cannot be read or parsed.
    ///
    /// # Examples
    ///
    /// ```
    /// let mut tlk = nwnrs_types::tlk::Tlk::from_res_pairs(
    ///     &[(None, None)],
    ///     nwnrs_types::resman::CachePolicy::Bypass,
    /// )?;
    ///
    /// assert!(tlk.get(5, nwnrs_types::localization::Gender::Male)?.is_none());
    /// # Ok::<(), nwnrs_types::tlk::TlkError>(())
    /// ```
    pub fn from_res_pairs(
        chain: &[(Option<Res>, Option<Res>)],
        cache_policy: CachePolicy,
    ) -> TlkResult<Self> {
        let mut pairs = Vec::with_capacity(chain.len());
        for (male, female) in chain {
            pairs.push(TlkPair {
                male:   male
                    .as_ref()
                    .map(|res| SingleTlk::from_res(res, cache_policy))
                    .transpose()?,
                female: female
                    .as_ref()
                    .map(|res| SingleTlk::from_res(res, cache_policy))
                    .transpose()?,
            });
        }
        Ok(Self::new(pairs))
    }

    /// Queries the chain for `str_ref` using the requested gender.
    ///
    /// # Errors
    ///
    /// Returns [`TlkError`] if the underlying IO read fails.
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
