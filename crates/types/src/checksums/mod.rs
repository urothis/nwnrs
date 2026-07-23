#![forbid(unsafe_code)]
#![doc = include_str!("README.md")]

mod digest;
mod types;

pub use digest::*;
pub use types::*;

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::checksums::{
        EMPTY_SHA1_DIGEST, Md5Digest, ParseSha1DigestError, SHA1_HEX_LEN, SHA256_HEX_LEN,
        Sha1Digest, Sha256Digest, md5_digest, parse_sha1_digest, sha1_digest, sha1_digest_reader,
        sha256_digest,
    };
}

#[cfg(test)]
mod tests {
    use crate::checksums::{
        md5_digest, parse_sha1_digest, sha1_digest, sha1_digest_reader, sha256_digest,
    };

    #[test]
    fn sha1_digest_matches_known_vector_and_parses_case_insensitively() {
        let digest = sha1_digest(b"abc");
        assert_eq!(
            digest.to_string(),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        assert_eq!(
            parse_sha1_digest("A9993E364706816ABA3E25717850C26C9CD0D89D"),
            Ok(digest)
        );
    }

    #[test]
    fn streamed_sha1_matches_in_memory_digest() {
        let bytes = b"streamed archive fingerprint";
        assert_eq!(
            sha1_digest_reader(bytes.as_slice()).expect("stream digest"),
            sha1_digest(bytes)
        );
    }

    #[test]
    fn md5_matches_known_vector() {
        assert_eq!(
            md5_digest(b"abc").to_string(),
            "900150983cd24fb0d6963f7d28e17f72"
        );
    }

    #[test]
    fn invalid_sha1_inputs_are_reported() {
        assert!(parse_sha1_digest("xyz").is_err());
    }

    #[test]
    fn sha256_matches_known_vector() {
        assert_eq!(
            sha256_digest(b"abc").to_string(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
