#![forbid(unsafe_code)]
#![doc = include_str!("README.md")]

mod manifest;
mod repository;

pub use manifest::*;
pub use repository::*;

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::nwsync::{
        HASH_TREE_DEPTH, MAGIC, Manifest, ManifestEntry, ManifestEntrySource, ManifestError,
        ManifestResult, ManifestSha1, NWSYNC_COMPRESSED_BUF_MAGIC_STR, NWSync, ResNWSyncError,
        ResNWSyncManifest, ResNWSyncResult, ResRefSha1, VERSION, new_resnwsync_manifest,
        nwsync_compressed_buf_magic, open_nwsync, open_or_create_nwsync, path_for_entry,
        read_manifest, read_manifest_file, write_manifest, write_manifest_file,
    };
}
