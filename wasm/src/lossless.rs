use nwnrs::prelude::checksums;
use serde::{Deserialize, Serialize};
use serde_json::to_vec as to_json_bytes;
use wasm_bindgen::JsValue;

use crate::bindings::js_error;

/// Hidden provenance metadata used to preserve exact original bytes for
/// untouched DTO values.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LosslessDtoMetadata {
    /// The original source bytes returned by the reader.
    pub original_bytes:       Vec<u8>,
    /// A stable semantic fingerprint of the DTO with this metadata removed.
    pub semantic_fingerprint: String,
}

pub(crate) fn semantic_fingerprint<T: Serialize>(
    value: &T,
    context: &str,
) -> Result<String, JsValue> {
    let json = to_json_bytes(value).map_err(|error| js_error(context, error))?;
    Ok(checksums::secure_hash(&json).to_string())
}

pub(crate) fn with_lossless_metadata<T>(
    mut value: T,
    original_bytes: Vec<u8>,
    metadata_slot: fn(&mut T) -> &mut Option<LosslessDtoMetadata>,
    context: &str,
) -> Result<T, JsValue>
where
    T: Serialize,
{
    *metadata_slot(&mut value) = None;
    let semantic_fingerprint = semantic_fingerprint(&value, context)?;
    *metadata_slot(&mut value) = Some(LosslessDtoMetadata {
        original_bytes,
        semantic_fingerprint,
    });
    Ok(value)
}

pub(crate) fn unchanged_lossless_bytes<T: Serialize>(
    value: &T,
    lossless: &Option<LosslessDtoMetadata>,
    context: &str,
) -> Result<Option<Vec<u8>>, JsValue> {
    let Some(lossless) = lossless else {
        return Ok(None);
    };
    if semantic_fingerprint(value, context)? == lossless.semantic_fingerprint {
        return Ok(Some(lossless.original_bytes.clone()));
    }
    Ok(None)
}
