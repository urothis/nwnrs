#![forbid(unsafe_code)]
//! Reader and writer for `2DA V2.0` tables.
//!
//! The representation is intentionally close to the human-edited text format: columns are named,
//! rows are ordered, and cells are optional strings. The crate also preserves the notion of a
//! table-wide default value.
//!
//! Use [`read_twoda`], [`write_twoda`], and [`TwoDa`] for most workflows.

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
