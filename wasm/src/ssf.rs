use std::io::Cursor;

use nwnrs::prelude::ssf;
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsValue;

use crate::{
    bindings::{js_error, js_error_message},
    lossless::{LosslessDtoMetadata, unchanged_lossless_bytes, with_lossless_metadata},
};

/// A wasm-facing SSF entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SsfEntryDto {
    /// The raw 16-byte resref slot.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub raw_resref: Vec<u8>,
    /// The sound resource reference.
    pub resref:     String,
    /// The associated TLK string reference.
    pub strref:     u32,
}

fn dto_to_ssf(value: &SsfRootDto) -> Result<ssf::SsfRoot, JsValue> {
    let mut ssf_value = ssf::SsfRoot::new();
    ssf_value.entries = value
        .entries
        .iter()
        .map(|entry| {
            let mut native = ssf::SsfEntry::new(&entry.resref, entry.strref);
            if !entry.raw_resref.is_empty() {
                native.raw_resref =
                    <[u8; 16]>::try_from(entry.raw_resref.clone()).map_err(|error| {
                        JsValue::from_str(&format!(
                            "SSF raw_resref must contain exactly 16 bytes: {error:?}"
                        ))
                    })?;
            }
            Ok(native)
        })
        .collect::<Result<Vec<_>, JsValue>>()?;
    Ok(ssf_value)
}

/// A wasm-facing SSF document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SsfRootDto {
    /// Ordered soundset entries.
    pub entries:  Vec<SsfEntryDto>,
    /// Internal provenance metadata for unchanged write-backs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lossless: Option<LosslessDtoMetadata>,
}

fn ssf_to_dto(value: &ssf::SsfRoot) -> SsfRootDto {
    SsfRootDto {
        entries:  value
            .entries
            .iter()
            .map(|entry| SsfEntryDto {
                raw_resref: entry.raw_resref.to_vec(),
                resref:     entry.resref.clone(),
                strref:     entry.strref,
            })
            .collect(),
        lossless: None,
    }
}

pub(crate) fn read_ssf_dto(bytes: &[u8]) -> Result<SsfRootDto, JsValue> {
    let mut cursor = Cursor::new(bytes);
    let value =
        ssf::read_ssf(&mut cursor).map_err(|error| js_error("failed to read SSF", error))?;
    with_lossless_metadata(
        ssf_to_dto(&value),
        bytes.to_vec(),
        |dto| &mut dto.lossless,
        "failed to fingerprint SSF DTO",
    )
    .map_err(|error| js_error_message(&error))
}

pub(crate) fn write_ssf_dto(value: &SsfRootDto) -> Result<Vec<u8>, JsValue> {
    if let Some(bytes) =
        unchanged_lossless_bytes(value, &value.lossless, "failed to fingerprint SSF DTO")
            .map_err(|error| js_error_message(&error))?
    {
        Ok(bytes)
    } else {
        let ssf_value = dto_to_ssf(value)?;
        let mut out = Vec::new();
        ssf::write_ssf(&mut out, &ssf_value)
            .map_err(|error| js_error("failed to write SSF", error))?;
        Ok(out)
    }
}

wasm_read_binding! {
    fn read_ssf_from_bytes(bytes: &[u8]) -> SsfRootDto {
        read_ssf_dto(bytes)
    }
    , serialize_context: "failed to serialize SSF",
}

wasm_write_binding! {
    fn write_ssf_to_bytes(value: JsValue) -> SsfRootDto
    , deserialize_context: "failed to deserialize SSF DTO",
    {
        write_ssf_dto(&value)
    }
}
