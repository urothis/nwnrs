use nwnrs_core::prelude::*;
use serde::{Deserialize, Serialize};

pub(crate) const HEADER_MAGIC: &str = "SSF ";
pub(crate) const HEADER_VERSION: &str = "V1.0";
pub(crate) const TABLE_OFFSET: u32 = 40;
pub(crate) const ENTRY_DATA_SIZE: usize = 20;

/// A single soundset slot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SsfEntry {
    /// The sound resource reference stored for the slot.
    pub resref: String,
    /// The localized string reference associated with the slot.
    pub strref: StrRef,
}

/// The decoded contents of an `SSF` file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SsfRoot {
    /// The ordered soundset entries in the file.
    pub entries: Vec<SsfEntry>,
}

/// Creates an empty `SSF` document.
pub fn new_ssf() -> SsfRoot {
    SsfRoot::default()
}
