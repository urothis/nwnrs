use std::io::{self, Read, Seek, SeekFrom, Write};

use nwnrs_util::{
    expect, from_nwnrs_encoding, read_bytes_or_err, read_str_or_err, to_nwnrs_encoding,
};
use tracing::{debug, instrument};

use crate::{
    GffCExoLocString, GffError, GffField, GffFieldKind, GffResult, GffRoot, GffStruct, GffValue,
    HEADER_SIZE, ensure_label,
};

#[derive(Debug, Clone)]
struct Header {
    struct_offset: u32,
    struct_count: u32,
    field_offset: u32,
    field_count: u32,
    label_offset: u32,
    label_count: u32,
    field_data_offset: u32,
    field_data_size: u32,
    field_indices_offset: u32,
    field_indices_size: u32,
    list_indices_offset: u32,
    list_indices_size: u32,
}

#[derive(Debug, Clone)]
struct RawStructEntry {
    id: i32,
    data_or_offset: i32,
    field_count: i32,
}

#[derive(Debug, Clone)]
struct RawFieldEntry {
    field_kind: GffFieldKind,
    label_index: i32,
    data_or_offset: i32,
}

#[derive(Debug, Default)]
struct WriteState {
    labels: Vec<String>,
    structs: Vec<WriteStructEntry>,
    fields: Vec<WriteFieldEntry>,
    field_data: Vec<u8>,
    field_indices: Vec<i32>,
    list_indices: Vec<i32>,
}

#[derive(Debug, Clone, Default)]
struct WriteStructEntry {
    id: i32,
    data_or_offset: i32,
    field_count: i32,
}

#[derive(Debug, Clone)]
struct WriteFieldEntry {
    field_kind: GffFieldKind,
    label_index: i32,
    data_or_offset: i32,
}

/// Reads a complete GFF document from `reader`.
#[instrument(level = "debug", skip_all, err)]
pub fn read_gff_root<R: Read + Seek>(reader: &mut R) -> GffResult<GffRoot> {
    let start = reader.stream_position()?;

    let file_type = read_str_or_err(reader, 4)?;
    let file_version = read_str_or_err(reader, 4)?;
    expect(file_type.len() == 4, "GFF file type must be 4 bytes")?;
    expect(
        file_version == "V3.2",
        format!("unsupported gff version {file_version}"),
    )?;

    let mut header = Header {
        struct_offset: read_u32(reader)?,
        struct_count: read_u32(reader)?,
        field_offset: read_u32(reader)?,
        field_count: read_u32(reader)?,
        label_offset: read_u32(reader)?,
        label_count: read_u32(reader)?,
        field_data_offset: read_u32(reader)?,
        field_data_size: read_u32(reader)?,
        field_indices_offset: read_u32(reader)?,
        field_indices_size: read_u32(reader)?,
        list_indices_offset: read_u32(reader)?,
        list_indices_size: read_u32(reader)?,
    };

    normalize_index_offsets(reader, start, &mut header)?;

    expect(
        usize::try_from(header.struct_offset).ok() == Some(HEADER_SIZE),
        "unexpected struct offset",
    )?;

    let labels = read_labels(reader, start, &header)?;
    let fields = read_field_entries(reader, start, &header)?;
    let field_indices = read_i32_array(
        reader,
        start + u64::from(header.field_indices_offset),
        header.field_indices_size,
    )?;
    let list_indices = read_i32_array(
        reader,
        start + u64::from(header.list_indices_offset),
        header.list_indices_size,
    )?;
    let structs = read_struct_entries(reader, start, &header)?;

    let root_struct = parse_struct(
        0,
        reader,
        start,
        &header,
        &labels,
        &fields,
        &field_indices,
        &list_indices,
        &structs,
    )?;

    let root = GffRoot {
        file_type,
        file_version,
        root: root_struct,
    };
    debug!(file_type = %root.file_type, "read gff root");
    Ok(root)
}

fn normalize_index_offsets<R: Seek>(
    reader: &mut R,
    start: u64,
    header: &mut Header,
) -> GffResult<()> {
    let file_end = reader.seek(SeekFrom::End(0))?;
    let declared_end =
        start + u64::from(header.list_indices_offset) + u64::from(header.list_indices_size);
    if file_end >= declared_end {
        return Ok(());
    }

    let shortfall = declared_end - file_end;
    let shortfall_u32 = u32::try_from(shortfall)
        .map_err(|_error| GffError::msg("GFF section shortfall exceeds 32-bit range"))?;

    expect(
        shortfall_u32 <= header.field_data_size,
        "GFF file truncated before field-data section",
    )?;
    expect(
        shortfall_u32
            <= header
                .field_indices_offset
                .saturating_sub(header.field_data_offset),
        "GFF index offset shortfall exceeds field-data span",
    )?;
    expect(
        shortfall_u32
            <= header
                .list_indices_offset
                .saturating_sub(header.field_indices_offset),
        "GFF index offset shortfall exceeds field-index span",
    )?;

    header.field_data_size -= shortfall_u32;
    header.field_indices_offset -= shortfall_u32;
    header.list_indices_offset -= shortfall_u32;

    Ok(())
}

/// Writes a complete GFF document to `writer`.
#[instrument(
    level = "debug",
    skip_all,
    err,
    fields(file_type = %root.file_type, version = %root.file_version)
)]
pub fn write_gff_root<W: Write + Seek>(writer: &mut W, root: &GffRoot) -> GffResult<()> {
    expect(root.file_type.len() == 4, "GFF file type must be 4 bytes")?;
    expect(
        root.file_version.len() == 4,
        "GFF file version must be 4 bytes",
    )?;
    expect(root.root.id == -1, "root struct id must be -1")?;

    let mut state = WriteState::default();
    let root_idx = collect_struct(&root.root, &mut state)?;
    expect(root_idx == 0, "root struct must serialize as struct 0")?;

    let start = writer.stream_position()?;
    writer.write_all(root.file_type.as_bytes())?;
    writer.write_all(root.file_version.as_bytes())?;

    let mut offset = to_u32_len(HEADER_SIZE, "GFF header size")?;

    write_u32(writer, offset)?;
    let struct_count = to_u32_len(state.structs.len(), "GFF struct count")?;
    write_u32(writer, struct_count)?;
    offset = offset
        .checked_add(struct_count.saturating_mul(12))
        .ok_or_else(|| GffError::msg("GFF struct table offset overflow"))?;

    write_u32(writer, offset)?;
    let field_count = to_u32_len(state.fields.len(), "GFF field count")?;
    write_u32(writer, field_count)?;
    offset = offset
        .checked_add(field_count.saturating_mul(12))
        .ok_or_else(|| GffError::msg("GFF field table offset overflow"))?;

    write_u32(writer, offset)?;
    let label_count = to_u32_len(state.labels.len(), "GFF label count")?;
    write_u32(writer, label_count)?;
    offset = offset
        .checked_add(label_count.saturating_mul(16))
        .ok_or_else(|| GffError::msg("GFF label table offset overflow"))?;

    write_u32(writer, offset)?;
    let field_data_size = to_u32_len(state.field_data.len(), "GFF field data size")?;
    write_u32(writer, field_data_size)?;
    offset = offset
        .checked_add(field_data_size)
        .ok_or_else(|| GffError::msg("GFF field data offset overflow"))?;

    write_u32(writer, offset)?;
    let field_indices_size = state
        .field_indices
        .len()
        .checked_mul(4)
        .ok_or_else(|| GffError::msg("GFF field indices size overflow"))?;
    let field_indices_size = to_u32_len(field_indices_size, "GFF field indices size")?;
    write_u32(writer, field_indices_size)?;
    offset = offset
        .checked_add(field_indices_size)
        .ok_or_else(|| GffError::msg("GFF field indices offset overflow"))?;

    write_u32(writer, offset)?;
    let list_indices_size = state
        .list_indices
        .len()
        .checked_mul(4)
        .ok_or_else(|| GffError::msg("GFF list indices size overflow"))?;
    write_u32(
        writer,
        to_u32_len(list_indices_size, "GFF list indices size")?,
    )?;

    for entry in &state.structs {
        write_i32(writer, entry.id)?;
        write_i32(writer, entry.data_or_offset)?;
        write_i32(writer, entry.field_count)?;
    }

    for entry in &state.fields {
        write_u32(writer, entry.field_kind as u32)?;
        write_i32(writer, entry.label_index)?;
        write_i32(writer, entry.data_or_offset)?;
    }

    for label in &state.labels {
        ensure_label(label)?;
        let mut padded = [0_u8; 16];
        let bytes = label.as_bytes();
        let slot = padded
            .get_mut(..bytes.len())
            .ok_or_else(|| GffError::msg("GFF label padding overflow"))?;
        slot.copy_from_slice(bytes);
        writer.write_all(&padded)?;
    }

    writer.write_all(&state.field_data)?;

    for value in &state.field_indices {
        write_i32(writer, *value)?;
    }

    for value in &state.list_indices {
        write_i32(writer, *value)?;
    }

    let expected_end = start
        + (HEADER_SIZE as u64)
        + (state.structs.len() as u64 * 12)
        + (state.fields.len() as u64 * 12)
        + (state.labels.len() as u64 * 16)
        + state.field_data.len() as u64
        + (state.field_indices.len() as u64 * 4)
        + (state.list_indices.len() as u64 * 4);
    expect(
        writer.stream_position()? == expected_end,
        "writer length mismatch",
    )?;

    debug!(
        structs = state.structs.len(),
        fields = state.fields.len(),
        "wrote gff root"
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn parse_struct<R: Read + Seek>(
    struct_idx: usize,
    reader: &mut R,
    start: u64,
    header: &Header,
    labels: &[String],
    fields: &[RawFieldEntry],
    field_indices: &[i32],
    list_indices: &[i32],
    structs: &[RawStructEntry],
) -> GffResult<GffStruct> {
    let entry = structs
        .get(struct_idx)
        .ok_or_else(|| GffError::msg(format!("invalid struct index {struct_idx}")))?;

    let field_refs: Vec<usize> = match entry.field_count {
        0 => Vec::new(),
        1 => vec![to_usize(entry.data_or_offset, "struct field index")?],
        count if count > 1 => {
            let start_idx = to_usize(entry.data_or_offset / 4, "field indices offset")?;
            let end_idx = start_idx + to_usize(count, "struct field count")?;
            field_indices
                .get(start_idx..end_idx)
                .ok_or_else(|| GffError::msg("field indices slice out of bounds"))?
                .iter()
                .map(|idx| to_usize(*idx, "field index"))
                .collect::<GffResult<Vec<_>>>()?
        }
        _ => return Err(GffError::msg("negative field count in struct")),
    };

    let mut gff_struct = GffStruct::new(entry.id);

    for field_idx in field_refs {
        let raw_field = fields
            .get(field_idx)
            .ok_or_else(|| GffError::msg(format!("invalid field index {field_idx}")))?;
        let label = labels
            .get(to_usize(raw_field.label_index, "label index")?)
            .ok_or_else(|| GffError::msg("invalid label index"))?
            .clone();

        if gff_struct.get_field(&label).is_some() {
            return Err(GffError::msg(format!("duplicate label in struct: {label}")));
        }

        let field = parse_field(
            raw_field,
            reader,
            start,
            header,
            labels,
            fields,
            field_indices,
            list_indices,
            structs,
        )?;
        gff_struct.put_field(label, field)?;
    }

    Ok(gff_struct)
}

#[allow(clippy::too_many_arguments)]
fn parse_field<R: Read + Seek>(
    raw: &RawFieldEntry,
    reader: &mut R,
    start: u64,
    header: &Header,
    labels: &[String],
    fields: &[RawFieldEntry],
    field_indices: &[i32],
    list_indices: &[i32],
    structs: &[RawStructEntry],
) -> GffResult<GffField> {
    let value = match raw.field_kind {
        GffFieldKind::Byte => GffValue::Byte(
            u8::try_from(raw.data_or_offset)
                .map_err(|_error| GffError::msg("byte field value out of range"))?,
        ),
        GffFieldKind::Char => GffValue::Char(
            i8::try_from(raw.data_or_offset)
                .map_err(|_error| GffError::msg("char field value out of range"))?,
        ),
        GffFieldKind::Word => GffValue::Word(
            u16::try_from(raw.data_or_offset)
                .map_err(|_error| GffError::msg("word field value out of range"))?,
        ),
        GffFieldKind::Short => GffValue::Short(
            i16::try_from(raw.data_or_offset)
                .map_err(|_error| GffError::msg("short field value out of range"))?,
        ),
        GffFieldKind::Dword => {
            GffValue::Dword(u32::from_ne_bytes(raw.data_or_offset.to_ne_bytes()))
        }
        GffFieldKind::Int => GffValue::Int(raw.data_or_offset),
        GffFieldKind::Float => GffValue::Float(f32::from_bits(u32::from_ne_bytes(
            raw.data_or_offset.to_ne_bytes(),
        ))),
        GffFieldKind::Dword64 => {
            seek_field_data(reader, start, header, raw.data_or_offset)?;
            GffValue::Dword64(read_u64(reader)?)
        }
        GffFieldKind::Int64 => {
            seek_field_data(reader, start, header, raw.data_or_offset)?;
            GffValue::Int64(read_i64(reader)?)
        }
        GffFieldKind::Double => {
            seek_field_data(reader, start, header, raw.data_or_offset)?;
            GffValue::Double(f64::from_bits(read_u64(reader)?))
        }
        GffFieldKind::CExoString => {
            seek_field_data(reader, start, header, raw.data_or_offset)?;
            let size = read_i32(reader)?;
            let bytes = read_bytes_or_err(reader, to_usize(size, "CExoString length")?)?;
            let decoded =
                from_nwnrs_encoding(&bytes).map_err(|error| GffError::msg(error.to_string()))?;
            GffValue::CExoString(decoded)
        }
        GffFieldKind::ResRef => {
            seek_field_data(reader, start, header, raw.data_or_offset)?;
            let size = usize::try_from(read_i8(reader)?)
                .map_err(|_error| GffError::msg("negative ResRef length"))?;
            let bytes = read_bytes_or_err(reader, size)?;
            GffValue::ResRef(String::from_utf8_lossy(&bytes).to_string())
        }
        GffFieldKind::CExoLocString => {
            seek_field_data(reader, start, header, raw.data_or_offset)?;
            let total_size = read_i32(reader)?;
            let payload_start = reader.stream_position()?;
            let str_ref = read_u32(reader)?;
            let count = read_i32(reader)?;
            let mut entries = Vec::with_capacity(to_usize(count, "locstring count")?);
            for _ in 0..count {
                let language = read_i32(reader)?;
                let size = read_i32(reader)?;
                let bytes = read_bytes_or_err(reader, to_usize(size, "locstring entry length")?)?;
                let decoded = from_nwnrs_encoding(&bytes)
                    .map_err(|error| GffError::msg(error.to_string()))?;
                entries.push((language, decoded));
            }
            let consumed = reader.stream_position()? - payload_start;
            expect(
                consumed
                    == u64::try_from(total_size)
                        .map_err(|_error| GffError::msg("negative CExoLocString payload size"))?,
                "invalid CExoLocString payload size",
            )?;
            GffValue::CExoLocString(GffCExoLocString { str_ref, entries })
        }
        GffFieldKind::Void => {
            seek_field_data(reader, start, header, raw.data_or_offset)?;
            let size = read_u32(reader)?;
            GffValue::Void(read_bytes_or_err(
                reader,
                usize::try_from(size)
                    .map_err(|_error| GffError::msg("void field size exceeds usize"))?,
            )?)
        }
        GffFieldKind::Struct => GffValue::Struct(parse_struct(
            to_usize(raw.data_or_offset, "struct field offset")?,
            reader,
            start,
            header,
            labels,
            fields,
            field_indices,
            list_indices,
            structs,
        )?),
        GffFieldKind::List => {
            let offset = to_usize(raw.data_or_offset / 4, "list offset")?;
            let count = *list_indices
                .get(offset)
                .ok_or_else(|| GffError::msg("list size offset out of bounds"))?;
            let start_idx = offset + 1;
            let end_idx = start_idx + to_usize(count, "list size")?;
            let list = list_indices
                .get(start_idx..end_idx)
                .ok_or_else(|| GffError::msg("list indices slice out of bounds"))?
                .iter()
                .map(|idx| {
                    parse_struct(
                        to_usize(*idx, "list struct index")?,
                        reader,
                        start,
                        header,
                        labels,
                        fields,
                        field_indices,
                        list_indices,
                        structs,
                    )
                })
                .collect::<GffResult<Vec<_>>>()?;
            GffValue::List(list)
        }
    };

    Ok(GffField::new(value))
}

fn collect_struct(structure: &GffStruct, state: &mut WriteState) -> GffResult<i32> {
    let struct_idx = to_i32_len(state.structs.len(), "GFF struct index")?;
    state.structs.push(WriteStructEntry {
        id: structure.id,
        ..WriteStructEntry::default()
    });

    let mut struct_field_ids = Vec::new();
    for (label, field) in structure.fields() {
        ensure_label(label)?;
        let label_index = to_i32_len(
            get_or_insert_label(label, &mut state.labels),
            "GFF label index",
        )?;
        let data_or_offset = match field.value() {
            GffValue::Byte(value) => i32::from(*value),
            GffValue::Char(value) => i32::from(*value),
            GffValue::Word(value) => i32::from(*value),
            GffValue::Short(value) => i32::from(*value),
            GffValue::Dword(value) => i32::from_ne_bytes(value.to_ne_bytes()),
            GffValue::Int(value) => *value,
            GffValue::Float(value) => i32::from_ne_bytes(value.to_bits().to_ne_bytes()),
            GffValue::Dword64(value) => {
                let offset = to_i32_len(state.field_data.len(), "GFF field data offset")?;
                state.field_data.extend_from_slice(&value.to_le_bytes());
                offset
            }
            GffValue::Int64(value) => {
                let offset = to_i32_len(state.field_data.len(), "GFF field data offset")?;
                state.field_data.extend_from_slice(&value.to_le_bytes());
                offset
            }
            GffValue::Double(value) => {
                let offset = to_i32_len(state.field_data.len(), "GFF field data offset")?;
                state
                    .field_data
                    .extend_from_slice(&value.to_bits().to_le_bytes());
                offset
            }
            GffValue::CExoString(value) => {
                let offset = to_i32_len(state.field_data.len(), "GFF field data offset")?;
                let encoded =
                    to_nwnrs_encoding(value).map_err(|error| GffError::msg(error.to_string()))?;
                state.field_data.extend_from_slice(
                    &to_i32_len(encoded.len(), "CExoString length")?.to_le_bytes(),
                );
                state.field_data.extend_from_slice(&encoded);
                offset
            }
            GffValue::ResRef(value) => {
                let offset = to_i32_len(state.field_data.len(), "GFF field data offset")?;
                expect(value.len() <= u8::MAX as usize, "ResRef too long for GFF")?;
                state.field_data.push(
                    u8::try_from(value.len())
                        .map_err(|_error| GffError::msg("ResRef too long for GFF"))?,
                );
                state.field_data.extend_from_slice(value.as_bytes());
                offset
            }
            GffValue::CExoLocString(value) => {
                let offset = to_i32_len(state.field_data.len(), "GFF field data offset")?;
                let mut payload = Vec::new();
                for (language, text) in &value.entries {
                    let encoded = to_nwnrs_encoding(text)
                        .map_err(|error| GffError::msg(error.to_string()))?;
                    payload.extend_from_slice(&language.to_le_bytes());
                    payload.extend_from_slice(
                        &to_i32_len(encoded.len(), "CExoLocString entry length")?.to_le_bytes(),
                    );
                    payload.extend_from_slice(&encoded);
                }
                state.field_data.extend_from_slice(
                    &to_i32_len(payload.len() + 8, "CExoLocString payload size")?.to_le_bytes(),
                );
                state
                    .field_data
                    .extend_from_slice(&value.str_ref.to_le_bytes());
                state.field_data.extend_from_slice(
                    &to_i32_len(value.entries.len(), "CExoLocString entry count")?.to_le_bytes(),
                );
                state.field_data.extend_from_slice(&payload);
                offset
            }
            GffValue::Void(value) => {
                let offset = to_i32_len(state.field_data.len(), "GFF field data offset")?;
                state
                    .field_data
                    .extend_from_slice(&to_u32_len(value.len(), "void length")?.to_le_bytes());
                state.field_data.extend_from_slice(value);
                offset
            }
            GffValue::Struct(child) => collect_struct(child, state)?,
            GffValue::List(list) => {
                let mut child_indices = Vec::with_capacity(list.len());
                for child in list {
                    child_indices.push(collect_struct(child, state)?);
                }
                let offset = to_i32_len(
                    state
                        .list_indices
                        .len()
                        .checked_mul(4)
                        .ok_or_else(|| GffError::msg("GFF list indices size overflow"))?,
                    "GFF list offset",
                )?;
                state
                    .list_indices
                    .push(to_i32_len(child_indices.len(), "GFF list size")?);
                state.list_indices.extend(child_indices);
                offset
            }
        };

        let field_idx = to_i32_len(state.fields.len(), "GFF field index")?;
        state.fields.push(WriteFieldEntry {
            field_kind: field.kind(),
            label_index,
            data_or_offset,
        });
        struct_field_ids.push(field_idx);
    }

    let entry = state
        .structs
        .get_mut(to_usize(struct_idx, "GFF struct index")?)
        .ok_or_else(|| GffError::msg("GFF struct entry out of range"))?;
    entry.field_count = to_i32_len(struct_field_ids.len(), "GFF struct field count")?;
    entry.data_or_offset = match struct_field_ids.len() {
        0 => 0,
        1 => *struct_field_ids
            .first()
            .ok_or_else(|| GffError::msg("missing GFF struct field index"))?,
        _ => {
            let offset = to_i32_len(
                state
                    .field_indices
                    .len()
                    .checked_mul(4)
                    .ok_or_else(|| GffError::msg("GFF field indices size overflow"))?,
                "GFF field indices offset",
            )?;
            state.field_indices.extend(struct_field_ids);
            offset
        }
    };

    Ok(struct_idx)
}

fn get_or_insert_label(label: &str, labels: &mut Vec<String>) -> usize {
    if let Some(idx) = labels.iter().position(|existing| existing == label) {
        idx
    } else {
        labels.push(label.to_string());
        labels.len() - 1
    }
}

fn read_labels<R: Read + Seek>(
    reader: &mut R,
    start: u64,
    header: &Header,
) -> GffResult<Vec<String>> {
    reader.seek(SeekFrom::Start(start + u64::from(header.label_offset)))?;
    (0..header.label_count)
        .map(|_| {
            let bytes = read_bytes_or_err(reader, 16)?;
            Ok(trim_trailing_nuls(&bytes))
        })
        .collect()
}

fn read_field_entries<R: Read + Seek>(
    reader: &mut R,
    start: u64,
    header: &Header,
) -> GffResult<Vec<RawFieldEntry>> {
    reader.seek(SeekFrom::Start(start + u64::from(header.field_offset)))?;
    (0..header.field_count)
        .map(|_| {
            let kind = read_u32(reader)?;
            let field_kind = GffFieldKind::from_u32(kind)
                .ok_or_else(|| GffError::msg(format!("invalid GFF field kind {kind}")))?;
            Ok(RawFieldEntry {
                field_kind,
                label_index: read_i32(reader)?,
                data_or_offset: read_i32(reader)?,
            })
        })
        .collect()
}

fn read_struct_entries<R: Read + Seek>(
    reader: &mut R,
    start: u64,
    header: &Header,
) -> GffResult<Vec<RawStructEntry>> {
    reader.seek(SeekFrom::Start(start + u64::from(header.struct_offset)))?;
    (0..header.struct_count)
        .map(|_| {
            Ok(RawStructEntry {
                id: read_i32(reader)?,
                data_or_offset: read_i32(reader)?,
                field_count: read_i32(reader)?,
            })
        })
        .collect()
}

fn read_i32_array<R: Read + Seek>(
    reader: &mut R,
    offset: u64,
    size_in_bytes: u32,
) -> GffResult<Vec<i32>> {
    reader.seek(SeekFrom::Start(offset))?;
    let count = usize::try_from(size_in_bytes)
        .map_err(|_error| GffError::msg("GFF i32 array size exceeds usize"))?
        / 4;
    (0..count)
        .map(|_| read_i32(reader).map_err(GffError::from))
        .collect()
}

fn seek_field_data<R: Seek>(
    reader: &mut R,
    start: u64,
    header: &Header,
    data_or_offset: i32,
) -> GffResult<()> {
    let offset = to_usize(data_or_offset, "field data offset")?;
    expect(
        usize::try_from(header.field_data_size)
            .ok()
            .is_some_and(|field_data_size| offset < field_data_size),
        "field data offset out of range",
    )?;
    reader.seek(SeekFrom::Start(
        start + u64::from(header.field_data_offset) + offset as u64,
    ))?;
    Ok(())
}

fn trim_trailing_nuls(bytes: &[u8]) -> String {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    String::from_utf8_lossy(bytes.get(..end).unwrap_or(&[])).to_string()
}

fn to_usize(value: i32, what: &str) -> GffResult<usize> {
    usize::try_from(value).map_err(|_error| GffError::msg(format!("negative {what}: {value}")))
}

fn to_i32_len(value: usize, what: &str) -> GffResult<i32> {
    i32::try_from(value).map_err(|_error| GffError::msg(format!("{what} exceeds 32-bit range")))
}

fn to_u32_len(value: usize, what: &str) -> GffResult<u32> {
    u32::try_from(value).map_err(|_error| GffError::msg(format!("{what} exceeds 32-bit range")))
}

fn read_i8<R: Read>(reader: &mut R) -> io::Result<i8> {
    let mut bytes = [0_u8; 1];
    reader.read_exact(&mut bytes)?;
    Ok(bytes[0] as i8)
}

fn read_u32<R: Read>(reader: &mut R) -> io::Result<u32> {
    let mut bytes = [0_u8; 4];
    reader.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_i32<R: Read>(reader: &mut R) -> io::Result<i32> {
    let mut bytes = [0_u8; 4];
    reader.read_exact(&mut bytes)?;
    Ok(i32::from_le_bytes(bytes))
}

fn read_u64<R: Read>(reader: &mut R) -> io::Result<u64> {
    let mut bytes = [0_u8; 8];
    reader.read_exact(&mut bytes)?;
    Ok(u64::from_le_bytes(bytes))
}

fn read_i64<R: Read>(reader: &mut R) -> io::Result<i64> {
    let mut bytes = [0_u8; 8];
    reader.read_exact(&mut bytes)?;
    Ok(i64::from_le_bytes(bytes))
}

fn write_u32<W: Write>(writer: &mut W, value: u32) -> io::Result<()> {
    writer.write_all(&value.to_le_bytes())
}

fn write_i32<W: Write>(writer: &mut W, value: i32) -> io::Result<()> {
    writer.write_all(&value.to_le_bytes())
}
