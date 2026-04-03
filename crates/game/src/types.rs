use nwn_erf::ErfError;
use nwn_key::KeyError;
use nwn_resdir::ResDirError;
use nwn_resnwsync::ResNWSyncError;
use std::fmt;
use std::io;

/// GFF-family extensions commonly treated as generic GFF payloads by the CLI.
pub const GFF_EXTENSIONS: &[&str] = &[
    "utc", "utd", "ute", "uti", "utm", "utp", "uts", "utt", "utw", "git", "are", "gic", "ifo",
    "fac", "dlg", "itp", "bic", "jrl", "gff", "gui",
];
/// KEY basenames loaded by default when no explicit key list is supplied.
pub const DEFAULT_KEYFILES: &[&str] = &["nwn_base", "nwn_base_loc", "nwn_retail", "nwn_retail_loc"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum Platform {
    Linux,
    MacOs,
    Windows,
}

#[derive(Debug)]
/// Errors returned by game-discovery and default resource-loading helpers.
pub enum GameError {
    /// An underlying IO operation failed.
    Io(io::Error),
    /// JSON parsing failed while reading Beamdog client settings.
    Json(serde_json::Error),
    /// KEY/BIF loading failed.
    Key(KeyError),
    /// ERF loading failed.
    Erf(ErfError),
    /// Directory-backed resource loading failed.
    ResDir(ResDirError),
    /// NWSync repository access failed.
    ResNWSync(ResNWSyncError),
    /// The requested installation layout or input set was invalid.
    Message(String),
}

impl GameError {
    pub(crate) fn msg(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }
}

impl fmt::Display for GameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::Json(error) => error.fmt(f),
            Self::Key(error) => error.fmt(f),
            Self::Erf(error) => error.fmt(f),
            Self::ResDir(error) => error.fmt(f),
            Self::ResNWSync(error) => error.fmt(f),
            Self::Message(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for GameError {}

impl From<io::Error> for GameError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for GameError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

impl From<KeyError> for GameError {
    fn from(value: KeyError) -> Self {
        Self::Key(value)
    }
}

impl From<ErfError> for GameError {
    fn from(value: ErfError) -> Self {
        Self::Erf(value)
    }
}

impl From<ResDirError> for GameError {
    fn from(value: ResDirError) -> Self {
        Self::ResDir(value)
    }
}

impl From<ResNWSyncError> for GameError {
    fn from(value: ResNWSyncError) -> Self {
        Self::ResNWSync(value)
    }
}

/// Result type for game-level helper operations.
pub type GameResult<T> = Result<T, GameError>;
