//! Supported native runtime platforms.

use std::{env, fmt};

use serde::{Deserialize, Serialize};

use crate::{RuntimeError, RuntimeResult};

/// An operating system supported by the native runtime.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OperatingSystem {
    /// Apple macOS.
    Macos,
    /// GNU/Linux.
    Linux,
    /// Microsoft Windows.
    Windows,
}

impl fmt::Display for OperatingSystem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Macos => formatter.write_str("macos"),
            Self::Linux => formatter.write_str("linux"),
            Self::Windows => formatter.write_str("windows"),
        }
    }
}

/// A CPU architecture supported by the native runtime.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum Architecture {
    /// The 64-bit ARM architecture.
    #[serde(rename = "aarch64")]
    Aarch64,
    /// The 64-bit x86 architecture.
    #[serde(rename = "x86_64")]
    X86_64,
}

impl fmt::Display for Architecture {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Aarch64 => formatter.write_str("aarch64"),
            Self::X86_64 => formatter.write_str("x86_64"),
        }
    }
}

/// A supported operating-system and CPU-architecture pair.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Platform {
    /// The executable operating system.
    pub os:           OperatingSystem,
    /// The executable CPU architecture.
    pub architecture: Architecture,
}

impl Platform {
    /// Returns the platform on which this crate was compiled.
    ///
    /// # Errors
    ///
    /// Returns an error when compiled for an unsupported operating system or
    /// architecture.
    pub fn host() -> RuntimeResult<Self> {
        let os = if cfg!(target_os = "macos") {
            OperatingSystem::Macos
        } else if cfg!(target_os = "linux") {
            OperatingSystem::Linux
        } else if cfg!(target_os = "windows") {
            OperatingSystem::Windows
        } else {
            return Err(RuntimeError::new(format!(
                "unsupported host operating system: {}",
                env::consts::OS
            )));
        };

        let architecture = if cfg!(target_arch = "aarch64") {
            Architecture::Aarch64
        } else if cfg!(target_arch = "x86_64") {
            Architecture::X86_64
        } else {
            return Err(RuntimeError::new(format!(
                "unsupported host architecture: {}",
                env::consts::ARCH
            )));
        };

        Ok(Self {
            os,
            architecture,
        })
    }

    /// Returns the target-pack directory component for this platform.
    #[must_use]
    pub fn directory_name(self) -> String {
        format!("{}-{}", self.os, self.architecture)
    }
}

impl fmt::Display for Platform {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}-{}", self.os, self.architecture)
    }
}
