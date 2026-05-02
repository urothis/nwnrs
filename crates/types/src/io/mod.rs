#![forbid(unsafe_code)]
#![doc = include_str!("README.md")]

mod errors;
mod helpers;

pub use errors::*;
pub use helpers::*;

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::io::{
        ExpectationError, SwappableEndian, expect, map_with_index, read_bytes_or_err,
        read_fixed_count_seq, read_str_or_err, swap_endian,
    };
}
