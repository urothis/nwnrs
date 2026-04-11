#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

mod io;
mod types;

pub use io::*;
pub use types::*;

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::{
        ManifestSha1, NWSYNC_COMPRESSED_BUF_MAGIC_STR, NWSync, ResNWSyncError, ResNWSyncManifest,
        ResNWSyncResult, ResRefSha1, new_resnwsync_manifest, nwsync_compressed_buf_magic,
        open_nwsync, open_or_create_nwsync,
    };
}
