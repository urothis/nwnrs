#![forbid(unsafe_code)]
//! Digest helpers used throughout the workspace.
//!
//! This crate provides small typed wrappers around SHA-1 and MD5 digests together with
//! conversion helpers. SHA-1 is used heavily by NWSync manifests, KEY/BIF metadata, and
//! archive payload tracking, while MD5 is used by the CLI metadata files that support
//! round-tripping unpacked content.
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
