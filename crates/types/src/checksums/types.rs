use std::{error::Error, fmt, str::FromStr};

/// The lowercase hexadecimal length of a SHA-1 digest.
pub const SHA1_HEX_LEN: usize = 40;
/// The lowercase hexadecimal length of a SHA-256 digest.
pub const SHA256_HEX_LEN: usize = 64;
/// The all-zero SHA-1 digest.
pub const EMPTY_SHA1_DIGEST: Sha1Digest = Sha1Digest([0_u8; 20]);

/// A 20-byte SHA-1 digest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Sha1Digest(pub(crate) [u8; 20]);

/// A 16-byte MD5 digest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Md5Digest(pub(crate) [u8; 16]);

/// A 32-byte SHA-256 digest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Sha256Digest(pub(crate) [u8; 32]);

/// An error returned when parsing a hexadecimal SHA-1 digest fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseSha1DigestError {
    pub(crate) input: String,
}

impl ParseSha1DigestError {
    pub(crate) fn new(input: &str) -> Self {
        Self {
            input: input.to_string(),
        }
    }
}

impl fmt::Display for ParseSha1DigestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "not a valid SHA-1 digest: {:?}", self.input)
    }
}

impl Error for ParseSha1DigestError {}

impl Sha1Digest {
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

impl AsRef<[u8]> for Sha1Digest {
    fn as_ref(&self) -> &[u8] {
        self.0.as_slice()
    }
}

impl fmt::Display for Sha1Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl FromStr for Sha1Digest {
    type Err = ParseSha1DigestError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        crate::checksums::parse_sha1_digest(s)
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

impl Sha256Digest {
    /// Creates a digest from its raw bytes.
    #[must_use]
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Returns the digest as a fixed-size byte array.
    #[must_use]
    pub fn into_bytes(self) -> [u8; 32] {
        self.0
    }

    /// Returns the digest as a borrowed byte array.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl AsRef<[u8]> for Sha256Digest {
    fn as_ref(&self) -> &[u8] {
        self.0.as_slice()
    }
}

impl fmt::Display for Sha256Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
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
