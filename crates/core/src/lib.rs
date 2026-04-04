#![forbid(unsafe_code)]
//! Core NWN vocabulary shared across format crates.
//!
//! This crate intentionally stays small. It defines the language ids, gender
//! selector, and dialog string reference type that appear across TLK, GFF, SSF,
//! and higher-level resource loading code.
//!
//! Use [`Language`] and [`resolve_language`] when you need to translate between
//! textual and numeric language identifiers.

mod resolve;
mod types;

pub use resolve::*;
pub use types::*;

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::{BAD_STRREF, Gender, Language, ParseLanguageError, StrRef, resolve_language};
}
