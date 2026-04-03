#![forbid(unsafe_code)]
//! Access to NWSync repositories as resource containers.
//!
//! This crate opens the SQLite-backed NWSync repository layout, maps manifest hashes to shard
//! payloads, and exposes individual manifests as [`nwn_resman::ResContainer`] values.
//!
//! Use [`open_nwsync`] to open a repository and [`new_resnwsync_manifest`] to materialize a
//! specific manifest as a container.

mod io;
mod types;

pub use io::*;
pub use types::*;

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::{
        ManifestSha1, NWSYNC_COMPRESSED_BUF_MAGIC_STR, NWSync, ResNWSyncError, ResNWSyncManifest,
        ResNWSyncResult, ResRefSha1, new_resnwsync_manifest, nwsync_compressed_buf_magic,
        open_nwsync,
    };
}
