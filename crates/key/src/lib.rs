#![forbid(unsafe_code)]
//! Reader and writer for KEY/BIF resource sets.
//!
//! KEY files index one or more BIF files and provide the canonical base-game
//! lookup table used by NWN installations. This crate opens KEY files, lazily
//! resolves the referenced BIFs, and exposes the aggregate result as a
//! [`KeyTable`] implementing [`nwnrs_resman::ResContainer`].
//!
//! The main entry points are [`read_key_table`], [`read_key_table_from_file`],
//! and [`write_key_and_bif`].

mod io;
mod types;

pub use io::*;
pub use types::*;

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::{
        BifResolver, KeyBifContents, KeyBifEntry, KeyBifVersion, KeyError, KeyResult, KeyTable,
        ResId, VariableResource, read_key_table, read_key_table_from_file, write_key_and_bif,
    };
}
