//! Exact executable identity and binary metadata.

use std::{
    fmt, fs,
    fs::File,
    path::{Path, PathBuf},
};

use crate::{Platform, RuntimeError, RuntimeResult, file_sha256, read_platform};

/// The stable SHA-256 identity of one file.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct FileSha256(pub(crate) [u8; 32]);

impl fmt::Display for FileSha256 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// The identity and platform encoded by one native executable file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BinaryIdentity {
    /// Canonical path to the binary.
    pub path:     PathBuf,
    /// SHA-256 of the complete binary file.
    pub sha256:   FileSha256,
    /// Platform encoded in the ELF, Mach-O, or PE header.
    pub platform: Platform,
}

impl BinaryIdentity {
    /// Reads and identifies an ELF, Mach-O, or PE binary.
    ///
    /// # Errors
    ///
    /// Returns an error when the path cannot be canonicalized or read, or when
    /// its binary format, architecture, or operating system is unsupported.
    pub fn read(path: impl AsRef<Path>) -> RuntimeResult<Self> {
        let requested = path.as_ref();
        let path = fs::canonicalize(requested).map_err(|error| {
            RuntimeError::new(format!(
                "failed to resolve binary {}: {error}",
                requested.display()
            ))
        })?;
        let mut file = File::open(&path).map_err(|error| {
            RuntimeError::new(format!("failed to open binary {}: {error}", path.display()))
        })?;
        let platform = read_platform(&mut file, &path)?;
        let sha256 = file_sha256(&path)?;

        Ok(Self {
            path,
            sha256,
            platform,
        })
    }
}
