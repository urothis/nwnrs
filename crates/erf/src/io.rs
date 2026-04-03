use crate::{Erf, ErfError, ErfResMeta, ErfResult, ErfVersion, HEADER_SIZE, VALID_ERF_TYPES};
use nwn_checksums::{EMPTY_SECURE_HASH, SecureHash};
use nwn_compressedbuf::{Algorithm, compress_writer as compress_buf_writer};
use nwn_exo::{EXO_RES_FILE_COMPRESSED_BUF_MAGIC, ExoResFileCompressionType};
use nwn_resman::{Res, SharedReadSeek, new_res_origin, shared_stream};
use nwn_resref::{ResRef, new_res_ref};
use nwn_util::{from_nwn_encoding, read_bytes_or_err, to_nwn_encoding};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::time::SystemTime;
use tracing::{debug, instrument};

/// Reads an ERF-family archive from a seekable reader.
///
/// The returned [`Erf`] contains lazily readable [`nwn_resman::Res`] entries backed by the
/// supplied stream.
#[instrument(level = "debug", skip_all, err)]
pub fn read_erf<R>(reader: R, filename: impl Into<String>) -> ErfResult<Erf>
where
    R: Read + Seek + Send + 'static,
{
    read_erf_shared(shared_stream(reader), filename.into())
}

/// Opens a file from disk and reads it as an ERF-family archive.
#[instrument(level = "debug", skip_all, err, fields(path = %path.as_ref().display()))]
pub fn read_erf_from_file(path: impl AsRef<Path>) -> ErfResult<Erf> {
    let path = path.as_ref();
    let file = File::open(path)?;
    read_erf(file, path.display().to_string())
}

/// Reads an ERF-family archive from a shared stream handle.
///
/// This is the most direct constructor when the caller already manages stream sharing.
#[instrument(level = "debug", skip_all, err, fields(path = %filename))]
pub fn read_erf_shared(stream: SharedReadSeek, filename: String) -> ErfResult<Erf> {
    let mut io = stream
        .lock()
        .map_err(|error| ErfError::msg(format!("erf stream lock poisoned: {error}")))?;
    io.seek(SeekFrom::Start(0))?;

    let file_type = read_fixed_string(io.as_mut(), 4)?;
    let file_version = match read_fixed_string(io.as_mut(), 4)?.as_str() {
        "V1.0" => ErfVersion::V1,
        "E1.0" => ErfVersion::E1,
        other => return Err(ErfError::msg(format!("unsupported erf version: {other}"))),
    };

    let loc_str_count = read_i32(io.as_mut())? as usize;
    let loc_string_size = read_i32(io.as_mut())? as u64;
    let entry_count = read_i32(io.as_mut())? as usize;
    let offset_to_loc_str = read_i32(io.as_mut())? as u64;
    let offset_to_key_list = read_i32(io.as_mut())? as u64;
    let offset_to_resource_list = read_i32(io.as_mut())? as u64;
    let build_year = read_i32(io.as_mut())?;
    let build_day = read_i32(io.as_mut())?;
    let str_ref = read_i32(io.as_mut())?;
    let oid = match file_version {
        ErfVersion::V1 => {
            io.seek(SeekFrom::Current(116))?;
            None
        }
        ErfVersion::E1 => {
            let oid = read_fixed_string(io.as_mut(), 24)?;
            io.seek(SeekFrom::Current(92))?;
            Some(normalize_oid(&oid)?)
        }
    };

    let mut loc_strings = BTreeMap::new();
    io.seek(SeekFrom::Start(offset_to_loc_str))?;
    for _ in 0..loc_str_count {
        let id = read_i32(io.as_mut())?;
        let len = read_i32(io.as_mut())? as usize;
        let bytes = read_bytes_or_err(io.as_mut(), len)?;
        loc_strings.insert(id, from_nwn_encoding(&bytes)?);
    }

    let _is_known_erf_type = VALID_ERF_TYPES.contains(&file_type.as_str());

    let offset_to_resource_list = normalize_resource_list_offset(
        io.as_mut(),
        offset_to_resource_list,
        entry_count,
        file_version,
    )?;
    io.seek(SeekFrom::Start(offset_to_resource_list))?;
    let mut resources = Vec::with_capacity(entry_count);
    for _ in 0..entry_count {
        let offset = u64::from(read_u32(io.as_mut())?);
        let disk_size = read_u32(io.as_mut())? as usize;
        let (compression, uncompressed_size) = match file_version {
            ErfVersion::V1 => (ExoResFileCompressionType::None, disk_size),
            ErfVersion::E1 => {
                let compression = ExoResFileCompressionType::from_u32(read_u32(io.as_mut())?)
                    .ok_or_else(|| ErfError::msg("invalid erf compression type"))?;
                let uncompressed_size = read_u32(io.as_mut())? as usize;
                (compression, uncompressed_size)
            }
        };

        resources.push(ErfResMeta {
            offset,
            disk_size,
            uncompressed_size,
            compression,
        });
    }

    let origin_container = format!("Erf:{filename}");
    let mut entries: indexmap::IndexMap<ResRef, Res> = indexmap::IndexMap::new();
    io.seek(SeekFrom::Start(offset_to_key_list))?;
    for (index, meta) in resources.iter().enumerate().take(entry_count) {
        let res_ref_raw = trim_trailing_nuls(&read_bytes_or_err(io.as_mut(), 16)?);
        let _id = read_i32(io.as_mut())?;
        let res_type = read_u16(io.as_mut())?;
        io.seek(SeekFrom::Current(2))?;
        if res_type == u16::MAX {
            continue;
        }

        let sha1 = if file_version == ErfVersion::E1 {
            read_secure_hash(io.as_mut())?
        } else {
            EMPTY_SECURE_HASH
        };

        let mut rr = match new_res_ref(res_ref_raw, nwn_restype::ResType(res_type)) {
            Ok(rr) => rr,
            Err(_) => new_res_ref(format!("invalid_{index}"), nwn_restype::ResType(res_type))?,
        };

        if let Some(existing) = entries.get(&rr) {
            if existing.io_offset() == meta.offset && existing.io_size() == meta.disk_size as i64 {
                continue;
            }
            rr = new_res_ref(format!("__erfdup__{index}"), nwn_restype::ResType(res_type))?;
        }

        let res = Res::new_with_stream(
            new_res_origin(origin_container.clone(), format!("{filename}: {rr}")),
            rr.clone(),
            SystemTime::UNIX_EPOCH,
            stream.clone(),
            meta.disk_size as i64,
            meta.offset,
            meta.compression,
            meta.uncompressed_size,
            sha1,
        );
        entries.insert(rr, res);
    }

    drop(io);

    let _has_oversized_loc_table =
        offset_to_loc_str + loc_string_size > HEADER_SIZE && entry_count == 0;

    let erf = Erf {
        mtime: SystemTime::UNIX_EPOCH,
        file_type,
        file_version,
        filename,
        build_year,
        build_day,
        str_ref,
        loc_strings,
        entries,
        oid,
    };
    debug!(entry_count = erf.entries.len(), file_type = %erf.file_type, "read erf archive");
    Ok(erf)
}

fn normalize_resource_list_offset<R: Read + Seek + ?Sized>(
    io: &mut R,
    declared_offset: u64,
    entry_count: usize,
    file_version: ErfVersion,
) -> ErfResult<u64> {
    if entry_count == 0 {
        return Ok(declared_offset);
    }

    let file_len = io.seek(SeekFrom::End(0))?;
    let entry_size = match file_version {
        ErfVersion::V1 => 8_u64,
        ErfVersion::E1 => 16_u64,
    };
    let probe_count = entry_count.min(5);

    if resource_table_offset_looks_valid(io, declared_offset, probe_count, entry_size, file_len)? {
        return Ok(declared_offset);
    }

    let mut candidates = Vec::new();
    for delta in -4_i64..=4_i64 {
        let Some(candidate) = declared_offset.checked_add_signed(delta) else {
            continue;
        };
        if resource_table_offset_looks_valid(io, candidate, probe_count, entry_size, file_len)? {
            candidates.push(candidate);
        }
    }

    if candidates.len() == 1 {
        return Ok(candidates.first().copied().unwrap_or(declared_offset));
    }

    Ok(declared_offset)
}

fn resource_table_offset_looks_valid<R: Read + Seek + ?Sized>(
    io: &mut R,
    offset: u64,
    probe_count: usize,
    entry_size: u64,
    file_len: u64,
) -> ErfResult<bool> {
    if offset + (probe_count as u64 * entry_size) > file_len {
        return Ok(false);
    }

    io.seek(SeekFrom::Start(offset))?;
    let mut previous_offset = 0_u64;
    for idx in 0..probe_count {
        let resource_offset = u64::from(read_u32(io)?);
        let resource_size = u64::from(read_u32(io)?);
        if idx > 0 && resource_offset < previous_offset {
            return Ok(false);
        }
        if resource_offset == 0 || resource_size == 0 || resource_offset + resource_size > file_len
        {
            return Ok(false);
        }
        if entry_size == 16 {
            io.seek(SeekFrom::Current(8))?;
        }
        previous_offset = resource_offset;
    }

    Ok(true)
}

#[allow(clippy::too_many_arguments)]
/// Writes an ERF-family archive.
///
/// `entries` defines the archive order. For each entry, `entry_writer` must write the raw
/// payload bytes and return the uncompressed byte length together with the payload SHA-1.
#[instrument(
    level = "debug",
    skip_all,
    err,
    fields(file_type, version = ?file_version, entry_count = entries.len())
)]
pub fn write_erf<W, F>(
    writer: &mut W,
    file_type: &str,
    file_version: ErfVersion,
    build_year: u32,
    build_day: u32,
    exocomp: ExoResFileCompressionType,
    compalg: Algorithm,
    loc_strings: &BTreeMap<i32, String>,
    str_ref: i32,
    entries: &[ResRef],
    erf_oid: Option<&str>,
    mut entry_writer: F,
) -> ErfResult<()>
where
    W: Write + Seek,
    F: FnMut(&ResRef, &mut dyn Write) -> ErfResult<(usize, SecureHash)>,
{
    if exocomp != ExoResFileCompressionType::None && file_version != ErfVersion::E1 {
        return Err(ErfError::msg("Compression requires E1"));
    }

    let mut encoded_loc_strings = Vec::with_capacity(loc_strings.len());
    let mut loc_string_size = 0_u64;
    for (id, text) in loc_strings {
        let encoded = to_nwn_encoding(text)?;
        loc_string_size += 8 + u64::try_from(encoded.len())
            .map_err(|_error| ErfError::msg("localized string length exceeds 64-bit range"))?;
        encoded_loc_strings.push((*id, encoded));
    }

    let offset_to_loc_str = HEADER_SIZE;
    let key_entry_size = match file_version {
        ErfVersion::V1 => 24_u64,
        ErfVersion::E1 => 44_u64,
    };
    let offset_to_key_list = offset_to_loc_str + loc_string_size;
    let key_list_size = key_entry_size
        * u64::try_from(entries.len())
            .map_err(|_error| ErfError::msg("ERF entry count exceeds 64-bit range"))?;
    let offset_to_resource_list = offset_to_key_list + key_list_size;
    let resource_entry_size = match file_version {
        ErfVersion::V1 => 8_u64,
        ErfVersion::E1 => 16_u64,
    };
    let resource_list_size = resource_entry_size
        * u64::try_from(entries.len())
            .map_err(|_error| ErfError::msg("ERF entry count exceeds 64-bit range"))?;

    writer.seek(SeekFrom::Start(0))?;
    write_padded_file_type(writer, file_type)?;
    match file_version {
        ErfVersion::V1 => writer.write_all(b"V1.0")?,
        ErfVersion::E1 => writer.write_all(b"E1.0")?,
    }
    write_i32(
        writer,
        to_i32_len(loc_strings.len(), "ERF localized string count")?,
    )?;
    write_i32(
        writer,
        to_i32_u64(loc_string_size, "ERF localized string block size")?,
    )?;
    write_i32(writer, to_i32_len(entries.len(), "ERF entry count")?)?;
    write_i32(
        writer,
        to_i32_u64(offset_to_loc_str, "ERF locstring offset")?,
    )?;
    write_i32(
        writer,
        to_i32_u64(offset_to_key_list, "ERF key list offset")?,
    )?;
    write_i32(
        writer,
        to_i32_u64(offset_to_resource_list, "ERF resource list offset")?,
    )?;
    write_i32(writer, to_i32_u32(build_year, "ERF build year")?)?;
    write_i32(writer, to_i32_u32(build_day, "ERF build day")?)?;
    write_i32(writer, str_ref)?;
    match file_version {
        ErfVersion::V1 => writer.write_all(&[0_u8; 116])?,
        ErfVersion::E1 => {
            writer.write_all(
                normalize_oid(erf_oid.unwrap_or("000000000000000000000000"))?.as_bytes(),
            )?;
            writer.write_all(&[0_u8; 92])?;
        }
    }

    for (id, encoded) in &encoded_loc_strings {
        write_i32(writer, *id)?;
        write_i32(
            writer,
            to_i32_len(encoded.len(), "ERF localized string length")?,
        )?;
        writer.write_all(encoded)?;
    }

    writer.write_all(&vec![
        0_u8;
        usize::try_from(key_list_size).map_err(|_error| {
            ErfError::msg("ERF key list size exceeds usize")
        })?
    ])?;
    writer.write_all(&vec![
        0_u8;
        usize::try_from(resource_list_size).map_err(
            |_error| ErfError::msg("ERF resource list size exceeds usize")
        )?
    ])?;

    let offset_to_resource_data = writer.stream_position()?;
    let mut written = Vec::<(ResRef, usize, usize, SecureHash)>::with_capacity(entries.len());
    for rr in entries {
        let pos = writer.stream_position()?;
        let (disk_size, uncompressed_size, sha1) = match exocomp {
            ExoResFileCompressionType::None => {
                let (bytes, sha1) = entry_writer(rr, writer)?;
                (bytes, bytes, sha1)
            }
            ExoResFileCompressionType::CompressedBuf => {
                let mut buffer = Vec::new();
                let (uncompressed_size, sha1) = entry_writer(rr, &mut buffer)?;
                compress_buf_writer(writer, &buffer, compalg, EXO_RES_FILE_COMPRESSED_BUF_MAGIC)?;
                let disk_size = usize::try_from(writer.stream_position()? - pos)
                    .map_err(|_error| ErfError::msg("ERF compressed entry size exceeds usize"))?;
                (disk_size, uncompressed_size, sha1)
            }
        };
        written.push((rr.clone(), disk_size, uncompressed_size, sha1));
    }

    let end_of_file = writer.stream_position()?;

    writer.seek(SeekFrom::Start(offset_to_key_list))?;
    for (index, (rr, _, _, sha1)) in written.iter().enumerate() {
        write_padded_resref(writer, rr)?;
        write_i32(writer, to_i32_len(index, "ERF resource index")?)?;
        write_u16(writer, rr.res_type().0)?;
        writer.write_all(&[0_u8; 2])?;
        if file_version == ErfVersion::E1 {
            writer.write_all(sha1.as_bytes())?;
        }
    }

    writer.seek(SeekFrom::Start(offset_to_resource_list))?;
    let mut current_offset = offset_to_resource_data;
    for (_, disk_size, uncompressed_size, _) in &written {
        write_u32(
            writer,
            to_u32_u64(current_offset, "ERF resource data offset")?,
        )?;
        write_u32(writer, to_u32_len(*disk_size, "ERF disk size")?)?;
        if file_version == ErfVersion::E1 {
            write_u32(writer, exocomp as u32)?;
            write_u32(
                writer,
                to_u32_len(*uncompressed_size, "ERF uncompressed size")?,
            )?;
        }
        current_offset += *disk_size as u64;
    }

    writer.seek(SeekFrom::Start(end_of_file))?;
    debug!(entry_count = written.len(), "wrote erf archive");
    Ok(())
}

fn normalize_oid(input: &str) -> ErfResult<String> {
    let normalized = input.trim().to_ascii_lowercase();
    if normalized.len() == 24 && normalized.chars().all(|ch| ch.is_ascii_hexdigit()) {
        Ok(normalized)
    } else {
        Err(ErfError::msg(format!("invalid oid: {input}")))
    }
}

fn trim_trailing_nuls(bytes: &[u8]) -> String {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    String::from_utf8_lossy(bytes.get(..end).unwrap_or(bytes)).to_string()
}

fn read_fixed_string<R: Read + ?Sized>(reader: &mut R, len: usize) -> io::Result<String> {
    let bytes = read_bytes_or_err(reader, len)?;
    String::from_utf8(bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn read_secure_hash<R: Read + ?Sized>(reader: &mut R) -> io::Result<SecureHash> {
    let mut bytes = [0_u8; 20];
    reader.read_exact(&mut bytes)?;
    Ok(SecureHash::new(bytes))
}

fn read_i32<R: Read + ?Sized>(reader: &mut R) -> io::Result<i32> {
    let mut bytes = [0_u8; 4];
    reader.read_exact(&mut bytes)?;
    Ok(i32::from_le_bytes(bytes))
}

fn read_u16<R: Read + ?Sized>(reader: &mut R) -> io::Result<u16> {
    let mut bytes = [0_u8; 2];
    reader.read_exact(&mut bytes)?;
    Ok(u16::from_le_bytes(bytes))
}

fn read_u32<R: Read + ?Sized>(reader: &mut R) -> io::Result<u32> {
    let mut bytes = [0_u8; 4];
    reader.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

fn write_i32<W: Write + ?Sized>(writer: &mut W, value: i32) -> io::Result<()> {
    writer.write_all(&value.to_le_bytes())
}

fn write_u16<W: Write + ?Sized>(writer: &mut W, value: u16) -> io::Result<()> {
    writer.write_all(&value.to_le_bytes())
}

fn to_i32_len(value: usize, what: &str) -> ErfResult<i32> {
    i32::try_from(value).map_err(|_error| ErfError::msg(format!("{what} exceeds 32-bit range")))
}

fn to_i32_u64(value: u64, what: &str) -> ErfResult<i32> {
    i32::try_from(value).map_err(|_error| ErfError::msg(format!("{what} exceeds 32-bit range")))
}

fn to_i32_u32(value: u32, what: &str) -> ErfResult<i32> {
    i32::try_from(value).map_err(|_error| ErfError::msg(format!("{what} exceeds 32-bit range")))
}

fn to_u32_len(value: usize, what: &str) -> ErfResult<u32> {
    u32::try_from(value).map_err(|_error| ErfError::msg(format!("{what} exceeds 32-bit range")))
}

fn to_u32_u64(value: u64, what: &str) -> ErfResult<u32> {
    u32::try_from(value).map_err(|_error| ErfError::msg(format!("{what} exceeds 32-bit range")))
}

fn write_u32<W: Write + ?Sized>(writer: &mut W, value: u32) -> io::Result<()> {
    writer.write_all(&value.to_le_bytes())
}

fn write_padded_resref<W: Write + ?Sized>(writer: &mut W, rr: &ResRef) -> io::Result<()> {
    let value = rr.res_ref();
    writer.write_all(value.as_bytes())?;
    writer.write_all(&vec![0_u8; 16 - value.len()])
}

fn write_padded_file_type<W: Write + ?Sized>(writer: &mut W, file_type: &str) -> io::Result<()> {
    let mut padded = file_type
        .chars()
        .take(4)
        .collect::<String>()
        .to_ascii_uppercase();
    while padded.len() < 4 {
        padded.push(' ');
    }
    writer.write_all(padded.as_bytes())
}
