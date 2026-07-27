//! Explicit presentation helpers for byte-oriented engine data.

use std::path::Path;

use nwnrs_types::encoding::{LegacyEncoding, NwnText};

/// Renders Windows-1252 engine bytes without replacement characters.
///
/// Undefined byte values remain visible as escaped opaque segments.
#[must_use]
pub fn display_engine_text(bytes: &[u8]) -> String {
    NwnText::decode(bytes, LegacyEncoding::Windows1252).display_text()
}

/// Renders a native filesystem path without replacing invalid native units.
#[must_use]
pub fn display_native_path(path: &Path) -> String {
    nwnrs_types::native_path::display(path)
}
