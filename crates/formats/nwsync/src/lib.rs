#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

mod io;
mod types;

pub use io::*;
pub use types::*;

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::{
        HASH_TREE_DEPTH, MAGIC, Manifest, ManifestEntry, ManifestEntrySource, ManifestError,
        ManifestResult, VERSION, path_for_entry, read_manifest, read_manifest_file, write_manifest,
        write_manifest_file,
    };
}
