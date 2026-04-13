use nwnrs::prelude::checksums;
use serde::{Deserialize, Serialize};
use serde_json::to_vec as to_json_bytes;

use crate::bindings::error_message;

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
) -> Result<String, String> {
    let json = to_json_bytes(value).map_err(|error| error_message(context, error))?;
    Ok(checksums::secure_hash(&json).to_string())
}

pub(crate) fn with_lossless_metadata<T>(
    mut value: T,
    original_bytes: Vec<u8>,
    metadata_slot: fn(&mut T) -> &mut Option<LosslessDtoMetadata>,
    context: &str,
) -> Result<T, String>
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

pub(crate) fn unchanged_lossless_bytes<T>(
    value: &T,
    lossless: &Option<LosslessDtoMetadata>,
    metadata_slot: fn(&mut T) -> &mut Option<LosslessDtoMetadata>,
    context: &str,
) -> Result<Option<Vec<u8>>, String>
where
    T: Clone + Serialize,
{
    let Some(lossless) = lossless else {
        return Ok(None);
    };
    let mut semantic = value.clone();
    *metadata_slot(&mut semantic) = None;
    if semantic_fingerprint(&semantic, context)? == lossless.semantic_fingerprint {
        return Ok(Some(lossless.original_bytes.clone()));
    }
    Ok(None)
}
