use std::{fmt, path::PathBuf};

use nwnrs_nwpkg::{NwpkgError, NwpkgErrorKind};
use nwnrs_nwscript::Cancelled;

/// Result returned by compiler-owned language and package operations.
pub type LanguageResult<T> = Result<T, LanguageOperationError>;

/// Structured failure returned by a language-service operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LanguageOperationError {
    /// Stable machine-readable failure code.
    pub code:           String,
    /// Human-readable explanation.
    pub message:        String,
    /// Exact related physical path, when available.
    pub path:           Option<PathBuf>,
    /// Semantic `nwpkg.toml` field path, when available.
    pub manifest_field: Vec<String>,
}

impl LanguageOperationError {
    /// Creates an operation failure with a stable code.
    #[must_use]
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code:           code.into(),
            message:        message.into(),
            path:           None,
            manifest_field: Vec::new(),
        }
    }

    /// Attaches a physical path to the failure.
    #[must_use]
    pub fn with_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.path = Some(path.into());
        self
    }
}

impl From<NwpkgError> for LanguageOperationError {
    fn from(error: NwpkgError) -> Self {
        let code = match error.kind() {
            NwpkgErrorKind::Io => "nwpkg.io",
            NwpkgErrorKind::Parse => "nwpkg.parse",
            NwpkgErrorKind::Validation => "nwpkg.validation",
            NwpkgErrorKind::NotFound => "nwpkg.notFound",
            NwpkgErrorKind::Cancelled => "nwpkg.cancelled",
            NwpkgErrorKind::Compiler => "nwpkg.compiler",
            _ => "nwpkg.failure",
        };
        Self {
            code:           code.to_string(),
            message:        error.message().to_string(),
            path:           error.path().map(PathBuf::from),
            manifest_field: error
                .manifest_field_path()
                .map_or_else(Vec::new, <[String]>::to_vec),
        }
    }
}

impl From<String> for LanguageOperationError {
    fn from(message: String) -> Self {
        Self::new("language.operation", message)
    }
}

impl From<&str> for LanguageOperationError {
    fn from(message: &str) -> Self {
        Self::new("language.operation", message)
    }
}

impl From<Cancelled> for LanguageOperationError {
    fn from(_error: Cancelled) -> Self {
        Self::new("language.cancelled", "language operation was cancelled")
    }
}

impl fmt::Display for LanguageOperationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for LanguageOperationError {}
