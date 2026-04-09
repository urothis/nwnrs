use std::io::{self, Read, Seek, SeekFrom, Write};

use nwnrs_util::prelude::*;
use tracing::{debug, instrument};

use crate::{ENTRY_DATA_SIZE, HEADER_MAGIC, HEADER_VERSION, TABLE_OFFSET, decode_resref, prelude::*};

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
        let mut raw_resref_bytes = [0_u8; 16];
        raw_resref_bytes.copy_from_slice(&raw_resref);

        Ok(SsfEntry {
            raw_resref: raw_resref_bytes,
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
        writer.write_all(&entry.stored_resref_bytes()?)?;
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

fn invalid_data(error: impl std::error::Error + Send + Sync + 'static) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}

fn invalid_message(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn to_u32(value: usize, what: &str) -> io::Result<u32> {
    u32::try_from(value).map_err(|_error| invalid_message(format!("{what} exceeds 32-bit range")))
}

#[allow(clippy::panic)]
#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use crate::{SsfEntry, new_ssf, read_ssf, write_ssf};

    #[test]
    fn ssf_preserves_raw_resref_bytes_when_only_strref_changes() {
        let mut original = Vec::new();
        original.extend_from_slice(b"SSF ");
        original.extend_from_slice(b"V1.0");
        original.extend_from_slice(&1_u32.to_le_bytes());
        original.extend_from_slice(&40_u32.to_le_bytes());
        original.extend_from_slice(&[0_u8; 24]);
        original.extend_from_slice(&44_u32.to_le_bytes());
        let mut raw = [0_u8; 16];
        if let Some(prefix) = raw.get_mut(..5) {
            prefix.copy_from_slice(b"HELLO");
        } else {
            panic!("fixture resref slice should be in bounds");
        }
        original.extend_from_slice(&raw);
        original.extend_from_slice(&7_u32.to_le_bytes());

        let mut cursor = Cursor::new(original.clone());
        let mut ssf = match read_ssf(&mut cursor) {
            Ok(ssf) => ssf,
            Err(error) => panic!("read ssf: {error}"),
        };
        if let Some(entry) = ssf.entries.get_mut(0) {
            entry.strref = 9;
        } else {
            panic!("fixture should contain one SSF entry");
        }

        let mut encoded = Vec::new();
        if let Err(error) = write_ssf(&mut encoded, &ssf) {
            panic!("write ssf: {error}");
        }

        assert_eq!(
            encoded.get(..44),
            original.get(..44),
            "header prefix should exist"
        );
        assert_eq!(
            encoded.get(44..60),
            original.get(44..60),
            "resref should exist"
        );
        assert_eq!(encoded.get(60..64), Some(&9_u32.to_le_bytes()[..]));
    }

    #[test]
    fn ssf_new_entry_uses_canonical_padding() {
        let mut ssf = new_ssf();
        ssf.entries.push(SsfEntry::new("hello", 7));

        let mut encoded = Vec::new();
        if let Err(error) = write_ssf(&mut encoded, &ssf) {
            panic!("write ssf: {error}");
        }

        assert_eq!(encoded.get(44..49), Some(&b"hello"[..]));
        assert!(
            encoded
                .get(49..60)
                .unwrap_or(&[])
                .iter()
                .all(|byte| *byte == 0)
        );
    }
}
