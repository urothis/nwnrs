#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

mod registry;
mod types;

pub use registry::*;
pub use types::*;

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::{
        RegisterResTypeError, ResType, get_res_ext, get_res_type, lookup_res_ext, lookup_res_type,
        register_custom_res_type, res_ext_registered, res_type_registered,
    };
}
