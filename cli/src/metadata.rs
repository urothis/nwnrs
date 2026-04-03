use crate::util::{
    RESOURCE_METADATA_FILENAME, ensure_output_file_ready, entry_is_dir, entry_is_file,
    normalize_key_bif_filename, should_skip_top_level_dir, sorted_dir_entries,
};
use nwn_checksums::prelude::*;
use nwn_erf::prelude::*;
use nwn_key::prelude::*;
use nwn_resman::prelude::*;
use nwn_resref::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ErfPackMetadata {
    pub(crate) source: PathBuf,
    pub(crate) source_md5: String,
    pub(crate) file_type: String,
    pub(crate) file_version: ErfVersion,
    pub(crate) build_year: i32,
    pub(crate) build_day: i32,
    pub(crate) str_ref: i32,
    pub(crate) loc_strings: BTreeMap<i32, String>,
    pub(crate) oid: Option<String>,
    pub(crate) entry_order: Vec<ResRef>,
    pub(crate) file_md5s: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KeyPackMetadata {
    pub(crate) source_key: PathBuf,
    pub(crate) source_key_md5: String,
    pub(crate) bifs: Vec<String>,
    pub(crate) bif_md5s: BTreeMap<String, String>,
    pub(crate) file_md5s: BTreeMap<String, String>,
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
    kind: MetadataKind,
    source: PathBuf,
    source_md5: String,
    file_type: String,
    file_version: ErfPackMetadataVersion,
    build_year: i32,
    build_day: i32,
    str_ref: i32,
    #[serde(default)]
    loc_strings: BTreeMap<i32, String>,
    oid: Option<String>,
    #[serde(default)]
    entry_order: Vec<String>,
    #[serde(default)]
    file_md5s: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct KeyPackMetadataFile {
    kind: MetadataKind,
    source_key: PathBuf,
    source_key_md5: String,
    version: KeyPackMetadataVersion,
    build_year: u32,
    build_day: u32,
    oid: Option<String>,
    #[serde(default)]
    bifs: Vec<String>,
    #[serde(default)]
    bif_md5s: BTreeMap<String, String>,
    #[serde(default)]
    file_md5s: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ResourceMetadataFile {
    kind: MetadataKind,
    source: PathBuf,
    source_md5: String,
    source_kind: String,
    #[serde(default)]
    file_md5s: BTreeMap<String, String>,
}

impl From<ErfVersion> for ErfPackMetadataVersion {
    fn from(value: ErfVersion) -> Self {
        match value {
            ErfVersion::V1 => Self::V1,
            ErfVersion::E1 => Self::E1,
        }
    }
}

impl From<ErfPackMetadataVersion> for ErfVersion {
    fn from(value: ErfPackMetadataVersion) -> Self {
        match value {
            ErfPackMetadataVersion::V1 => Self::V1,
            ErfPackMetadataVersion::E1 => Self::E1,
        }
    }
}

impl From<KeyBifVersion> for KeyPackMetadataVersion {
    fn from(value: KeyBifVersion) -> Self {
        match value {
            KeyBifVersion::V1 => Self::V1,
            KeyBifVersion::E1 => Self::E1,
        }
    }
}

impl From<KeyPackMetadata> for KeyPackMetadataFile {
    fn from(value: KeyPackMetadata) -> Self {
        Self {
            kind: MetadataKind::Key,
            source_key: value.source_key,
            source_key_md5: value.source_key_md5,
            version: KeyPackMetadataVersion::V1,
            build_year: 0,
            build_day: 0,
            oid: None,
            bifs: value.bifs,
            bif_md5s: value.bif_md5s,
            file_md5s: value.file_md5s,
        }
    }
}

impl ErfPackMetadata {
    fn from_file(value: ErfPackMetadataFile, metadata_path: &Path) -> Result<Self, String> {
        Ok(Self {
            source: value.source,
            source_md5: value.source_md5,
            file_type: value.file_type,
            file_version: value.file_version.into(),
            build_year: value.build_year,
            build_day: value.build_day,
            str_ref: value.str_ref,
            loc_strings: value.loc_strings,
            oid: value.oid,
            entry_order: parse_entry_order(value.entry_order, metadata_path)?,
            file_md5s: value.file_md5s,
        })
    }
}

impl KeyPackMetadata {
    fn from_file(value: KeyPackMetadataFile) -> Self {
        Self {
            source_key: value.source_key,
            source_key_md5: value.source_key_md5,
            bifs: value.bifs,
            bif_md5s: value.bif_md5s,
            file_md5s: value.file_md5s,
        }
    }
}

pub(crate) fn write_erf_pack_metadata(
    destination: &Path,
    input: &Path,
    erf: &nwn_erf::Erf,
    force: bool,
) -> Result<(), String> {
    let metadata_path = destination.join(RESOURCE_METADATA_FILENAME);
    ensure_output_file_ready(&metadata_path, force)?;
    let value = ErfPackMetadataFile {
        kind: MetadataKind::Erf,
        source: input.to_path_buf(),
        source_md5: md5_digest(
            fs::read(input)
                .map_err(|error| format!("failed to read {}: {error}", input.display()))?,
        )
        .to_string(),
        file_type: erf.file_type.clone(),
        file_version: erf.file_version.into(),
        build_year: erf.build_year,
        build_day: erf.build_day,
        str_ref: erf.str_ref,
        loc_strings: erf.loc_strings().clone(),
        oid: erf.oid().map(str::to_string),
        entry_order: serialize_entry_order(&erf.contents()),
        file_md5s: snapshot_packable_files(destination)?,
    };
    fs::write(
        &metadata_path,
        serde_json::to_string_pretty(&value)
            .map_err(|error| format!("failed to serialize {}: {error}", metadata_path.display()))?,
    )
    .map_err(|error| format!("failed to write {}: {error}", metadata_path.display()))?;
    Ok(())
}

pub(crate) fn write_key_pack_metadata(
    destination: &Path,
    key_path: &Path,
    key: &nwn_key::KeyTable,
    force: bool,
) -> Result<(), String> {
    let metadata_path = destination.join(RESOURCE_METADATA_FILENAME);
    ensure_output_file_ready(&metadata_path, force)?;
    let bifs = key.bifs();
    let mut bif_md5s = BTreeMap::new();
    for bif in &bifs {
        let source = resolve_existing_key_bif_path(key_path, bif)?;
        let digest = md5_digest(
            fs::read(&source)
                .map_err(|error| format!("failed to read {}: {error}", source.display()))?,
        )
        .to_string();
        bif_md5s.insert(bif.clone(), digest);
    }
    let value = KeyPackMetadataFile {
        kind: MetadataKind::Key,
        source_key: key_path.to_path_buf(),
        source_key_md5: md5_digest(
            fs::read(key_path)
                .map_err(|error| format!("failed to read {}: {error}", key_path.display()))?,
        )
        .to_string(),
        version: key.version().into(),
        build_year: key.build_year(),
        build_day: key.build_day(),
        oid: key.oid().map(str::to_string),
        bifs,
        bif_md5s,
        file_md5s: snapshot_packable_files(destination)?,
    };

    fs::write(
        &metadata_path,
        serde_json::to_string_pretty(&value)
            .map_err(|error| format!("failed to serialize {}: {error}", metadata_path.display()))?,
    )
    .map_err(|error| format!("failed to write {}: {error}", metadata_path.display()))?;
    Ok(())
}

pub(crate) fn write_resource_metadata(
    destination: &Path,
    input: &Path,
    source_kind: &str,
    force: bool,
) -> Result<(), String> {
    let metadata_path = destination.join(RESOURCE_METADATA_FILENAME);
    ensure_output_file_ready(&metadata_path, force)?;
    let value = ResourceMetadataFile {
        kind: MetadataKind::Resource,
        source: input.to_path_buf(),
        source_md5: md5_digest(
            fs::read(input)
                .map_err(|error| format!("failed to read {}: {error}", input.display()))?,
        )
        .to_string(),
        source_kind: source_kind.to_string(),
        file_md5s: snapshot_packable_files(destination)?,
    };
    fs::write(
        &metadata_path,
        serde_json::to_string_pretty(&value)
            .map_err(|error| format!("failed to serialize {}: {error}", metadata_path.display()))?,
    )
    .map_err(|error| format!("failed to write {}: {error}", metadata_path.display()))?;
    Ok(())
}

pub(crate) fn read_erf_pack_metadata(input: &Path) -> Result<Option<ErfPackMetadata>, String> {
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

pub(crate) fn read_key_pack_metadata(input: &Path) -> Result<Option<KeyPackMetadata>, String> {
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

pub(crate) fn should_copy_original_erf(
    metadata: &ErfPackMetadata,
    input: &Path,
) -> Result<bool, String> {
    if !metadata.source.is_file() {
        return Ok(false);
    }
    let current = snapshot_packable_files(input)?;
    if current != metadata.file_md5s {
        return Ok(false);
    }
    let source_md5 = md5_digest(
        fs::read(&metadata.source)
            .map_err(|error| format!("failed to read {}: {error}", metadata.source.display()))?,
    )
    .to_string();
    Ok(source_md5 == metadata.source_md5)
}

pub(crate) fn should_copy_original_key(
    metadata: &KeyPackMetadata,
    input: &Path,
) -> Result<bool, String> {
    if !metadata.source_key.is_file() {
        return Ok(false);
    }
    let current = snapshot_packable_files(input)?;
    if current != metadata.file_md5s {
        return Ok(false);
    }
    let source_md5 =
        md5_digest(fs::read(&metadata.source_key).map_err(|error| {
            format!("failed to read {}: {error}", metadata.source_key.display())
        })?)
        .to_string();
    if source_md5 != metadata.source_key_md5 {
        return Ok(false);
    }
    for bif in &metadata.bifs {
        let path = resolve_existing_key_bif_path(&metadata.source_key, bif)?;
        let digest = md5_digest(
            fs::read(&path)
                .map_err(|error| format!("failed to read {}: {error}", path.display()))?,
        )
        .to_string();
        if metadata.bif_md5s.get(bif) != Some(&digest) {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(crate) fn copy_original_key_set(
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

pub(crate) fn resolve_existing_key_bif_path(
    key_path: &Path,
    filename: &str,
) -> Result<PathBuf, String> {
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
    let metadata_path = input.join(RESOURCE_METADATA_FILENAME);
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

fn serialize_entry_order(entry_order: &[ResRef]) -> Vec<String> {
    entry_order.iter().map(ToString::to_string).collect()
}

fn parse_entry_order(entries: Vec<String>, metadata_path: &Path) -> Result<Vec<ResRef>, String> {
    entries
        .into_iter()
        .map(|entry| {
            new_resolved_res_ref_from_filename(&entry)
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
        if !entry_is_file(&entry.path, false)? || is_pack_metadata_file(&entry.path) {
            continue;
        }
        let relative = entry
            .path
            .strip_prefix(root)
            .map_err(|error| format!("failed to relativize {}: {error}", entry.path.display()))?
            .to_string_lossy()
            .replace('\\', "/");
        let digest = md5_digest(
            fs::read(&entry.path)
                .map_err(|error| format!("failed to read {}: {error}", entry.path.display()))?,
        )
        .to_string();
        out.insert(relative, digest);
    }
    Ok(())
}

fn is_pack_metadata_file(path: &Path) -> bool {
    path.file_name().and_then(|name| name.to_str()) == Some(RESOURCE_METADATA_FILENAME)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nwn_restype::ResType;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn erf_metadata_file_round_trips_with_derived_dto() -> TestResult {
        let metadata = ErfPackMetadata {
            source: PathBuf::from("example.erf"),
            source_md5: "abc123".to_string(),
            file_type: "ERF ".to_string(),
            file_version: ErfVersion::V1,
            build_year: 2025,
            build_day: 92,
            str_ref: 7,
            loc_strings: BTreeMap::from([(0, "Hello".to_string())]),
            oid: Some("0123456789abcdef01234567".to_string()),
            entry_order: vec![new_resolved_res_ref_from_filename("alpha.uti")?.into()],
            file_md5s: BTreeMap::from([("alpha.uti".to_string(), "deadbeef".to_string())]),
        };

        let file = ErfPackMetadataFile {
            kind: MetadataKind::Erf,
            source: metadata.source.clone(),
            source_md5: metadata.source_md5.clone(),
            file_type: metadata.file_type.clone(),
            file_version: metadata.file_version.into(),
            build_year: metadata.build_year,
            build_day: metadata.build_day,
            str_ref: metadata.str_ref,
            loc_strings: metadata.loc_strings.clone(),
            oid: metadata.oid.clone(),
            entry_order: serialize_entry_order(&metadata.entry_order),
            file_md5s: metadata.file_md5s.clone(),
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
            source_key: PathBuf::from("test.key"),
            source_key_md5: "abc123".to_string(),
            bifs: vec!["data/test.bif".to_string()],
            bif_md5s: BTreeMap::from([("data/test.bif".to_string(), "bifhash".to_string())]),
            file_md5s: BTreeMap::from([("alpha.uti".to_string(), "reshash".to_string())]),
        };

        let json = serde_json::to_string(&KeyPackMetadataFile::from(metadata.clone()))?;
        let parsed = serde_json::from_str::<KeyPackMetadataFile>(&json)?;
        assert_eq!(KeyPackMetadata::from_file(parsed), metadata);

        Ok(())
    }

    #[test]
    fn entry_order_parser_uses_resolved_resref_strings() -> TestResult {
        let entries = parse_entry_order(vec!["alpha.uti".to_string()], Path::new("meta.json"))?;
        assert_eq!(
            entries,
            vec![nwn_resref::new_res_ref("alpha", ResType(2025))?]
        );
        Ok(())
    }
}
