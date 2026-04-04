#![forbid(unsafe_code)]
//! High-level helpers for working against a real NWN installation.
//!
//! This crate sits above the low-level container and format crates. It knows
//! how to locate the game root and user directory, choose the conventional KEY
//! load order, include override directories and NWSync manifests, and return a
//! ready-to-query [`ResMan`](nwnrs_resman::ResMan).
//!
//! The primary entry points are [`find_nwnrs_root`], [`find_user_root`], and
//! [`new_default_resman`].

use nwnrs_erf::read_erf_from_file;
use nwnrs_key::read_key_table;
use nwnrs_resdir::read_resdir;
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
        DEFAULT_KEYFILES, GFF_EXTENSIONS, GameError, GameResult, find_nwnrs_root, find_user_root,
        new_default_resman,
    };
}
