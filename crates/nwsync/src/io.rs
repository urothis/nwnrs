use std::{
    collections::HashMap,
    fs::File,
    io::{self, BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
};

use nwnrs_checksums::prelude::*;
use nwnrs_resref::prelude::*;
use nwnrs_restype::prelude::*;
use tracing::{debug, instrument};

use crate::{
    HASH_TREE_DEPTH, MAGIC, Manifest, ManifestEntry, ManifestError, ManifestResult, VERSION,
};

/// Returns the on-disk payload path for a hashed manifest entry.
#[instrument(
    level = "debug",
    skip_all,
    err,
    fields(path = %root_directory.as_ref().display(), manifest_sha1 = %sha1_hex)
)]
pub fn path_for_entry(
    root_directory: impl AsRef<Path>,
    sha1_hex: &str,
    hash_tree_depth: usize,
) -> ManifestResult<PathBuf> {
    check(
        sha1_hex.len() >= hash_tree_depth * 2,
        "sha1 string is too short for requested hash tree depth",
    )?;
    let mut path = root_directory.as_ref().join("data").join("sha1");
    for index in 0..hash_tree_depth {
        let start = index * 2;
        path = path.join(&sha1_hex[start..start + 2]);
    }
    Ok(path.join(sha1_hex))
}

/// Reads a manifest from a stream.
#[instrument(level = "debug", skip_all, err)]
pub fn read_manifest<R: Read>(reader: &mut R) -> ManifestResult<Manifest> {
    let magic = read_fixed_string(reader, 4)?;
    check(magic == "NSYM", "Not a manifest (invalid magic bytes)")?;

    let version = read_u32(reader)?;
    check(
        version == VERSION,
        format!("Unsupported manifest version {version}"),
    )?;

    let entry_count = read_u32(reader)?;
    let mapping_count = read_u32(reader)?;
    check(
        entry_count > 0,
        "No entries in manifest. This is not supported.",
    )?;

    let mut manifest = Manifest::new(HASH_TREE_DEPTH);
    manifest.version = version;

    for index in 0..entry_count {
        let sha1 = read_secure_hash(reader)?;
        let size = read_u32(reader)?;
        let resref = read_resref(reader)?;
        check(
            resref.resolve().is_some(),
            format!(
                "Entry at position {} does not resolve to a valid resref: {:?}",
                index, resref
            ),
        )?;

        manifest.add_entry(ManifestEntry::new(sha1, size, resref));
    }

    for index in 0..mapping_count {
        let entry_index = read_u32(reader)? as usize;
        let resref = read_resref(reader)?;
        check(
            entry_index < manifest.entries.len(),
            format!("Mapping {index} references non-existent entry {entry_index}"),
        )?;

        let mapped = manifest.entries.get(entry_index).cloned().ok_or_else(|| {
            ManifestError::msg(format!(
                "Mapping {index} references non-existent entry {entry_index}"
            ))
        })?;
        manifest.add_entry(ManifestEntry::new(mapped.sha1, mapped.size, resref));
    }

    debug!(
        entry_count = manifest.entries.len(),
        version = manifest.version,
        "read nwsync manifest"
    );
    Ok(manifest)
}

/// Reads a manifest file from disk.
#[instrument(level = "debug", skip_all, err, fields(path = %path.as_ref().display()))]
pub fn read_manifest_file(path: impl AsRef<Path>) -> ManifestResult<Manifest> {
    let file = File::open(path.as_ref())?;
    let mut reader = BufReader::new(file);
    read_manifest(&mut reader)
}

/// Writes a manifest to a stream.
#[instrument(
    level = "debug",
    skip_all,
    err,
    fields(entry_count = manifest.entries.len(), version = manifest.version)
)]
pub fn write_manifest<W: Write>(writer: &mut W, manifest: &Manifest) -> ManifestResult<()> {
    check(manifest.version == VERSION, "Unsupported manifest version")?;

    let mut seen_hashes = HashMap::<SecureHash, u32>::new();
    let mut entry_count = 0_u32;
    let mut mapping_count = 0_u32;
    let mut entries_bytes = Vec::new();
    let mut mapping_bytes = Vec::new();

    let mut sorted_entries = manifest.entries.clone();
    sorted_entries.sort_by(|a, b| {
        a.sha1.to_string().cmp(&b.sha1.to_string()).then_with(|| {
            a.resref
                .res_ref()
                .to_ascii_lowercase()
                .cmp(&b.resref.res_ref().to_ascii_lowercase())
        })
    });

    for entry in sorted_entries {
        if let Some(index) = seen_hashes.get(&entry.sha1).copied() {
            write_u32(&mut mapping_bytes, index)?;
            write_resref(&mut mapping_bytes, &entry.resref)?;
            mapping_count += 1;
        } else {
            seen_hashes.insert(entry.sha1, entry_count);
            entry_count += 1;

            entries_bytes.write_all(entry.sha1.as_bytes())?;
            write_u32(&mut entries_bytes, entry.size)?;
            write_resref(&mut entries_bytes, &entry.resref)?;
        }
    }

    writer.write_all(MAGIC)?;
    write_u32(writer, manifest.version)?;
    write_u32(writer, entry_count)?;
    write_u32(writer, mapping_count)?;
    writer.write_all(&entries_bytes)?;
    writer.write_all(&mapping_bytes)?;

    debug!(entry_count, mapping_count, "wrote nwsync manifest");
    Ok(())
}

/// Writes a manifest file to disk.
#[instrument(level = "debug", skip_all, err, fields(path = %path.as_ref().display()))]
pub fn write_manifest_file(path: impl AsRef<Path>, manifest: &Manifest) -> ManifestResult<()> {
    let file = File::create(path.as_ref())?;
    let mut writer = BufWriter::new(file);
    write_manifest(&mut writer, manifest)?;
    writer.flush()?;
    Ok(())
}

fn read_resref<R: Read>(reader: &mut R) -> ManifestResult<ResRef> {
    let mut raw = [0_u8; 16];
    reader.read_exact(&mut raw)?;
    let end = raw.iter().position(|byte| *byte == 0).unwrap_or(raw.len());
    let res_ref = String::from_utf8_lossy(raw.get(..end).unwrap_or(&raw)).to_ascii_lowercase();
    let res_type = ResType(read_u16(reader)?);
    Ok(new_res_ref(res_ref, res_type)?)
}

fn write_resref<W: Write>(writer: &mut W, resref: &ResRef) -> ManifestResult<()> {
    let normalized = resref.res_ref().to_ascii_lowercase();
    writer.write_all(normalized.as_bytes())?;
    writer.write_all(&vec![0_u8; 16 - normalized.len()])?;
    write_u16(writer, resref.res_type().0)?;
    Ok(())
}

fn read_secure_hash<R: Read>(reader: &mut R) -> ManifestResult<SecureHash> {
    let mut bytes = [0_u8; 20];
    reader.read_exact(&mut bytes)?;
    Ok(SecureHash::new(bytes))
}

fn read_fixed_string<R: Read>(reader: &mut R, size: usize) -> io::Result<String> {
    let mut bytes = vec![0_u8; size];
    reader.read_exact(&mut bytes)?;
    String::from_utf8(bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn read_u16<R: Read>(reader: &mut R) -> io::Result<u16> {
    let mut bytes = [0_u8; 2];
    reader.read_exact(&mut bytes)?;
    Ok(u16::from_le_bytes(bytes))
}

fn read_u32<R: Read>(reader: &mut R) -> io::Result<u32> {
    let mut bytes = [0_u8; 4];
    reader.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

fn write_u16<W: Write>(writer: &mut W, value: u16) -> io::Result<()> {
    writer.write_all(&value.to_le_bytes())
}

fn write_u32<W: Write>(writer: &mut W, value: u32) -> io::Result<()> {
    writer.write_all(&value.to_le_bytes())
}

fn check(condition: bool, message: impl Into<String>) -> ManifestResult<()> {
    if condition {
        Ok(())
    } else {
        Err(ManifestError::msg(message))
    }
}
