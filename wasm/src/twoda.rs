use std::io::Cursor;

use nwnrs::prelude::twoda;
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsValue;

use crate::{
    bindings::{js_error, js_error_message},
    lossless::{LosslessDtoMetadata, unchanged_lossless_bytes, with_lossless_metadata},
};

/// A wasm-facing `2DA` document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TwoDaDto {
    /// The table-wide default cell value.
    pub default_value: Option<String>,
    /// Ordered column names.
    pub columns:       Vec<String>,
    /// Ordered table rows.
    pub rows:          Vec<Vec<Option<String>>>,
    /// Stored row labels.
    pub row_labels:    Vec<String>,
    /// Internal provenance metadata for unchanged write-backs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lossless:      Option<LosslessDtoMetadata>,
}

pub(crate) fn unchanged_twoda_bytes(value: &TwoDaDto) -> Result<Option<Vec<u8>>, JsValue> {
    let mut semantic = value.clone();
    semantic.lossless = None;
    unchanged_lossless_bytes(
        &semantic,
        &value.lossless,
        |dto| &mut dto.lossless,
        "failed to fingerprint 2DA DTO",
    )
    .map_err(|error| js_error_message(&error))
}

fn twoda_to_dto(value: &twoda::TwoDa) -> TwoDaDto {
    TwoDaDto {
        default_value: value.default(),
        columns:       value.columns().to_vec(),
        rows:          value.rows.clone(),
        row_labels:    (0..value.rows.len())
            .map(|idx| value.row_label(idx).unwrap_or_default().to_string())
            .collect(),
        lossless:      None,
    }
}

fn dto_to_twoda(value: &TwoDaDto) -> Result<twoda::TwoDa, JsValue> {
    let mut twoda = if let Some(lossless) = &value.lossless {
        twoda::read_twoda(Cursor::new(lossless.original_bytes.clone()))
            .map_err(|error| js_error("failed to read original 2DA bytes", error))?
    } else {
        twoda::TwoDa::new()
    };
    twoda.set_default(value.default_value.clone());
    twoda
        .set_columns(value.columns.clone())
        .map_err(|error| js_error("failed to update 2DA columns", error))?;
    twoda
        .replace_rows(value.rows.clone(), value.row_labels.clone())
        .map_err(|error| js_error("failed to update 2DA rows", error))?;
    Ok(twoda)
}

pub(crate) fn read_twoda_dto(bytes: &[u8]) -> Result<TwoDaDto, JsValue> {
    let value = twoda::read_twoda(Cursor::new(bytes))
        .map_err(|error| js_error("failed to read 2DA", error))?;
    with_lossless_metadata(
        twoda_to_dto(&value),
        bytes.to_vec(),
        |dto| &mut dto.lossless,
        "failed to fingerprint 2DA DTO",
    )
    .map_err(|error| js_error_message(&error))
}

pub(crate) fn write_twoda_dto(value: &TwoDaDto) -> Result<Vec<u8>, JsValue> {
    if let Some(bytes) = unchanged_twoda_bytes(value)? {
        Ok(bytes)
    } else {
        let twoda = dto_to_twoda(value)?;
        let mut out = Vec::new();
        twoda::write_twoda(&mut out, &twoda, false)
            .map_err(|error| js_error("failed to write 2DA", error))?;
        Ok(out)
    }
}

wasm_read_binding! {
    fn read_twoda_from_bytes(bytes: &[u8]) -> TwoDaDto {
        read_twoda_dto(bytes)
    }
    , serialize_context: "failed to serialize 2DA",
}

wasm_write_binding! {
    fn write_twoda_to_bytes(value: JsValue) -> TwoDaDto
    , deserialize_context: "failed to deserialize 2DA DTO",
    {
        write_twoda_dto(&value)
    }
}
