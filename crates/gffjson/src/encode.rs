use nwnrs_core::prelude::*;
use nwnrs_gff::prelude::*;
use serde_json::{Map, Number, Value};
use tracing::instrument;

use crate::prelude::*;

/// Converts a GFF struct to the JSON representation used by this crate.
#[instrument(level = "debug", skip_all, err)]
pub fn gff_struct_to_json_value(structure: &GffStruct) -> GffJsonResult<Value> {
    let mut object = Map::new();
    if structure.id != -1 {
        object.insert("__struct_id".to_string(), Value::from(structure.id));
    }

    for (label, field) in structure.fields() {
        let mut container = Map::new();
        container.insert(
            "type".to_string(),
            Value::String(field_type_name(field.value()).to_string()),
        );

        match field.value() {
            GffValue::Byte(value) => {
                container.insert("value".to_string(), Value::from(*value));
            }
            GffValue::Char(value) => {
                container.insert("value".to_string(), Value::from(*value));
            }
            GffValue::Word(value) => {
                container.insert("value".to_string(), Value::from(*value));
            }
            GffValue::Short(value) => {
                container.insert("value".to_string(), Value::from(*value));
            }
            GffValue::Dword(value) => {
                container.insert("value".to_string(), Value::from(*value));
            }
            GffValue::Int(value) => {
                container.insert("value".to_string(), Value::from(*value));
            }
            GffValue::Float(value) => {
                container.insert(
                    "value".to_string(),
                    Value::Number(number_from_f64(f64::from(*value))?),
                );
            }
            GffValue::Dword64(value) => {
                container.insert("value".to_string(), Value::from(*value));
            }
            GffValue::Int64(value) => {
                container.insert("value".to_string(), Value::from(*value));
            }
            GffValue::Double(value) => {
                container.insert("value".to_string(), Value::Number(number_from_f64(*value)?));
            }
            GffValue::CExoString(value) | GffValue::ResRef(value) => {
                container.insert("value".to_string(), Value::String(value.clone()));
            }
            GffValue::CExoLocString(value) => {
                let mut entries = Map::new();
                for (language, text) in &value.entries {
                    entries.insert(language.to_string(), Value::String(text.clone()));
                }
                if value.str_ref != BAD_STRREF {
                    entries.insert("id".to_string(), Value::from(value.str_ref));
                }
                container.insert("value".to_string(), Value::Object(entries));
            }
            GffValue::Void(value) => {
                container.insert("value64".to_string(), Value::String(base64_encode(value)));
            }
            GffValue::Struct(value) => {
                container.insert("value".to_string(), gff_struct_to_json_value(value)?);
                container.insert("__struct_id".to_string(), Value::from(value.id));
            }
            GffValue::List(value) => {
                let mut items = Vec::with_capacity(value.len());
                for element in value {
                    items.push(gff_struct_to_json_value(element)?);
                }
                container.insert("value".to_string(), Value::Array(items));
            }
        }

        object.insert(label.clone(), Value::Object(container));
    }

    Ok(Value::Object(object))
}

/// Converts a GFF root to a JSON value.
#[instrument(
    level = "debug",
    skip_all,
    err,
    fields(file_type = %root.file_type)
)]
pub fn gff_root_to_json_value(root: &GffRoot) -> GffJsonResult<Value> {
    let mut object = match gff_struct_to_json_value(&root.root)? {
        Value::Object(object) => object,
        _ => unreachable!("GFF struct conversion always yields an object"),
    };
    object.insert(
        "__data_type".to_string(),
        Value::String(root.file_type.clone()),
    );
    Ok(Value::Object(object))
}

/// Serializes a GFF root to compact JSON.
#[instrument(
    level = "debug",
    skip_all,
    err,
    fields(file_type = %root.file_type)
)]
pub fn gff_root_to_json_string(root: &GffRoot) -> GffJsonResult<String> {
    Ok(serde_json::to_string(&gff_root_to_json_value(root)?)?)
}

/// Serializes a GFF root to pretty-printed JSON.
#[instrument(
    level = "debug",
    skip_all,
    err,
    fields(file_type = %root.file_type)
)]
pub fn gff_root_to_pretty_json_string(root: &GffRoot) -> GffJsonResult<String> {
    Ok(serde_json::to_string_pretty(&gff_root_to_json_value(
        root,
    )?)?)
}

pub(crate) fn base64_encode(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let Some(&first) = chunk.first() else {
            continue;
        };
        let second = *chunk.get(1).unwrap_or(&0);
        let third = *chunk.get(2).unwrap_or(&0);
        let triple = (u32::from(first) << 16) | (u32::from(second) << 8) | u32::from(third);

        output.push(base64_char(((triple >> 18) & 0x3f) as u8));
        output.push(base64_char(((triple >> 12) & 0x3f) as u8));
        output.push(if chunk.len() > 1 {
            base64_char(((triple >> 6) & 0x3f) as u8)
        } else {
            '='
        });
        output.push(if chunk.len() > 2 {
            base64_char((triple & 0x3f) as u8)
        } else {
            '='
        });
    }
    output
}

fn field_type_name(value: &GffValue) -> &'static str {
    match value {
        GffValue::Byte(_) => "byte",
        GffValue::Char(_) => "char",
        GffValue::Word(_) => "word",
        GffValue::Short(_) => "short",
        GffValue::Dword(_) => "dword",
        GffValue::Int(_) => "int",
        GffValue::Float(_) => "float",
        GffValue::Dword64(_) => "dword64",
        GffValue::Int64(_) => "int64",
        GffValue::Double(_) => "double",
        GffValue::CExoString(_) => "cexostring",
        GffValue::ResRef(_) => "resref",
        GffValue::CExoLocString(_) => "cexolocstring",
        GffValue::Void(_) => "void",
        GffValue::Struct(_) => "struct",
        GffValue::List(_) => "list",
    }
}

fn number_from_f64(value: f64) -> GffJsonResult<Number> {
    Number::from_f64(value).ok_or_else(|| {
        GffJsonError::msg(format!("non-finite float value {value} is not valid JSON"))
    })
}

fn base64_char(index: u8) -> char {
    match crate::BASE64_ALPHABET.get(usize::from(index)).copied() {
        Some(byte) => char::from(byte),
        None => unreachable!("base64 alphabet index must be in range"),
    }
}
