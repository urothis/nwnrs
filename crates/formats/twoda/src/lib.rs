#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

mod io;
mod types;

pub use io::*;
pub use types::*;

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::{
        Cell, Row, TWO_DA_HEADER, TwoDa, TwoDaError, TwoDaResult, as_2da, escape_field, read_twoda,
        write_twoda,
    };
}
