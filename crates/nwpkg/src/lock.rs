#![allow(missing_docs)]

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use nwnrs_types::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{
    PACKAGE_LOCK_FILENAME,
    fs::{
        ensure_output_file_ready, entry_is_dir, entry_is_file, is_project_control_file,
        normalize_key_bif_filename, should_skip_top_level_dir, sorted_dir_entries,
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErfPackMetadata {
    pub source:                PathBuf,
    pub source_sha256:         String,
    pub file_type:             String,
    pub file_version:          erf::ErfVersion,
    pub build_year:            i32,
    pub build_day:             i32,
    pub str_ref:               i32,
    pub loc_strings:           BTreeMap<i32, String>,
    pub oid:                   Option<String>,
    pub resource_list_padding: u64,
    pub entry_order:           Vec<resman::ResRef>,
    pub entry_algorithms:      BTreeMap<resman::ResRef, compressedbuf::Algorithm>,
    pub file_sha256s:          BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyPackMetadata {
    pub source_key:        PathBuf,
    pub source_key_sha256: String,
    pub bifs:              Vec<String>,
    pub bif_sha256s:       BTreeMap<String, String>,
    pub file_sha256s:      BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourcePackMetadata {
    pub source:        PathBuf,
    pub source_sha256: String,
    pub source_kind:   String,
    pub file_name:     String,
    pub file_sha256s:  BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum MetadataKind {
    Erf,
    Key,
    Resource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum ErfPackMetadataVersion {
    V1,
    E1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum KeyPackMetadataVersion {
    V1,
    E1,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MetadataKindProbe {
    kind: MetadataKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ErfPackMetadataFile {
    kind:                  MetadataKind,
    source:                PathBuf,
    source_sha256:         String,
    file_type:             String,
    file_version:          ErfPackMetadataVersion,
    build_year:            i32,
    build_day:             i32,
    str_ref:               i32,
    #[serde(default)]
    loc_strings:           BTreeMap<i32, String>,
    oid:                   Option<String>,
    #[serde(default)]
    resource_list_padding: u64,
    #[serde(default)]
    entry_order:           Vec<String>,
    #[serde(default)]
    entry_algorithms:      BTreeMap<String, u32>,
    #[serde(default)]
    file_sha256s:          BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct KeyPackMetadataFile {
    kind:              MetadataKind,
    source_key:        PathBuf,
    source_key_sha256: String,
    version:           KeyPackMetadataVersion,
    build_year:        u32,
    build_day:         u32,
    oid:               Option<String>,
    #[serde(default)]
    bifs:              Vec<String>,
    #[serde(default)]
    bif_sha256s:       BTreeMap<String, String>,
    #[serde(default)]
    file_sha256s:      BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ResourcePackMetadataFile {
    kind:          MetadataKind,
    source:        PathBuf,
    source_sha256: String,
    source_kind:   String,
    file_name:     String,
    #[serde(default)]
    file_sha256s:  BTreeMap<String, String>,
}

impl From<erf::ErfVersion> for ErfPackMetadataVersion {
    fn from(value: erf::ErfVersion) -> Self {
        match value {
            erf::ErfVersion::V1 => Self::V1,
            erf::ErfVersion::E1 => Self::E1,
        }
    }
}

impl From<ErfPackMetadataVersion> for erf::ErfVersion {
    fn from(value: ErfPackMetadataVersion) -> Self {
        match value {
            ErfPackMetadataVersion::V1 => Self::V1,
            ErfPackMetadataVersion::E1 => Self::E1,
        }
    }
}

impl From<key::KeyBifVersion> for KeyPackMetadataVersion {
    fn from(value: key::KeyBifVersion) -> Self {
        match value {
            key::KeyBifVersion::V1 => Self::V1,
            key::KeyBifVersion::E1 => Self::E1,
        }
    }
}

impl From<KeyPackMetadata> for KeyPackMetadataFile {
    fn from(value: KeyPackMetadata) -> Self {
        Self {
            kind:              MetadataKind::Key,
            source_key:        value.source_key,
            source_key_sha256: value.source_key_sha256,
            version:           KeyPackMetadataVersion::V1,
            build_year:        0,
            build_day:         0,
            oid:               None,
            bifs:              value.bifs,
            bif_sha256s:       value.bif_sha256s,
            file_sha256s:      value.file_sha256s,
        }
    }
}

impl ErfPackMetadata {
    fn from_file(value: ErfPackMetadataFile, metadata_path: &Path) -> Result<Self, String> {
        Ok(Self {
            source:                value.source,
            source_sha256:         value.source_sha256,
            file_type:             value.file_type,
            file_version:          value.file_version.into(),
            build_year:            value.build_year,
            build_day:             value.build_day,
            str_ref:               value.str_ref,
            loc_strings:           value.loc_strings,
            oid:                   value.oid,
            resource_list_padding: value.resource_list_padding,
            entry_order:           parse_entry_order(value.entry_order, metadata_path)?,
            entry_algorithms:      parse_entry_algorithms(value.entry_algorithms, metadata_path)?,
            file_sha256s:          value.file_sha256s,
        })
    }
}

impl KeyPackMetadata {
    fn from_file(value: KeyPackMetadataFile) -> Self {
        Self {
            source_key:        value.source_key,
            source_key_sha256: value.source_key_sha256,
            bifs:              value.bifs,
            bif_sha256s:       value.bif_sha256s,
            file_sha256s:      value.file_sha256s,
        }
    }
}

impl ResourcePackMetadata {
    fn from_file(value: ResourcePackMetadataFile) -> Self {
        Self {
            source:        value.source,
            source_sha256: value.source_sha256,
            source_kind:   value.source_kind,
            file_name:     value.file_name,
            file_sha256s:  value.file_sha256s,
        }
    }
}

pub fn write_erf_pack_metadata(
    destination: &Path,
    input: &Path,
    erf: &erf::Erf,
    force: bool,
) -> Result<(), String> {
    let metadata_path = destination.join(PACKAGE_LOCK_FILENAME);
    ensure_output_file_ready(&metadata_path, force)?;
    let value = ErfPackMetadataFile {
        kind:                  MetadataKind::Erf,
        source:                input.to_path_buf(),
        source_sha256:         checksums::sha256_digest(
            fs::read(input)
                .map_err(|error| format!("failed to read {}: {error}", input.display()))?,
        )
        .to_string(),
        file_type:             erf.file_type.clone(),
        file_version:          erf.file_version.into(),
        build_year:            erf.build_year,
        build_day:             erf.build_day,
        str_ref:               erf.str_ref,
        loc_strings:           erf.loc_strings().clone(),
        oid:                   erf.oid().map(str::to_string),
        resource_list_padding: erf.resource_list_padding(),
        entry_order:           serialize_entry_order(&resman::ResContainer::contents(erf)),
        entry_algorithms:      serialize_entry_algorithms(erf),
        file_sha256s:          snapshot_packable_files(destination)?,
    };
    fs::write(
        &metadata_path,
        serde_json::to_string_pretty(&value)
            .map_err(|error| format!("failed to serialize {}: {error}", metadata_path.display()))?,
    )
    .map_err(|error| format!("failed to write {}: {error}", metadata_path.display()))?;
    Ok(())
}

pub fn write_key_pack_metadata(
    destination: &Path,
    key_path: &Path,
    key: &key::KeyTable,
    force: bool,
) -> Result<(), String> {
    let metadata_path = destination.join(PACKAGE_LOCK_FILENAME);
    ensure_output_file_ready(&metadata_path, force)?;
    let bifs = key.bifs();
    let mut bif_sha256s = BTreeMap::new();
    for bif in &bifs {
        let source = resolve_existing_key_bif_path(key_path, bif)?;
        let digest = checksums::sha256_digest(
            fs::read(&source)
                .map_err(|error| format!("failed to read {}: {error}", source.display()))?,
        )
        .to_string();
        bif_sha256s.insert(bif.clone(), digest);
    }
    let value = KeyPackMetadataFile {
        kind: MetadataKind::Key,
        source_key: key_path.to_path_buf(),
        source_key_sha256: checksums::sha256_digest(
            fs::read(key_path)
                .map_err(|error| format!("failed to read {}: {error}", key_path.display()))?,
        )
        .to_string(),
        version: key.version().into(),
        build_year: key.build_year(),
        build_day: key.build_day(),
        oid: key.oid().map(str::to_string),
        bifs,
        bif_sha256s,
        file_sha256s: snapshot_packable_files(destination)?,
    };

    fs::write(
        &metadata_path,
        serde_json::to_string_pretty(&value)
            .map_err(|error| format!("failed to serialize {}: {error}", metadata_path.display()))?,
    )
    .map_err(|error| format!("failed to write {}: {error}", metadata_path.display()))?;
    Ok(())
}

pub fn write_resource_pack_metadata(
    destination: &Path,
    input: &Path,
    source_kind: &str,
    file_name: &str,
    force: bool,
) -> Result<(), String> {
    let metadata_path = destination.join(PACKAGE_LOCK_FILENAME);
    ensure_output_file_ready(&metadata_path, force)?;
    let value = ResourcePackMetadataFile {
        kind:          MetadataKind::Resource,
        source:        input.to_path_buf(),
        source_sha256: checksums::sha256_digest(
            fs::read(input)
                .map_err(|error| format!("failed to read {}: {error}", input.display()))?,
        )
        .to_string(),
        source_kind:   source_kind.to_string(),
        file_name:     file_name.to_string(),
        file_sha256s:  snapshot_packable_files(destination)?,
    };
    fs::write(
        &metadata_path,
        serde_json::to_string_pretty(&value)
            .map_err(|error| format!("failed to serialize {}: {error}", metadata_path.display()))?,
    )
    .map_err(|error| format!("failed to write {}: {error}", metadata_path.display()))?;
    Ok(())
}

pub fn write_new_erf_pack_metadata(
    destination: &Path,
    file_type: &str,
    file_version: erf::ErfVersion,
    force: bool,
) -> Result<(), String> {
    let metadata_path = destination.join(PACKAGE_LOCK_FILENAME);
    ensure_output_file_ready(&metadata_path, force)?;
    let value = ErfPackMetadataFile {
        kind:                  MetadataKind::Erf,
        source:                PathBuf::new(),
        source_sha256:         String::new(),
        file_type:             file_type.to_string(),
        file_version:          file_version.into(),
        build_year:            0,
        build_day:             0,
        str_ref:               0,
        loc_strings:           BTreeMap::new(),
        oid:                   None,
        resource_list_padding: 0,
        entry_order:           Vec::new(),
        entry_algorithms:      BTreeMap::new(),
        file_sha256s:          snapshot_packable_files(destination)?,
    };
    fs::write(
        &metadata_path,
        serde_json::to_string_pretty(&value)
            .map_err(|error| format!("failed to serialize {}: {error}", metadata_path.display()))?,
    )
    .map_err(|error| format!("failed to write {}: {error}", metadata_path.display()))?;
    Ok(())
}

pub fn write_new_key_pack_metadata(destination: &Path, force: bool) -> Result<(), String> {
    let metadata_path = destination.join(PACKAGE_LOCK_FILENAME);
    ensure_output_file_ready(&metadata_path, force)?;
    let value = KeyPackMetadataFile {
        kind:              MetadataKind::Key,
        source_key:        PathBuf::new(),
        source_key_sha256: String::new(),
        version:           KeyPackMetadataVersion::V1,
        build_year:        0,
        build_day:         0,
        oid:               None,
        bifs:              Vec::new(),
        bif_sha256s:       BTreeMap::new(),
        file_sha256s:      snapshot_packable_files(destination)?,
    };
    fs::write(
        &metadata_path,
        serde_json::to_string_pretty(&value)
            .map_err(|error| format!("failed to serialize {}: {error}", metadata_path.display()))?,
    )
    .map_err(|error| format!("failed to write {}: {error}", metadata_path.display()))?;
    Ok(())
}

pub fn write_new_resource_pack_metadata(
    destination: &Path,
    source_kind: &str,
    file_name: &str,
    force: bool,
) -> Result<(), String> {
    let metadata_path = destination.join(PACKAGE_LOCK_FILENAME);
    ensure_output_file_ready(&metadata_path, force)?;
    let value = ResourcePackMetadataFile {
        kind:          MetadataKind::Resource,
        source:        PathBuf::new(),
        source_sha256: String::new(),
        source_kind:   source_kind.to_string(),
        file_name:     file_name.to_string(),
        file_sha256s:  snapshot_packable_files(destination)?,
    };
    fs::write(
        &metadata_path,
        serde_json::to_string_pretty(&value)
            .map_err(|error| format!("failed to serialize {}: {error}", metadata_path.display()))?,
    )
    .map_err(|error| format!("failed to write {}: {error}", metadata_path.display()))?;
    Ok(())
}

pub fn read_erf_pack_metadata(input: &Path) -> Result<Option<ErfPackMetadata>, String> {
    let Some((metadata_path, text)) = read_metadata_text(input)? else {
        return Ok(None);
    };
    if parse_metadata_kind(&text, &metadata_path)? != MetadataKind::Erf {
        return Ok(None);
    }
    let file: ErfPackMetadataFile = serde_json::from_str(&text)
        .map_err(|error| format!("failed to parse {}: {error}", metadata_path.display()))?;
    Ok(Some(ErfPackMetadata::from_file(file, &metadata_path)?))
}

pub fn read_key_pack_metadata(input: &Path) -> Result<Option<KeyPackMetadata>, String> {
    let Some((metadata_path, text)) = read_metadata_text(input)? else {
        return Ok(None);
    };
    if parse_metadata_kind(&text, &metadata_path)? != MetadataKind::Key {
        return Ok(None);
    }
    let file: KeyPackMetadataFile = serde_json::from_str(&text)
        .map_err(|error| format!("failed to parse {}: {error}", metadata_path.display()))?;
    Ok(Some(KeyPackMetadata::from_file(file)))
}

pub fn read_resource_pack_metadata(input: &Path) -> Result<Option<ResourcePackMetadata>, String> {
    let Some((metadata_path, text)) = read_metadata_text(input)? else {
        return Ok(None);
    };
    if parse_metadata_kind(&text, &metadata_path)? != MetadataKind::Resource {
        return Ok(None);
    }
    let file: ResourcePackMetadataFile = serde_json::from_str(&text)
        .map_err(|error| format!("failed to parse {}: {error}", metadata_path.display()))?;
    Ok(Some(ResourcePackMetadata::from_file(file)))
}

pub fn should_copy_original_erf(metadata: &ErfPackMetadata, input: &Path) -> Result<bool, String> {
    if !metadata.source.is_file() {
        return Ok(false);
    }
    let current = snapshot_packable_files(input)?;
    if current != metadata.file_sha256s {
        return Ok(false);
    }
    let source_sha256 = checksums::sha256_digest(
        fs::read(&metadata.source)
            .map_err(|error| format!("failed to read {}: {error}", metadata.source.display()))?,
    )
    .to_string();
    Ok(source_sha256 == metadata.source_sha256)
}

pub fn should_copy_original_key(metadata: &KeyPackMetadata, input: &Path) -> Result<bool, String> {
    if !metadata.source_key.is_file() {
        return Ok(false);
    }
    let current = snapshot_packable_files(input)?;
    if current != metadata.file_sha256s {
        return Ok(false);
    }
    let source_sha256 =
        checksums::sha256_digest(fs::read(&metadata.source_key).map_err(|error| {
            format!("failed to read {}: {error}", metadata.source_key.display())
        })?)
        .to_string();
    if source_sha256 != metadata.source_key_sha256 {
        return Ok(false);
    }
    for bif in &metadata.bifs {
        let path = resolve_existing_key_bif_path(&metadata.source_key, bif)?;
        let digest = checksums::sha256_digest(
            fs::read(&path)
                .map_err(|error| format!("failed to read {}: {error}", path.display()))?,
        )
        .to_string();
        if metadata.bif_sha256s.get(bif) != Some(&digest) {
            return Ok(false);
        }
    }
    Ok(true)
}

pub fn should_copy_original_resource(
    metadata: &ResourcePackMetadata,
    input: &Path,
) -> Result<bool, String> {
    if !metadata.source.is_file() {
        return Ok(false);
    }
    let current = snapshot_packable_files(input)?;
    if current != metadata.file_sha256s {
        return Ok(false);
    }
    let source_sha256 = checksums::sha256_digest(
        fs::read(&metadata.source)
            .map_err(|error| format!("failed to read {}: {error}", metadata.source.display()))?,
    )
    .to_string();
    Ok(source_sha256 == metadata.source_sha256)
}

pub fn copy_original_key_set(
    metadata: &KeyPackMetadata,
    output_key: &Path,
    force: bool,
) -> Result<(), String> {
    ensure_output_file_ready(output_key, force)?;
    fs::copy(&metadata.source_key, output_key).map_err(|error| {
        format!(
            "failed to copy original key {} to {}: {error}",
            metadata.source_key.display(),
            output_key.display()
        )
    })?;

    let output_root = output_key.parent().unwrap_or_else(|| Path::new("."));
    for bif in &metadata.bifs {
        let source = resolve_existing_key_bif_path(&metadata.source_key, bif)?;
        let target = output_root.join(normalize_key_bif_filename(bif));
        ensure_output_file_ready(&target, force)?;
        fs::copy(&source, &target).map_err(|error| {
            format!(
                "failed to copy original bif {} to {}: {error}",
                source.display(),
                target.display()
            )
        })?;
    }
    Ok(())
}

pub fn resolve_existing_key_bif_path(key_path: &Path, filename: &str) -> Result<PathBuf, String> {
    let parent = key_path.parent().unwrap_or_else(|| Path::new("."));
    let normalized = normalize_key_bif_filename(filename);
    let direct = parent.join(&normalized);
    if direct.is_file() {
        return Ok(direct);
    }
    if let Some(basename) = Path::new(&normalized).file_name() {
        let basename_candidate = parent.join(basename);
        if basename_candidate.is_file() {
            return Ok(basename_candidate);
        }
    }
    Err(format!(
        "key file referenced bif {} but it cannot be found beside {}",
        filename,
        key_path.display()
    ))
}

fn read_metadata_text(input: &Path) -> Result<Option<(PathBuf, String)>, String> {
    let metadata_path = if input.is_dir() {
        input.join(PACKAGE_LOCK_FILENAME)
    } else if input.file_name().and_then(|value| value.to_str()) == Some(PACKAGE_LOCK_FILENAME) {
        input.to_path_buf()
    } else {
        return Ok(None);
    };
    if !metadata_path.is_file() {
        return Ok(None);
    }
    let text = fs::read_to_string(&metadata_path)
        .map_err(|error| format!("failed to read {}: {error}", metadata_path.display()))?;
    Ok(Some((metadata_path, text)))
}

fn parse_metadata_kind(text: &str, metadata_path: &Path) -> Result<MetadataKind, String> {
    serde_json::from_str::<MetadataKindProbe>(text)
        .map(|probe| probe.kind)
        .map_err(|error| format!("failed to parse {}: {error}", metadata_path.display()))
}

fn serialize_entry_order(entry_order: &[resman::ResRef]) -> Vec<String> {
    entry_order.iter().map(ToString::to_string).collect()
}

fn parse_entry_order(
    entries: Vec<String>,
    metadata_path: &Path,
) -> Result<Vec<resman::ResRef>, String> {
    entries
        .into_iter()
        .map(|entry| {
            resman::ResolvedResRef::from_filename_preserving_case(&entry)
                .map(Into::into)
                .map_err(|error| {
                    format!(
                        "invalid metadata entry_order value {:?} in {}: {}",
                        entry,
                        metadata_path.display(),
                        error
                    )
                })
        })
        .collect()
}

fn serialize_entry_algorithms(erf: &erf::Erf) -> BTreeMap<String, u32> {
    let mut out = BTreeMap::new();
    for rr in resman::ResContainer::contents(erf) {
        let Some(res) = erf.entries().get(&rr) else {
            continue;
        };
        if let Some(algorithm) = res.compressed_buf_algorithm() {
            out.insert(rr.to_string(), algorithm as u32);
        }
    }
    out
}

fn parse_entry_algorithms(
    entries: BTreeMap<String, u32>,
    metadata_path: &Path,
) -> Result<BTreeMap<resman::ResRef, compressedbuf::Algorithm>, String> {
    entries
        .into_iter()
        .map(|(entry, algorithm)| {
            let rr = resman::ResolvedResRef::from_filename_preserving_case(&entry)
                .map(Into::into)
                .map_err(|error| {
                    format!(
                        "invalid metadata entry_algorithms key {:?} in {}: {}",
                        entry,
                        metadata_path.display(),
                        error
                    )
                })?;
            let algorithm = compressedbuf::Algorithm::from_u32(algorithm).map_err(|error| {
                format!(
                    "invalid metadata entry_algorithms value for {:?} in {}: {}",
                    entry,
                    metadata_path.display(),
                    error
                )
            })?;
            Ok((rr, algorithm))
        })
        .collect()
}

fn snapshot_packable_files(root: &Path) -> Result<BTreeMap<String, String>, String> {
    let mut result = BTreeMap::new();
    snapshot_packable_files_inner(root, root, &mut result)?;
    Ok(result)
}

fn snapshot_packable_files_inner(
    root: &Path,
    current: &Path,
    out: &mut BTreeMap<String, String>,
) -> Result<(), String> {
    for entry in sorted_dir_entries(current)? {
        if should_skip_top_level_dir(&entry.path) {
            continue;
        }
        if entry_is_dir(&entry.path, false)? {
            snapshot_packable_files_inner(root, &entry.path, out)?;
            continue;
        }
        if !entry_is_file(&entry.path, false)? || is_project_control_file(&entry.path) {
            continue;
        }
        let relative = entry
            .path
            .strip_prefix(root)
            .map_err(|error| format!("failed to relativize {}: {error}", entry.path.display()))?
            .to_string_lossy()
            .replace('\\', "/");
        let digest = checksums::sha256_digest(
            fs::read(&entry.path)
                .map_err(|error| format!("failed to read {}: {error}", entry.path.display()))?,
        )
        .to_string();
        out.insert(relative, digest);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn unique_test_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock drift before unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("nwnrs-nwpkg-{prefix}-{nanos}"))
    }

    #[test]
    fn erf_metadata_file_round_trips_with_derived_dto() -> TestResult {
        let metadata = ErfPackMetadata {
            source:                PathBuf::from("example.erf"),
            source_sha256:         "abc123".to_string(),
            file_type:             "ERF ".to_string(),
            file_version:          erf::ErfVersion::V1,
            build_year:            2025,
            build_day:             92,
            str_ref:               7,
            loc_strings:           BTreeMap::from([(0, "Hello".to_string())]),
            oid:                   Some("0123456789abcdef01234567".to_string()),
            resource_list_padding: 12,
            entry_order:           vec![resman::ResolvedResRef::from_filename("alpha.uti")?.into()],
            entry_algorithms:      BTreeMap::from([(
                resman::ResolvedResRef::from_filename("alpha.uti")?.into(),
                compressedbuf::Algorithm::Zlib,
            )]),
            file_sha256s:          BTreeMap::from([(
                "alpha.uti".to_string(),
                "deadbeef".to_string(),
            )]),
        };

        let file = ErfPackMetadataFile {
            kind:                  MetadataKind::Erf,
            source:                metadata.source.clone(),
            source_sha256:         metadata.source_sha256.clone(),
            file_type:             metadata.file_type.clone(),
            file_version:          metadata.file_version.into(),
            build_year:            metadata.build_year,
            build_day:             metadata.build_day,
            str_ref:               metadata.str_ref,
            loc_strings:           metadata.loc_strings.clone(),
            oid:                   metadata.oid.clone(),
            resource_list_padding: metadata.resource_list_padding,
            entry_order:           serialize_entry_order(&metadata.entry_order),
            entry_algorithms:      {
                let mut algorithms = BTreeMap::new();
                algorithms.insert(
                    "alpha.uti".to_string(),
                    compressedbuf::Algorithm::Zlib as u32,
                );
                algorithms
            },
            file_sha256s:          metadata.file_sha256s.clone(),
        };

        let json = serde_json::to_string(&file)?;
        let parsed = serde_json::from_str::<ErfPackMetadataFile>(&json)?;
        assert_eq!(
            ErfPackMetadata::from_file(parsed, Path::new("pack_metadata.json"))?,
            metadata
        );

        Ok(())
    }

    #[test]
    fn key_metadata_file_round_trips_with_derived_dto() -> TestResult {
        let metadata = KeyPackMetadata {
            source_key:        PathBuf::from("test.key"),
            source_key_sha256: "abc123".to_string(),
            bifs:              vec!["data/test.bif".to_string()],
            bif_sha256s:       BTreeMap::from([(
                "data/test.bif".to_string(),
                "bifhash".to_string(),
            )]),
            file_sha256s:      BTreeMap::from([("alpha.uti".to_string(), "reshash".to_string())]),
        };

        let json = serde_json::to_string(&KeyPackMetadataFile::from(metadata.clone()))?;
        let parsed = serde_json::from_str::<KeyPackMetadataFile>(&json)?;
        assert_eq!(KeyPackMetadata::from_file(parsed), metadata);

        Ok(())
    }

    #[test]
    fn entry_order_parser_uses_resolved_resref_strings() -> TestResult {
        let entries = parse_entry_order(vec!["Alpha.uti".to_string()], Path::new("meta.json"))?;
        assert_eq!(
            entries,
            vec![resman::ResRef::new("Alpha", resman::ResType(2025))?]
        );
        Ok(())
    }

    #[test]
    fn snapshot_excludes_control_files() {
        let root = unique_test_dir("snapshot");
        fs::create_dir_all(&root).expect("create temp dir");
        fs::write(root.join(PACKAGE_LOCK_FILENAME), "{}").expect("write lock");
        fs::write(root.join(crate::PROJECT_MANIFEST_FILENAME), "[project]")
            .expect("write manifest");
        fs::write(root.join("alpha.utc"), b"bytes").expect("write resource");

        let snapshot = snapshot_packable_files(&root).expect("snapshot files");
        assert_eq!(snapshot.len(), 1);
        assert!(snapshot.contains_key("alpha.utc"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn unchanged_sha256_snapshot_reuses_original_erf() -> TestResult {
        let root = unique_test_dir("original-erf-reuse");
        let source = root.join("original.mod");
        let project = root.join("project");
        fs::create_dir_all(&project)?;
        fs::write(&source, b"original archive bytes")?;
        fs::write(project.join("module.ifo.json"), b"editable source")?;

        let metadata = ErfPackMetadata {
            source:                source.clone(),
            source_sha256:         checksums::sha256_digest(fs::read(&source)?).to_string(),
            file_type:             "MOD ".to_string(),
            file_version:          erf::ErfVersion::V1,
            build_year:            0,
            build_day:             0,
            str_ref:               -1,
            loc_strings:           BTreeMap::new(),
            oid:                   None,
            resource_list_padding: 0,
            entry_order:           Vec::new(),
            entry_algorithms:      BTreeMap::new(),
            file_sha256s:          snapshot_packable_files(&project)?,
        };

        assert!(should_copy_original_erf(&metadata, &project)?);
        fs::write(project.join("module.ifo.json"), b"edited source")?;
        assert!(!should_copy_original_erf(&metadata, &project)?);

        let _ = fs::remove_dir_all(root);
        Ok(())
    }
}
