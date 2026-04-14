use std::io::Cursor;

use nwnrs::prelude::gff;
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsValue;

use crate::{
    bindings::{js_error, js_error_message},
    lossless::{LosslessDtoMetadata, unchanged_lossless_bytes, with_lossless_metadata},
};

/// A wasm-facing localized GFF string.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GffLocStringDto {
    /// The fallback TLK string reference.
    pub str_ref: u32,
    /// Inline localized overrides.
    pub entries: Vec<GffLocStringEntryDto>,
}

/// A single localized entry inside a GFF localized string.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GffLocStringEntryDto {
    /// The NWN language identifier.
    pub language: i32,
    /// The localized text.
    pub text:     String,
}

/// A labeled field in a GFF structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GffFieldDto {
    /// The field label.
    pub label: String,
    /// The field value.
    pub value: GffValueDto,
}

/// A wasm-facing GFF structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GffStructDto {
    /// The stored structure id.
    pub id:     i32,
    /// The ordered fields in the structure.
    pub fields: Vec<GffFieldDto>,
}

/// A wasm-facing full GFF document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GffRootDto {
    /// The four-byte file type.
    pub file_type:    String,
    /// The four-byte file version.
    pub file_version: String,
    /// The root structure.
    pub root:         GffStructDto,
    /// Internal provenance metadata for unchanged write-backs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lossless:     Option<LosslessDtoMetadata>,
}

/// A wasm-facing GFF value.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value")]
pub enum GffValueDto {
    /// An unsigned 8-bit integer.
    Byte(u8),
    /// A signed 8-bit integer.
    Char(i8),
    /// An unsigned 16-bit integer.
    Word(u16),
    /// A signed 16-bit integer.
    Short(i16),
    /// An unsigned 32-bit integer.
    Dword(u32),
    /// A signed 32-bit integer.
    Int(i32),
    /// A 32-bit float.
    Float(f32),
    /// An unsigned 64-bit integer.
    Dword64(u64),
    /// A signed 64-bit integer.
    Int64(i64),
    /// A 64-bit float.
    Double(f64),
    /// A counted string.
    CExoString(String),
    /// A resource reference string.
    ResRef(String),
    /// A localized string table.
    CExoLocString(GffLocStringDto),
    /// An opaque byte blob.
    Void(Vec<u8>),
    /// A nested structure.
    Struct(GffStructDto),
    /// A list of nested structures.
    List(Vec<GffStructDto>),
}

fn gff_root_to_dto(root: &gff::GffRoot) -> GffRootDto {
    GffRootDto {
        file_type:    root.file_type.clone(),
        file_version: root.file_version.clone(),
        root:         gff_struct_to_dto(&root.root),
        lossless:     None,
    }
}

fn gff_struct_to_dto(value: &gff::GffStruct) -> GffStructDto {
    GffStructDto {
        id:     value.id,
        fields: value
            .fields()
            .iter()
            .map(|(label, field)| GffFieldDto {
                label: label.clone(),
                value: gff_value_to_dto(field.value()),
            })
            .collect(),
    }
}

fn gff_value_to_dto(value: &gff::GffValue) -> GffValueDto {
    match value {
        gff::GffValue::Byte(value) => GffValueDto::Byte(*value),
        gff::GffValue::Char(value) => GffValueDto::Char(*value),
        gff::GffValue::Word(value) => GffValueDto::Word(*value),
        gff::GffValue::Short(value) => GffValueDto::Short(*value),
        gff::GffValue::Dword(value) => GffValueDto::Dword(*value),
        gff::GffValue::Int(value) => GffValueDto::Int(*value),
        gff::GffValue::Float(value) => GffValueDto::Float(*value),
        gff::GffValue::Dword64(value) => GffValueDto::Dword64(*value),
        gff::GffValue::Int64(value) => GffValueDto::Int64(*value),
        gff::GffValue::Double(value) => GffValueDto::Double(*value),
        gff::GffValue::CExoString(value) => GffValueDto::CExoString(value.clone()),
        gff::GffValue::ResRef(value) => GffValueDto::ResRef(value.clone()),
        gff::GffValue::CExoLocString(value) => GffValueDto::CExoLocString(GffLocStringDto {
            str_ref: value.str_ref,
            entries: value
                .entries
                .iter()
                .map(|(language, text)| GffLocStringEntryDto {
                    language: *language,
                    text:     text.clone(),
                })
                .collect(),
        }),
        gff::GffValue::Void(value) => GffValueDto::Void(value.clone()),
        gff::GffValue::Struct(value) => GffValueDto::Struct(gff_struct_to_dto(value)),
        gff::GffValue::List(value) => {
            GffValueDto::List(value.iter().map(gff_struct_to_dto).collect())
        }
    }
}

fn dto_to_gff_root(value: &GffRootDto) -> Result<gff::GffRoot, JsValue> {
    let mut root = gff::GffRoot::new(&value.file_type);
    root.file_version.clone_from(&value.file_version);
    root.root = dto_to_gff_struct(&value.root)?;
    Ok(root)
}

fn dto_to_gff_struct(value: &GffStructDto) -> Result<gff::GffStruct, JsValue> {
    let mut result = gff::GffStruct::new(value.id);
    for field in &value.fields {
        result
            .put_field(
                &field.label,
                gff::GffField::new(dto_to_gff_value(&field.value)?),
            )
            .map_err(|error| js_error("failed to build GFF struct", error))?;
    }
    Ok(result)
}

fn dto_to_gff_value(value: &GffValueDto) -> Result<gff::GffValue, JsValue> {
    Ok(match value {
        GffValueDto::Byte(value) => gff::GffValue::Byte(*value),
        GffValueDto::Char(value) => gff::GffValue::Char(*value),
        GffValueDto::Word(value) => gff::GffValue::Word(*value),
        GffValueDto::Short(value) => gff::GffValue::Short(*value),
        GffValueDto::Dword(value) => gff::GffValue::Dword(*value),
        GffValueDto::Int(value) => gff::GffValue::Int(*value),
        GffValueDto::Float(value) => gff::GffValue::Float(*value),
        GffValueDto::Dword64(value) => gff::GffValue::Dword64(*value),
        GffValueDto::Int64(value) => gff::GffValue::Int64(*value),
        GffValueDto::Double(value) => gff::GffValue::Double(*value),
        GffValueDto::CExoString(value) => gff::GffValue::CExoString(value.clone()),
        GffValueDto::ResRef(value) => gff::GffValue::ResRef(value.clone()),
        GffValueDto::CExoLocString(value) => gff::GffValue::CExoLocString(gff::GffCExoLocString {
            str_ref: value.str_ref,
            entries: value
                .entries
                .iter()
                .map(|entry| (entry.language, entry.text.clone()))
                .collect(),
        }),
        GffValueDto::Void(value) => gff::GffValue::Void(value.clone()),
        GffValueDto::Struct(value) => gff::GffValue::Struct(dto_to_gff_struct(value)?),
        GffValueDto::List(value) => gff::GffValue::List(
            value
                .iter()
                .map(dto_to_gff_struct)
                .collect::<Result<Vec<_>, _>>()?,
        ),
    })
}

pub(crate) fn read_gff_dto(bytes: &[u8]) -> Result<GffRootDto, JsValue> {
    let mut cursor = Cursor::new(bytes);
    let root =
        gff::read_gff_root(&mut cursor).map_err(|error| js_error("failed to read GFF", error))?;
    with_lossless_metadata(
        gff_root_to_dto(&root),
        bytes.to_vec(),
        |dto| &mut dto.lossless,
        "failed to fingerprint GFF DTO",
    )
    .map_err(|error| js_error_message(&error))
}

pub(crate) fn write_gff_dto(value: &GffRootDto) -> Result<Vec<u8>, JsValue> {
    if let Some(bytes) = unchanged_lossless_bytes(
        value,
        value.lossless.as_ref(),
        |dto| &mut dto.lossless,
        "failed to fingerprint GFF DTO",
    )
    .map_err(|error| js_error_message(&error))?
    {
        Ok(bytes)
    } else {
        let edited = dto_to_gff_root(value)?;
        let root = if let Some(lossless) = &value.lossless {
            let mut cursor = Cursor::new(lossless.original_bytes.clone());
            let mut parsed = gff::read_gff_root(&mut cursor)
                .map_err(|error| js_error("failed to read original GFF bytes", error))?;
            gff::merge_root_preserving_provenance(&mut parsed, &edited)
                .map_err(|error| js_error("failed to merge edited GFF", error))?;
            parsed
        } else {
            edited
        };
        let mut out = Cursor::new(Vec::new());
        gff::write_gff_root(&mut out, &root)
            .map_err(|error| js_error("failed to write GFF", error))?;
        Ok(out.into_inner())
    }
}

wasm_read_binding! {
    fn read_gff_from_bytes(bytes: &[u8]) -> GffRootDto {
        read_gff_dto(bytes)
    }
    , serialize_context: "failed to serialize GFF",
}

wasm_write_binding! {
    fn write_gff_to_bytes(value: JsValue) -> GffRootDto
    , deserialize_context: "failed to deserialize GFF DTO",
    {
        write_gff_dto(&value)
    }
}
