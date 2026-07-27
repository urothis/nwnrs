use std::fmt;

use encoding_rs::{Encoding, WINDOWS_1250, WINDOWS_1252};
use tracing::instrument;

use crate::encoding::{EncodingConversionError, UnknownEncodingError};

/// A legacy single-byte encoding used by Neverwinter Nights resources.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LegacyEncoding {
    /// Windows Western European (code page 1252).
    #[default]
    Windows1252,
    /// Windows Central European (code page 1250).
    Windows1250,
}

impl LegacyEncoding {
    /// Resolves a supported encoding label.
    ///
    /// # Errors
    ///
    /// Returns [`UnknownEncodingError`] when the label is not one of the
    /// explicitly supported Neverwinter Nights encodings.
    pub fn from_label(label: &str) -> Result<Self, UnknownEncodingError> {
        match label
            .trim()
            .to_ascii_lowercase()
            .replace(['-', '_'], "")
            .as_str()
        {
            "windows1252" | "cp1252" => Ok(Self::Windows1252),
            "windows1250" | "cp1250" => Ok(Self::Windows1250),
            _ => Err(UnknownEncodingError::new(label)),
        }
    }

    /// Returns the canonical WHATWG encoding label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Windows1252 => "windows-1252",
            Self::Windows1250 => "windows-1250",
        }
    }

    /// Returns the encoding selected by an NWN language identifier.
    ///
    /// Polish (5) uses Windows-1250. All currently supported Western European
    /// languages use Windows-1252.
    #[must_use]
    pub const fn for_language_id(language_id: u32) -> Self {
        if language_id == 5 {
            Self::Windows1250
        } else {
            Self::Windows1252
        }
    }

    pub(crate) const fn implementation(self) -> &'static Encoding {
        match self {
            Self::Windows1252 => WINDOWS_1252,
            Self::Windows1250 => WINDOWS_1250,
        }
    }

    /// Returns whether a byte has a defined character mapping in this code
    /// page.
    ///
    /// WHATWG codecs map the historically undefined Windows code-page slots
    /// to C1 controls for web compatibility. NWN resource preservation needs
    /// the actual code-page definition so those bytes remain opaque.
    #[must_use]
    pub const fn is_defined_byte(self, byte: u8) -> bool {
        match self {
            Self::Windows1252 => !matches!(byte, 0x81 | 0x8d | 0x8f | 0x90 | 0x9d),
            Self::Windows1250 => !matches!(byte, 0x81 | 0x83 | 0x88 | 0x90 | 0x98),
        }
    }
}

/// Immutable text policy supplied to native resource readers and writers.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NwnTextContext {
    language_id: u32,
    encoding:    LegacyEncoding,
}

impl NwnTextContext {
    /// Returns the default English NWN text policy.
    #[must_use]
    pub const fn english() -> Self {
        Self::for_language_id(0)
    }

    /// Creates a context for the supplied NWN language identifier.
    #[must_use]
    pub const fn for_language_id(language_id: u32) -> Self {
        Self {
            language_id,
            encoding: LegacyEncoding::for_language_id(language_id),
        }
    }

    /// Creates a context with an explicitly selected legacy encoding.
    #[must_use]
    pub const fn new(language_id: u32, encoding: LegacyEncoding) -> Self {
        Self {
            language_id,
            encoding,
        }
    }

    /// Returns the NWN language identifier associated with this context.
    #[must_use]
    pub const fn language_id(self) -> u32 {
        self.language_id
    }

    /// Returns the legacy encoding associated with this context.
    #[must_use]
    pub const fn encoding(self) -> LegacyEncoding {
        self.encoding
    }

    /// Returns a copy using the encoding derived from another language id.
    #[must_use]
    pub const fn with_language_id(self, language_id: u32) -> Self {
        let _ = self;
        Self::for_language_id(language_id)
    }
}

/// Encodes text using an explicit legacy encoding.
///
/// # Errors
///
/// Returns [`EncodingConversionError`] if the text cannot be represented.
#[instrument(level = "debug", skip_all, err, fields(input_len = value.len(), encoding = encoding.label()))]
pub fn encode_legacy(
    encoding: LegacyEncoding,
    value: &str,
) -> Result<Vec<u8>, EncodingConversionError> {
    let encoded = encode_with(encoding.implementation(), value, "encode legacy NWN text")?;
    if encoded.iter().any(|byte| !encoding.is_defined_byte(*byte)) {
        Err(EncodingConversionError::new(
            encoding.implementation().name(),
            "encode legacy NWN text containing an undefined code-page slot",
        ))
    } else {
        Ok(encoded)
    }
}

/// Decodes bytes using an explicit legacy encoding.
///
/// # Errors
///
/// Returns [`EncodingConversionError`] if any byte is undefined in the
/// selected encoding.
#[instrument(level = "debug", skip_all, err, fields(input_len = bytes.len(), encoding = encoding.label()))]
pub fn decode_legacy(
    encoding: LegacyEncoding,
    bytes: &[u8],
) -> Result<String, EncodingConversionError> {
    if bytes.iter().any(|byte| !encoding.is_defined_byte(*byte)) {
        return Err(EncodingConversionError::new(
            encoding.implementation().name(),
            "decode undefined legacy NWN byte",
        ));
    }
    decode_with(encoding.implementation(), bytes, "decode legacy NWN text")
}

/// Decodes legacy bytes into a reversible internal string.
///
/// Undefined bytes are represented by private-use scalars in the range
/// `U+E000..=U+E0FF`. This function is intended for legacy text parsers that
/// must retain source layout in strings while preserving every original byte.
#[must_use]
pub fn decode_legacy_preserving(encoding: LegacyEncoding, bytes: &[u8]) -> String {
    let mut output = String::new();
    for byte in bytes {
        match decode_legacy(encoding, std::slice::from_ref(byte)) {
            Ok(value) => output.push_str(&value),
            Err(_) => {
                if let Some(value) = char::from_u32(0xe000 + u32::from(*byte)) {
                    output.push(value);
                }
            }
        }
    }
    output
}

/// Encodes a reversible internal legacy-text string.
///
/// Private-use scalars produced by [`decode_legacy_preserving`] are restored
/// to their exact opaque bytes. All other text is encoded strictly.
///
/// # Errors
///
/// Returns [`EncodingConversionError`] if edited text cannot be represented.
pub fn encode_legacy_preserving(
    encoding: LegacyEncoding,
    value: &str,
) -> Result<Vec<u8>, EncodingConversionError> {
    let mut output = Vec::new();
    let mut text = String::new();
    for character in value.chars() {
        let scalar = u32::from(character);
        if (0xe000..=0xe0ff).contains(&scalar) {
            if !text.is_empty() {
                output.extend_from_slice(&encode_legacy(encoding, &text)?);
                text.clear();
            }
            output.push(u8::try_from(scalar - 0xe000).unwrap_or_default());
        } else {
            text.push(character);
        }
    }
    if !text.is_empty() {
        output.extend_from_slice(&encode_legacy(encoding, &text)?);
    }
    Ok(output)
}

/// Converts a reversible internal legacy string into an escaped display form.
///
/// Opaque bytes become `\xNN`; known color controls retain their structured
/// spelling.
#[must_use]
pub fn display_legacy_preserving(encoding: LegacyEncoding, value: &str) -> String {
    encode_legacy_preserving(encoding, value).map_or_else(
        |_| value.to_string(),
        |bytes| NwnText::decode(&bytes, encoding).display_text(),
    )
}

/// A structured segment in byte-preserving Neverwinter Nights text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NwnTextSegment {
    /// Text that decoded successfully with the selected legacy encoding.
    Text(String),
    /// Bytes that are undefined or otherwise intentionally opaque.
    Opaque(Vec<u8>),
    /// A known NWN color-start token containing exact RGB bytes.
    ColorStart([u8; 3]),
    /// A known NWN color-end token.
    ColorEnd,
}

/// Lossless native Neverwinter Nights text.
///
/// The original bytes are retained so a no-op read/write round trip is byte
/// identical. Known color tokens are structured and undecodable bytes remain
/// opaque instead of being replaced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NwnText {
    encoding:       LegacyEncoding,
    original_bytes: Option<Vec<u8>>,
    segments:       Vec<NwnTextSegment>,
}

impl NwnText {
    /// Decodes native bytes without discarding undefined bytes or color
    /// controls.
    #[must_use]
    pub fn decode(bytes: &[u8], encoding: LegacyEncoding) -> Self {
        let mut segments = Vec::new();
        let mut text = String::new();
        let mut opaque = Vec::new();
        let mut offset = 0;

        while offset < bytes.len() {
            let Some(tail) = bytes.get(offset..) else {
                break;
            };
            if tail.starts_with(b"</c>") {
                flush_text(&mut segments, &mut text);
                flush_opaque(&mut segments, &mut opaque);
                segments.push(NwnTextSegment::ColorEnd);
                offset += 4;
                continue;
            }
            if let Some([b'<', b'c', red, green, blue, b'>']) = tail.get(..6) {
                flush_text(&mut segments, &mut text);
                flush_opaque(&mut segments, &mut opaque);
                segments.push(NwnTextSegment::ColorStart([*red, *green, *blue]));
                offset += 6;
                continue;
            }

            let Some(&byte) = tail.first() else {
                break;
            };
            match decode_legacy(encoding, std::slice::from_ref(&byte)) {
                Ok(character) => {
                    flush_opaque(&mut segments, &mut opaque);
                    text.push_str(&character);
                }
                Err(_) => {
                    flush_text(&mut segments, &mut text);
                    opaque.push(byte);
                }
            }
            offset += 1;
        }

        flush_text(&mut segments, &mut text);
        flush_opaque(&mut segments, &mut opaque);
        let opaque_bytes = segments
            .iter()
            .map(|segment| match segment {
                NwnTextSegment::Opaque(bytes) => bytes.len(),
                _ => 0,
            })
            .sum::<usize>();
        if opaque_bytes > 0 {
            tracing::warn!(
                encoding = encoding.label(),
                opaque_bytes,
                "preserving bytes undefined in the selected NWN text encoding"
            );
        }
        Self {
            encoding,
            original_bytes: Some(bytes.to_vec()),
            segments,
        }
    }

    /// Creates edited text in the selected encoding.
    #[must_use]
    pub fn from_text(value: impl Into<String>, encoding: LegacyEncoding) -> Self {
        Self {
            encoding,
            original_bytes: None,
            segments: vec![NwnTextSegment::Text(value.into())],
        }
    }

    /// Creates edited text from explicit structured segments.
    #[must_use]
    pub fn from_segments(segments: Vec<NwnTextSegment>, encoding: LegacyEncoding) -> Self {
        Self {
            encoding,
            original_bytes: None,
            segments,
        }
    }

    /// Returns the legacy encoding carried by this value.
    #[must_use]
    pub const fn encoding(&self) -> LegacyEncoding {
        self.encoding
    }

    /// Returns the structured text segments.
    #[must_use]
    pub fn segments(&self) -> &[NwnTextSegment] {
        &self.segments
    }

    /// Returns whether decoding encountered opaque bytes.
    #[must_use]
    pub fn has_opaque_bytes(&self) -> bool {
        self.segments
            .iter()
            .any(|segment| matches!(segment, NwnTextSegment::Opaque(_)))
    }

    /// Encodes this value, preserving the original bytes when unchanged.
    ///
    /// # Errors
    ///
    /// Returns [`EncodingConversionError`] if an edited text segment cannot be
    /// represented in the selected encoding.
    pub fn encode(&self) -> Result<Vec<u8>, EncodingConversionError> {
        if let Some(original) = &self.original_bytes {
            return Ok(original.clone());
        }

        let mut output = Vec::new();
        for segment in &self.segments {
            match segment {
                NwnTextSegment::Text(value) => {
                    output.extend_from_slice(&encode_legacy(self.encoding, value)?);
                }
                NwnTextSegment::Opaque(bytes) => output.extend_from_slice(bytes),
                NwnTextSegment::ColorStart(rgb) => {
                    output.extend_from_slice(b"<c");
                    output.extend_from_slice(rgb);
                    output.push(b'>');
                }
                NwnTextSegment::ColorEnd => output.extend_from_slice(b"</c>"),
            }
        }
        Ok(output)
    }

    /// Returns a display form that escapes opaque and binary color bytes.
    #[must_use]
    pub fn display_text(&self) -> String {
        let mut output = String::new();
        for segment in &self.segments {
            match segment {
                NwnTextSegment::Text(value) => output.push_str(value),
                NwnTextSegment::Opaque(bytes) => append_escaped_bytes(&mut output, bytes),
                NwnTextSegment::ColorStart(rgb) => {
                    output.push_str("<c");
                    append_escaped_bytes(&mut output, rgb);
                    output.push('>');
                }
                NwnTextSegment::ColorEnd => output.push_str("</c>"),
            }
        }
        output
    }
}

impl fmt::Display for NwnText {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.display_text())
    }
}

pub(crate) fn encode_with(
    encoding: &'static Encoding,
    value: &str,
    operation: &'static str,
) -> Result<Vec<u8>, EncodingConversionError> {
    let (encoded, _, had_errors) = encoding.encode(value);
    if had_errors {
        Err(EncodingConversionError::new(encoding.name(), operation))
    } else {
        Ok(encoded.into_owned())
    }
}

pub(crate) fn decode_with(
    encoding: &'static Encoding,
    bytes: &[u8],
    operation: &'static str,
) -> Result<String, EncodingConversionError> {
    let (decoded, _, had_errors) = encoding.decode(bytes);
    if had_errors {
        Err(EncodingConversionError::new(encoding.name(), operation))
    } else {
        Ok(decoded.into_owned())
    }
}

fn flush_text(segments: &mut Vec<NwnTextSegment>, text: &mut String) {
    if !text.is_empty() {
        segments.push(NwnTextSegment::Text(std::mem::take(text)));
    }
}

fn flush_opaque(segments: &mut Vec<NwnTextSegment>, bytes: &mut Vec<u8>) {
    if !bytes.is_empty() {
        segments.push(NwnTextSegment::Opaque(std::mem::take(bytes)));
    }
}

fn append_escaped_bytes(output: &mut String, bytes: &[u8]) {
    for byte in bytes {
        output.push_str(&format!("\\x{byte:02X}"));
    }
}

#[cfg(test)]
mod tests {
    use super::{LegacyEncoding, NwnText, NwnTextSegment, decode_legacy, encode_legacy};

    #[test]
    fn explicit_encodings_round_trip_representable_text() {
        let value = "café";
        let encoded = encode_legacy(LegacyEncoding::Windows1252, value);
        assert_eq!(encoded, Ok(vec![b'c', b'a', b'f', 0xe9]));
        assert_eq!(
            encoded.and_then(|bytes| decode_legacy(LegacyEncoding::Windows1252, &bytes)),
            Ok(value.to_string())
        );
    }

    #[test]
    fn nwn_text_preserves_color_and_undefined_bytes() {
        let bytes = b"hello<c\x01\x80\xff>world</c>\x81";
        let text = NwnText::decode(bytes, LegacyEncoding::Windows1252);
        assert!(text.has_opaque_bytes());
        assert!(
            text.segments()
                .contains(&NwnTextSegment::ColorStart([0x01, 0x80, 0xff]))
        );
        assert_eq!(text.encode(), Ok(bytes.to_vec()));
    }

    #[test]
    fn unknown_angle_content_is_plain_text() {
        let bytes = b"<CUSTOM42>";
        let text = NwnText::decode(bytes, LegacyEncoding::Windows1252);
        assert_eq!(
            text.segments(),
            &[NwnTextSegment::Text("<CUSTOM42>".to_string())]
        );
        assert_eq!(text.encode(), Ok(bytes.to_vec()));
    }

    #[test]
    fn polish_selects_windows_1250() {
        assert_eq!(
            LegacyEncoding::for_language_id(5),
            LegacyEncoding::Windows1250
        );
        assert_eq!(
            LegacyEncoding::for_language_id(0),
            LegacyEncoding::Windows1252
        );
    }

    #[test]
    fn preserving_conversion_round_trips_undefined_bytes() {
        let bytes = b"a\x81b";
        let decoded = super::decode_legacy_preserving(LegacyEncoding::Windows1252, bytes);
        assert_eq!(
            super::encode_legacy_preserving(LegacyEncoding::Windows1252, &decoded),
            Ok(bytes.to_vec())
        );
    }
}
