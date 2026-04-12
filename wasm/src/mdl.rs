use std::io::Cursor;

use nwnrs::prelude::mdl;
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsValue;

use crate::{
    bindings::js_error,
    lossless::{LosslessDtoMetadata, unchanged_lossless_bytes, with_lossless_metadata},
};

/// A wasm-facing MDL encoding discriminator.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MdlEncodingDto {
    /// Source-faithful ASCII MDL text.
    Ascii,
    /// Canonical ASCII text lowered from compiled MDL bytes.
    Compiled,
}

/// A wasm-facing MDL document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MdlDto {
    /// The original payload encoding.
    pub encoding: MdlEncodingDto,
    /// Canonical ASCII MDL text.
    pub text:     String,
    /// Internal provenance metadata for unchanged write-backs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lossless: Option<LosslessDtoMetadata>,
}

fn mdl_to_dto(bytes: &[u8]) -> Result<MdlDto, JsValue> {
    let parsed =
        mdl::parse_model_bytes(bytes).map_err(|error| js_error("failed to read MDL", error))?;
    match parsed {
        mdl::ParsedModel::Ascii(model) => Ok(MdlDto {
            encoding: MdlEncodingDto::Ascii,
            text:     model.to_text(),
            lossless: None,
        }),
        mdl::ParsedModel::Compiled(model) => Ok(MdlDto {
            encoding: MdlEncodingDto::Compiled,
            text:     mdl::lower_binary_model_to_ascii(&model)
                .map_err(|error| js_error("failed to lower compiled MDL", error))?
                .to_text(),
            lossless: None,
        }),
    }
}

pub(crate) fn read_mdl_dto(bytes: &[u8]) -> Result<MdlDto, JsValue> {
    with_lossless_metadata(
        mdl_to_dto(bytes)?,
        bytes.to_vec(),
        |dto| &mut dto.lossless,
        "failed to fingerprint MDL DTO",
    )
}

pub(crate) fn write_mdl_dto(value: &MdlDto) -> Result<Vec<u8>, JsValue> {
    if let Some(bytes) =
        unchanged_lossless_bytes(value, &value.lossless, "failed to fingerprint MDL DTO")?
    {
        return Ok(bytes);
    }

    match value.encoding {
        MdlEncodingDto::Ascii => {
            let model = mdl::parse_ascii_model(&value.text)
                .map_err(|error| js_error("failed to parse ASCII MDL", error))?;
            let mut out = Cursor::new(Vec::new());
            mdl::write_ascii_model(&mut out, &model)
                .map_err(|error| js_error("failed to write ASCII MDL", error))?;
            Ok(out.into_inner())
        }
        MdlEncodingDto::Compiled => Err(js_error(
            "failed to write compiled MDL",
            "edited compiled MDL writes are not supported yet; keep the DTO unchanged for exact \
             roundtrips or switch encoding to ascii",
        )),
    }
}

wasm_read_binding! {
    fn read_mdl_from_bytes(bytes: &[u8]) -> MdlDto {
        read_mdl_dto(bytes)
    }
    , serialize_context: "failed to serialize MDL",
}

wasm_write_binding! {
    fn write_mdl_to_bytes(value: JsValue) -> MdlDto
    , deserialize_context: "failed to deserialize MDL DTO",
    {
        write_mdl_dto(&value)
    }
}
