#![forbid(unsafe_code)]
#![doc = include_str!("README.md")]
mod io;
mod types;

pub use io::*;
pub use types::*;

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::ssf::{SsfEntry, SsfError, SsfResult, SsfRoot, read_ssf, write_ssf};
}
