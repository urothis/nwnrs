#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

mod io;
mod types;

pub use io::*;
pub use types::*;

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::{
        DATA_ELEMENT_SIZE, HEADER_SIZE, SingleTlk, Tlk, TlkEntry, TlkError, TlkPair, TlkResult,
        read_single_tlk, read_single_tlk_from_res, write_single_tlk,
    };
}
