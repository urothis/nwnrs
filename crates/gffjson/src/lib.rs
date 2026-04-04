#![forbid(unsafe_code)]
//! JSON bridge for the [`nwnrs_gff`] data model.
//!
//! This crate defines a stable JSON representation for [`nwnrs_gff::GffRoot`]
//! documents. It is primarily used by the CLI unpack/pack workflow so that
//! GFF-family resources can be edited as human-readable JSON and rebuilt
//! without losing type information.
//!
//! Start with [`gff_root_to_json_value`], [`gff_root_to_pretty_json_string`],
//! [`gff_root_from_json_value`], and [`gff_root_from_json_str`].
mod decode;
mod encode;
mod types;

pub use decode::*;
pub use encode::*;
pub use types::*;

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::{
        GffJsonError, GffJsonResult, gff_root_from_json_str, gff_root_from_json_value,
        gff_root_to_json_string, gff_root_to_json_value, gff_root_to_pretty_json_string,
        gff_struct_from_json_value, gff_struct_to_json_value,
    };
}
