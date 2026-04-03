#![forbid(unsafe_code)]
//! Registry of NWN resource types and file extensions.
//!
//! NWN stores resource kinds as numeric ids, while most user-facing workflows
//! operate on file extensions. This crate bridges the two and also allows
//! callers to register additional custom mappings when working with
//! project-specific resource types.

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
