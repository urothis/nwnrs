use std::{
    cmp::Ordering,
    collections::HashMap,
    fs::File,
    io::{self, BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
};

use nwnrs_types::{checksums::prelude::*, resman::prelude::*};
use tracing::{debug, instrument};

use crate::nwsync::{
    HASH_TREE_DEPTH, MAGIC, Manifest, ManifestEntry, ManifestEntrySource, ManifestError,
    ManifestResult, VERSION,
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
///
/// # Errors
///
/// Returns [`ManifestError`] if the data cannot be read or does not conform to
/// the `NWSync` manifest format.
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

    let mut primary_positions = Vec::with_capacity(entry_count as usize);
    for index in 0..entry_count {
        let sha1 = read_secure_hash(reader)?;
        let size = read_u32(reader)?;
        let (resref, raw_resref) = read_resref(reader)?;
        check(
            resref.resolve().is_some(),
            format!("Entry at position {index} does not resolve to a valid resref: {resref:?}"),
        )?;

        primary_positions.push(manifest.entries.len());
        manifest.add_entry(ManifestEntry {
            sha1,
            size,
            resref,
            raw_resref,
            source: ManifestEntrySource::Primary,
        });
    }

    for index in 0..mapping_count {
        let entry_index = read_u32(reader)? as usize;
        let (resref, raw_resref) = read_resref(reader)?;
        check(
            entry_index < primary_positions.len(),
            format!("Mapping {index} references non-existent entry {entry_index}"),
        )?;

        let mapped = manifest
            .entries
            .get(*primary_positions.get(entry_index).ok_or_else(|| {
                ManifestError::msg(format!(
                    "Mapping {index} references non-existent entry {entry_index}"
                ))
            })?)
            .cloned()
            .ok_or_else(|| {
                ManifestError::msg(format!(
                    "Mapping {index} references non-existent entry {entry_index}"
                ))
            })?;
        manifest.add_entry(ManifestEntry {
            sha1: mapped.sha1,
            size: mapped.size,
            resref,
            raw_resref,
            source: ManifestEntrySource::Mapping {
                target: *primary_positions.get(entry_index).ok_or_else(|| {
                    ManifestError::msg(format!(
                        "Mapping {index} references non-existent entry {entry_index}"
                    ))
                })?,
            },
        });
    }

    debug!(
        entry_count = manifest.entries.len(),
        version = manifest.version,
        "read nwsync manifest"
    );
    Ok(manifest)
}

/// Reads a manifest file from disk.
///
/// # Errors
///
/// Returns [`ManifestError`] if the file cannot be opened or parsed as a valid
/// manifest.
#[instrument(level = "debug", skip_all, err, fields(path = %path.as_ref().display()))]
pub fn read_manifest_file(path: impl AsRef<Path>) -> ManifestResult<Manifest> {
    let file = File::open(path.as_ref())?;
    let mut reader = BufReader::new(file);
    read_manifest(&mut reader)
}

/// Writes a manifest to a stream.
///
/// # Errors
///
/// Returns [`ManifestError`] if the manifest is invalid or the write fails.
#[instrument(
    level = "debug",
    skip_all,
    err,
    fields(entry_count = manifest.entries.len(), version = manifest.version)
)]
pub fn write_manifest<W: Write>(writer: &mut W, manifest: &Manifest) -> ManifestResult<()> {
    check(manifest.version == VERSION, "Unsupported manifest version")?;

    let sorted_positions = sorted_manifest_positions(manifest);
    let mut seen_hashes = HashMap::new();
    let mut primary_positions = Vec::new();
    let mut mapping_positions = Vec::new();

    for position in sorted_positions {
        let entry = manifest
            .entries
            .get(position)
            .ok_or_else(|| ManifestError::msg("manifest entry index out of range"))?;
        if let Some(ordinal) = seen_hashes.get(&entry.sha1).copied() {
            mapping_positions.push((ordinal, position));
        } else {
            let ordinal = to_u32(primary_positions.len(), "manifest primary ordinal")?;
            seen_hashes.insert(entry.sha1, ordinal);
            primary_positions.push(position);
        }
    }

    writer.write_all(MAGIC)?;
    write_u32(writer, manifest.version)?;
    write_u32(
        writer,
        to_u32(primary_positions.len(), "manifest primary count")?,
    )?;
    write_u32(
        writer,
        to_u32(mapping_positions.len(), "manifest mapping count")?,
    )?;

    for position in &primary_positions {
        let entry = manifest
            .entries
            .get(*position)
            .ok_or_else(|| ManifestError::msg("primary entry index out of range"))?;
        writer.write_all(entry.sha1.as_bytes())?;
        write_u32(writer, entry.size)?;
        write_resref(writer, entry)?;
    }

    for (ordinal, position) in &mapping_positions {
        let entry = manifest
            .entries
            .get(*position)
            .ok_or_else(|| ManifestError::msg("mapping entry index out of range"))?;
        write_u32(writer, *ordinal)?;
        write_resref(writer, entry)?;
    }

    debug!(
        entry_count = manifest.entries.len(),
        version = manifest.version,
        "wrote nwsync manifest"
    );
    Ok(())
}

fn sorted_manifest_positions(manifest: &Manifest) -> Vec<usize> {
    let mut positions = (0..manifest.entries.len()).collect::<Vec<_>>();
    positions.sort_by(|left, right| {
        compare_manifest_entries(manifest.entries.get(*left), manifest.entries.get(*right))
    });
    positions
}

fn compare_manifest_entries(
    left: Option<&ManifestEntry>,
    right: Option<&ManifestEntry>,
) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => left
            .sha1
            .as_bytes()
            .cmp(right.sha1.as_bytes())
            .then_with(|| left.resref.res_ref().cmp(right.resref.res_ref())),
        (None, Some(_)) => Ordering::Less,
        (Some(_), None) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

/// Writes a manifest file to disk.
///
/// # Errors
///
/// Returns [`ManifestError`] if the file cannot be created or the manifest
/// cannot be written.
#[instrument(level = "debug", skip_all, err, fields(path = %path.as_ref().display()))]
pub fn write_manifest_file(path: impl AsRef<Path>, manifest: &Manifest) -> ManifestResult<()> {
    let file = File::create(path.as_ref())?;
    let mut writer = BufWriter::new(file);
    write_manifest(&mut writer, manifest)?;
    writer.flush()?;
    Ok(())
}

fn read_resref<R: Read>(reader: &mut R) -> ManifestResult<(ResRef, [u8; 16])> {
    let mut raw = [0_u8; 16];
    reader.read_exact(&mut raw)?;
    let end = raw.iter().position(|byte| *byte == 0).unwrap_or(raw.len());
    let res_ref = String::from_utf8_lossy(raw.get(..end).unwrap_or(&raw)).to_ascii_lowercase();
    let res_type = ResType(read_u16(reader)?);
    Ok((ResRef::new(res_ref, res_type)?, raw))
}

fn write_resref<W: Write>(writer: &mut W, entry: &ManifestEntry) -> ManifestResult<()> {
    let normalized = entry.resref.res_ref().to_ascii_lowercase();
    let mut raw = [0_u8; 16];
    if let Some(prefix) = raw.get_mut(..normalized.len()) {
        prefix.copy_from_slice(normalized.as_bytes());
    }
    writer.write_all(&raw)?;
    write_u16(writer, entry.resref.res_type().0)?;
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

fn to_u32(value: usize, what: &str) -> ManifestResult<u32> {
    u32::try_from(value)
        .map_err(|_error| ManifestError::msg(format!("{what} exceeds 32-bit range")))
}

fn check(condition: bool, message: impl Into<String>) -> ManifestResult<()> {
    if condition {
        Ok(())
    } else {
        Err(ManifestError::msg(message))
    }
}

#[cfg(test)]
mod tests {
    use nwnrs_types::{
        checksums::secure_hash,
        resman::{ResRef, ResType},
    };

    use crate::nwsync::{
        Manifest, ManifestEntry, ManifestEntrySource, read_manifest, write_manifest,
    };

    #[test]
    fn manifest_edit_rewrites_canonical_primary_and_mapping_structure() {
        let primary_rr = match ResRef::new("hello", ResType(2017)) {
            Ok(resref) => resref,
            Err(error) => panic!("resref: {error}"),
        };
        let mapping_rr = match ResRef::new("world", ResType(2017)) {
            Ok(resref) => resref,
            Err(error) => panic!("resref: {error}"),
        };
        let mut manifest = Manifest::default();
        manifest.entries.push(ManifestEntry {
            sha1:       secure_hash(b"first"),
            size:       5,
            resref:     primary_rr.clone(),
            raw_resref: *b"HELLO\0\0\0\0\0\0\0\0\0\0\0",
            source:     ManifestEntrySource::Primary,
        });
        manifest.entries.push(ManifestEntry {
            sha1:       secure_hash(b"first"),
            size:       5,
            resref:     mapping_rr,
            raw_resref: *b"WORLD\0\0\0\0\0\0\0\0\0\0\0",
            source:     ManifestEntrySource::Mapping {
                target: 0
            },
        });

        let mut encoded = Vec::new();
        if let Err(error) = write_manifest(&mut encoded, &manifest) {
            panic!("write manifest: {error}");
        }

        let decoded = match read_manifest(&mut &encoded[..]) {
            Ok(manifest) => manifest,
            Err(error) => panic!("read manifest: {error}"),
        };
        assert_eq!(decoded.entries.len(), 2);
        assert_eq!(
            decoded.entries.first().map(|entry| &entry.source),
            Some(&ManifestEntrySource::Primary)
        );
        assert_eq!(
            decoded.entries.get(1).map(|entry| &entry.source),
            Some(&ManifestEntrySource::Mapping {
                target: 0
            })
        );

        let mut edited = decoded.clone();
        if let Some(entry) = edited.entries.get_mut(0) {
            entry.size = 7;
        } else {
            panic!("decoded manifest should contain primary entry");
        }
        let mut reencoded = Vec::new();
        if let Err(error) = write_manifest(&mut reencoded, &edited) {
            panic!("rewrite manifest: {error}");
        }
        let redecoded = match read_manifest(&mut &reencoded[..]) {
            Ok(manifest) => manifest,
            Err(error) => panic!("re-read manifest: {error}"),
        };
        assert_eq!(
            redecoded.entries.get(1).map(|entry| &entry.source),
            Some(&ManifestEntrySource::Mapping {
                target: 0
            })
        );
        assert_eq!(
            redecoded.entries.first().map(|entry| entry.raw_resref),
            Some(*b"hello\0\0\0\0\0\0\0\0\0\0\0")
        );
        assert_eq!(
            redecoded.entries.get(1).map(|entry| entry.raw_resref),
            Some(*b"world\0\0\0\0\0\0\0\0\0\0\0")
        );
    }

    #[test]
    fn manifest_write_canonicalizes_order_and_hash_mappings() {
        let alpha = ResRef::new("alpha", ResType(2017)).unwrap_or_else(|error| {
            panic!("alpha resref: {error}");
        });
        let beta = ResRef::new("beta", ResType(2017)).unwrap_or_else(|error| {
            panic!("beta resref: {error}");
        });
        let gamma = ResRef::new("gamma", ResType(2017)).unwrap_or_else(|error| {
            panic!("gamma resref: {error}");
        });

        let sha1_a = secure_hash(b"payload-a");
        let sha1_b = secure_hash(b"payload-b");
        let mut manifest = Manifest::default();
        manifest.entries.push(ManifestEntry {
            sha1:       sha1_b,
            size:       9,
            resref:     gamma.clone(),
            raw_resref: *b"GAMMA\0\0\0\0\0\0\0\0\0\0\0",
            source:     ManifestEntrySource::Primary,
        });
        manifest.entries.push(ManifestEntry {
            sha1:       sha1_a,
            size:       9,
            resref:     beta.clone(),
            raw_resref: *b"BETA\0\0\0\0\0\0\0\0\0\0\0\0",
            source:     ManifestEntrySource::Primary,
        });
        manifest.entries.push(ManifestEntry {
            sha1:       sha1_a,
            size:       9,
            resref:     alpha.clone(),
            raw_resref: *b"ALPHA\0\0\0\0\0\0\0\0\0\0\0",
            source:     ManifestEntrySource::Primary,
        });

        let mut encoded = Vec::new();
        if let Err(error) = write_manifest(&mut encoded, &manifest) {
            panic!("write manifest: {error}");
        }

        let decoded = match read_manifest(&mut &encoded[..]) {
            Ok(manifest) => manifest,
            Err(error) => panic!("read manifest: {error}"),
        };
        assert_eq!(decoded.entries.len(), 3);
        assert_eq!(
            decoded.entries.first().map(|entry| &entry.resref),
            Some(&alpha)
        );
        assert_eq!(
            decoded.entries.get(1).map(|entry| &entry.resref),
            Some(&gamma)
        );
        assert_eq!(
            decoded.entries.get(2).map(|entry| &entry.resref),
            Some(&beta)
        );
        assert_eq!(
            decoded.entries.first().map(|entry| &entry.source),
            Some(&ManifestEntrySource::Primary)
        );
        assert_eq!(
            decoded.entries.get(1).map(|entry| &entry.source),
            Some(&ManifestEntrySource::Primary)
        );
        assert_eq!(
            decoded.entries.get(2).map(|entry| &entry.source),
            Some(&ManifestEntrySource::Mapping {
                target: 0
            })
        );
    }
}
