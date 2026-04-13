#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

mod manager;
mod types;

pub use manager::*;
pub use types::*;

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::{
        CachePolicy, MEMORY_CACHE_THRESHOLD, ReadSeek, Res, ResContainer, ResIoSpawner, ResMan,
        ResManError, ResManResult, ResOrigin, SharedReadSeek, new_res_origin, shared_stream,
    };
}
