use std::{fmt, io};

use nwnrs_types::{
    encoding::{EncodingConversionError, NwnText, NwnTextContext, encode_legacy},
    resman::{CachePolicy, Res, ResManError, ResType},
};

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

    /// Creates an ASCII-model payload using an explicit native text policy.
    ///
    /// # Errors
    ///
    /// Returns [`ModelError`] when the text is not representable in the
    /// context's legacy encoding.
    pub fn from_text(text: &str, context: NwnTextContext) -> ModelResult<Self> {
        Ok(Self {
            bytes: encode_legacy(context.encoding(), text)?,
        })
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

    /// Decodes the payload as byte-preserving native model text.
    #[must_use]
    pub fn native_text(&self, context: NwnTextContext) -> NwnText {
        NwnText::decode(&self.bytes, context.encoding())
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
        crate::mdl::read_model(&mut file)
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
    /// Native model text could not be encoded.
    Encoding(EncodingConversionError),
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
            Self::Encoding(error) => error.fmt(f),
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

impl From<EncodingConversionError> for ModelError {
    fn from(value: EncodingConversionError) -> Self {
        Self::Encoding(value)
    }
}

/// Result type for model operations.
pub type ModelResult<T> = Result<T, ModelError>;

#[cfg(test)]
mod tests {
    use nwnrs_types::encoding::NwnTextContext;

    use crate::mdl::{MODEL_RES_TYPE, Model};

    #[test]
    fn model_text_roundtrip_preserves_bytes() {
        let context = NwnTextContext::english();
        let model = Model::from_text("newmodel demo\n", context).expect("encode model text");
        assert_eq!(model.native_text(context).display_text(), "newmodel demo\n");
        assert_eq!(model.bytes(), b"newmodel demo\n");
        assert_eq!(MODEL_RES_TYPE.0, 2002);
    }
}
