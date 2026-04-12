#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

mod resolve;
mod types;

pub use resolve::*;
pub use types::*;

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::{BAD_STRREF, Gender, Language, ParseLanguageError, StrRef, resolve_language};
}
