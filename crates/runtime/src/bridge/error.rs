//! Stable errors exposed by the NWScript bridge.

use std::{error::Error, fmt};

/// Stable error codes exposed to NWScript.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(i32)]
pub enum BridgeErrorCode {
    /// No bridge error has been recorded.
    None = 0,
    /// The requested namespace is not registered.
    UnknownNamespace = 1,
    /// The requested function is not registered.
    UnknownFunction = 2,
    /// An argument or return value was missing or had the wrong type.
    InvalidArgument = 3,
    /// The exact target pack does not provide the required capability.
    MissingCapability = 4,
    /// A validated native engine operation failed.
    Engine = 5,
    /// A script attempted to reenter the per-thread bridge state.
    Reentrant = 6,
}

impl BridgeErrorCode {
    /// Returns the stable integer representation used by NWScript.
    #[must_use]
    pub const fn value(self) -> i32 {
        match self {
            Self::None => 0,
            Self::UnknownNamespace => 1,
            Self::UnknownFunction => 2,
            Self::InvalidArgument => 3,
            Self::MissingCapability => 4,
            Self::Engine => 5,
            Self::Reentrant => 6,
        }
    }
}

/// An error produced while dispatching a call from NWScript.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BridgeError {
    code:    BridgeErrorCode,
    message: String,
}

impl BridgeError {
    /// Creates a bridge error with a stable public code.
    #[must_use]
    pub fn new(code: BridgeErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// Returns the stable error code.
    #[must_use]
    pub const fn code(&self) -> BridgeErrorCode {
        self.code
    }

    /// Returns the diagnostic message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for BridgeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for BridgeError {}

/// Result returned by safe bridge operations.
pub type BridgeResult<T> = Result<T, BridgeError>;
