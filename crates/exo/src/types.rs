use std::fmt;

use tracing::{debug, instrument};

/// Compression markers stored by EXO resource containers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum ExoResFileCompressionType {
    /// The payload is stored without compression.
    None = 0,
    /// The payload is stored as a compressed buffer.
    CompressedBuf = 1,
}

impl ExoResFileCompressionType {
    /// Converts a raw numeric marker into a compression type.
    #[instrument(level = "debug", fields(value))]
    pub fn from_u32(value: u32) -> Option<Self> {
        let result = match value {
            0 => Self::None,
            1 => Self::CompressedBuf,
            _ => return None,
        };
        debug!(compression = %result, "resolved EXO compression marker");
        Some(result)
    }
}

impl fmt::Display for ExoResFileCompressionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => f.write_str("None"),
            Self::CompressedBuf => f.write_str("CompressedBuf"),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::ExoResFileCompressionType;

    #[test]
    fn converts_and_formats_known_compression_markers() {
        assert_eq!(
            ExoResFileCompressionType::from_u32(0),
            Some(ExoResFileCompressionType::None)
        );
        assert_eq!(
            ExoResFileCompressionType::from_u32(1),
            Some(ExoResFileCompressionType::CompressedBuf)
        );
        assert_eq!(ExoResFileCompressionType::from_u32(7), None);
        assert_eq!(ExoResFileCompressionType::CompressedBuf.to_string(), "CompressedBuf");
    }
}
