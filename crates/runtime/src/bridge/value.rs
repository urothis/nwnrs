//! Values transported between NWScript and the safe dispatcher.

use super::{BridgeError, BridgeErrorCode};

/// A three-dimensional vector exchanged with NWScript.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vector {
    /// X coordinate.
    pub x: f32,
    /// Y coordinate.
    pub y: f32,
    /// Z coordinate.
    pub z: f32,
}

/// One value held by the native NWScript bridge.
#[derive(Clone, Debug, PartialEq)]
pub enum BridgeValue {
    /// Signed NWScript integer.
    Integer(i32),
    /// Single-precision NWScript float.
    Float(f32),
    /// Neverwinter Nights object identifier.
    Object(u32),
    /// Owned NWScript string bytes.
    String(Vec<u8>),
    /// Three-dimensional vector.
    Vector(Vector),
}

/// Severity attached to one log message emitted by NWScript.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScriptLogLevel {
    /// Highly detailed execution tracing.
    Trace,
    /// Diagnostic information useful while developing.
    Debug,
    /// Normal operational information.
    Info,
    /// A recoverable or suspicious condition.
    Warn,
    /// A failed operation requiring attention.
    Error,
}

impl TryFrom<i32> for ScriptLogLevel {
    type Error = BridgeError;

    fn try_from(value: i32) -> Result<Self, BridgeError> {
        match value {
            0 => Ok(Self::Trace),
            1 => Ok(Self::Debug),
            2 => Ok(Self::Info),
            3 => Ok(Self::Warn),
            4 => Ok(Self::Error),
            _ => Err(BridgeError::new(
                BridgeErrorCode::InvalidArgument,
                format!("invalid NWNRS log level {value}; expected 0 through 4"),
            )),
        }
    }
}

/// One validated log record sent from NWScript to the native runtime.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScriptLog {
    /// Requested severity.
    pub level:   ScriptLogLevel,
    /// Original NWScript string bytes.
    pub message: Vec<u8>,
}

impl BridgeValue {
    pub(super) fn kind(&self) -> &'static str {
        match self {
            Self::Integer(_) => "integer",
            Self::Float(_) => "float",
            Self::Object(_) => "object",
            Self::String(_) => "string",
            Self::Vector(_) => "vector",
        }
    }
}
