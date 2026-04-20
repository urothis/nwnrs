use std::{
    collections::{BTreeMap, hash_map::RandomState},
    fs::File,
    io::{self, Read, Seek, SeekFrom, Write},
    path::Path,
    time::SystemTime,
};

use nwnrs_checksums::prelude::*;
use nwnrs_compressedbuf::prelude::*;
use nwnrs_encoding::prelude::*;
use nwnrs_exo::prelude::*;
use nwnrs_io::prelude::*;
use nwnrs_resman::prelude::*;
use nwnrs_resref::prelude::*;
use tracing::{debug, instrument};

use crate::{
    Erf, ErfError, ErfResMeta, ErfResult, ErfVersion, ErfWriteOptions, HEADER_SIZE, VALID_ERF_TYPES,
};

/// Reads an ERF-family archive from a seekable reader.
///
/// The returned [`Erf`] contains lazily readable [`nwnrs_resman::Res`] entries
/// backed by the supplied stream.
///
/// # Errors
///
/// Returns [`ErfError`] if the data cannot be read or does not conform to an ERF-family format.
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
/// This is the most direct constructor when the caller already manages stream
/// sharing.
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

    let loc_str_count = usize::try_from(read_i32(io.as_mut())?)
        .map_err(|e| ErfError::msg(format!("ERF loc string count is negative: {e}")))?;
    let loc_string_size = u64::try_from(read_i32(io.as_mut())?)
        .map_err(|e| ErfError::msg(format!("ERF loc string size is negative: {e}")))?;
    let entry_count = usize::try_from(read_i32(io.as_mut())?)
        .map_err(|e| ErfError::msg(format!("ERF entry count is negative: {e}")))?;
    let offset_to_loc_str = u64::try_from(read_i32(io.as_mut())?)
        .map_err(|e| ErfError::msg(format!("ERF loc string offset is negative: {e}")))?;
    let offset_to_key_list = u64::try_from(read_i32(io.as_mut())?)
        .map_err(|e| ErfError::msg(format!("ERF key list offset is negative: {e}")))?;
    let offset_to_resource_list = u64::try_from(read_i32(io.as_mut())?)
        .map_err(|e| ErfError::msg(format!("ERF resource list offset is negative: {e}")))?;
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
        let len = usize::try_from(read_i32(io.as_mut())?)
            .map_err(|e| ErfError::msg(format!("ERF loc string length is negative: {e}")))?;
        let bytes = read_bytes_or_err(io.as_mut(), len)?;
        loc_strings.insert(id, from_nwnrs_encoding(&bytes)?);
    }

    let _is_known_erf_type = VALID_ERF_TYPES.contains(&file_type.as_str());

    let key_entry_size = match file_version {
        ErfVersion::V1 => 24_u64,
        ErfVersion::E1 => 44_u64,
    };
    let resource_entry_size = match file_version {
        ErfVersion::V1 => 8_u64,
        ErfVersion::E1 => 16_u64,
    };
    let expected_resource_list_offset = offset_to_key_list
        + key_entry_size
            * u64::try_from(entry_count)
                .map_err(|_error| ErfError::msg("ERF entry count exceeds 64-bit range"))?;
    let file_len = io.seek(SeekFrom::End(0))?;
    let resource_list_size = resource_entry_size
        .checked_mul(
            u64::try_from(entry_count)
                .map_err(|_error| ErfError::msg("ERF entry count exceeds 64-bit range"))?,
        )
        .ok_or_else(|| ErfError::msg("ERF resource list size overflow"))?;
    expect(
        offset_to_resource_list
            .checked_add(resource_list_size)
            .is_some_and(|end| end <= file_len),
        "ERF resource list offset out of range",
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
        let disk_size_u64 = u64::try_from(disk_size)
            .map_err(|_error| ErfError::msg("ERF resource size exceeds 64-bit range"))?;
        expect(offset != 0, "ERF resource offset must be non-zero")?;
        expect(
            offset
                .checked_add(disk_size_u64)
                .is_some_and(|end| end <= file_len),
            "ERF resource payload out of range",
        )?;

        resources.push(ErfResMeta {
            offset,
            disk_size,
            uncompressed_size,
            compression,
        });
    }

    let origin_container = format!("Erf:{filename}");
    let mut entries: indexmap::IndexMap<ResRef, Res, RandomState> =
        indexmap::IndexMap::with_hasher(RandomState::new());
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

        let mut rr = match ResRef::new(res_ref_raw, nwnrs_restype::ResType(res_type)) {
            Ok(rr) => rr,
            Err(_) => ResRef::new(format!("invalid_{index}"), nwnrs_restype::ResType(res_type))?,
        };

        if let Some(existing) = entries.get(&rr) {
            if existing.io_offset() == meta.offset
                && existing.io_size()
                    == i64::try_from(meta.disk_size).map_err(|e| {
                        ErfError::msg(format!("ERF resource size exceeds i64 range: {e}"))
                    })?
            {
                continue;
            }
            rr = ResRef::new(
                format!("__erfdup__{index}"),
                nwnrs_restype::ResType(res_type),
            )?;
        }

        let res = Res::new_with_stream(
            new_res_origin(origin_container.clone(), format!("{filename}: {rr}")),
            rr.clone(),
            SystemTime::UNIX_EPOCH,
            stream.clone(),
            i64::try_from(meta.disk_size)
                .map_err(|e| ErfError::msg(format!("ERF resource size exceeds i64 range: {e}")))?,
            meta.offset,
            meta.compression,
            read_compressed_buf_algorithm(io.as_mut(), meta)?,
            meta.uncompressed_size,
            sha1,
        );
        entries.insert(rr, res);
    }

    drop(io);

    // TODO: possibly dead code - value is never read
    #[allow(clippy::no_effect_underscore_binding)]
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
        resource_list_padding: offset_to_resource_list
            .saturating_sub(expected_resource_list_offset),
    };
    debug!(entry_count = erf.entries.len(), file_type = %erf.file_type, "read erf archive");
    Ok(erf)
}

fn read_compressed_buf_algorithm<R: Read + Seek + ?Sized>(
    io: &mut R,
    meta: &ErfResMeta,
) -> ErfResult<Option<Algorithm>> {
    if meta.compression != ExoResFileCompressionType::CompressedBuf {
        return Ok(None);
    }

    let current = io.stream_position()?;
    io.seek(SeekFrom::Start(meta.offset))?;
    let _magic = read_u32(io)?;
    let _version = read_u32(io)?;
    let algorithm = Algorithm::from_u32(read_u32(io)?)
        .map_err(|_error| ErfError::msg("invalid compressed buffer algorithm"))?;
    io.seek(SeekFrom::Start(current))?;
    Ok(Some(algorithm))
}

#[allow(clippy::too_many_arguments)]
/// Writes an ERF-family archive.
///
/// `entries` defines the archive order. For each entry, `entry_writer` must
/// write the raw payload bytes and return the uncompressed byte length together
/// with the payload SHA-1.
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
    entry_writer: F,
    entry_algorithm: impl FnMut(&ResRef) -> Algorithm,
) -> ErfResult<()>
where
    W: Write + Seek,
    F: FnMut(&ResRef, &mut dyn Write) -> ErfResult<(usize, SecureHash)>,
{
    write_erf_with_options(
        writer,
        file_type,
        file_version,
        build_year,
        build_day,
        exocomp,
        compalg,
        loc_strings,
        str_ref,
        entries,
        erf_oid,
        ErfWriteOptions::default(),
        entry_writer,
        entry_algorithm,
    )
}

#[allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::items_after_test_module,
    clippy::panic
)]
#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, io::Cursor};

    use nwnrs_checksums::prelude::secure_hash;
    use nwnrs_compressedbuf::prelude::Algorithm;
    use nwnrs_exo::prelude::ExoResFileCompressionType;
    use nwnrs_resref::ResolvedResRef;

    use super::{ErfVersion, read_erf, write_erf_with_options};
    use crate::ErfWriteOptions;

    #[test]
    fn malformed_erf_resource_list_offset_is_rejected() {
        let entry = ResolvedResRef::from_filename("test.utc")
            .expect("resref")
            .into();
        let mut encoded = Cursor::new(Vec::new());
        write_erf_with_options(
            &mut encoded,
            "ERF ",
            ErfVersion::V1,
            2026,
            98,
            ExoResFileCompressionType::None,
            Algorithm::None,
            &BTreeMap::new(),
            -1,
            &[entry],
            None,
            ErfWriteOptions::default(),
            |_rr, out| {
                out.write_all(b"abc")?;
                Ok((3, secure_hash(b"abc")))
            },
            |_rr| Algorithm::None,
        )
        .expect("encode erf");
        let mut bytes = encoded.into_inner();

        let resource_list_offset =
            u32::from_le_bytes(bytes[28..32].try_into().expect("resource list offset"));
        bytes[28..32].copy_from_slice(&(resource_list_offset + 1).to_le_bytes());

        let error =
            read_erf(Cursor::new(bytes), "broken.erf".to_string()).expect_err("malformed erf");
        assert!(
            error.to_string().contains("offset out of range")
                || error.to_string().contains("payload out of range")
                || error.to_string().contains("invalid")
                || error.to_string().contains("not found"),
            "unexpected error: {error}"
        );
    }
}

#[allow(clippy::too_many_arguments)]
/// Writes an ERF-family archive with explicit preserved-layout options.
pub fn write_erf_with_options<W, F>(
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
    options: ErfWriteOptions,
    entry_writer: F,
    entry_algorithm: impl FnMut(&ResRef) -> Algorithm,
) -> ErfResult<()>
where
    W: Write + Seek,
    F: FnMut(&ResRef, &mut dyn Write) -> ErfResult<(usize, SecureHash)>,
{
    write_erf_inner(
        writer,
        file_type,
        file_version,
        build_year,
        build_day,
        exocomp,
        compalg,
        loc_strings,
        str_ref,
        entries,
        erf_oid,
        options.resource_list_padding,
        entry_writer,
        entry_algorithm,
    )
}

#[allow(clippy::too_many_arguments)]
/// Writes a decoded ERF-family archive back out using its preserved layout.
pub fn write_erf_archive<W>(writer: &mut W, value: &Erf) -> ErfResult<()>
where
    W: Write + Seek,
{
    let entries = value.entries().keys().cloned().collect::<Vec<_>>();
    let mut payloads = BTreeMap::new();
    let mut algorithms = BTreeMap::new();
    let mut exocomp = ExoResFileCompressionType::None;

    for (rr, res) in value.entries() {
        payloads.insert(rr.clone(), res.read_all(CachePolicy::Bypass)?);
        let algorithm = res.compressed_buf_algorithm().unwrap_or(Algorithm::None);
        if algorithm != Algorithm::None {
            exocomp = ExoResFileCompressionType::CompressedBuf;
        }
        algorithms.insert(rr.clone(), algorithm);
    }

    write_erf_with_options(
        writer,
        &value.file_type,
        value.file_version,
        u32::try_from(value.build_year)
            .map_err(|_error| ErfError::msg("ERF build year exceeds 32-bit range"))?,
        u32::try_from(value.build_day)
            .map_err(|_error| ErfError::msg("ERF build day exceeds 32-bit range"))?,
        exocomp,
        Algorithm::None,
        value.loc_strings(),
        value.str_ref,
        &entries,
        value.oid(),
        ErfWriteOptions {
            resource_list_padding: value.resource_list_padding(),
        },
        |rr, out| {
            let bytes = payloads
                .get(rr)
                .ok_or_else(|| io::Error::other(format!("missing ERF payload for {rr}")))?;
            out.write_all(bytes)?;
            Ok((bytes.len(), secure_hash(bytes)))
        },
        |rr| algorithms.get(rr).copied().unwrap_or(Algorithm::None),
    )
}

#[allow(clippy::too_many_arguments)]
/// Internal ERF-family archive writer with explicit padding control.
fn write_erf_inner<W, F>(
    writer: &mut W,
    file_type: &str,
    file_version: ErfVersion,
    build_year: u32,
    build_day: u32,
    exocomp: ExoResFileCompressionType,
    _compalg: Algorithm,
    loc_strings: &BTreeMap<i32, String>,
    str_ref: i32,
    entries: &[ResRef],
    erf_oid: Option<&str>,
    resource_list_padding: u64,
    mut entry_writer: F,
    mut entry_algorithm: impl FnMut(&ResRef) -> Algorithm,
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
        let encoded = to_nwnrs_encoding(text)?;
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
    let offset_to_resource_list = offset_to_key_list + key_list_size + resource_list_padding;
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
        usize::try_from(resource_list_padding).map_err(
            |_error| ErfError::msg("ERF resource list padding exceeds usize")
        )?
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
                let algorithm = entry_algorithm(rr);
                compress_writer(
                    writer,
                    &buffer,
                    algorithm,
                    EXO_RES_FILE_COMPRESSED_BUF_MAGIC,
                )?;
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
