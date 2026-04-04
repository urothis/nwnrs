#![forbid(unsafe_code)]
//! Reader and writer for ERF-family archives.
//!
//! NWN uses the same broad archive structure for `ERF`, `MOD`, `HAK`, and `NWM`
//! files. This crate parses those containers into an [`Erf`] that also
//! implements [`nwnrs_resman::ResContainer`], so archive entries can participate
//! directly in layered resource resolution.
//!
//! Start with [`read_erf`], [`read_erf_from_file`], [`read_erf_shared`], and
//! [`write_erf`].

mod io;
mod types;

pub use io::*;
pub use types::*;

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::{
        Erf, ErfError, ErfResult, ErfVersion, read_erf, read_erf_from_file, read_erf_shared,
        write_erf,
    };
}
