#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

mod io;
mod types;

pub use io::*;
pub use types::*;

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::{
        BifResolver, KeyBifContents, KeyBifEntry, KeyBifVersion, KeyError, KeyResult, KeyTable,
        ResId, VariableResource, read_key_table, read_key_table_from_file, write_key_and_bif,
        write_key_table_archive,
    };
}
