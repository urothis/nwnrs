use std::{fmt, io};

/// Errors returned while assembling frontend-neutral scenes.
#[derive(Debug)]
pub enum SceneError {
    /// An underlying IO operation failed.
    Io(io::Error),
    /// A requested resource could not be resolved.
    MissingResource(String),
    /// The caller cancelled scene assembly.
    Cancelled,
    /// An NWN resource payload could not be interpreted.
    InvalidResource(String),
    /// Scene assembly failed.
    Scene(String),
}

impl SceneError {
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

    /// Creates a cancellation error.
    #[must_use]
    pub const fn cancelled() -> Self {
        Self::Cancelled
    }
}

impl fmt::Display for SceneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::MissingResource(resource) => write!(f, "missing resource: {resource}"),
            Self::Cancelled => f.write_str("scene assembly cancelled"),
            Self::InvalidResource(message) | Self::Scene(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for SceneError {}

impl From<io::Error> for SceneError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

/// Result type for scene assembly.
pub type SceneResult<T> = Result<T, SceneError>;
