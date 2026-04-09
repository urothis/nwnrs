use std::{error::Error, fmt, str::FromStr};

/// A TLK string reference.
pub type StrRef = u32;
/// The sentinel string reference used for “no string”.
pub const BAD_STRREF: StrRef = u32::MAX;

/// A supported NWN language identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum Language {
    /// English resources.
    English = 0,
    /// French resources.
    French = 1,
    /// German resources.
    German = 2,
    /// Italian resources.
    Italian = 3,
    /// Spanish resources.
    Spanish = 4,
    /// Polish resources.
    Polish = 5,
}

/// A gender selector for TLK lookups.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Gender {
    /// Male dialogue.
    Male,
    /// Female dialogue.
    Female,
}

/// An error returned when a language identifier cannot be parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseLanguageError {
    pub(crate) input: String,
    pub(crate) reason: String,
}

impl ParseLanguageError {
    pub(crate) fn new(input: &str, reason: impl Into<String>) -> Self {
        Self {
            input: input.to_string(),
            reason: reason.into(),
        }
    }
}

impl fmt::Display for ParseLanguageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: Not a valid language ({})", self.input, self.reason)
    }
}

impl Error for ParseLanguageError {}

impl Language {
    /// Returns the numeric NWN language id.
    pub fn id(self) -> u32 {
        self as u32
    }

    /// Returns the two-letter NWN language code.
    pub fn short_code(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::French => "fr",
            Self::German => "de",
            Self::Italian => "it",
            Self::Spanish => "es",
            Self::Polish => "pl",
        }
    }

    /// Resolves a language from its numeric NWN id.
    pub fn from_id(id: u32) -> Option<Self> {
        match id {
            0 => Some(Self::English),
            1 => Some(Self::French),
            2 => Some(Self::German),
            3 => Some(Self::Italian),
            4 => Some(Self::Spanish),
            5 => Some(Self::Polish),
            _ => None,
        }
    }
}

impl FromStr for Language {
    type Err = ParseLanguageError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        crate::resolve_language(input)
    }
}
