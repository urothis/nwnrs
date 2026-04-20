use std::{
    collections::hash_map::RandomState,
    fs::{self, File},
    io::{self, BufWriter, Read, Seek, SeekFrom, Write},
    path::Path,
    sync::Mutex,
};

use nwnrs_checksums::prelude::*;
use nwnrs_compressedbuf::prelude::*;
use nwnrs_exo::prelude::*;
use nwnrs_resman::prelude::*;
use nwnrs_resref::prelude::*;
use nwnrs_restype::prelude::*;
use tracing::{debug, instrument};

use crate::prelude::*;

/// Reads a KEY file from a reader and a caller-supplied BIF resolver.
///
/// The resolver is stored for lazy BIF loading and is only invoked when a
/// referenced resource is actually demanded.
///
/// # Errors
///
/// Returns [`KeyError`] if the data cannot be read or does not conform to the
/// KEY format.
#[instrument(level = "debug", skip_all, err)]
pub fn read_key_table<R>(
    reader: R,
    label: impl Into<String>,
    resolver: BifResolver,
) -> KeyResult<KeyTable>
where
    R: Read + Seek,
{
    read_key_table_from_reader(reader, label.into(), resolver)
}

/// Opens a KEY file from disk and resolves BIF paths relative to the KEY
/// directory.
///
/// # Errors
///
/// Returns [`KeyError`] if the file cannot be opened or parsed.
#[instrument(level = "debug", skip_all, err, fields(path = %path.as_ref().display()))]
pub fn read_key_table_from_file(path: impl AsRef<Path>) -> KeyResult<KeyTable> {
    let path = path.as_ref();
    let parent = path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    let resolver: BifResolver = std::sync::Arc::new(move |filename: &str| {
        let normalized = normalize_bif_filename(filename);
        let direct = parent.join(&normalized);
        if direct.is_file() {
            return Ok(Some(shared_stream(File::open(direct)?)));
        }

        if let Some(basename) = Path::new(&normalized).file_name() {
            let basename_candidate = parent.join(basename);
            if basename_candidate.is_file() {
                return Ok(Some(shared_stream(File::open(basename_candidate)?)));
            }
        }

        Ok(None)
    });

    let file = File::open(path)?;
    read_key_table_from_reader(file, path.display().to_string(), resolver)
}

#[allow(clippy::needless_pass_by_value)]
fn read_key_table_from_reader<R>(
    mut reader: R,
    label: String,
    resolver: BifResolver,
) -> KeyResult<KeyTable>
where
    R: Read + Seek,
{
    let io_start = reader.stream_position()?;
    let file_type = read_fixed_string(&mut reader, 4)?;
    if file_type != "KEY " {
        return Err(KeyError::msg("invalid key magic"));
    }

    let file_version = read_fixed_string(&mut reader, 4)?;
    let version = match file_version.as_str() {
        "V1  " => KeyBifVersion::V1,
        "E1  " => KeyBifVersion::E1,
        _ => {
            return Err(KeyError::msg(format!(
                "unsupported key version {file_version}"
            )));
        }
    };

    let bif_count = read_u32(&mut reader)? as usize;
    let key_count = read_u32(&mut reader)? as usize;
    let offset_to_file_table = u64::from(read_u32(&mut reader)?);
    let offset_to_key_table = u64::from(read_u32(&mut reader)?);
    let build_year = read_u32(&mut reader)?;
    let build_day = read_u32(&mut reader)?;
    let oid = match version {
        KeyBifVersion::V1 => {
            reader.seek(SeekFrom::Current(32))?;
            None
        }
        KeyBifVersion::E1 => {
            let oid = read_fixed_string(&mut reader, 24)?;
            reader.seek(SeekFrom::Current(8))?;
            Some(oid)
        }
    };
    let normalized_oid = oid.as_deref().map(normalize_oid).transpose()?;

    reader.seek(SeekFrom::Start(io_start + offset_to_file_table))?;
    let mut file_table = Vec::with_capacity(bif_count);
    for _ in 0..bif_count {
        let file_size = read_u32(&mut reader)?;
        let filename_offset = read_u32(&mut reader)?;
        let filename_size = read_u16(&mut reader)?;
        let drives = read_u16(&mut reader)?;
        file_table.push((file_size, filename_offset, filename_size, drives));
    }

    let mut bifs = Vec::with_capacity(bif_count);
    for (_, filename_offset, filename_size, drives) in &file_table {
        reader.seek(SeekFrom::Start(io_start + u64::from(*filename_offset)))?;
        let filename = trim_trailing_nuls(&read_bytes(&mut reader, usize::from(*filename_size))?);
        let resolver_filename = normalize_bif_filename(&filename);
        bifs.push(crate::BifHandle {
            filename,
            resolver_filename,
            expected_version: version,
            expected_oid: normalized_oid.clone(),
            drives: *drives,
            resolver: resolver.clone(),
            loaded: Mutex::new(None),
        });
    }

    let mut resref_id_lookup = indexmap::IndexMap::with_hasher(RandomState::new());
    reader.seek(SeekFrom::Start(io_start + offset_to_key_table))?;
    for _ in 0..key_count {
        let res_ref_raw = trim_trailing_nuls(&read_bytes(&mut reader, 16)?);
        let res_type = read_u16(&mut reader)?;
        let res_id = read_u32(&mut reader)?;
        let bif_idx = (res_id >> 20) as usize;
        if bif_idx >= bifs.len() {
            return Err(KeyError::msg(format!(
                "while reading res {res_id}={res_ref_raw}.{res_type}, bifidx not indiced by \
                 keyfile: {bif_idx}"
            )));
        }

        let sha1 = if version == KeyBifVersion::E1 {
            read_secure_hash(&mut reader)?
        } else {
            EMPTY_SECURE_HASH
        };

        let rr = ResRef::new(res_ref_raw, ResType(res_type))?;
        resref_id_lookup.insert(
            rr,
            crate::KeyEntry {
                res_id,
                sha1,
            },
        );
    }

    Ok(KeyTable {
        version,
        label,
        build_year,
        build_day,
        bifs,
        resref_id_lookup,
        oid: normalized_oid,
        raw_oid: oid,
    })
}

pub(crate) fn read_bif(
    stream: nwnrs_resman::SharedReadSeek,
    filename: &str,
    expected_version: KeyBifVersion,
    expected_oid: Option<&str>,
) -> KeyResult<crate::LoadedBif> {
    let mut reader = stream
        .lock()
        .map_err(|error| KeyError::msg(format!("bif stream lock poisoned: {error}")))?;
    reader.seek(SeekFrom::Start(0))?;

    let file_type = read_fixed_string(reader.as_mut(), 4)?;
    if file_type != "BIFF" {
        return Err(KeyError::msg(format!("invalid bif magic in {filename}")));
    }

    let version = match read_fixed_string(reader.as_mut(), 4)?.as_str() {
        "V1  " => KeyBifVersion::V1,
        "E1  " => KeyBifVersion::E1,
        other => return Err(KeyError::msg(format!("unsupported bif version {other}"))),
    };

    if version != expected_version {
        return Err(KeyError::msg("bif version mismatches key version"));
    }

    let variable_count = read_u32(reader.as_mut())? as usize;
    let fixed_count = read_u32(reader.as_mut())?;
    let variable_table_offset = u64::from(read_u32(reader.as_mut())?);
    let raw_oid = if version == KeyBifVersion::E1 {
        let oid = read_fixed_string(reader.as_mut(), 24)?;
        let normalized = normalize_oid(&oid)?;
        if let Some(expected_oid) = expected_oid
            && normalized != expected_oid
        {
            return Err(KeyError::msg(format!(
                "bif oid ({normalized}) mismatches key oid ({expected_oid})"
            )));
        }
        Some(oid)
    } else {
        None
    };

    if fixed_count != 0 {
        return Err(KeyError::msg("fixed resources in bif not supported"));
    }

    reader.seek(SeekFrom::Start(variable_table_offset))?;
    let mut variable_resources = indexmap::IndexMap::with_hasher(RandomState::new());
    for _ in 0..variable_count {
        let full_id = read_u32(reader.as_mut())?;
        let offset = u64::from(read_u32(reader.as_mut())?);
        let file_size = read_u32(reader.as_mut())? as usize;
        let _res_type = read_u32(reader.as_mut())?;
        let (compression_type, uncompressed_size) = if version == KeyBifVersion::E1 {
            let compression = ExoResFileCompressionType::from_u32(read_u32(reader.as_mut())?)
                .ok_or_else(|| KeyError::msg("invalid bif compression type"))?;
            let uncompressed_size = read_u32(reader.as_mut())? as usize;
            (compression, uncompressed_size)
        } else {
            (ExoResFileCompressionType::None, file_size)
        };

        variable_resources.insert(
            full_id & 0x000f_ffff,
            VariableResource {
                id: full_id,
                io_offset: offset,
                io_size: file_size,
                compression_type,
                uncompressed_size,
            },
        );
    }

    drop(reader);

    Ok(crate::LoadedBif {
        stream,
        file_type,
        file_version: version,
        variable_resources,
        oid: raw_oid.as_deref().map(normalize_oid).transpose()?,
        raw_oid,
    })
}

#[allow(clippy::too_many_arguments)]
/// Writes a KEY file together with its referenced BIF files.
///
/// `bifs` controls both the emitted BIF set and their resource order. For each
/// resource, `writer` must write the raw payload bytes and return the
/// uncompressed size together with the payload SHA-1.
///
/// # Errors
///
/// Returns [`KeyResult`] if the write fails.
#[instrument(
    level = "debug",
    skip_all,
    err,
    fields(key_name, entry_count = bifs.len(), version = ?version)
)]
pub fn write_key_and_bif<F>(
    version: KeyBifVersion,
    exocomp: ExoResFileCompressionType,
    compalg: Algorithm,
    dest_dir: impl AsRef<Path>,
    key_name: &str,
    bif_prefix: &str,
    bifs: &[KeyBifEntry],
    build_year: u32,
    build_day: u32,
    key_oid: Option<&str>,
    mut writer: F,
) -> KeyResult<()>
where
    F: FnMut(&ResRef, &mut dyn Write) -> KeyResult<(usize, SecureHash)>,
{
    if exocomp != ExoResFileCompressionType::None && version != KeyBifVersion::E1 {
        return Err(KeyError::msg("Compression requires E1"));
    }

    let dest_dir = dest_dir.as_ref();
    fs::create_dir_all(dest_dir)?;
    let key_oid = normalize_oid(key_oid.unwrap_or("000000000000000000000000"))?;

    let mut file_table = std::io::Cursor::new(Vec::new());
    let mut filenames = std::io::Cursor::new(Vec::new());
    let mut bif_results = Vec::with_capacity(bifs.len());

    for (bif_idx, bif) in bifs.iter().enumerate() {
        let filename_for_bif = build_bif_filename(bif_prefix, bif);
        let normalized_filename = normalize_bif_filename(&filename_for_bif);
        let bif_path = dest_dir.join(&normalized_filename);
        if let Some(parent) = bif_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let bif_file = File::create(&bif_path)?;
        let mut bif_writer = BufWriter::new(bif_file);
        let written = write_bif(
            bif_idx,
            version,
            exocomp,
            compalg,
            bif.bif_oid.as_deref().unwrap_or(&key_oid),
            &mut bif_writer,
            &bif.entries,
            &mut writer,
        )?;
        bif_writer.flush()?;
        let bif_disk_size = written.0;
        bif_results.push(written);

        write_u32(
            &mut file_table,
            to_u32_usize(bif_disk_size.saturating_sub(20), "BIF disk size")?,
        )?;
        write_u32(
            &mut file_table,
            to_u32_u64(
                crate::HEADER_SIZE
                    + (u64::try_from(bifs.len())
                        .map_err(|_error| KeyError::msg("too many BIF entries"))?
                        * 12)
                    + filenames.position(),
                "BIF filename offset",
            )?,
        )?;
        write_u16(
            &mut file_table,
            to_u16_len(filename_for_bif.len(), "BIF filename length")?,
        )?;
        write_u16(&mut file_table, bif.drives)?;
        filenames.write_all(filename_for_bif.as_bytes())?;
        filenames.write_all(&[0_u8])?;
    }

    let file_table_size = file_table.position();
    let filenames_size = filenames.position();
    let total_resref_count: usize = bifs.iter().map(|bif| bif.entries.len()).sum();

    let key_path = dest_dir.join(format!("{key_name}.key"));
    let key_file = File::create(&key_path)?;
    let mut key_writer = BufWriter::new(key_file);
    key_writer.write_all(b"KEY ")?;
    match version {
        KeyBifVersion::V1 => key_writer.write_all(b"V1  ")?,
        KeyBifVersion::E1 => key_writer.write_all(b"E1  ")?,
    }
    write_u32(&mut key_writer, to_u32_len(bifs.len(), "KEY BIF count")?)?;
    write_u32(
        &mut key_writer,
        to_u32_len(total_resref_count, "KEY resource count")?,
    )?;
    write_u32(
        &mut key_writer,
        to_u32_u64(crate::HEADER_SIZE, "KEY header size")?,
    )?;
    write_u32(
        &mut key_writer,
        to_u32_u64(
            crate::HEADER_SIZE + file_table_size + filenames_size,
            "KEY resource table offset",
        )?,
    )?;
    write_u32(&mut key_writer, build_year)?;
    write_u32(&mut key_writer, build_day)?;
    match version {
        KeyBifVersion::V1 => key_writer.write_all(&[0_u8; 32])?,
        KeyBifVersion::E1 => {
            key_writer.write_all(key_oid.as_bytes())?;
            key_writer.write_all(&[0_u8; 8])?;
        }
    }

    file_table.seek(SeekFrom::Start(0))?;
    filenames.seek(SeekFrom::Start(0))?;
    io::copy(&mut file_table, &mut key_writer)?;
    io::copy(&mut filenames, &mut key_writer)?;

    for (bif_idx, bif) in bifs.iter().enumerate() {
        for (res_idx, resref) in bif.entries.iter().enumerate() {
            write_padded_resref(&mut key_writer, resref.res_ref())?;
            write_u16(&mut key_writer, resref.res_type().0)?;
            let id =
                (to_u32_len(bif_idx, "BIF index")? << 20) + to_u32_len(res_idx, "resource index")?;
            write_u32(&mut key_writer, id)?;
            if version == KeyBifVersion::E1 {
                let sha1 = bif_results
                    .get(bif_idx)
                    .and_then(|result| result.1.get(res_idx))
                    .map(|entry| &entry.1)
                    .ok_or_else(|| KeyError::msg("missing E1 SHA entry for packed resource"))?;
                key_writer.write_all(sha1.as_bytes())?;
            }
        }
    }

    key_writer.flush()?;
    debug!(
        bif_count = bifs.len(),
        resource_count = total_resref_count,
        "wrote key and bif set"
    );
    Ok(())
}

fn write_bif<F>(
    bif_idx: usize,
    version: KeyBifVersion,
    exocomp: ExoResFileCompressionType,
    compalg: Algorithm,
    oid: &str,
    writer: &mut dyn WriteSeek,
    entries: &[ResRef],
    entry_writer: &mut F,
) -> KeyResult<crate::WriteBifResult>
where
    F: FnMut(&ResRef, &mut dyn Write) -> KeyResult<(usize, SecureHash)>,
{
    writer.seek(SeekFrom::Start(0))?;
    writer.write_all(b"BIFF")?;
    match version {
        KeyBifVersion::V1 => writer.write_all(b"V1  ")?,
        KeyBifVersion::E1 => writer.write_all(b"E1  ")?,
    }

    let variable_table_offset = match version {
        KeyBifVersion::V1 => 20_u32,
        KeyBifVersion::E1 => 44_u32,
    };
    write_u32(writer, to_u32_len(entries.len(), "BIF entry count")?)?;
    write_u32(writer, 0)?;
    write_u32(writer, variable_table_offset)?;
    if version == KeyBifVersion::E1 {
        writer.write_all(oid.as_bytes())?;
    }

    let entry_size = match version {
        KeyBifVersion::V1 => 16_usize,
        KeyBifVersion::E1 => 24_usize,
    };
    writer.write_all(&vec![0_u8; entry_size * entries.len()])?;
    let var_table_start = u64::from(variable_table_offset);

    let mut entry_meta =
        indexmap::IndexMap::<ResRef, (usize, usize, SecureHash), RandomState>::with_hasher(
            RandomState::new(),
        );
    let mut sha_entries = Vec::with_capacity(entries.len());
    for resref in entries {
        let pos = writer.stream_position()?;
        let (compressed_size, uncompressed_size, sha1) = match exocomp {
            ExoResFileCompressionType::None => {
                let (bytes, sha1) = entry_writer(resref, writer)?;
                (bytes, bytes, sha1)
            }
            ExoResFileCompressionType::CompressedBuf => {
                let mut buffer = Vec::new();
                let (uncompressed_size, sha1) = entry_writer(resref, &mut buffer)?;
                nwnrs_compressedbuf::compress_writer(
                    writer,
                    &buffer,
                    compalg,
                    EXO_RES_FILE_COMPRESSED_BUF_MAGIC,
                )?;
                let compressed_size = usize::try_from(writer.stream_position()? - pos)
                    .map_err(|_error| KeyError::msg("compressed BIF entry size exceeds usize"))?;
                (compressed_size, uncompressed_size, sha1)
            }
        };
        entry_meta.insert(resref.clone(), (uncompressed_size, compressed_size, sha1));
        sha_entries.push((resref.clone(), sha1));
    }

    let end = writer.stream_position()?;
    writer.seek(SeekFrom::Start(var_table_start))?;
    let mut offset = var_table_start
        + u64::try_from(entries.len().saturating_mul(entry_size))
            .map_err(|_error| KeyError::msg("BIF variable table size exceeds 64-bit range"))?;
    for (idx, resref) in entries.iter().enumerate() {
        let id = (to_u32_len(bif_idx, "BIF index")? << 20) + to_u32_len(idx, "BIF resource id")?;
        let (uncompressed_size, compressed_size, _) = entry_meta
            .get(resref)
            .ok_or_else(|| KeyError::msg(format!("missing written entry metadata for {resref}")))?;
        write_u32(writer, id)?;
        write_u32(writer, to_u32_u64(offset, "BIF entry offset")?)?;
        write_u32(
            writer,
            to_u32_usize(*compressed_size, "BIF compressed entry size")?,
        )?;
        offset += *compressed_size as u64;
        write_u32(writer, u32::from(resref.res_type().0))?;
        if version == KeyBifVersion::E1 {
            write_u32(writer, exocomp as u32)?;
            write_u32(
                writer,
                to_u32_usize(*uncompressed_size, "BIF uncompressed entry size")?,
            )?;
        }
    }
    writer.seek(SeekFrom::Start(end))?;

    Ok((
        usize::try_from(end).map_err(|_error| KeyError::msg("BIF size exceeds usize"))?,
        sha_entries,
    ))
}

/// Writes a KEY/BIF resource set using provenance preserved on a loaded
/// [`KeyTable`].
///
/// # Errors
///
/// Returns [`KeyResult`] if the write fails.
pub fn write_key_table_archive(
    value: &KeyTable,
    dest_dir: impl AsRef<Path>,
    key_name: &str,
) -> KeyResult<()> {
    let mut bifs = Vec::with_capacity(value.bifs.len());
    let bif_contents = value.bif_contents()?;
    let mut payloads =
        indexmap::IndexMap::<ResRef, Vec<u8>, RandomState>::with_hasher(RandomState::new());

    for (bif_idx, contents) in bif_contents.into_iter().enumerate() {
        let handle = value
            .bifs
            .get(bif_idx)
            .ok_or_else(|| KeyError::msg("missing bif handle"))?;
        let loaded = handle.load()?;
        for rr in &contents.resources {
            payloads.insert(rr.clone(), value.demand(rr)?.read_all(CachePolicy::Bypass)?);
        }

        let path = Path::new(&contents.filename);
        let name = path
            .file_stem()
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| KeyError::msg(format!("invalid bif filename {}", contents.filename)))?
            .to_string();
        let directory = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .map(|parent| parent.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();

        bifs.push(crate::KeyBifEntry {
            directory,
            name,
            recorded_filename: Some(contents.filename.clone()),
            drives: handle.drives,
            bif_oid: loaded.raw_oid.clone(),
            entries: contents.resources,
        });
    }

    write_key_and_bif(
        value.version,
        infer_key_exocomp(value)?,
        infer_key_algorithm(value)?,
        dest_dir,
        key_name,
        "",
        &bifs,
        value.build_year,
        value.build_day,
        value.raw_oid.as_deref(),
        |rr, io| {
            let bytes = payloads
                .get(rr)
                .ok_or_else(|| io::Error::other(format!("missing payload for {rr}")))?;
            io.write_all(bytes)?;
            Ok((bytes.len(), secure_hash(bytes)))
        },
    )
}

trait WriteSeek: Write + Seek {}
impl<T: Write + Seek> WriteSeek for T {}

fn normalize_oid(input: &str) -> KeyResult<String> {
    let normalized = input.trim().to_ascii_lowercase();
    if normalized.len() == 24 && normalized.chars().all(|ch| ch.is_ascii_hexdigit()) {
        Ok(normalized)
    } else {
        Err(KeyError::msg(format!("invalid oid: {input}")))
    }
}

fn normalize_bif_filename(filename: &str) -> String {
    filename.replace('\\', "/")
}

fn build_bif_filename(bif_prefix: &str, bif: &crate::KeyBifEntry) -> String {
    if let Some(filename) = &bif.recorded_filename {
        return filename.clone();
    }

    let prefix = bif_prefix.trim_matches(|ch| ch == '/' || ch == '\\');
    if prefix.is_empty() {
        format!("{}.bif", bif.name)
    } else {
        format!("{}\\{}.bif", prefix, bif.name)
    }
}

fn infer_key_exocomp(value: &KeyTable) -> KeyResult<ExoResFileCompressionType> {
    for bif in &value.bifs {
        let loaded = bif.load()?;
        if loaded
            .variable_resources
            .values()
            .any(|resource| resource.compression_type != ExoResFileCompressionType::None)
        {
            return Ok(ExoResFileCompressionType::CompressedBuf);
        }
    }
    Ok(ExoResFileCompressionType::None)
}

fn infer_key_algorithm(value: &KeyTable) -> KeyResult<Algorithm> {
    for rr in value.contents() {
        let res = value.demand(&rr)?;
        if let Some(algorithm) = res.compressed_buf_algorithm()
            && algorithm != Algorithm::None
        {
            return Ok(algorithm);
        }
    }
    Ok(Algorithm::None)
}

fn trim_trailing_nuls(bytes: &[u8]) -> String {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    String::from_utf8_lossy(bytes.get(..end).unwrap_or(bytes)).to_string()
}

fn read_bytes<R: Read + ?Sized>(reader: &mut R, size: usize) -> io::Result<Vec<u8>> {
    let mut bytes = vec![0_u8; size];
    reader.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn read_fixed_string<R: Read + ?Sized>(reader: &mut R, size: usize) -> io::Result<String> {
    let bytes = read_bytes(reader, size)?;
    String::from_utf8(bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn read_secure_hash<R: Read + ?Sized>(reader: &mut R) -> io::Result<SecureHash> {
    let mut bytes = [0_u8; 20];
    reader.read_exact(&mut bytes)?;
    Ok(SecureHash::new(bytes))
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

fn write_u16<W: Write + ?Sized>(writer: &mut W, value: u16) -> io::Result<()> {
    writer.write_all(&value.to_le_bytes())
}

fn write_u32<W: Write + ?Sized>(writer: &mut W, value: u32) -> io::Result<()> {
    writer.write_all(&value.to_le_bytes())
}

fn write_padded_resref<W: Write + ?Sized>(writer: &mut W, resref: &str) -> io::Result<()> {
    writer.write_all(resref.as_bytes())?;
    writer.write_all(&vec![0_u8; 16 - resref.len()])
}

fn to_u32_len(value: usize, what: &str) -> KeyResult<u32> {
    u32::try_from(value).map_err(|_error| KeyError::msg(format!("{what} exceeds 32-bit range")))
}

fn to_u32_usize(value: usize, what: &str) -> KeyResult<u32> {
    u32::try_from(value).map_err(|_error| KeyError::msg(format!("{what} exceeds 32-bit range")))
}

fn to_u32_u64(value: u64, what: &str) -> KeyResult<u32> {
    u32::try_from(value).map_err(|_error| KeyError::msg(format!("{what} exceeds 32-bit range")))
}

fn to_u16_len(value: usize, what: &str) -> KeyResult<u16> {
    u16::try_from(value).map_err(|_error| KeyError::msg(format!("{what} exceeds 16-bit range")))
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use nwnrs_compressedbuf::Algorithm;
    use nwnrs_exo::ExoResFileCompressionType;
    use nwnrs_resman::{CachePolicy, ResContainer};
    use nwnrs_resref::{ResRef, ResolvedResRef};

    use super::{read_key_table_from_file, write_key_and_bif, write_key_table_archive};
    use crate::{KeyBifEntry, KeyBifVersion};

    fn unique_test_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock drift before unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("nwnrs-key-{prefix}-{nanos}"))
    }

    #[test]
    fn key_archive_roundtrip_preserves_recorded_bif_names_and_multi_bif_ids() {
        let source_dir = unique_test_dir("source");
        let output_dir = unique_test_dir("output");
        fs::create_dir_all(&source_dir).expect("create source dir");
        fs::create_dir_all(&output_dir).expect("create output dir");

        let alpha: ResRef = ResolvedResRef::from_filename("alpha.uti")
            .expect("alpha resref")
            .into();
        let beta: ResRef = ResolvedResRef::from_filename("beta.utc")
            .expect("beta resref")
            .into();

        let payloads: HashMap<ResRef, Vec<u8>> = HashMap::from([
            (alpha.clone(), b"alpha-bytes".to_vec()),
            (beta.clone(), b"beta-bytes".to_vec()),
        ]);

        write_key_and_bif(
            KeyBifVersion::E1,
            ExoResFileCompressionType::None,
            Algorithm::None,
            &source_dir,
            "chitin",
            "",
            &[
                KeyBifEntry {
                    directory:         String::new(),
                    name:              "data_a".to_string(),
                    recorded_filename: Some("Data\\First.BIF".to_string()),
                    drives:            7,
                    bif_oid:           Some("fedcba987654321001234567".to_string()),
                    entries:           vec![alpha.clone()],
                },
                KeyBifEntry {
                    directory:         String::new(),
                    name:              "data_b".to_string(),
                    recorded_filename: Some("Data\\Second.BIF".to_string()),
                    drives:            9,
                    bif_oid:           Some("fedcba987654321001234567".to_string()),
                    entries:           vec![beta.clone()],
                },
            ],
            2025,
            97,
            Some("fedcba987654321001234567"),
            |rr, io| {
                let bytes = payloads.get(rr).expect("payload for resref");
                io.write_all(bytes)?;
                Ok((bytes.len(), nwnrs_checksums::secure_hash(bytes)))
            },
        )
        .expect("write key+bif");

        let key_path = source_dir.join("chitin.key");
        let key = read_key_table_from_file(&key_path).expect("read key");
        assert_eq!(
            key.bifs(),
            vec![
                "Data\\First.BIF".to_string(),
                "Data\\Second.BIF".to_string()
            ]
        );
        assert_eq!(key.raw_oid(), Some("fedcba987654321001234567"));
        assert_eq!(
            key.demand(&beta)
                .expect("demand second bif resource")
                .read_all(CachePolicy::Bypass)
                .expect("read second bif resource"),
            b"beta-bytes"
        );

        write_key_table_archive(&key, &output_dir, "chitin").expect("rewrite key archive");

        assert_eq!(
            fs::read(source_dir.join("chitin.key")).expect("read source key"),
            fs::read(output_dir.join("chitin.key")).expect("read output key")
        );
        assert_eq!(
            fs::read(source_dir.join("Data/First.BIF")).expect("read source first bif"),
            fs::read(output_dir.join("Data/First.BIF")).expect("read output first bif")
        );
        assert_eq!(
            fs::read(source_dir.join("Data/Second.BIF")).expect("read source second bif"),
            fs::read(output_dir.join("Data/Second.BIF")).expect("read output second bif")
        );
    }
}
