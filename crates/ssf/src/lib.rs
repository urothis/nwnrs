#![forbid(unsafe_code)]
//! Reader and writer for soundset (`SSF`) files.
//!
//! SSF files are small, fixed-layout tables that map soundset slots to resource references and
//! dialog string references. This crate keeps the representation deliberately simple.
mod io;
mod types;

pub use io::*;
pub use types::*;

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::{SsfEntry, SsfRoot, new_ssf, read_ssf, write_ssf};
}
