#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]
mod io;
mod types;

pub use io::*;
pub use types::*;

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::{SsfEntry, SsfRoot, new_ssf, read_ssf, write_ssf};
}
