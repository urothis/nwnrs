use std::{borrow::Cow, fmt};

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

    /// Returns this payload as UTF-8 when it is valid UTF-8.
    pub fn as_str(&self) -> Result<&str, std::str::Utf8Error> {
        std::str::from_utf8(&self.0)
    }

    /// Returns a lossily decoded view for display-only use.
    #[must_use]
    pub fn to_string_lossy(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(&self.0)
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
        let mut bytes = Vec::with_capacity(text.len());
        for character in text.chars() {
            let value = u32::from(character);
            if let Ok(byte) = u8::try_from(value) {
                bytes.push(byte);
            } else {
                let mut encoded = [0_u8; 4];
                bytes.extend_from_slice(character.encode_utf8(&mut encoded).as_bytes());
            }
        }
        Self(bytes)
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

impl From<String> for ScriptString {
    fn from(value: String) -> Self {
        Self(value.into_bytes())
    }
}

impl From<&str> for ScriptString {
    fn from(value: &str) -> Self {
        Self(value.as_bytes().to_vec())
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
