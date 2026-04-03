#![forbid(unsafe_code)]
//! Shared support code used across the workspace.
//!
//! This crate houses the intentionally generic pieces that would otherwise be
//! duplicated: encoding selection and conversion, IO helpers, endian swapping,
//! and simple expectation-style errors for format validation.

mod encoding;
mod errors;
mod io;

pub use encoding::*;
pub use errors::*;
pub use io::*;

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::{
        EncodingConversionError, ExpectationError, NativeEncodingError, SwappableEndian,
        UnknownEncodingError, clear_native_encoding, detect_system_native_encoding, expect,
        from_native_encoding, from_nwn_encoding, get_native_encoding, get_native_encoding_name,
        get_nwn_encoding, get_nwn_encoding_name, map_with_index, read_bytes_or_err,
        read_fixed_count_seq, read_str_or_err, set_native_encoding, set_nwn_encoding, swap_endian,
        to_native_encoding, to_nwn_encoding,
    };
}
