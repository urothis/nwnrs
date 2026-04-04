#![forbid(unsafe_code)]
//! Shared EXO-level constants and enums.
//!
//! This crate is intentionally narrow. It contains the compression markers and
//! magic values that appear in EXO-backed container formats such as ERF,
//! KEY/BIF, and compressed buffers.

mod constants;
mod types;

pub use constants::*;
pub use types::*;

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::{EXO_RES_FILE_COMPRESSED_BUF_MAGIC, ExoResFileCompressionType};
}
