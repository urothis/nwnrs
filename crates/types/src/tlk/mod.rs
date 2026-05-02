#![forbid(unsafe_code)]
#![doc = include_str!("README.md")]

mod io;
mod types;

pub use io::*;
pub use nwnrs_types::resman::CachePolicy;
pub use types::*;

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use nwnrs_types::resman::CachePolicy;

    pub use crate::tlk::{
        DATA_ELEMENT_SIZE, HEADER_SIZE, SingleTlk, Tlk, TlkEntry, TlkError, TlkLayerWriteTarget,
        TlkPair, TlkResult, TlkWriteStream, read_single_tlk, write_single_tlk, write_tlk_chain,
    };
}
