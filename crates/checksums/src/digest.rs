use sha1::{Digest as _, Sha1};
use tracing::instrument;

use crate::{Md5Digest, ParseSecureHashError, SECURE_HASH_HEX_LEN, SecureHash};

/// Computes the SHA-1 digest for `data`.
#[instrument(level = "debug", skip_all)]
pub fn secure_hash(data: impl AsRef<[u8]>) -> SecureHash {
    let digest = Sha1::digest(data.as_ref());
    let mut bytes = [0_u8; 20];
    bytes.copy_from_slice(&digest);
    SecureHash::new(bytes)
}

/// Parses a lowercase or uppercase hexadecimal SHA-1 digest.
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
#[instrument(level = "debug", skip_all)]
pub fn md5_digest(data: impl AsRef<[u8]>) -> Md5Digest {
    Md5Digest::new(md5::compute(data).0)
}
