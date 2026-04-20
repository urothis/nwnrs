use std::{cell::Cell, env};

use encoding_rs::{Encoding, WINDOWS_1252};
use tracing::instrument;

use crate::{EncodingConversionError, NativeEncodingError, UnknownEncodingError};

thread_local! {
    static NWNRS_ENCODING: Cell<&'static Encoding> = Cell::new(WINDOWS_1252);
    static NATIVE_ENCODING: Cell<Option<&'static Encoding>> = const { Cell::new(None) };
}

/// Returns the encoding currently used for NWN text data.
pub fn get_nwnrs_encoding() -> &'static Encoding {
    NWNRS_ENCODING.with(Cell::get)
}

/// Returns the canonical label for the current NWN text encoding.
#[must_use]
pub fn get_nwnrs_encoding_name() -> &'static str {
    get_nwnrs_encoding().name()
}

/// Sets the encoding used for NWN text data.
///
/// # Errors
///
/// Returns [`UnknownEncodingError`] if `label` does not map to a known encoding.
#[instrument(level = "debug", skip_all, err, fields(label = %label))]
pub fn set_nwnrs_encoding(label: &str) -> Result<(), UnknownEncodingError> {
    let encoding =
        Encoding::for_label(label.as_bytes()).ok_or_else(|| UnknownEncodingError::new(label))?;
    NWNRS_ENCODING.with(|slot| slot.set(encoding));
    Ok(())
}

/// Returns the configured or detected native system encoding.
///
/// # Errors
///
/// Returns [`NativeEncodingError`] if the system encoding cannot be detected.
#[instrument(level = "debug", err)]
pub fn get_native_encoding() -> Result<&'static Encoding, NativeEncodingError> {
    if let Some(encoding) = NATIVE_ENCODING.with(Cell::get) {
        return Ok(encoding);
    }

    let encoding = detect_system_native_encoding()?;
    NATIVE_ENCODING.with(|slot| slot.set(Some(encoding)));
    Ok(encoding)
}

/// Returns the canonical label for the native system encoding.
///
/// # Errors
///
/// Returns [`NativeEncodingError`] if the system encoding cannot be detected.
#[instrument(level = "debug", err)]
pub fn get_native_encoding_name() -> Result<&'static str, NativeEncodingError> {
    Ok(get_native_encoding()?.name())
}

/// Overrides the detected native system encoding.
///
/// # Errors
///
/// Returns [`UnknownEncodingError`] if `label` does not map to a known encoding.
#[instrument(level = "debug", skip_all, err, fields(label = %label))]
pub fn set_native_encoding(label: &str) -> Result<(), UnknownEncodingError> {
    let encoding =
        Encoding::for_label(label.as_bytes()).ok_or_else(|| UnknownEncodingError::new(label))?;
    NATIVE_ENCODING.with(|slot| slot.set(Some(encoding)));
    Ok(())
}

/// Clears any cached native encoding so it will be detected again on demand.
pub fn clear_native_encoding() {
    NATIVE_ENCODING.with(|slot| slot.set(None));
}

/// Detects the process-native text encoding for the current platform.
///
/// # Errors
///
/// Returns [`NativeEncodingError`] if the system encoding cannot be determined.
#[instrument(level = "debug", err)]
pub fn detect_system_native_encoding() -> Result<&'static Encoding, NativeEncodingError> {
    #[cfg(windows)]
    {
        detect_windows_native_encoding()
    }

    #[cfg(not(windows))]
    {
        detect_unix_native_encoding()
    }
}

/// Encodes a string using the current NWN encoding.
///
/// # Errors
///
/// Returns [`EncodingConversionError`] if the string cannot be represented in the current NWN encoding.
#[instrument(level = "debug", skip_all, err, fields(input_len = value.len()))]
pub fn to_nwnrs_encoding(value: &str) -> Result<Vec<u8>, EncodingConversionError> {
    encode_with(get_nwnrs_encoding(), value, "encode text for NWN")
}

/// Decodes bytes using the current NWN encoding.
///
/// # Errors
///
/// Returns [`EncodingConversionError`] if the bytes cannot be decoded with the current NWN encoding.
#[instrument(level = "debug", skip_all, err, fields(input_len = bytes.len()))]
pub fn from_nwnrs_encoding(bytes: &[u8]) -> Result<String, EncodingConversionError> {
    decode_with(get_nwnrs_encoding(), bytes, "decode text from NWN")
}

/// Encodes a string using the current native system encoding.
///
/// # Errors
///
/// Returns [`EncodingConversionError`] if the system encoding cannot be detected or the string cannot be represented in it.
#[instrument(level = "debug", skip_all, err, fields(input_len = value.len()))]
pub fn to_native_encoding(value: &str) -> Result<Vec<u8>, EncodingConversionError> {
    let encoding = get_native_encoding().map_err(|error| {
        EncodingConversionError::new(error.to_string(), "encode text for native output")
    })?;
    encode_with(encoding, value, "encode text for native output")
}

/// Decodes bytes using the current native system encoding.
///
/// # Errors
///
/// Returns [`EncodingConversionError`] if the system encoding cannot be detected or the bytes cannot be decoded with it.
#[instrument(level = "debug", skip_all, err, fields(input_len = bytes.len()))]
pub fn from_native_encoding(bytes: &[u8]) -> Result<String, EncodingConversionError> {
    let encoding = get_native_encoding().map_err(|error| {
        EncodingConversionError::new(error.to_string(), "decode text from native input")
    })?;
    decode_with(encoding, bytes, "decode text from native input")
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

#[cfg(not(windows))]
fn detect_unix_native_encoding() -> Result<&'static Encoding, NativeEncodingError> {
    for key in ["LC_ALL", "LC_CTYPE", "LANG"] {
        if let Ok(value) = env::var(key)
            && let Some(encoding) = parse_locale_encoding(&value)
        {
            return Ok(encoding);
        }
    }

    Err(NativeEncodingError::new(
        "unable to determine native encoding from LC_ALL, LC_CTYPE, or LANG",
    ))
}

#[cfg(windows)]
fn detect_windows_native_encoding() -> Result<&'static Encoding, NativeEncodingError> {
    if let Ok(chcp_output) = std::process::Command::new("chcp").output() {
        if let Ok(output_str) = String::from_utf8(chcp_output.stdout) {
            if let Some(code_page_str) = output_str
                .lines()
                .find(|line| line.contains("Active code page:"))
                .and_then(|line| line.split(':').nth(1))
                .map(|s| s.trim())
            {
                if let Ok(code_page) = code_page_str.parse::<u16>() {
                    if let Some(encoding) = codepage::to_encoding(code_page) {
                        return Ok(encoding);
                    }
                }
            }
        }
    }

    if let Some(encoding) = codepage::to_encoding(1252) {
        Ok(encoding)
    } else {
        Err(NativeEncodingError::new(
            "unable to determine Windows native encoding",
        ))
    }
}

pub(crate) fn parse_locale_encoding(locale: &str) -> Option<&'static Encoding> {
    let trimmed = locale.trim();
    if trimmed.is_empty() {
        return None;
    }

    let without_modifier = trimmed.split('@').next().unwrap_or(trimmed);
    let candidate = without_modifier
        .split_once('.')
        .map_or(without_modifier, |(_, encoding)| encoding);

    Encoding::for_label(candidate.trim().as_bytes())
}
