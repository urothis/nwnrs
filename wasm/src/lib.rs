#![forbid(unsafe_code)]
//! WebAssembly bindings for NWN1EE types and utilities.
//! This crate re-exports the public API of the `nwnrs-prelude` crate with
//! WebAssembly bindings enabled. It is intended for use in browser-based
//! applications that need to interact with NWN1EE data and services.

use nwnrs_prelude::prelude::*;
use wasm_bindgen::{JsValue, prelude::wasm_bindgen};

/// Reads an `SSF` document from a byte array and returns it as a JavaScript
/// value.
#[wasm_bindgen]
pub fn read_ssf_from_bytes(bytes: &[u8]) -> Result<JsValue, JsValue> {
    let mut cursor = std::io::Cursor::new(bytes);
    ssf::read_ssf(&mut cursor)
        .map_err(|err| JsValue::from_str(&format!("failed to read SSF: {err}")))
        .and_then(|ssf| {
            serde_wasm_bindgen::to_value(&ssf)
                .map_err(|err| JsValue::from_str(&format!("failed to serialize SSF: {err}")))
        })
}
