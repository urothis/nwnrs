use crate::{EncodingConversionError, NativeEncodingError, UnknownEncodingError};
use encoding_rs::{Encoding, WINDOWS_1252};
use std::cell::Cell;
use std::env;
use tracing::instrument;

thread_local! {
    static NWN_ENCODING: Cell<&'static Encoding> = Cell::new(WINDOWS_1252);
    static NATIVE_ENCODING: Cell<Option<&'static Encoding>> = const { Cell::new(None) };
}

/// Returns the encoding currently used for NWN text data.
pub fn get_nwn_encoding() -> &'static Encoding {
    NWN_ENCODING.with(Cell::get)
}

/// Returns the canonical label for the current NWN text encoding.
pub fn get_nwn_encoding_name() -> &'static str {
    get_nwn_encoding().name()
}

/// Sets the encoding used for NWN text data.
#[instrument(level = "debug", skip_all, err, fields(label = %label))]
pub fn set_nwn_encoding(label: &str) -> Result<(), UnknownEncodingError> {
    let encoding =
        Encoding::for_label(label.as_bytes()).ok_or_else(|| UnknownEncodingError::new(label))?;
    NWN_ENCODING.with(|slot| slot.set(encoding));
    Ok(())
}

/// Returns the configured or detected native system encoding.
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
#[instrument(level = "debug", err)]
pub fn get_native_encoding_name() -> Result<&'static str, NativeEncodingError> {
    Ok(get_native_encoding()?.name())
}

/// Overrides the detected native system encoding.
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
#[instrument(level = "debug", skip_all, err, fields(input_len = value.len()))]
pub fn to_nwn_encoding(value: &str) -> Result<Vec<u8>, EncodingConversionError> {
    encode_with(get_nwn_encoding(), value, "encode text for NWN")
}

/// Decodes bytes using the current NWN encoding.
#[instrument(level = "debug", skip_all, err, fields(input_len = bytes.len()))]
pub fn from_nwn_encoding(bytes: &[u8]) -> Result<String, EncodingConversionError> {
    decode_with(get_nwn_encoding(), bytes, "decode text from NWN")
}

/// Encodes a string using the current native system encoding.
#[instrument(level = "debug", skip_all, err, fields(input_len = value.len()))]
pub fn to_native_encoding(value: &str) -> Result<Vec<u8>, EncodingConversionError> {
    let encoding = get_native_encoding().map_err(|error| {
        EncodingConversionError::new(error.to_string(), "encode text for native output")
    })?;
    encode_with(encoding, value, "encode text for native output")
}

/// Decodes bytes using the current native system encoding.
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
    let code_page = unsafe { GetACP() };
    codepage::to_encoding(code_page).ok_or_else(|| {
        NativeEncodingError::new(format!(
            "unable to map Windows ANSI code page {code_page} to an encoding"
        ))
    })
}

pub(crate) fn parse_locale_encoding(locale: &str) -> Option<&'static Encoding> {
    let trimmed = locale.trim();
    if trimmed.is_empty() {
        return None;
    }

    let without_modifier = trimmed.split('@').next().unwrap_or(trimmed);
    let candidate = without_modifier
        .split_once('.')
        .map(|(_, encoding)| encoding)
        .unwrap_or(without_modifier);

    Encoding::for_label(candidate.trim().as_bytes())
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetACP() -> u32;
}
