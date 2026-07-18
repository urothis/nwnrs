//! Shared test helpers for NWScript integration tests.

use std::{error::Error, io};

use nwnrs_nwscript::prelude::{NW_SCRIPT_BINARY_RES_TYPE, NW_SCRIPT_SOURCE_RES_TYPE};
use nwnrs_types::test_support::{
    read_resource_bytes, require_game_resource,
    skip_if_game_resources_unavailable as skip_if_unavailable,
};

/// Builds one test-friendly `io::Error`.
pub fn test_error(message: impl Into<String>) -> io::Error {
    io::Error::other(message.into())
}

/// Loads one NWScript source file from the installed game resources.
pub fn load_nss_bytes(path: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    let trimmed = path.trim();
    let resref = trimmed.strip_suffix(".nss").unwrap_or(trimmed);
    require_game_resource(read_resource_bytes(resref, NW_SCRIPT_SOURCE_RES_TYPE))
}

/// Loads one compiled NWScript file from the installed game resources.
#[allow(dead_code)]
pub fn load_ncs_bytes(path: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    let trimmed = path.trim();
    let resref = trimmed.strip_suffix(".ncs").unwrap_or(trimmed);
    require_game_resource(read_resource_bytes(resref, NW_SCRIPT_BINARY_RES_TYPE))
}

/// Preserves install-backed resource failures in NWScript integration tests.
pub fn skip_if_game_resources_unavailable(error: Box<dyn Error>) -> Result<(), Box<dyn Error>> {
    skip_if_unavailable(error)
}
