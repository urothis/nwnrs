#![forbid(unsafe_code)]
//! Minimal weighted least-recently-used cache.
//!
//! The cache is small on purpose: it tracks insertion order, access count, and total weight,
//! which is sufficient for the resource-manager and TLK caches elsewhere in the workspace.
//!
//! Use [`WeightedLru`] when eviction should be based on approximate byte size rather than item
//! count alone.

mod cache;
mod types;

pub use types::*;

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::{Weight, WeightedLru};
}
