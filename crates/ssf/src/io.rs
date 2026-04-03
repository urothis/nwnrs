use crate::{ENTRY_DATA_SIZE, HEADER_MAGIC, HEADER_VERSION, SsfEntry, SsfRoot, TABLE_OFFSET};
use nwn_util::{expect, read_bytes_or_err, read_fixed_count_seq, read_str_or_err};
use std::io::{self, Read, Seek, SeekFrom, Write};
use tracing::{debug, instrument};

/// Reads an `SSF` document from `reader`.
#[instrument(level = "debug", skip_all, err)]
pub fn read_ssf<R: Read + Seek>(reader: &mut R) -> io::Result<SsfRoot> {
    let file_type = read_str_or_err(reader, 4)?;
    expect(
        file_type == HEADER_MAGIC,
        format!("expected {HEADER_MAGIC:?}, got {file_type:?}"),
    )
    .map_err(invalid_data)?;

    let file_version = read_str_or_err(reader, 4)?;
    expect(
        file_version == HEADER_VERSION,
        format!("expected {HEADER_VERSION:?}, got {file_version:?}"),
    )
    .map_err(invalid_data)?;

    let entry_count = read_u32(reader)? as usize;
    let table_offset = read_u32(reader)?;
    expect(
        table_offset == TABLE_OFFSET,
        format!("expected table offset {TABLE_OFFSET}, got {table_offset}"),
    )
    .map_err(invalid_data)?;

    let padding = read_bytes_or_err(reader, 24)?;
    expect(
        padding.iter().all(|byte| *byte == 0),
        "expected 24 bytes of zero padding",
    )
    .map_err(invalid_data)?;

    let entry_offsets = read_fixed_count_seq(reader, entry_count, |_, reader| read_u32(reader))?;
    let entries = read_fixed_count_seq(reader, entry_count, |idx, reader| {
        let offset = entry_offsets
            .get(idx)
            .copied()
            .ok_or_else(|| invalid_message("SSF entry offset index out of range"))?;
        reader.seek(SeekFrom::Start(u64::from(offset)))?;
        let raw_resref = read_bytes_or_err(reader, 16)?;
        let strref = read_u32(reader)?;

        Ok(SsfEntry {
            resref: decode_resref(&raw_resref),
            strref,
        })
    })?;

    let root = SsfRoot { entries };
    debug!(entry_count = root.entries.len(), "read ssf");
    Ok(root)
}

/// Writes an `SSF` document to `writer`.
#[instrument(level = "debug", skip_all, err, fields(entry_count = ssf.entries.len()))]
pub fn write_ssf<W: Write>(writer: &mut W, ssf: &SsfRoot) -> io::Result<()> {
    writer.write_all(HEADER_MAGIC.as_bytes())?;
    writer.write_all(HEADER_VERSION.as_bytes())?;
    writer.write_all(&to_u32(ssf.entries.len(), "SSF entry count")?.to_le_bytes())?;
    writer.write_all(&TABLE_OFFSET.to_le_bytes())?;
    writer.write_all(&[0_u8; 24])?;

    for (idx, _) in ssf.entries.iter().enumerate() {
        let table_offset = usize::try_from(TABLE_OFFSET)
            .map_err(|_error| invalid_message("SSF table offset exceeds usize"))?;
        let offset = ssf
            .entries
            .len()
            .checked_mul(4)
            .and_then(|value| value.checked_add(table_offset))
            .and_then(|value| value.checked_add(idx.saturating_mul(ENTRY_DATA_SIZE)))
            .ok_or_else(|| invalid_message("SSF entry offset overflow"))?;
        let offset = to_u32(offset, "SSF entry offset")?;
        writer.write_all(&offset.to_le_bytes())?;
    }

    for entry in &ssf.entries {
        expect(
            entry.resref.len() <= 16,
            format!("resref {:?} exceeds 16 bytes", entry.resref),
        )
        .map_err(invalid_data)?;

        let mut padded = [0_u8; 16];
        let bytes = entry.resref.as_bytes();
        let slot = padded
            .get_mut(..bytes.len())
            .ok_or_else(|| invalid_message("SSF resref padding overflow"))?;
        slot.copy_from_slice(bytes);
        writer.write_all(&padded)?;
        writer.write_all(&entry.strref.to_le_bytes())?;
    }

    debug!(entry_count = ssf.entries.len(), "wrote ssf");
    Ok(())
}

fn read_u32<R: Read>(reader: &mut R) -> io::Result<u32> {
    let mut bytes = [0_u8; 4];
    reader.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

fn decode_resref(raw: &[u8]) -> String {
    let end = raw.iter().position(|byte| *byte == 0).unwrap_or(raw.len());
    String::from_utf8_lossy(raw.get(..end).unwrap_or(&[])).to_string()
}

fn invalid_data(error: impl std::error::Error + Send + Sync + 'static) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}

fn invalid_message(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn to_u32(value: usize, what: &str) -> io::Result<u32> {
    u32::try_from(value).map_err(|_error| invalid_message(format!("{what} exceeds 32-bit range")))
}
