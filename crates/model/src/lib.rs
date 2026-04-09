#![forbid(unsafe_code)]
//! Reader and writer helpers for Neverwinter Nights model (`MDL`) payloads.
//!
//! This crate currently treats models as raw byte payloads while providing a
//! first-class API surface for resource-manager integration and UTF-8 text
//! access when the caller wants to inspect source-style models.

mod io;
mod types;

pub use io::*;
pub use types::*;

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::{
        MODEL_RES_TYPE, Model, ModelError, ModelResult, read_model, read_model_from_file,
        read_model_from_res, write_model,
    };
}
