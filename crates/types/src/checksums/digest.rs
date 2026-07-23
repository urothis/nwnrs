use std::io::{self, Read};

use sha1::{Digest as _, Sha1};
use sha2::Sha256;
use tracing::instrument;

use crate::checksums::prelude::*;

/// Computes the SHA-1 digest for `data`.
///
/// # Examples
///
/// ```
/// let digest = nwnrs_types::checksums::sha1_digest(b"abc");
/// assert_eq!(digest.to_string(), "a9993e364706816aba3e25717850c26c9cd0d89d");
/// ```
#[instrument(level = "debug", skip_all)]
pub fn sha1_digest(data: impl AsRef<[u8]>) -> Sha1Digest {
    let digest = Sha1::digest(data.as_ref());
    let mut bytes = [0_u8; 20];
    bytes.copy_from_slice(&digest);
    Sha1Digest::new(bytes)
}

/// Computes a SHA-1 digest by streaming bytes from `reader`.
///
/// This avoids materializing large archives solely to fingerprint them.
///
/// # Errors
///
/// Returns the first I/O error encountered while reading.
pub fn sha1_digest_reader(mut reader: impl Read) -> io::Result<Sha1Digest> {
    let mut hasher = Sha1::new();
    let mut buffer = [0_u8; 128 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        let chunk = buffer
            .get(..read)
            .ok_or_else(|| io::Error::other("SHA-1 reader returned an invalid byte count"))?;
        hasher.update(chunk);
    }
    let digest = hasher.finalize();
    let mut bytes = [0_u8; 20];
    bytes.copy_from_slice(&digest);
    Ok(Sha1Digest::new(bytes))
}

/// Parses a lowercase or uppercase hexadecimal SHA-1 digest.
///
/// # Errors
///
/// Returns [`ParseSha1DigestError`] if the input is not a valid 40-character
/// hex SHA-1 string.
///
/// # Examples
///
/// ```
/// let digest = nwnrs_types::checksums::parse_sha1_digest(
///     "A9993E364706816ABA3E25717850C26C9CD0D89D",
/// )?;
/// assert_eq!(digest.to_string(), "a9993e364706816aba3e25717850c26c9cd0d89d");
/// # Ok::<(), nwnrs_types::checksums::ParseSha1DigestError>(())
/// ```
#[instrument(level = "debug", skip_all, err, fields(input_len = input.len()))]
pub fn parse_sha1_digest(input: &str) -> Result<Sha1Digest, ParseSha1DigestError> {
    if input.len() != SHA1_HEX_LEN {
        return Err(ParseSha1DigestError::new(input));
    }

    let mut bytes = [0_u8; 20];
    let (chunks, _remainder) = input.as_bytes().as_chunks::<2>();
    for (slot, chunk) in bytes.iter_mut().zip(chunks) {
        let chunk =
            std::str::from_utf8(chunk).map_err(|_error| ParseSha1DigestError::new(input))?;
        *slot = u8::from_str_radix(chunk, 16).map_err(|_error| ParseSha1DigestError::new(input))?;
    }

    Ok(Sha1Digest::new(bytes))
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

/// Computes the SHA-256 digest for `data`.
///
/// # Examples
///
/// ```
/// let digest = nwnrs_types::checksums::sha256_digest(b"abc");
/// assert_eq!(
///     digest.to_string(),
///     "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
/// );
/// ```
#[instrument(level = "debug", skip_all)]
pub fn sha256_digest(data: impl AsRef<[u8]>) -> Sha256Digest {
    let digest = <Sha256 as sha2::Digest>::digest(data.as_ref());
    let mut bytes = [0_u8; 32];
    bytes.copy_from_slice(&digest);
    Sha256Digest::new(bytes)
}
