#![forbid(unsafe_code)]
#![doc = include_str!("README.md")]

mod constants;
mod types;

pub use constants::*;
pub use types::*;

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::exo::{EXO_RES_FILE_COMPRESSED_BUF_MAGIC, ExoResFileCompressionType};
}
