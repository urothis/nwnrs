#![forbid(unsafe_code)]
//! NWN resource-reference parsing and formatting.
//!
//! A resource reference combines a case-insensitive name with a numeric
//! resource type. This crate validates those values, resolves known file
//! extensions through [`nwnrs_restype`], and provides helpers for converting
//! between `name.ext` filenames and typed resource references.

mod parse;
mod types;

pub use parse::*;
pub use types::*;

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::{
        RESREF_MAX_LENGTH, ResRef, ResRefError, ResolvedResRef, is_valid_resref_part1, new_res_ref,
        new_resolved_res_ref, new_resolved_res_ref_from_filename, try_new_resolved_res_ref,
    };
}
