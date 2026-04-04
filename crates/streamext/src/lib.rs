#![forbid(unsafe_code)]
//! Stream helpers for size-prefixed binary formats.
//!
//! Several nwnrs-adjacent formats use compact little-endian length prefixes. This
//! crate provides small generic helpers for reading and writing such values
//! without pulling in a larger binary codec framework.

mod io;
mod size_prefix;

pub use io::*;
pub use size_prefix::*;

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::{
        SizePrefix, read_array, read_bytes, read_fixed_count_seq, read_fixed_value,
        read_size_prefixed_bytes, read_size_prefixed_seq, read_size_prefixed_string, read_string,
        write_size_prefixed_bytes, write_size_prefixed_seq, write_size_prefixed_string,
    };
}
