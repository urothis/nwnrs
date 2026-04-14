use std::io::Cursor;

use nwnrs::prelude::{localization::Language, tlk};
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsValue;

use crate::{
    bindings::{js_error, js_error_message},
    lossless::{LosslessDtoMetadata, unchanged_lossless_bytes, with_lossless_metadata},
};

/// A wasm-facing TLK entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TlkEntryDto {
    /// The localized text.
    pub text:              String,
    /// Original encoded text bytes when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_text:          Option<Vec<u8>>,
    /// The associated sound resource reference.
    pub sound_res_ref:     String,
    /// Raw 16-byte sound resource slot.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub raw_sound_res_ref: Vec<u8>,
    /// The sound length in seconds.
    pub sound_length:      f32,
    /// Raw IEEE-754 bits for the stored sound length field.
    #[serde(default)]
    pub sound_length_bits: u32,
    /// Raw TLK entry flags.
    #[serde(default)]
    pub flags:             i32,
    /// Stored volume variance field.
    #[serde(default)]
    pub volume_variance:   i32,
    /// Stored pitch variance field.
    #[serde(default)]
    pub pitch_variance:    i32,
}

fn dto_to_tlk(value: &SingleTlkDto) -> Result<tlk::SingleTlk, JsValue> {
    let mut tlk = tlk::SingleTlk::new();
    tlk.language = Language::from_id(value.language_id)
        .ok_or_else(|| JsValue::from_str("invalid TLK language id"))?;
    for (str_ref, entry) in value.entries.iter().enumerate() {
        if let Some(entry) = entry {
            let raw_sound_res_ref = if entry.raw_sound_res_ref.is_empty() {
                tlk::TlkEntry::new(&entry.text, &entry.sound_res_ref, entry.sound_length)
                    .raw_sound_res_ref
            } else {
                <[u8; 16]>::try_from(entry.raw_sound_res_ref.clone()).map_err(|error| {
                    JsValue::from_str(&format!(
                        "TLK raw_sound_res_ref must contain exactly 16 bytes: {error:?}"
                    ))
                })?
            };
            let mut next =
                tlk::TlkEntry::new(&entry.text, &entry.sound_res_ref, entry.sound_length);
            next.raw_text.clone_from(&entry.raw_text);
            next.raw_sound_res_ref = raw_sound_res_ref;
            next.sound_length_bits = if entry.sound_length_bits == 0 {
                entry.sound_length.to_bits()
            } else {
                entry.sound_length_bits
            };
            next.flags = entry.flags;
            next.volume_variance = entry.volume_variance;
            next.pitch_variance = entry.pitch_variance;
            tlk.set_entry(
                u32::try_from(str_ref)
                    .map_err(|error| js_error("invalid TLK strref index", error))?,
                next,
            );
        }
    }
    Ok(tlk)
}

/// A wasm-facing single-language TLK table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SingleTlkDto {
    /// The NWN language id.
    pub language_id: u32,
    /// Sparse TLK entries by strref position.
    pub entries:     Vec<Option<TlkEntryDto>>,
    /// Internal provenance metadata for unchanged write-backs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lossless:    Option<LosslessDtoMetadata>,
}

fn tlk_to_dto(value: &mut tlk::SingleTlk) -> Result<SingleTlkDto, JsValue> {
    let highest = value.highest();
    let entries = if highest < 0 {
        Vec::new()
    } else {
        (0..=u32::try_from(highest)
            .map_err(|error| js_error("invalid TLK highest entry", error))?)
            .map(|str_ref| {
                value
                    .get(str_ref)
                    .map_err(|error| js_error("failed to read TLK entry", error))
                    .map(|entry| {
                        entry.map(|entry| TlkEntryDto {
                            text:              entry.text,
                            raw_text:          entry.raw_text,
                            sound_res_ref:     entry.sound_res_ref,
                            raw_sound_res_ref: entry.raw_sound_res_ref.to_vec(),
                            sound_length:      entry.sound_length,
                            sound_length_bits: entry.sound_length_bits,
                            flags:             entry.flags,
                            volume_variance:   entry.volume_variance,
                            pitch_variance:    entry.pitch_variance,
                        })
                    })
            })
            .collect::<Result<Vec<_>, _>>()?
    };

    Ok(SingleTlkDto {
        language_id: value.language.id(),
        entries,
        lossless: None,
    })
}

pub(crate) fn read_tlk_dto(bytes: &[u8]) -> Result<SingleTlkDto, JsValue> {
    let mut value = tlk::read_single_tlk(
        Cursor::new(bytes.to_vec()),
        nwnrs::resman::CachePolicy::Bypass,
    )
    .map_err(|error| js_error("failed to read TLK", error))?;
    with_lossless_metadata(
        tlk_to_dto(&mut value)?,
        bytes.to_vec(),
        |dto| &mut dto.lossless,
        "failed to fingerprint TLK DTO",
    )
    .map_err(|error| js_error_message(&error))
}

pub(crate) fn write_tlk_dto(value: &SingleTlkDto) -> Result<Vec<u8>, JsValue> {
    if let Some(bytes) = unchanged_lossless_bytes(
        value,
        &value.lossless,
        |dto| &mut dto.lossless,
        "failed to fingerprint TLK DTO",
    )
    .map_err(|error| js_error_message(&error))?
    {
        Ok(bytes)
    } else {
        let mut tlk = dto_to_tlk(value)?;
        let mut out = Cursor::new(Vec::new());
        tlk::write_single_tlk(&mut out, &mut tlk)
            .map_err(|error| js_error("failed to write TLK", error))?;
        Ok(out.into_inner())
    }
}

wasm_read_binding! {
    fn read_tlk_from_bytes(bytes: &[u8]) -> SingleTlkDto {
        read_tlk_dto(bytes)
    }
    , serialize_context: "failed to serialize TLK",
}

wasm_write_binding! {
    fn write_tlk_to_bytes(value: JsValue) -> SingleTlkDto
    , deserialize_context: "failed to deserialize TLK DTO",
    {
        write_tlk_dto(&value)
    }
}
