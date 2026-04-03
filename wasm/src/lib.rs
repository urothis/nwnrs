#![forbid(unsafe_code)]
//! WebAssembly bindings for NWN1EE types and utilities.
//! This crate re-exports the public API of the `nwn-prelude` crate with WebAssembly bindings enabled. It is intended for use in browser-based applications that need to interact with NWN1EE data and services.

/// Re-export the public API of the `nwn-prelude` crate.
pub use nwn_prelude::prelude::*;
