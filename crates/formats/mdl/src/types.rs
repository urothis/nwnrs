use std::{fmt, io, str::Utf8Error};

use nwnrs_resman::prelude::*;
use nwnrs_restype::prelude::*;

/// NWN resource type id for `mdl`.
pub const MODEL_RES_TYPE: ResType = ResType(2002);

#[derive(Clone, PartialEq, Eq)]
/// Raw Neverwinter Nights model payload.
pub struct Model {
    bytes: Vec<u8>,
}

impl Model {
    /// Creates a model from raw bytes.
    #[must_use]
    pub fn new(bytes: Vec<u8>) -> Self {
        Self {
            bytes,
        }
    }

    /// Creates a model from UTF-8 text.
    pub fn from_text(text: impl Into<String>) -> Self {
        Self {
            bytes: text.into().into_bytes(),
        }
    }

    /// Returns the raw model bytes.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Returns the size of the payload in bytes.
    #[must_use]
    pub fn byte_len(&self) -> usize {
        self.bytes.len()
    }

    /// Returns the model as UTF-8 text when valid.
    ///
    /// # Errors
    ///
    /// Returns [`Utf8Error`] if the payload is not valid UTF-8.
    pub fn as_text(&self) -> Result<&str, Utf8Error> {
        std::str::from_utf8(&self.bytes)
    }

    /// Consumes the model and returns the raw payload.
    #[must_use]
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    /// Reads an `MDL` payload from disk.
    ///
    /// # Errors
    ///
    /// Returns [`ModelError`] if the file cannot be opened or read.
    pub fn from_file(path: impl AsRef<std::path::Path>) -> ModelResult<Self> {
        let mut file = std::fs::File::open(path.as_ref())?;
        crate::read_model(&mut file)
    }

    /// Reads an `MDL` payload from a [`Res`].
    ///
    /// # Errors
    ///
    /// Returns [`ModelError`] if the resource is not an MDL type or reading
    /// fails.
    pub fn from_res(res: &Res, cache_policy: CachePolicy) -> ModelResult<Self> {
        if res.resref().res_type() != MODEL_RES_TYPE {
            return Err(ModelError::msg(format!(
                "expected mdl resource, got {}",
                res.resref()
            )));
        }

        Ok(Self::new(res.read_all(cache_policy)?))
    }
}

impl fmt::Debug for Model {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Model")
            .field("byte_len", &self.byte_len())
            .field("utf8", &self.as_text().is_ok())
            .finish()
    }
}

#[derive(Debug)]
/// Errors returned while reading or writing model payloads.
pub enum ModelError {
    /// An underlying IO operation failed.
    Io(io::Error),
    /// Resource-manager access failed.
    ResMan(ResManError),
    /// The payload did not identify a model resource.
    Message(String),
}

impl ModelError {
    pub(crate) fn msg(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }
}

impl fmt::Display for ModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::ResMan(error) => error.fmt(f),
            Self::Message(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for ModelError {}

impl From<io::Error> for ModelError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<ResManError> for ModelError {
    fn from(value: ResManError) -> Self {
        Self::ResMan(value)
    }
}

/// Result type for model operations.
pub type ModelResult<T> = Result<T, ModelError>;

#[allow(clippy::panic)]
#[cfg(test)]
mod tests {
    use crate::{MODEL_RES_TYPE, Model};

    #[test]
    fn model_text_roundtrip_preserves_bytes() {
        let model = Model::from_text("newmodel demo\n");
        assert_eq!(model.as_text().unwrap_or(""), "newmodel demo\n");
        assert_eq!(model.bytes(), b"newmodel demo\n");
        assert_eq!(MODEL_RES_TYPE.0, 2002);
    }
}
