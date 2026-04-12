#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

mod io;
mod merge;
mod types;

pub use io::*;
pub use merge::*;
pub use types::*;

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::{
        GffCExoLocString, GffError, GffField, GffFieldKind, GffResult, GffRoot, GffStruct,
        GffValue, merge_root_preserving_provenance, new_c_exo_loc_string, new_gff_root,
        new_gff_struct, read_gff_root, write_gff_root,
    };
}
