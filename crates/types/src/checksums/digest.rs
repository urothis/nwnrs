use sha1::{Digest as _, Sha1};
use tracing::instrument;

use crate::checksums::prelude::*;

/// Computes the SHA-1 digest for `data`.
///
/// # Examples
///
/// ```
/// let digest = nwnrs_types::checksums::secure_hash(b"abc");
/// assert_eq!(digest.to_string(), "a9993e364706816aba3e25717850c26c9cd0d89d");
/// ```
#[instrument(level = "debug", skip_all)]
pub fn secure_hash(data: impl AsRef<[u8]>) -> SecureHash {
    let digest = Sha1::digest(data.as_ref());
    let mut bytes = [0_u8; 20];
    bytes.copy_from_slice(&digest);
    SecureHash::new(bytes)
}

/// Parses a lowercase or uppercase hexadecimal SHA-1 digest.
///
/// # Errors
///
/// Returns [`ParseSecureHashError`] if the input is not a valid 40-character
/// hex SHA-1 string.
///
/// # Examples
///
/// ```
/// let digest = nwnrs_types::checksums::parse_secure_hash(
///     "A9993E364706816ABA3E25717850C26C9CD0D89D",
/// )?;
/// assert_eq!(digest.to_string(), "a9993e364706816aba3e25717850c26c9cd0d89d");
/// # Ok::<(), nwnrs_types::checksums::ParseSecureHashError>(())
/// ```
#[instrument(level = "debug", skip_all, err, fields(input_len = input.len()))]
pub fn parse_secure_hash(input: &str) -> Result<SecureHash, ParseSecureHashError> {
    if input.len() != SECURE_HASH_HEX_LEN {
        return Err(ParseSecureHashError::new(input));
    }

    let mut bytes = [0_u8; 20];
    for (slot, chunk) in bytes.iter_mut().zip(input.as_bytes().chunks_exact(2)) {
        let chunk =
            std::str::from_utf8(chunk).map_err(|_error| ParseSecureHashError::new(input))?;
        *slot = u8::from_str_radix(chunk, 16).map_err(|_error| ParseSecureHashError::new(input))?;
    }

    Ok(SecureHash::new(bytes))
}

/// Computes the MD5 digest for `data`.
///
/// # Examples
///
/// ```
/// let digest = nwnrs_types::checksums::md5_digest(b"abc");
/// assert_eq!(digest.to_string(), "900150983cd24fb0d6963f7d28e17f72");
/// ```
#[instrument(level = "debug", skip_all)]
pub fn md5_digest(data: impl AsRef<[u8]>) -> Md5Digest {
    Md5Digest::new(md5::compute(data).0)
}
