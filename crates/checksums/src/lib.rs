#![forbid(unsafe_code)]
//! Digest helpers used throughout the workspace.
//!
//! This crate provides small typed wrappers around SHA-1 and MD5 digests
//! together with conversion helpers. SHA-1 is used heavily by NWSync manifests,
//! KEY/BIF metadata, and archive payload tracking, while MD5 is used by the CLI
//! metadata files that support round-tripping unpacked content.
//!
//! Start with [`secure_hash`], [`parse_secure_hash`], and [`md5_digest`].

mod digest;
mod types;

pub use digest::*;
pub use types::*;

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::{
        EMPTY_SECURE_HASH, Md5Digest, ParseSecureHashError, SECURE_HASH_HEX_LEN, SecureHash,
        md5_digest, parse_secure_hash, secure_hash,
    };
}

#[allow(clippy::panic)]
#[cfg(test)]
mod tests {
    use crate::{md5_digest, parse_secure_hash, secure_hash};

    #[test]
    fn secure_hash_matches_known_vector_and_parses_case_insensitively() {
        let digest = secure_hash(b"abc");
        assert_eq!(
            digest.to_string(),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        assert_eq!(
            parse_secure_hash("A9993E364706816ABA3E25717850C26C9CD0D89D"),
            Ok(digest)
        );
    }

    #[test]
    fn md5_and_invalid_sha1_inputs_are_reported() {
        assert_eq!(
            md5_digest(b"abc").to_string(),
            "900150983cd24fb0d6963f7d28e17f72"
        );
        assert!(parse_secure_hash("xyz").is_err());
    }
}
