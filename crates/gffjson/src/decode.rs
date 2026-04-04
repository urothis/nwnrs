use nwnrs_core::prelude::*;
use nwnrs_gff::prelude::*;
use nwnrs_util::prelude::*;
use serde_json::{Map, Value};
use tracing::instrument;

use crate::prelude::*;

/// Populates a GFF struct from its JSON representation.
#[instrument(level = "debug", skip_all, err)]
pub fn gff_struct_from_json_value(value: &Value, result: &mut GffStruct) -> GffJsonResult<()> {
    let object = expect_object(value, "GFF struct JSON must be an object")?;
    if let Some(struct_id) = object.get("__struct_id") {
        result.id = parse_i32(struct_id, "__struct_id")?;
    }

    for (label, field) in object {
        if label.starts_with("__") {
            continue;
        }

        let container = expect_object(field, &format!("field {label:?} must be an object"))?;
        let field_type = expect_string(
            container
                .get("type")
                .ok_or_else(|| GffJsonError::msg(format!("field {label:?} is missing type")))?,
            &format!("field {label:?} type must be a string"),
        )?;

        let parsed = match field_type {
            "byte" => GffValue::Byte(parse_u8(require_value(container, label)?, label)?),
            "char" => GffValue::Char(parse_i8(require_value(container, label)?, label)?),
            "word" => GffValue::Word(parse_u16(require_value(container, label)?, label)?),
            "short" => GffValue::Short(parse_i16(require_value(container, label)?, label)?),
            "dword" => GffValue::Dword(parse_u32(require_value(container, label)?, label)?),
            "int" => GffValue::Int(parse_i32(require_value(container, label)?, label)?),
            "float" => GffValue::Float(parse_f32(require_value(container, label)?, label)?),
            "dword64" => GffValue::Dword64(parse_u64(require_value(container, label)?, label)?),
            "int64" => GffValue::Int64(parse_i64(require_value(container, label)?, label)?),
            "double" => {
                GffValue::Double(parse_json_double(require_value(container, label)?, label)?)
            }
            "cexostring" => GffValue::CExoString(
                expect_string(
                    require_value(container, label)?,
                    &format!("field {label:?} must be a string"),
                )?
                .to_string(),
            ),
            "resref" => GffValue::ResRef(
                expect_string(
                    require_value(container, label)?,
                    &format!("field {label:?} must be a string"),
                )?
                .to_string(),
            ),
            "void" => GffValue::Void(parse_void_value(container, label)?),
            "struct" => {
                let mut structure = new_gff_struct(-1);
                gff_struct_from_json_value(require_value(container, label)?, &mut structure)?;
                GffValue::Struct(structure)
            }
            "cexolocstring" => GffValue::CExoLocString(parse_loc_string(container, label)?),
            "list" => {
                let entries = expect_array(
                    require_value(container, label)?,
                    &format!("field {label:?} list value must be an array"),
                )?;
                let mut list = Vec::with_capacity(entries.len());
                for entry in entries {
                    let mut structure = new_gff_struct(-1);
                    gff_struct_from_json_value(entry, &mut structure)?;
                    list.push(structure);
                }
                GffValue::List(list)
            }
            _ => {
                return Err(GffJsonError::msg(format!(
                    "unknown field type {:?}",
                    field_type
                )));
            }
        };

        result.put_value(label.clone(), parsed)?;
    }

    Ok(())
}

/// Parses a GFF root from a JSON value.
#[instrument(level = "debug", skip_all, err)]
pub fn gff_root_from_json_value(value: &Value) -> GffJsonResult<GffRoot> {
    let object = expect_object(value, "GFF root JSON must be an object")?;
    let data_type = expect_string(
        object
            .get("__data_type")
            .ok_or_else(|| GffJsonError::msg("GFF root JSON is missing __data_type"))?,
        "__data_type must be a string",
    )?;

    let mut root = new_gff_root("GFF ");
    gff_struct_from_json_value(value, &mut root.root)?;
    root.file_type = data_type.to_string();
    Ok(root)
}

/// Parses a GFF root from a JSON string.
#[instrument(level = "debug", skip_all, err, fields(input_len = input.len()))]
pub fn gff_root_from_json_str(input: &str) -> GffJsonResult<GffRoot> {
    let value: Value = serde_json::from_str(input)?;
    gff_root_from_json_value(&value)
}

pub(crate) fn base64_decode(input: &str) -> GffJsonResult<Vec<u8>> {
    expect(input.len().is_multiple_of(4), "invalid base64 length")?;

    let mut output = Vec::with_capacity(input.len() / 4 * 3);
    for chunk in input.as_bytes().chunks(4) {
        let mut values = [0_u8; 4];
        let mut padding = 0;

        for (idx, byte) in chunk.iter().enumerate() {
            let decoded = match *byte {
                b'A'..=b'Z' => byte - b'A',
                b'a'..=b'z' => byte - b'a' + 26,
                b'0'..=b'9' => byte - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                b'=' => {
                    padding += 1;
                    0
                }
                _ => return Err(GffJsonError::msg("invalid base64 character")),
            };
            if let Some(slot) = values.get_mut(idx) {
                *slot = decoded;
            }
        }

        let triple = (u32::from(values[0]) << 18)
            | (u32::from(values[1]) << 12)
            | (u32::from(values[2]) << 6)
            | u32::from(values[3]);

        output.push(((triple >> 16) & 0xff) as u8);
        if padding < 2 {
            output.push(((triple >> 8) & 0xff) as u8);
        }
        if padding < 1 {
            output.push((triple & 0xff) as u8);
        }
    }

    Ok(output)
}

fn require_value<'a>(container: &'a Map<String, Value>, label: &str) -> GffJsonResult<&'a Value> {
    container
        .get("value")
        .ok_or_else(|| GffJsonError::msg(format!("field {label:?} is missing value")))
}

fn expect_object<'a>(value: &'a Value, message: &str) -> GffJsonResult<&'a Map<String, Value>> {
    value
        .as_object()
        .ok_or_else(|| GffJsonError::msg(message.to_string()))
}

fn expect_array<'a>(value: &'a Value, message: &str) -> GffJsonResult<&'a [Value]> {
    value
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| GffJsonError::msg(message.to_string()))
}

fn expect_string<'a>(value: &'a Value, message: &str) -> GffJsonResult<&'a str> {
    value
        .as_str()
        .ok_or_else(|| GffJsonError::msg(message.to_string()))
}

fn parse_i64(value: &Value, label: &str) -> GffJsonResult<i64> {
    if let Some(number) = value.as_i64() {
        Ok(number)
    } else if let Some(number) = value.as_u64() {
        i64::try_from(number).map_err(|_error| {
            GffJsonError::msg(format!("field {label:?} integer value out of range"))
        })
    } else {
        Err(GffJsonError::msg(format!(
            "field {label:?} must be an integer"
        )))
    }
}

fn parse_u64(value: &Value, label: &str) -> GffJsonResult<u64> {
    if let Some(number) = value.as_u64() {
        Ok(number)
    } else if let Some(number) = value.as_i64() {
        u64::try_from(number).map_err(|_error| {
            GffJsonError::msg(format!("field {label:?} integer value out of range"))
        })
    } else {
        Err(GffJsonError::msg(format!(
            "field {label:?} must be an integer"
        )))
    }
}

fn parse_i32(value: &Value, label: &str) -> GffJsonResult<i32> {
    i32::try_from(parse_i64(value, label)?)
        .map_err(|_error| GffJsonError::msg(format!("field {label:?} integer value out of range")))
}

fn parse_u32(value: &Value, label: &str) -> GffJsonResult<u32> {
    u32::try_from(parse_u64(value, label)?)
        .map_err(|_error| GffJsonError::msg(format!("field {label:?} integer value out of range")))
}

fn parse_i16(value: &Value, label: &str) -> GffJsonResult<i16> {
    i16::try_from(parse_i64(value, label)?)
        .map_err(|_error| GffJsonError::msg(format!("field {label:?} integer value out of range")))
}

fn parse_u16(value: &Value, label: &str) -> GffJsonResult<u16> {
    u16::try_from(parse_u64(value, label)?)
        .map_err(|_error| GffJsonError::msg(format!("field {label:?} integer value out of range")))
}

fn parse_i8(value: &Value, label: &str) -> GffJsonResult<i8> {
    i8::try_from(parse_i64(value, label)?)
        .map_err(|_error| GffJsonError::msg(format!("field {label:?} integer value out of range")))
}

fn parse_u8(value: &Value, label: &str) -> GffJsonResult<u8> {
    u8::try_from(parse_u64(value, label)?)
        .map_err(|_error| GffJsonError::msg(format!("field {label:?} integer value out of range")))
}

fn parse_f32(value: &Value, label: &str) -> GffJsonResult<f32> {
    let float = value
        .as_f64()
        .or_else(|| {
            value
                .as_i64()
                .and_then(|value| value.to_string().parse::<f64>().ok())
        })
        .or_else(|| {
            value
                .as_u64()
                .and_then(|value| value.to_string().parse::<f64>().ok())
        })
        .ok_or_else(|| GffJsonError::msg(format!("field {label:?} must be numeric")))?;
    float
        .to_string()
        .parse::<f32>()
        .map_err(|_error| GffJsonError::msg(format!("field {label:?} float value out of range")))
}

fn parse_json_double(value: &Value, label: &str) -> GffJsonResult<f64> {
    match value {
        Value::Number(number) if number.is_f64() => number
            .as_f64()
            .ok_or_else(|| GffJsonError::msg(format!("field {label:?} must be a JSON float"))),
        _ => Err(GffJsonError::msg(format!(
            "field {label:?} must be a JSON float"
        ))),
    }
}

fn parse_loc_string(
    container: &Map<String, Value>,
    label: &str,
) -> GffJsonResult<GffCExoLocString> {
    let value = expect_object(
        require_value(container, label)?,
        &format!("field {label:?} locstring value must be an object"),
    )?;

    let mut loc = new_c_exo_loc_string();
    for (key, entry) in value {
        if key == "id" {
            loc.str_ref = parse_u32(entry, label)?;
        } else {
            loc.entries.push((
                key.parse::<i32>().map_err(|_error| {
                    GffJsonError::msg(format!(
                        "field {label:?} locstring key {key:?} is not an integer"
                    ))
                })?,
                expect_string(
                    entry,
                    &format!("field {label:?} locstring entry must be a string"),
                )?
                .to_string(),
            ));
        }
    }

    if loc.str_ref == BAD_STRREF
        && let Some(legacy_id) = container.get("id")
    {
        loc.str_ref = parse_u32(legacy_id, label)?;
    }

    Ok(loc)
}

fn parse_void_value(container: &Map<String, Value>, label: &str) -> GffJsonResult<Vec<u8>> {
    if let Some(value64) = container.get("value64") {
        let encoded = expect_string(
            value64,
            &format!("field {label:?} value64 must be a string"),
        )?;
        return base64_decode(encoded);
    }

    let legacy = expect_string(
        require_value(container, label)?,
        &format!("field {label:?} void value must be a string"),
    )?;
    Ok(legacy.as_bytes().to_vec())
}
