use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Map, Number, Value};

use crate::gff::{GffCExoLocString, GffError, GffField, GffResult, GffRoot, GffStruct, GffValue};

/// Parses a canonical `neverwinter.nim` GFF JSON document from UTF-8 bytes.
///
/// # Errors
///
/// Returns an error when `data` is not valid JSON or does not represent a
/// valid typed GFF document.
pub fn gff_root_from_json_bytes(data: impl AsRef<[u8]>) -> GffResult<GffRoot> {
    let json = serde_json::from_slice(data.as_ref())
        .map_err(|error| GffError::msg(format!("invalid GFF JSON: {error}")))?;
    gff_root_from_json(&json)
}

/// Renders a GFF document as canonical pretty-printed `neverwinter.nim` JSON.
///
/// # Errors
///
/// Returns an error when a GFF value cannot be represented as JSON or the
/// resulting JSON cannot be serialized.
pub fn gff_root_to_json_bytes(root: &GffRoot) -> GffResult<Vec<u8>> {
    let json = gff_root_to_json(root)?;
    serde_json::to_vec_pretty(&json)
        .map_err(|error| GffError::msg(format!("failed to serialize GFF JSON: {error}")))
}

/// Converts a GFF document to the canonical `neverwinter.nim` JSON shape.
///
/// # Errors
///
/// Returns an error when a floating-point value cannot be represented by JSON.
pub fn gff_root_to_json(root: &GffRoot) -> GffResult<Value> {
    let mut object = struct_to_json(&root.root)?;
    object.insert(
        "__data_type".to_string(),
        Value::String(root.file_type.clone()),
    );
    sort_object(&mut object);
    Ok(Value::Object(object))
}

/// Parses the canonical `neverwinter.nim` JSON shape into a GFF document.
///
/// # Errors
///
/// Returns an error when required metadata is missing or a typed field value is
/// invalid.
pub fn gff_root_from_json(json: &Value) -> GffResult<GffRoot> {
    let object = as_object(json, "GFF root")?;
    let file_type = object
        .get("__data_type")
        .and_then(Value::as_str)
        .ok_or_else(|| GffError::msg("GFF JSON root requires string __data_type"))?;
    if file_type.len() != 4 {
        return Err(GffError::msg(
            "GFF JSON __data_type must be exactly 4 bytes",
        ));
    }

    Ok(GffRoot {
        file_type:       file_type.to_string(),
        file_version:    "V3.2".to_string(),
        root:            struct_from_json(object, -1)?,
        source_bytes:    None,
        source_snapshot: None,
    })
}

fn struct_to_json(value: &GffStruct) -> GffResult<Map<String, Value>> {
    let mut object = Map::new();
    if value.id != -1 {
        object.insert("__struct_id".to_string(), Value::from(value.id));
    }

    let mut fields = value.fields().iter().collect::<Vec<_>>();
    fields.sort_by(|(left, _), (right, _)| {
        left.to_ascii_lowercase().cmp(&right.to_ascii_lowercase())
    });
    for (label, field) in fields {
        object.insert(label.clone(), field_to_json(field)?);
    }
    Ok(object)
}

fn field_to_json(field: &GffField) -> GffResult<Value> {
    let mut object = Map::new();
    let (kind, value) = match field.value() {
        GffValue::Byte(value) => ("byte", Value::from(*value)),
        GffValue::Char(value) => ("char", Value::from(*value)),
        GffValue::Word(value) => ("word", Value::from(*value)),
        GffValue::Short(value) => ("short", Value::from(*value)),
        GffValue::Dword(value) => ("dword", Value::from(*value)),
        GffValue::Int(value) => ("int", Value::from(*value)),
        GffValue::Dword64(value) => ("dword64", Value::from(*value)),
        GffValue::Int64(value) => ("int64", Value::from(*value)),
        GffValue::Float(value) => (
            "float",
            Value::Number(
                Number::from_f64(f64::from(*value))
                    .ok_or_else(|| GffError::msg("GFF float is not finite"))?,
            ),
        ),
        GffValue::Double(value) => (
            "double",
            Value::Number(
                Number::from_f64(*value)
                    .ok_or_else(|| GffError::msg("GFF double is not finite"))?,
            ),
        ),
        GffValue::CExoString(value) => ("cexostring", Value::String(value.clone())),
        GffValue::ResRef(value) => ("resref", Value::String(value.clone())),
        GffValue::CExoLocString(value) => {
            let mut entries = Map::new();
            for (language, text) in &value.entries {
                entries.insert(language.to_string(), Value::String(text.clone()));
            }
            if value.str_ref != crate::localization::BAD_STRREF {
                entries.insert("id".to_string(), Value::from(value.str_ref));
            }
            ("cexolocstring", Value::Object(entries))
        }
        GffValue::Void(value) => {
            object.insert("type".to_string(), Value::String("void".to_string()));
            object.insert("value64".to_string(), Value::String(STANDARD.encode(value)));
            return Ok(Value::Object(object));
        }
        GffValue::Struct(value) => {
            let struct_json = Value::Object(struct_to_json(value)?);
            object.insert("type".to_string(), Value::String("struct".to_string()));
            object.insert("value".to_string(), struct_json);
            object.insert("__struct_id".to_string(), Value::from(value.id));
            return Ok(Value::Object(object));
        }
        GffValue::List(value) => {
            let values = value
                .iter()
                .map(|entry| struct_to_json(entry).map(Value::Object))
                .collect::<GffResult<Vec<_>>>()?;
            ("list", Value::Array(values))
        }
    };

    object.insert("type".to_string(), Value::String(kind.to_string()));
    object.insert("value".to_string(), value);
    Ok(Value::Object(object))
}

fn struct_from_json(object: &Map<String, Value>, default_id: i32) -> GffResult<GffStruct> {
    let id = object
        .get("__struct_id")
        .map(|value| json_i64(value, "__struct_id").and_then(to_i32))
        .transpose()?
        .unwrap_or(default_id);
    let mut result = GffStruct::new(id);

    for (label, encoded) in object {
        if label.starts_with("__") {
            continue;
        }
        let field = field_from_json(label, encoded)?;
        result.put_field(label, field)?;
    }
    Ok(result)
}

fn field_from_json(label: &str, encoded: &Value) -> GffResult<GffField> {
    let object = as_object(encoded, &format!("field {label}"))?;
    let kind = object
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| GffError::msg(format!("field {label} requires string type")))?;
    let value = object.get("value");
    let required = || value.ok_or_else(|| GffError::msg(format!("field {label} requires value")));

    let parsed = match kind {
        "byte" => GffValue::Byte(to_u8(json_u64(required()?, label)?)?),
        "char" => GffValue::Char(to_i8(json_i64(required()?, label)?)?),
        "word" => GffValue::Word(to_u16(json_u64(required()?, label)?)?),
        "short" => GffValue::Short(to_i16(json_i64(required()?, label)?)?),
        "dword" => GffValue::Dword(to_u32(json_u64(required()?, label)?)?),
        "int" => GffValue::Int(to_i32(json_i64(required()?, label)?)?),
        "dword64" => GffValue::Dword64(json_u64(required()?, label)?),
        "int64" => GffValue::Int64(json_i64(required()?, label)?),
        "float" => GffValue::Float(json_f32(required()?, label)?),
        "double" => GffValue::Double(json_f64(required()?, label)?),
        "cexostring" => GffValue::CExoString(json_string(required()?, label)?.to_string()),
        "resref" => GffValue::ResRef(json_string(required()?, label)?.to_string()),
        "void" => {
            let encoded = object
                .get("value64")
                .or(value)
                .ok_or_else(|| GffError::msg(format!("field {label} requires value64")))?;
            let encoded = json_string(encoded, label)?;
            let bytes = STANDARD.decode(encoded).map_err(|error| {
                GffError::msg(format!("field {label} has invalid base64: {error}"))
            })?;
            GffValue::Void(bytes)
        }
        "cexolocstring" => {
            let entries = as_object(required()?, &format!("field {label} value"))?;
            let mut loc = GffCExoLocString::default();
            for (language, text) in entries {
                if language == "id" {
                    loc.str_ref = to_u32(json_u64(text, label)?)?;
                } else {
                    let language = language.parse::<i32>().map_err(|error| {
                        GffError::msg(format!("field {label} has invalid language id: {error}"))
                    })?;
                    loc.entries
                        .push((language, json_string(text, label)?.to_string()));
                }
            }
            if loc.str_ref == crate::localization::BAD_STRREF
                && let Some(id) = object.get("id")
            {
                loc.str_ref = to_u32(json_u64(id, label)?)?;
            }
            GffValue::CExoLocString(loc)
        }
        "struct" => GffValue::Struct(struct_from_json(
            as_object(required()?, &format!("field {label} value"))?,
            -1,
        )?),
        "list" => {
            let values = required()?
                .as_array()
                .ok_or_else(|| GffError::msg(format!("field {label} value must be an array")))?;
            let structs = values
                .iter()
                .map(|value| {
                    as_object(value, &format!("field {label} list entry"))
                        .and_then(|entry| struct_from_json(entry, -1))
                })
                .collect::<GffResult<Vec<_>>>()?;
            GffValue::List(structs)
        }
        other => {
            return Err(GffError::msg(format!(
                "field {label} has unknown type {other}"
            )))
        }
    };

    Ok(GffField::new(parsed))
}

fn sort_object(object: &mut Map<String, Value>) {
    let mut entries = std::mem::take(object).into_iter().collect::<Vec<_>>();
    entries.sort_by(|(left, _), (right, _)| {
        left.to_ascii_lowercase().cmp(&right.to_ascii_lowercase())
    });
    object.extend(entries);
}

fn as_object<'a>(value: &'a Value, context: &str) -> GffResult<&'a Map<String, Value>> {
    value
        .as_object()
        .ok_or_else(|| GffError::msg(format!("{context} must be an object")))
}

fn json_string<'a>(value: &'a Value, context: &str) -> GffResult<&'a str> {
    value
        .as_str()
        .ok_or_else(|| GffError::msg(format!("{context} must be a string")))
}

fn json_u64(value: &Value, context: &str) -> GffResult<u64> {
    value
        .as_u64()
        .ok_or_else(|| GffError::msg(format!("{context} must be an unsigned integer")))
}

fn json_i64(value: &Value, context: &str) -> GffResult<i64> {
    value
        .as_i64()
        .ok_or_else(|| GffError::msg(format!("{context} must be an integer")))
}

fn json_f64(value: &Value, context: &str) -> GffResult<f64> {
    value
        .as_f64()
        .filter(|number| number.is_finite())
        .ok_or_else(|| GffError::msg(format!("{context} must be a finite number")))
}

fn json_f32(value: &Value, context: &str) -> GffResult<f32> {
    let number = value
        .as_number()
        .ok_or_else(|| GffError::msg(format!("{context} must be a number")))?;
    let converted = number
        .to_string()
        .parse::<f32>()
        .map_err(|error| GffError::msg(format!("{context} is not a valid f32: {error}")))?;
    if !converted.is_finite() {
        return Err(GffError::msg(format!(
            "field {context} is outside f32 range"
        )));
    }
    Ok(converted)
}

macro_rules! checked_integer {
    ($name:ident, $target:ty, $source:ty) => {
        fn $name(value: $source) -> GffResult<$target> {
            <$target>::try_from(value).map_err(|error| GffError::msg(error.to_string()))
        }
    };
}

checked_integer!(to_u8, u8, u64);
checked_integer!(to_i8, i8, i64);
checked_integer!(to_u16, u16, u64);
checked_integer!(to_i16, i16, i64);
checked_integer!(to_u32, u32, u64);
checked_integer!(to_i32, i32, i64);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_json_round_trips_typed_values() -> GffResult<()> {
        let mut root = GffRoot::new("UTC ");
        root.put_value("Byte", GffValue::Byte(7))?;
        root.put_value("Blob", GffValue::Void(vec![0, 1, 255]))?;
        root.put_value(
            "List",
            GffValue::List(vec![{
                let mut value = GffStruct::new(42);
                value.put_value("Name", GffValue::CExoString("demo".to_string()))?;
                value
            }]),
        )?;

        let json = gff_root_to_json(&root)?;
        let reparsed = gff_root_from_json(&json)?;
        assert_eq!(gff_root_to_json(&reparsed)?, json);
        assert_eq!(
            gff_root_to_json(&gff_root_from_json_bytes(gff_root_to_json_bytes(&root)?)?)?,
            json
        );
        Ok(())
    }

    #[test]
    fn float_json_rejects_values_outside_f32_range() {
        let result = gff_root_from_json_bytes(
            br#"{"__data_type":"UTC ","Float":{"type":"float","value":3.5e38}}"#,
        );
        assert!(
            result.is_err_and(|error| error.to_string().contains("outside f32 range")),
            "out-of-range float should be rejected"
        );
    }
}
