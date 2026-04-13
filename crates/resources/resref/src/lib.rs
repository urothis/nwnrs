#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

mod parse;
mod types;

pub use parse::*;
pub use types::*;

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::{
        RESREF_MAX_LENGTH, ResRef, ResRefError, ResolvedResRef, is_valid_resref_part1,
    };
}
