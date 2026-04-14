use std::{error::Error, fmt, str::FromStr};

/// The lowercase hexadecimal length of a SHA-1 digest.
pub const SECURE_HASH_HEX_LEN: usize = 40;
/// The all-zero SHA-1 digest.
pub const EMPTY_SECURE_HASH: SecureHash = SecureHash([0_u8; 20]);

/// A 20-byte SHA-1 digest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct SecureHash(pub(crate) [u8; 20]);

/// A 16-byte MD5 digest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Md5Digest(pub(crate) [u8; 16]);

/// An error returned when parsing a hexadecimal SHA-1 digest fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseSecureHashError {
    pub(crate) input: String,
}

impl ParseSecureHashError {
    pub(crate) fn new(input: &str) -> Self {
        Self {
            input: input.to_string(),
        }
    }
}

impl fmt::Display for ParseSecureHashError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Not a valid SHA1: {:?}", self.input)
    }
}

impl Error for ParseSecureHashError {}

impl SecureHash {
    /// Creates a digest from its raw bytes.
    #[must_use]
    pub fn new(bytes: [u8; 20]) -> Self {
        Self(bytes)
    }

    /// Returns the digest as a fixed-size byte array.
    #[must_use]
    pub fn into_bytes(self) -> [u8; 20] {
        self.0
    }

    /// Returns the digest as a borrowed byte array.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 20] {
        &self.0
    }
}

impl AsRef<[u8]> for SecureHash {
    fn as_ref(&self) -> &[u8] {
        self.0.as_slice()
    }
}

impl fmt::Display for SecureHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl FromStr for SecureHash {
    type Err = ParseSecureHashError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        crate::parse_secure_hash(s)
    }
}

impl Md5Digest {
    /// Creates a digest from its raw bytes.
    #[must_use]
    pub fn new(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    /// Returns the digest as a fixed-size byte array.
    #[must_use]
    pub fn into_bytes(self) -> [u8; 16] {
        self.0
    }

    /// Returns the digest as a borrowed byte array.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

impl AsRef<[u8]> for Md5Digest {
    fn as_ref(&self) -> &[u8] {
        self.0.as_slice()
    }
}

impl fmt::Display for Md5Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}
