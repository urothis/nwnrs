#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

mod encoding;
mod errors;

pub use encoding::*;
pub use errors::*;

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::{
        EncodingConversionError, NativeEncodingError, UnknownEncodingError, clear_native_encoding,
        detect_system_native_encoding, from_native_encoding, from_nwnrs_encoding,
        get_native_encoding, get_native_encoding_name, get_nwnrs_encoding, get_nwnrs_encoding_name,
        set_native_encoding, set_nwnrs_encoding, to_native_encoding, to_nwnrs_encoding,
    };
}
