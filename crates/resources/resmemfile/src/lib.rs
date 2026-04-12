#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

mod read;
mod types;

pub use read::*;
pub use types::*;

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::{
        ResMemFile, ResMemFileError, ResMemFileResult, read_resmemfile, read_resmemfile_arc,
    };
}
