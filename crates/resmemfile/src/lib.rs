#![forbid(unsafe_code)]
//! In-memory [`nwnrs_resman::ResContainer`] implementation.
//!
//! This crate turns a byte buffer into a single resource entry. It is mainly
//! useful in tests, synthetic pipelines, and cases where a decoded or
//! downloaded payload should be treated like any other container-backed
//! resource.

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
