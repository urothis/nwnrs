#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

use nwnrs_key::read_key_table;
use nwnrs_resman::shared_stream;

mod builder;
mod discovery;
mod keyload;
mod types;

pub use builder::*;
pub use discovery::*;
pub use types::*;

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::{
        DEFAULT_KEYFILES, GFF_EXTENSIONS, InstallError, InstallResult, find_nwnrs_root,
        find_user_root, new_default_resman, resolve_language_root,
    };
}
