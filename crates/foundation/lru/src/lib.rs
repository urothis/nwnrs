#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

mod cache;
mod types;

pub use types::*;

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::{Weight, WeightedLru};
}
