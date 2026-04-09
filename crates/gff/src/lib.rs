#![forbid(unsafe_code)]
//! Typed reader and writer for `GFF V3.2`.
//!
//! Generic File Format documents back a large share of NWN gameplay data. This
//! crate models a document as a [`GffRoot`] containing nested [`GffStruct`]
//! values and typed [`GffValue`] fields while preserving field order for
//! round-tripping.
//!
//! Use [`read_gff_root`] and [`write_gff_root`] for binary IO, and construct
//! new documents with [`GffRoot::new`] or the convenience helpers re-exported
//! from this crate.

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
