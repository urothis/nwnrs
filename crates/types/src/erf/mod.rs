#![forbid(unsafe_code)]
#![doc = include_str!("README.md")]

mod io;
mod types;

pub use io::*;
pub use types::*;

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::erf::{
        Erf, ErfError, ErfResult, ErfVersion, ErfWriteOptions, read_erf, read_erf_from_file,
        read_erf_shared, write_erf, write_erf_archive, write_erf_with_options,
    };
}
