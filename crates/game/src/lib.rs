#![forbid(unsafe_code)]
//! High-level helpers for working against a real NWN installation.
//!
//! This crate sits above the low-level container and format crates. It knows how to locate
//! the game root and user directory, choose the conventional KEY load order, include override
//! directories and NWSync manifests, and return a ready-to-query [`ResMan`](nwn_resman::ResMan).
//!
//! The primary entry points are [`find_nwn_root`], [`find_user_root`], and
//! [`new_default_resman`].

use nwn_erf::read_erf_from_file;
use nwn_key::read_key_table;
use nwn_resdir::read_resdir;
use nwn_resman::shared_stream;

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
        DEFAULT_KEYFILES, GFF_EXTENSIONS, GameError, GameResult, find_nwn_root, find_user_root,
        new_default_resman,
    };
}

use crate::keyload::load_key;
