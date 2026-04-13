#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

mod read;
mod types;

pub use read::*;
pub use types::*;

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::{ResFile, ResFileError, ResFileResult, read_resfile, read_resfile_as};
}
