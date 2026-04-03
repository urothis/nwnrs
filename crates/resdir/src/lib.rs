#![forbid(unsafe_code)]
//! Directory-backed [`nwn_resman::ResContainer`] implementation.
//!
//! `nwn-resdir` scans an on-disk directory tree, resolves filenames into NWN resource
//! references, and exposes the result through the shared resource-container abstraction.
//! Override folders and unpacked working directories are the primary use cases.

mod read;
mod types;

pub use read::*;
pub use types::*;

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::{ResDir, ResDirError, ResDirResult, read_resdir};
}
