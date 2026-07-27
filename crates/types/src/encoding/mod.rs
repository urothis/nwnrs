#![forbid(unsafe_code)]
#![doc = include_str!("README.md")]

mod api;
mod errors;

pub use api::*;
pub use errors::*;

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::encoding::{
        EncodingConversionError, LegacyEncoding, NwnText, NwnTextContext, NwnTextSegment,
        UnknownEncodingError, decode_legacy, decode_legacy_preserving, display_legacy_preserving,
        encode_legacy, encode_legacy_preserving,
    };
}
