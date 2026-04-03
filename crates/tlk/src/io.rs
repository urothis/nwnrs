use std::io::{self, Cursor, Read, Seek, SeekFrom, Write};

use nwn_core::{Language, StrRef};
use nwn_lru::WeightedLru;
use nwn_resman::{Res, shared_stream};
use nwn_util::{from_nwn_encoding, read_bytes_or_err, to_nwn_encoding};
use tracing::{debug, instrument};

use crate::{DATA_ELEMENT_SIZE, HEADER_SIZE, SingleTlk, TlkEntry, TlkError, TlkResult};

/// Reads a single-language TLK table from a reader.
#[instrument(level = "debug", skip_all, err, fields(use_cache))]
pub fn read_single_tlk<R>(mut reader: R, use_cache: bool) -> TlkResult<SingleTlk>
where
    R: Read + Seek + Send + 'static,
{
    let start = reader.stream_position()?;
    let stream = shared_stream(reader);
    let mut locked = stream
        .lock()
        .map_err(|error| TlkError::msg(format!("tlk stream lock poisoned: {error}")))?;

    expect_header(&mut *locked, b"TLK ")?;
    expect_header(&mut *locked, b"V3.0")?;
    let language_id = u32::try_from(read_i32(&mut *locked)?)
        .map_err(|_error| TlkError::msg("invalid negative tlk language id"))?;
    let language = Language::from_id(language_id)
        .ok_or_else(|| TlkError::msg(format!("invalid tlk language id {}", language_id)))?;
    let entry_count = usize::try_from(read_i32(&mut *locked)?)
        .map_err(|_error| TlkError::msg("invalid negative tlk entry count"))?;
    let entries_offset = u64::try_from(read_i32(&mut *locked)?)
        .map_err(|_error| TlkError::msg("invalid negative tlk entries offset"))?;
    drop(locked);

    let mut result = SingleTlk::new();
    result.language = language;
    result.stream = Some(stream);
    result.io_start_pos = start;
    result.io_entry_count = entry_count;
    result.io_entries_offset = entries_offset;
    result.use_cache = use_cache;
    result.io_cache = Some(WeightedLru::new(
        std::mem::size_of::<TlkEntry>() * entry_count.max(1) / 2,
        1,
    ));
    debug!(entry_count = result.io_entry_count, language = ?result.language, "read tlk");
    Ok(result)
}

/// Writes a single-language TLK table.
///
/// Missing string references up to [`SingleTlk::highest`] are emitted as empty
/// entries.
#[instrument(level = "debug", skip_all, err, fields(language = ?tlk.language))]
pub fn write_single_tlk<W: Write + Seek>(writer: &mut W, tlk: &mut SingleTlk) -> TlkResult<()> {
    let max_id = u32::try_from(tlk.highest().max(0))
        .map_err(|_error| TlkError::msg("TLK highest string reference exceeds 32-bit range"))?;
    let entry_count = max_id + 1;
    let entries_table_offset = writer.stream_position()? + HEADER_SIZE;
    let entries_table_size = DATA_ELEMENT_SIZE * u64::from(entry_count);
    let string_data_offset = entries_table_offset + entries_table_size;

    writer.write_all(b"TLK ")?;
    writer.write_all(b"V3.0")?;
    write_i32(
        writer,
        i32::try_from(tlk.language.id())
            .map_err(|_error| TlkError::msg("TLK language id exceeds 32-bit range"))?,
    )?;
    write_u32(writer, entry_count)?;
    write_u32(
        writer,
        u32::try_from(string_data_offset)
            .map_err(|_error| TlkError::msg("TLK string data offset exceeds 32-bit range"))?,
    )?;

    let current_pos = writer.stream_position()?;
    if current_pos < string_data_offset {
        let padding_len = usize::try_from(string_data_offset - current_pos)
            .map_err(|_error| TlkError::msg("TLK padding length exceeds usize"))?;
        writer.write_all(&vec![0_u8; padding_len])?;
    }

    let entries_capacity = usize::try_from(entries_table_size)
        .map_err(|_error| TlkError::msg("TLK entries table exceeds usize"))?;
    let mut entries_table = Cursor::new(Vec::with_capacity(entries_capacity));
    let mut offset = 0_i32;
    for index in 0..entry_count {
        if let Some(entry) = tlk.get(index)?.filter(TlkEntry::has_value) {
            let mut flags = 0;
            if !entry.text.is_empty() {
                flags += 0x1;
            }
            if !entry.sound_res_ref.is_empty() {
                flags += 0x6;
            }

            write_i32(&mut entries_table, flags)?;
            let mut sound_res_ref = entry.sound_res_ref.chars().take(16).collect::<String>();
            while sound_res_ref.len() < 16 {
                sound_res_ref.push('\0');
            }
            entries_table.write_all(sound_res_ref.as_bytes())?;
            write_i32(&mut entries_table, 0)?;
            write_i32(&mut entries_table, 0)?;

            let text = to_nwn_encoding(&entry.text.replace('\r', ""))?;
            write_i32(&mut entries_table, offset)?;
            let text_len = i32::try_from(text.len())
                .map_err(|_error| TlkError::msg("TLK text length exceeds 32-bit range"))?;
            write_i32(&mut entries_table, text_len)?;
            offset = offset
                .checked_add(text_len)
                .ok_or_else(|| TlkError::msg("TLK text offset overflow"))?;
            write_f32(&mut entries_table, entry.sound_length)?;

            writer.write_all(&text)?;
        } else {
            write_i32(&mut entries_table, 0)?;
            entries_table.write_all(&[0_u8; 16])?;
            write_i32(&mut entries_table, 0)?;
            write_i32(&mut entries_table, 0)?;
            write_i32(&mut entries_table, 0)?;
            write_i32(&mut entries_table, 0)?;
            write_f32(&mut entries_table, 0.0)?;
        }
    }

    writer.seek(SeekFrom::Start(entries_table_offset))?;
    entries_table.set_position(0);
    writer.write_all(entries_table.get_ref())?;
    debug!(entry_count, "wrote tlk");
    Ok(())
}

/// Reads a single-language TLK table from a [`Res`].
#[instrument(level = "debug", skip_all, err, fields(use_cache))]
pub fn read_single_tlk_from_res(res: &Res, use_cache: bool) -> TlkResult<SingleTlk> {
    SingleTlk::from_res(res, use_cache)
}

pub(crate) fn get_from_io(tlk: &SingleTlk, str_ref: StrRef) -> TlkResult<(usize, TlkEntry)> {
    let stream = tlk
        .stream
        .as_ref()
        .ok_or_else(|| TlkError::msg("tlk is not stream-backed"))?;
    let mut stream = stream
        .lock()
        .map_err(|error| TlkError::msg(format!("tlk stream lock poisoned: {error}")))?;

    stream.seek(SeekFrom::Start(
        tlk.io_start_pos + HEADER_SIZE + DATA_ELEMENT_SIZE * u64::from(str_ref),
    ))?;
    let _flags = read_i32(stream.as_mut())?;
    let sound_res_ref = trim_sound_resref(&read_bytes_or_err(stream.as_mut(), 16)?);
    let _volume_variance = read_i32(stream.as_mut())?;
    let _pitch_variance = read_i32(stream.as_mut())?;
    let offset_to_string = u64::try_from(read_i32(stream.as_mut())?)
        .map_err(|_error| TlkError::msg("invalid negative tlk string offset"))?;
    let string_size = usize::try_from(read_i32(stream.as_mut())?)
        .map_err(|_error| TlkError::msg("invalid negative tlk string size"))?;
    let sound_length = round4(read_f32(stream.as_mut())?).max(0.0);

    stream.seek(SeekFrom::Start(
        tlk.io_start_pos + tlk.io_entries_offset + offset_to_string,
    ))?;
    let text = from_nwn_encoding(&read_bytes_or_err(stream.as_mut(), string_size)?)?;
    let entry = TlkEntry {
        text,
        sound_res_ref,
        sound_length,
    };
    let weight = std::mem::size_of::<TlkEntry>() + entry.sound_res_ref.len() + entry.text.len();

    Ok((weight, entry))
}

fn trim_sound_resref(bytes: &[u8]) -> String {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    String::from_utf8_lossy(bytes.get(..end).unwrap_or(&[]))
        .trim_matches(|ch: char| ch == '\u{00c0}' || ch.is_ascii_whitespace())
        .to_string()
}

fn round4(value: f32) -> f32 {
    (value * 10_000.0).round() / 10_000.0
}

fn expect_header<R: Read + ?Sized>(reader: &mut R, expected: &[u8]) -> TlkResult<()> {
    let actual = read_bytes_or_err(reader, expected.len())?;
    if actual == expected {
        Ok(())
    } else {
        Err(TlkError::msg(format!(
            "invalid tlk header: expected {:?}, got {:?}",
            String::from_utf8_lossy(expected),
            String::from_utf8_lossy(&actual)
        )))
    }
}

fn read_i32<R: Read + ?Sized>(reader: &mut R) -> io::Result<i32> {
    let mut bytes = [0_u8; 4];
    reader.read_exact(&mut bytes)?;
    Ok(i32::from_le_bytes(bytes))
}

fn read_f32<R: Read + ?Sized>(reader: &mut R) -> io::Result<f32> {
    let mut bytes = [0_u8; 4];
    reader.read_exact(&mut bytes)?;
    Ok(f32::from_le_bytes(bytes))
}

fn write_i32<W: Write + ?Sized>(writer: &mut W, value: i32) -> io::Result<()> {
    writer.write_all(&value.to_le_bytes())
}

fn write_u32<W: Write + ?Sized>(writer: &mut W, value: u32) -> io::Result<()> {
    writer.write_all(&value.to_le_bytes())
}

fn write_f32<W: Write + ?Sized>(writer: &mut W, value: f32) -> io::Result<()> {
    writer.write_all(&value.to_le_bytes())
}
