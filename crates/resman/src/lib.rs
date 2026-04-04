#![forbid(unsafe_code)]
//! Shared resource abstraction and layered resource manager.
//!
//! Most crates in this workspace eventually converge on the types defined here:
//! [`Res`] represents a single resolvable payload, [`ResContainer`] represents
//! a source of resources, and [`ResMan`] resolves those containers in
//! precedence order with optional LRU caching.
//!
//! If you are integrating multiple NWN data sources, this is the crate to start
//! with.

mod manager;
mod types;

pub use manager::*;
pub use types::*;

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::{
        MEMORY_CACHE_THRESHOLD, ReadSeek, Res, ResContainer, ResIoSpawner, ResMan, ResManError,
        ResManResult, ResOrigin, SharedReadSeek, new_res_origin, shared_stream,
    };
}
