#![forbid(unsafe_code)]
//! Reader, writer, and query helpers for dialog table (`TLK`) files.
//!
//! The TLK format stores localized string entries keyed by [`nwn_core::StrRef`]. This crate
//! supports standalone male/female tables, overlay chains, lazy entry reads, and optional LRU
//! caching for stream-backed access.
//!
//! Start with [`read_single_tlk`], [`write_single_tlk`], [`SingleTlk`], and [`Tlk`].

mod io;
mod types;

pub use io::*;
pub use types::*;

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::{
        DATA_ELEMENT_SIZE, HEADER_SIZE, SingleTlk, Tlk, TlkEntry, TlkError, TlkPair, TlkResult,
        read_single_tlk, read_single_tlk_from_res, write_single_tlk,
    };
}
