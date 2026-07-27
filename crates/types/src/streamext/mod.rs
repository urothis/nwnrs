#![forbid(unsafe_code)]
#![doc = include_str!("README.md")]

mod io;
mod size_prefix;

pub use io::*;
pub use size_prefix::*;

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::streamext::{
        SizePrefix, read_array, read_bytes, read_fixed_count_seq, read_fixed_value, read_nwn_text,
        read_size_prefixed_bytes, read_size_prefixed_nwn_text, read_size_prefixed_seq,
        write_size_prefixed_bytes, write_size_prefixed_nwn_text, write_size_prefixed_seq,
    };
}
