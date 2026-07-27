use std::{
    error::Error,
    fmt,
    path::{Path, PathBuf},
};

/// Broad category for an [`NwpkgError`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum NwpkgErrorKind {
    /// A filesystem operation failed.
    Io,
    /// A manifest or lockfile could not be decoded.
    Parse,
    /// A manifest, dependency graph, or resource violated an nwpkg rule.
    Validation,
    /// A requested package or resource could not be found.
    NotFound,
    /// Work was cancelled by the caller.
    Cancelled,
    /// NWScript preprocessing or compilation failed.
    Compiler,
}

/// Structured failure produced by nwpkg APIs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NwpkgError {
    kind:           NwpkgErrorKind,
    path:           Option<PathBuf>,
    manifest_field: Option<Vec<String>>,
    message:        String,
}

impl NwpkgError {
    /// Creates an error with no associated filesystem path.
    #[must_use]
    pub fn new(kind: NwpkgErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            path: None,
            manifest_field: None,
            message: message.into(),
        }
    }

    /// Creates an error associated with `path`.
    #[must_use]
    pub fn at(kind: NwpkgErrorKind, path: impl Into<PathBuf>, message: impl Into<String>) -> Self {
        Self {
            kind,
            path: Some(path.into()),
            manifest_field: None,
            message: message.into(),
        }
    }

    /// Creates an error associated with one semantic manifest field.
    #[must_use]
    pub fn manifest_field(
        kind: NwpkgErrorKind,
        field: &[&str],
        message: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            path: None,
            manifest_field: Some(field.iter().map(|segment| (*segment).to_string()).collect()),
            message: message.into(),
        }
    }

    /// Creates a filesystem-associated error that also identifies the
    /// manifest field responsible for that path.
    #[must_use]
    pub fn at_manifest_field(
        kind: NwpkgErrorKind,
        path: impl Into<PathBuf>,
        field: &[&str],
        message: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            path: Some(path.into()),
            manifest_field: Some(field.iter().map(|segment| (*segment).to_string()).collect()),
            message: message.into(),
        }
    }

    /// Returns the error category.
    #[must_use]
    pub const fn kind(&self) -> NwpkgErrorKind {
        self.kind
    }

    /// Returns the related filesystem path, when available.
    #[must_use]
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Returns the exact manifest field associated with this error.
    #[must_use]
    pub fn manifest_field_path(&self) -> Option<&[String]> {
        self.manifest_field.as_deref()
    }

    /// Returns the path-independent diagnostic text.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for NwpkgError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(path) = &self.path {
            write!(formatter, "{}: {}", path.display(), self.message)
        } else {
            formatter.write_str(&self.message)
        }
    }
}

impl Error for NwpkgError {}

impl From<NwpkgError> for String {
    fn from(value: NwpkgError) -> Self {
        value.to_string()
    }
}
