use std::{fmt, io};

/// Errors returned while assembling renderer-neutral scenes.
#[derive(Debug)]
pub enum RendererError {
    /// An underlying IO operation failed.
    Io(io::Error),
    /// A requested resource could not be resolved.
    MissingResource(String),
    /// An NWN resource payload could not be interpreted.
    InvalidResource(String),
    /// Scene assembly failed.
    Scene(String),
}

impl RendererError {
    /// Creates a missing-resource error.
    #[must_use]
    pub fn missing(resource: impl Into<String>) -> Self {
        Self::MissingResource(resource.into())
    }

    /// Creates an invalid-resource error.
    #[must_use]
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::InvalidResource(message.into())
    }

    /// Creates a general scene-assembly error.
    #[must_use]
    pub fn scene(message: impl Into<String>) -> Self {
        Self::Scene(message.into())
    }
}

impl fmt::Display for RendererError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::MissingResource(resource) => write!(f, "missing resource: {resource}"),
            Self::InvalidResource(message) | Self::Scene(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for RendererError {}

impl From<io::Error> for RendererError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

/// Result type for renderer scene assembly.
pub type RendererResult<T> = Result<T, RendererError>;
