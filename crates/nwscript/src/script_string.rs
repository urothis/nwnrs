use std::fmt;

use nwnrs_types::encoding::{LegacyEncoding, NwnText, encode_legacy};
use serde::{Deserialize, Serialize};

/// An encoding-neutral NWScript string payload.
///
/// NWScript source and NCS bytecode carry string data as bytes. This type
/// preserves those bytes without assuming UTF-8 or a particular legacy
/// character encoding.
#[derive(Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ScriptString(Vec<u8>);

impl ScriptString {
    /// Creates a script string from its exact byte payload.
    #[must_use]
    pub const fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    /// Returns the exact byte payload.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Returns the payload length in bytes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns whether the payload contains no bytes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Consumes the value and returns its exact byte payload.
    #[must_use]
    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }

    /// Returns a Windows-1252 display form that escapes undefined bytes and
    /// binary color payload bytes without replacing data.
    #[must_use]
    pub fn display_text(&self) -> String {
        NwnText::decode(&self.0, LegacyEncoding::Windows1252).display_text()
    }

    /// Concatenates two payloads without decoding or re-encoding either one.
    #[must_use]
    pub fn concat(&self, other: &Self) -> Self {
        let mut bytes = Vec::with_capacity(self.0.len().saturating_add(other.0.len()));
        bytes.extend_from_slice(&self.0);
        bytes.extend_from_slice(&other.0);
        Self(bytes)
    }

    pub(crate) fn from_lexed_text(text: &str) -> Self {
        Self(
            text.chars()
                .map(|character| match u8::try_from(u32::from(character)) {
                    Ok(byte) => byte,
                    Err(error) => unreachable!(
                        "byte-oriented lexer produced a scalar outside U+0000 through U+00FF: \
                         {error}"
                    ),
                })
                .collect(),
        )
    }

    /// Encodes authored text as an exact Windows-1252 runtime payload.
    ///
    /// # Errors
    ///
    /// Returns an error when the text is not representable in Windows-1252.
    pub fn try_from_text(
        value: &str,
    ) -> Result<Self, nwnrs_types::encoding::EncodingConversionError> {
        encode_legacy(LegacyEncoding::Windows1252, value).map(Self)
    }
}

impl fmt::Debug for ScriptString {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("b\"")?;
        for &byte in &self.0 {
            for character in std::ascii::escape_default(byte) {
                formatter.write_str(&char::from(character).to_string())?;
            }
        }
        formatter.write_str("\"")
    }
}

impl From<Vec<u8>> for ScriptString {
    fn from(value: Vec<u8>) -> Self {
        Self(value)
    }
}

impl AsRef<[u8]> for ScriptString {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::ScriptString;

    #[test]
    fn lexed_text_round_trips_every_byte() {
        let text = [0x00_u8, 0x7f, 0x80, 0xff]
            .into_iter()
            .map(char::from)
            .collect::<String>();
        assert_eq!(
            ScriptString::from_lexed_text(&text).as_bytes(),
            &[0x00, 0x7f, 0x80, 0xff]
        );
    }
}
