#![forbid(unsafe_code)]
//! Single-file [`nwnrs_resman::ResContainer`] implementation.
//!
//! This crate wraps one on-disk file as a one-entry resource container. It is
//! useful when a caller already knows the intended [`nwnrs_resref::ResRef`] and
//! wants to feed a loose file into the same APIs used for directories and
//! archives.

mod read;
mod types;

pub use read::*;
pub use types::*;

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::{ResFile, ResFileError, ResFileResult, read_resfile, read_resfile_as};
}

#[cfg(test)]
#[path = "../tests/unit/mod.rs"]
mod tests;
