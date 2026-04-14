use std::io::Cursor;

use nwnrs::{
    prelude::{compressedbuf, erf},
    resman::CachePolicy,
};
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsValue;

use crate::{
    bindings::{js_error, js_error_message},
    lossless::{LosslessDtoMetadata, unchanged_lossless_bytes, with_lossless_metadata},
};

/// A wasm-facing ERF archive version.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum ErfVersionDto {
    /// Legacy archive layout.
    V1,
    /// Enhanced-edition archive layout.
    E1,
}

impl From<erf::ErfVersion> for ErfVersionDto {
    fn from(value: erf::ErfVersion) -> Self {
        match value {
            erf::ErfVersion::V1 => Self::V1,
            erf::ErfVersion::E1 => Self::E1,
        }
    }
}

impl From<ErfVersionDto> for erf::ErfVersion {
    fn from(value: ErfVersionDto) -> Self {
        match value {
            ErfVersionDto::V1 => Self::V1,
            ErfVersionDto::E1 => Self::E1,
        }
    }
}

/// A wasm-facing compressed-buffer algorithm.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CompressedBufAlgorithmDto {
    /// No compressed-buffer wrapper.
    None,
    /// Zlib-compressed payload.
    Zlib,
    /// Zstd-compressed payload.
    Zstd,
}

impl From<compressedbuf::Algorithm> for CompressedBufAlgorithmDto {
    fn from(value: compressedbuf::Algorithm) -> Self {
        match value {
            compressedbuf::Algorithm::None => Self::None,
            compressedbuf::Algorithm::Zlib => Self::Zlib,
            compressedbuf::Algorithm::Zstd => Self::Zstd,
        }
    }
}

impl From<CompressedBufAlgorithmDto> for compressedbuf::Algorithm {
    fn from(value: CompressedBufAlgorithmDto) -> Self {
        match value {
            CompressedBufAlgorithmDto::None => Self::None,
            CompressedBufAlgorithmDto::Zlib => Self::Zlib,
            CompressedBufAlgorithmDto::Zstd => Self::Zstd,
        }
    }
}

/// A localized ERF header string.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErfLocStringDto {
    /// The localized string id.
    pub id:   i32,
    /// The localized text.
    pub text: String,
}

fn dto_to_erf_bytes(value: &ErfDto) -> Result<Vec<u8>, JsValue> {
    let mut loc_strings = std::collections::BTreeMap::new();
    for entry in &value.loc_strings {
        loc_strings.insert(entry.id, entry.text.clone());
    }
    let entries = value
        .entries
        .iter()
        .map(|entry| {
            nwnrs::prelude::resref::ResolvedResRef::from_filename(&entry.filename)
                .map(Into::into)
                .map_err(|error| js_error("invalid ERF entry filename", error))
        })
        .collect::<Result<Vec<nwnrs::prelude::resref::ResRef>, JsValue>>()?;
    let algorithms = value
        .entries
        .iter()
        .map(|entry| {
            entry
                .compressed_buf_algorithm
                .unwrap_or(CompressedBufAlgorithmDto::None)
                .into()
        })
        .collect::<Vec<_>>();
    let exocomp = if algorithms
        .iter()
        .any(|algorithm| *algorithm != compressedbuf::Algorithm::None)
    {
        nwnrs::prelude::exo::ExoResFileCompressionType::CompressedBuf
    } else {
        nwnrs::prelude::exo::ExoResFileCompressionType::None
    };
    let mut out = Cursor::new(Vec::new());
    erf::write_erf_with_options(
        &mut out,
        &value.file_type,
        value.file_version.into(),
        u32::try_from(value.build_year).unwrap_or(0),
        u32::try_from(value.build_day).unwrap_or(0),
        exocomp,
        compressedbuf::Algorithm::None,
        &loc_strings,
        value.str_ref,
        &entries,
        value.oid.as_deref(),
        erf::ErfWriteOptions {
            resource_list_padding: value.resource_list_padding,
        },
        |rr, io| {
            let idx = entries
                .iter()
                .position(|entry| entry == rr)
                .ok_or_else(|| std::io::Error::other(format!("missing ERF entry {rr}")))?;
            let bytes = value
                .entries
                .get(idx)
                .map(|entry| entry.bytes.as_slice())
                .ok_or_else(|| std::io::Error::other(format!("missing ERF DTO bytes for {rr}")))?;
            io.write_all(bytes)?;
            Ok((bytes.len(), nwnrs::prelude::checksums::secure_hash(bytes)))
        },
        |rr| {
            entries
                .iter()
                .position(|entry| entry == rr)
                .and_then(|idx| algorithms.get(idx).copied())
                .unwrap_or(compressedbuf::Algorithm::None)
        },
    )
    .map_err(|error| js_error("failed to write ERF", error))?;
    Ok(out.into_inner())
}

/// A wasm-facing ERF archive entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErfEntryDto {
    /// The `name.ext` filename for the entry.
    pub filename:                 String,
    /// The raw entry payload bytes.
    pub bytes:                    Vec<u8>,
    /// The compressed-buffer algorithm when present.
    pub compressed_buf_algorithm: Option<CompressedBufAlgorithmDto>,
}

/// A wasm-facing ERF-family archive.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErfDto {
    /// The four-byte archive type tag.
    pub file_type:             String,
    /// The archive version string.
    pub file_version:          ErfVersionDto,
    /// The stored build year.
    pub build_year:            i32,
    /// The stored build day.
    pub build_day:             i32,
    /// The archive string reference.
    pub str_ref:               i32,
    /// The enhanced-edition OID when present.
    pub oid:                   Option<String>,
    /// Preserved padding between the key list and resource list.
    #[serde(default)]
    pub resource_list_padding: u64,
    /// Localized strings stored in the archive header.
    pub loc_strings:           Vec<ErfLocStringDto>,
    /// Archive entries in order.
    pub entries:               Vec<ErfEntryDto>,
    /// Internal provenance metadata for unchanged write-backs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lossless:              Option<LosslessDtoMetadata>,
}

fn erf_to_dto(value: &erf::Erf) -> Result<ErfDto, JsValue> {
    let entries = value
        .entries()
        .iter()
        .map(|(rr, res)| {
            Ok(ErfEntryDto {
                filename:                 rr.to_string(),
                bytes:                    res
                    .read_all(CachePolicy::Bypass)
                    .map_err(|error| js_error("failed to read ERF entry bytes", error))?,
                compressed_buf_algorithm: res.compressed_buf_algorithm().map(Into::into),
            })
        })
        .collect::<Result<Vec<_>, JsValue>>()?;

    Ok(ErfDto {
        file_type: value.file_type.clone(),
        file_version: value.file_version.into(),
        build_year: value.build_year,
        build_day: value.build_day,
        str_ref: value.str_ref,
        oid: value.oid().map(str::to_string),
        resource_list_padding: value.resource_list_padding(),
        loc_strings: value
            .loc_strings()
            .iter()
            .map(|(id, text)| ErfLocStringDto {
                id:   *id,
                text: text.clone(),
            })
            .collect(),
        entries,
        lossless: None,
    })
}

pub(crate) fn read_erf_dto(bytes: &[u8], filename: &str) -> Result<ErfDto, JsValue> {
    let value = erf::read_erf(Cursor::new(bytes.to_vec()), filename.to_string())
        .map_err(|error| js_error("failed to read ERF", error))?;
    with_lossless_metadata(
        erf_to_dto(&value)?,
        bytes.to_vec(),
        |dto| &mut dto.lossless,
        "failed to fingerprint ERF DTO",
    )
    .map_err(|error| js_error_message(&error))
}

pub(crate) fn write_erf_dto(value: &ErfDto) -> Result<Vec<u8>, JsValue> {
    if let Some(bytes) = unchanged_lossless_bytes(
        value,
        value.lossless.as_ref(),
        |dto| &mut dto.lossless,
        "failed to fingerprint ERF DTO",
    )
    .map_err(|error| js_error_message(&error))?
    {
        Ok(bytes)
    } else {
        dto_to_erf_bytes(value)
    }
}

wasm_read_binding! {
    fn read_erf_from_bytes(bytes: &[u8], filename: &str) -> ErfDto {
        read_erf_dto(bytes, filename)
    }
    , serialize_context: "failed to serialize ERF",
}

wasm_write_binding! {
    fn write_erf_to_bytes(value: JsValue) -> ErfDto
    , deserialize_context: "failed to deserialize ERF DTO",
    {
        write_erf_dto(&value)
    }
}
