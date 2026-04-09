#![forbid(unsafe_code)]
//! Reader and writer for NWSync manifest files.
//!
//! NWSync manifests map resource references to payload hashes and sizes. This
//! crate handles the standalone manifest file format itself; repository access
//! and shard lookup live in [`nwnrs_resnwsync`].
//!
//! Start with [`read_manifest`], [`read_manifest_file`], [`write_manifest`],
//! and [`write_manifest_file`].

mod io;
mod types;

pub use io::*;
pub use types::*;

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::{
        HASH_TREE_DEPTH, MAGIC, Manifest, ManifestEntry, ManifestEntrySource, ManifestError,
        ManifestResult, VERSION, path_for_entry, read_manifest, read_manifest_file,
        write_manifest, write_manifest_file,
    };
}
