//! Lossless native-filesystem path presentation and ordering.

use std::path::Path;

/// Returns a reversible display identity for a native filesystem path.
///
/// Valid UTF-8 paths retain their ordinary spelling. Paths that cannot be
/// represented as UTF-8 use a platform-tagged hexadecimal encoding of their
/// native bytes or UTF-16 code units. This avoids silently replacing path
/// data at diagnostics, transport, and logging boundaries.
#[must_use]
pub fn display(path: &Path) -> String {
    if let Some(value) = path.to_str() {
        return value.to_string();
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt as _;
        format!("unix:{}", hexadecimal(path.as_os_str().as_bytes()))
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt as _;
        let bytes = path
            .as_os_str()
            .encode_wide()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        format!("windows:{}", hexadecimal(&bytes))
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        "unsupported-native-path".to_string()
    }
}

/// Returns an exact native path key with ASCII case folding.
///
/// This is suitable for NWN logical resource ordering and collision checks.
/// It must not be used as physical filesystem identity.
#[must_use]
pub fn ascii_folded_key(path: &Path) -> Vec<u8> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt as _;
        path.as_os_str()
            .as_bytes()
            .iter()
            .map(u8::to_ascii_lowercase)
            .collect()
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt as _;
        path.as_os_str()
            .encode_wide()
            .map(|unit| match u8::try_from(unit) {
                Ok(ascii) => u16::from(ascii.to_ascii_lowercase()),
                Err(_) => unit,
            })
            .flat_map(u16::to_le_bytes)
            .collect()
    }
    #[cfg(not(any(unix, windows)))]
    {
        path.as_os_str()
            .as_encoded_bytes()
            .iter()
            .map(u8::to_ascii_lowercase)
            .collect()
    }
}

fn hexadecimal(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let mut output = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        if write!(&mut output, "{byte:02x}").is_err() {
            return "hex-encoding-failed".to_string();
        }
    }
    output
}
