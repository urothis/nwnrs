use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{self, BufReader, Read, Write},
    path::{Path, PathBuf},
};

use nwnrs_nwscript::CancellationToken;
use nwnrs_types::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{
    NwpkgError, NwpkgErrorKind, PACKAGE_LOCK_FILENAME, PROJECT_MANIFEST_FILENAME,
    fs::{
        atomic_write, ensure_output_file_ready, is_project_control_file, should_skip_top_level_dir,
        sorted_dir_entries_with_cancellation, validate_portable_relative_path,
    },
};

mod key_copy;

pub use key_copy::{
    copy_original_key_set, copy_original_key_set_with_cancellation, resolve_existing_key_bif_path,
    resolve_existing_key_bif_path_with_cancellation,
};

#[derive(Debug, Clone, PartialEq, Eq)]
/// Persisted provenance and content snapshot for an ERF-family package.
pub struct ErfPackMetadata {
    /// Original archive path.
    pub source:                PathBuf,
    /// SHA-256 of the original archive.
    pub source_sha256:         String,
    /// Four-byte ERF file type.
    pub file_type:             String,
    /// ERF format version.
    pub file_version:          erf::ErfVersion,
    /// Original build year field.
    pub build_year:            i32,
    /// Original build day field.
    pub build_day:             i32,
    /// Original localized-string reference.
    pub str_ref:               i32,
    /// Embedded localized strings.
    pub loc_strings:           BTreeMap<i32, String>,
    /// Exact original byte payloads for embedded localized strings.
    pub loc_string_bytes:      BTreeMap<i32, Vec<u8>>,
    /// Optional extended archive identifier.
    pub oid:                   Option<String>,
    /// Preserved resource-list padding.
    pub resource_list_padding: u64,
    /// Stable resource entry order.
    pub entry_order:           Vec<resman::ResRef>,
    /// Per-entry compression algorithms.
    pub entry_algorithms:      BTreeMap<resman::ResRef, compressedbuf::Algorithm>,
    /// SHA-256 snapshot of editable package files.
    pub file_sha256s:          BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Persisted provenance and content snapshot for a KEY/BIF package.
pub struct KeyPackMetadata {
    /// Original KEY path.
    pub source_key:        PathBuf,
    /// SHA-256 of the original KEY.
    pub source_key_sha256: String,
    /// BIF filenames referenced by the KEY.
    pub bifs:              Vec<String>,
    /// SHA-256 values for original BIF files.
    pub bif_sha256s:       BTreeMap<String, String>,
    /// SHA-256 snapshot of editable package files.
    pub file_sha256s:      BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Persisted provenance and content snapshot for a single resource package.
pub struct ResourcePackMetadata {
    /// Original resource path.
    pub source:        PathBuf,
    /// SHA-256 of the original resource.
    pub source_sha256: String,
    /// Source representation kind.
    pub source_kind:   String,
    /// Editable source filename.
    pub file_name:     String,
    /// SHA-256 snapshot of editable package files.
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
    kind: Option<MetadataKind>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
    #[serde(default)]
    loc_string_bytes:      BTreeMap<i32, Vec<u8>>,
    oid:                   Option<String>,
    #[serde(default)]
    resource_list_padding: u64,
    #[serde(default)]
    entry_order:           Vec<String>,
    #[serde(default)]
    entry_algorithms:      BTreeMap<String, u32>,
    #[serde(default)]
    file_sha256s:          BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dependency_resolution: Option<crate::DependencyLock>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct KeyPackMetadataFile {
    kind:                  MetadataKind,
    source_key:            PathBuf,
    source_key_sha256:     String,
    version:               KeyPackMetadataVersion,
    build_year:            u32,
    build_day:             u32,
    oid:                   Option<String>,
    #[serde(default)]
    bifs:                  Vec<String>,
    #[serde(default)]
    bif_sha256s:           BTreeMap<String, String>,
    #[serde(default)]
    file_sha256s:          BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dependency_resolution: Option<crate::DependencyLock>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ResourcePackMetadataFile {
    kind:                  MetadataKind,
    source:                PathBuf,
    source_sha256:         String,
    source_kind:           String,
    file_name:             String,
    #[serde(default)]
    file_sha256s:          BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dependency_resolution: Option<crate::DependencyLock>,
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
            kind:                  MetadataKind::Key,
            source_key:            value.source_key,
            source_key_sha256:     value.source_key_sha256,
            version:               KeyPackMetadataVersion::V1,
            build_year:            0,
            build_day:             0,
            oid:                   None,
            bifs:                  value.bifs,
            bif_sha256s:           value.bif_sha256s,
            file_sha256s:          value.file_sha256s,
            dependency_resolution: None,
        }
    }
}

impl ErfPackMetadata {
    fn from_file(value: ErfPackMetadataFile, metadata_path: &Path) -> Result<Self, NwpkgError> {
        validate_optional_source_sha256(
            &value.source,
            &value.source_sha256,
            "source_sha256",
            metadata_path,
        )?;
        validate_file_snapshot(&value.file_sha256s, metadata_path)?;
        if value.file_type.len() != 4 {
            return Err(NwpkgError::at(
                NwpkgErrorKind::Validation,
                metadata_path,
                "ERF file_type must contain exactly four bytes",
            ));
        }
        Ok(Self {
            source:                value.source,
            source_sha256:         value.source_sha256,
            file_type:             value.file_type,
            file_version:          value.file_version.into(),
            build_year:            value.build_year,
            build_day:             value.build_day,
            str_ref:               value.str_ref,
            loc_strings:           value.loc_strings,
            loc_string_bytes:      value.loc_string_bytes,
            oid:                   value.oid,
            resource_list_padding: value.resource_list_padding,
            entry_order:           parse_entry_order(value.entry_order, metadata_path)?,
            entry_algorithms:      parse_entry_algorithms(value.entry_algorithms, metadata_path)?,
            file_sha256s:          value.file_sha256s,
        })
    }
}

impl KeyPackMetadata {
    fn from_file(value: KeyPackMetadataFile, metadata_path: &Path) -> Result<Self, NwpkgError> {
        validate_optional_source_sha256(
            &value.source_key,
            &value.source_key_sha256,
            "source_key_sha256",
            metadata_path,
        )?;
        validate_file_snapshot(&value.file_sha256s, metadata_path)?;
        let mut normalized_bifs = BTreeMap::<String, &str>::new();
        for bif in &value.bifs {
            validate_portable_relative_path(bif, "BIF filename", metadata_path)?;
            let logical = bif.replace('\\', "/").to_lowercase();
            if let Some(existing) = normalized_bifs.insert(logical.clone(), bif) {
                return Err(NwpkgError::at(
                    NwpkgErrorKind::Validation,
                    metadata_path,
                    format!(
                        "BIF filenames collide case-insensitively for {logical:?}: {existing:?} \
                         and {bif:?}"
                    ),
                ));
            }
        }
        let mut normalized_hashes = BTreeMap::<String, &str>::new();
        for (bif, digest) in &value.bif_sha256s {
            validate_portable_relative_path(bif, "BIF hash key", metadata_path)?;
            let logical = bif.replace('\\', "/").to_lowercase();
            if normalized_hashes.insert(logical.clone(), digest).is_some() {
                return Err(NwpkgError::at(
                    NwpkgErrorKind::Validation,
                    metadata_path,
                    format!("BIF hash keys collide case-insensitively for {logical:?}"),
                ));
            }
            validate_sha256(digest, &format!("bif_sha256s[{bif:?}]"), metadata_path)?;
        }
        let bif_names = normalized_bifs.keys().collect::<BTreeSet<_>>();
        let hash_names = normalized_hashes.keys().collect::<BTreeSet<_>>();
        if bif_names != hash_names {
            return Err(NwpkgError::at(
                NwpkgErrorKind::Validation,
                metadata_path,
                "bifs and bif_sha256s must describe exactly the same case-insensitive paths",
            ));
        }
        Ok(Self {
            source_key:        value.source_key,
            source_key_sha256: value.source_key_sha256,
            bifs:              value.bifs,
            bif_sha256s:       value.bif_sha256s,
            file_sha256s:      value.file_sha256s,
        })
    }
}

impl ResourcePackMetadata {
    fn from_file(
        value: ResourcePackMetadataFile,
        metadata_path: &Path,
    ) -> Result<Self, NwpkgError> {
        validate_optional_source_sha256(
            &value.source,
            &value.source_sha256,
            "source_sha256",
            metadata_path,
        )?;
        validate_file_snapshot(&value.file_sha256s, metadata_path)?;
        validate_portable_relative_path(&value.file_name, "resource file_name", metadata_path)?;
        Ok(Self {
            source:        value.source,
            source_sha256: value.source_sha256,
            source_kind:   value.source_kind,
            file_name:     value.file_name,
            file_sha256s:  value.file_sha256s,
        })
    }
}

/// Writes an ERF provenance lockfile using an atomic replacement.
///
/// # Errors
///
/// Returns an error when source hashing, snapshotting, serialization, or
/// persistence fails.
pub fn write_erf_pack_metadata(
    destination: &Path,
    input: &Path,
    erf: &erf::Erf,
    force: bool,
) -> Result<(), NwpkgError> {
    write_erf_pack_metadata_with_cancellation(
        destination,
        input,
        erf,
        force,
        &CancellationToken::new(),
    )
}

/// Cancellation-aware form of [`write_erf_pack_metadata`].
pub fn write_erf_pack_metadata_with_cancellation(
    destination: &Path,
    input: &Path,
    erf: &erf::Erf,
    force: bool,
    cancellation: &CancellationToken,
) -> Result<(), NwpkgError> {
    check_cancelled(cancellation)?;
    let metadata_path = destination.join(PACKAGE_LOCK_FILENAME);
    ensure_output_file_ready(&metadata_path, force)?;
    let value = ErfPackMetadataFile {
        kind:                  MetadataKind::Erf,
        source:                input.to_path_buf(),
        source_sha256:         hash_file(input, cancellation)?,
        file_type:             erf.file_type.clone(),
        file_version:          erf.file_version.into(),
        build_year:            erf.build_year,
        build_day:             erf.build_day,
        str_ref:               erf.str_ref,
        loc_strings:           erf.loc_strings().clone(),
        loc_string_bytes:      erf
            .localized_native_texts()
            .iter()
            .map(|(id, text)| text.encode().map(|bytes| (*id, bytes)))
            .collect::<Result<_, _>>()
            .map_err(|error| {
                NwpkgError::at(
                    NwpkgErrorKind::Validation,
                    input,
                    format!("failed to preserve ERF localized text: {error}"),
                )
            })?,
        oid:                   erf.oid().map(str::to_string),
        resource_list_padding: erf.resource_list_padding(),
        entry_order:           serialize_entry_order(&resman::ResContainer::contents(erf)),
        entry_algorithms:      serialize_entry_algorithms(erf),
        file_sha256s:          snapshot_packable_files(destination, cancellation)?,
        dependency_resolution: None,
    };
    ErfPackMetadata::from_file(value.clone(), &metadata_path)?;
    write_metadata(&metadata_path, &value, force)
}

/// Writes a KEY/BIF provenance lockfile using an atomic replacement.
///
/// # Errors
///
/// Returns an error when referenced files cannot be resolved or persisted.
pub fn write_key_pack_metadata(
    destination: &Path,
    key_path: &Path,
    key: &key::KeyTable,
    force: bool,
) -> Result<(), NwpkgError> {
    write_key_pack_metadata_with_cancellation(
        destination,
        key_path,
        key,
        force,
        &CancellationToken::new(),
    )
}

/// Cancellation-aware form of [`write_key_pack_metadata`].
pub fn write_key_pack_metadata_with_cancellation(
    destination: &Path,
    key_path: &Path,
    key: &key::KeyTable,
    force: bool,
    cancellation: &CancellationToken,
) -> Result<(), NwpkgError> {
    check_cancelled(cancellation)?;
    let metadata_path = destination.join(PACKAGE_LOCK_FILENAME);
    ensure_output_file_ready(&metadata_path, force)?;
    let bifs = key.bifs();
    let mut bif_sha256s = BTreeMap::new();
    for bif in &bifs {
        check_cancelled(cancellation)?;
        let source = resolve_existing_key_bif_path_with_cancellation(key_path, bif, cancellation)?;
        let digest = hash_file(&source, cancellation)?;
        bif_sha256s.insert(bif.clone(), digest);
    }
    let value = KeyPackMetadataFile {
        kind: MetadataKind::Key,
        source_key: key_path.to_path_buf(),
        source_key_sha256: hash_file(key_path, cancellation)?,
        version: key.version().into(),
        build_year: key.build_year(),
        build_day: key.build_day(),
        oid: key.oid().map(str::to_string),
        bifs,
        bif_sha256s,
        file_sha256s: snapshot_packable_files(destination, cancellation)?,
        dependency_resolution: None,
    };
    KeyPackMetadata::from_file(value.clone(), &metadata_path)?;
    write_metadata(&metadata_path, &value, force)
}

/// Writes a single-resource provenance lockfile using atomic replacement.
///
/// # Errors
///
/// Returns an error when hashing, snapshotting, serialization, or persistence
/// fails.
pub fn write_resource_pack_metadata(
    destination: &Path,
    input: &Path,
    source_kind: &str,
    file_name: &str,
    force: bool,
) -> Result<(), NwpkgError> {
    write_resource_pack_metadata_with_cancellation(
        destination,
        input,
        source_kind,
        file_name,
        force,
        &CancellationToken::new(),
    )
}

/// Cancellation-aware form of [`write_resource_pack_metadata`].
pub fn write_resource_pack_metadata_with_cancellation(
    destination: &Path,
    input: &Path,
    source_kind: &str,
    file_name: &str,
    force: bool,
    cancellation: &CancellationToken,
) -> Result<(), NwpkgError> {
    check_cancelled(cancellation)?;
    let metadata_path = destination.join(PACKAGE_LOCK_FILENAME);
    ensure_output_file_ready(&metadata_path, force)?;
    let value = ResourcePackMetadataFile {
        kind:                  MetadataKind::Resource,
        source:                input.to_path_buf(),
        source_sha256:         hash_file(input, cancellation)?,
        source_kind:           source_kind.to_string(),
        file_name:             file_name.to_string(),
        file_sha256s:          snapshot_packable_files(destination, cancellation)?,
        dependency_resolution: None,
    };
    ResourcePackMetadata::from_file(value.clone(), &metadata_path)?;
    write_metadata(&metadata_path, &value, force)
}

/// Writes initial metadata for a newly-created ERF-family package.
///
/// # Errors
///
/// Returns an error when snapshotting or persistence fails.
pub fn write_new_erf_pack_metadata(
    destination: &Path,
    file_type: &str,
    file_version: erf::ErfVersion,
    force: bool,
) -> Result<(), NwpkgError> {
    write_new_erf_pack_metadata_with_cancellation(
        destination,
        file_type,
        file_version,
        force,
        &CancellationToken::new(),
    )
}

/// Cancellation-aware form of [`write_new_erf_pack_metadata`].
pub fn write_new_erf_pack_metadata_with_cancellation(
    destination: &Path,
    file_type: &str,
    file_version: erf::ErfVersion,
    force: bool,
    cancellation: &CancellationToken,
) -> Result<(), NwpkgError> {
    check_cancelled(cancellation)?;
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
        loc_string_bytes:      BTreeMap::new(),
        oid:                   None,
        resource_list_padding: 0,
        entry_order:           Vec::new(),
        entry_algorithms:      BTreeMap::new(),
        file_sha256s:          snapshot_packable_files(destination, cancellation)?,
        dependency_resolution: None,
    };
    ErfPackMetadata::from_file(value.clone(), &metadata_path)?;
    write_metadata(&metadata_path, &value, force)
}

/// Writes initial metadata for a newly-created KEY/BIF package.
///
/// # Errors
///
/// Returns an error when snapshotting or persistence fails.
pub fn write_new_key_pack_metadata(destination: &Path, force: bool) -> Result<(), NwpkgError> {
    write_new_key_pack_metadata_with_cancellation(destination, force, &CancellationToken::new())
}

/// Cancellation-aware form of [`write_new_key_pack_metadata`].
pub fn write_new_key_pack_metadata_with_cancellation(
    destination: &Path,
    force: bool,
    cancellation: &CancellationToken,
) -> Result<(), NwpkgError> {
    check_cancelled(cancellation)?;
    let metadata_path = destination.join(PACKAGE_LOCK_FILENAME);
    ensure_output_file_ready(&metadata_path, force)?;
    let value = KeyPackMetadataFile {
        kind:                  MetadataKind::Key,
        source_key:            PathBuf::new(),
        source_key_sha256:     String::new(),
        version:               KeyPackMetadataVersion::V1,
        build_year:            0,
        build_day:             0,
        oid:                   None,
        bifs:                  Vec::new(),
        bif_sha256s:           BTreeMap::new(),
        file_sha256s:          snapshot_packable_files(destination, cancellation)?,
        dependency_resolution: None,
    };
    KeyPackMetadata::from_file(value.clone(), &metadata_path)?;
    write_metadata(&metadata_path, &value, force)
}

/// Writes initial metadata for a newly-created single-resource package.
///
/// # Errors
///
/// Returns an error when snapshotting or persistence fails.
pub fn write_new_resource_pack_metadata(
    destination: &Path,
    source_kind: &str,
    file_name: &str,
    force: bool,
) -> Result<(), NwpkgError> {
    write_new_resource_pack_metadata_with_cancellation(
        destination,
        source_kind,
        file_name,
        force,
        &CancellationToken::new(),
    )
}

/// Cancellation-aware form of [`write_new_resource_pack_metadata`].
pub fn write_new_resource_pack_metadata_with_cancellation(
    destination: &Path,
    source_kind: &str,
    file_name: &str,
    force: bool,
    cancellation: &CancellationToken,
) -> Result<(), NwpkgError> {
    check_cancelled(cancellation)?;
    let metadata_path = destination.join(PACKAGE_LOCK_FILENAME);
    ensure_output_file_ready(&metadata_path, force)?;
    let value = ResourcePackMetadataFile {
        kind:                  MetadataKind::Resource,
        source:                PathBuf::new(),
        source_sha256:         String::new(),
        source_kind:           source_kind.to_string(),
        file_name:             file_name.to_string(),
        file_sha256s:          snapshot_packable_files(destination, cancellation)?,
        dependency_resolution: None,
    };
    ResourcePackMetadata::from_file(value.clone(), &metadata_path)?;
    write_metadata(&metadata_path, &value, force)
}

/// Reads ERF metadata when `input` identifies an ERF lockfile.
///
/// # Errors
///
/// Returns an error for malformed or unreadable lockfiles.
pub fn read_erf_pack_metadata(input: &Path) -> Result<Option<ErfPackMetadata>, NwpkgError> {
    read_erf_pack_metadata_with_cancellation(input, &CancellationToken::new())
}

/// Reads ERF metadata with bounded, cancellable lockfile I/O.
pub fn read_erf_pack_metadata_with_cancellation(
    input: &Path,
    cancellation: &CancellationToken,
) -> Result<Option<ErfPackMetadata>, NwpkgError> {
    let Some((metadata_path, text)) = read_metadata_text(input, cancellation)? else {
        return Ok(None);
    };
    check_cancelled(cancellation)?;
    if parse_metadata_kind(&text, &metadata_path)? != Some(MetadataKind::Erf) {
        return Ok(None);
    }
    let file: ErfPackMetadataFile = toml::from_str(&text).map_err(|error| {
        NwpkgError::at(
            NwpkgErrorKind::Parse,
            &metadata_path,
            format!("failed to parse ERF lockfile: {error}"),
        )
    })?;
    let metadata = ErfPackMetadata::from_file(file, &metadata_path)?;
    check_cancelled(cancellation)?;
    Ok(Some(metadata))
}

/// Reads KEY/BIF metadata when `input` identifies a KEY lockfile.
///
/// # Errors
///
/// Returns an error for malformed or unreadable lockfiles.
pub fn read_key_pack_metadata(input: &Path) -> Result<Option<KeyPackMetadata>, NwpkgError> {
    read_key_pack_metadata_with_cancellation(input, &CancellationToken::new())
}

/// Reads KEY/BIF metadata with bounded, cancellable lockfile I/O.
pub fn read_key_pack_metadata_with_cancellation(
    input: &Path,
    cancellation: &CancellationToken,
) -> Result<Option<KeyPackMetadata>, NwpkgError> {
    let Some((metadata_path, text)) = read_metadata_text(input, cancellation)? else {
        return Ok(None);
    };
    check_cancelled(cancellation)?;
    if parse_metadata_kind(&text, &metadata_path)? != Some(MetadataKind::Key) {
        return Ok(None);
    }
    let file: KeyPackMetadataFile = toml::from_str(&text).map_err(|error| {
        NwpkgError::at(
            NwpkgErrorKind::Parse,
            &metadata_path,
            format!("failed to parse KEY lockfile: {error}"),
        )
    })?;
    let metadata = KeyPackMetadata::from_file(file, &metadata_path)?;
    check_cancelled(cancellation)?;
    Ok(Some(metadata))
}

/// Reads resource metadata when `input` identifies a resource lockfile.
///
/// # Errors
///
/// Returns an error for malformed or unreadable lockfiles.
pub fn read_resource_pack_metadata(
    input: &Path,
) -> Result<Option<ResourcePackMetadata>, NwpkgError> {
    read_resource_pack_metadata_with_cancellation(input, &CancellationToken::new())
}

/// Reads resource metadata with bounded, cancellable lockfile I/O.
pub fn read_resource_pack_metadata_with_cancellation(
    input: &Path,
    cancellation: &CancellationToken,
) -> Result<Option<ResourcePackMetadata>, NwpkgError> {
    let Some((metadata_path, text)) = read_metadata_text(input, cancellation)? else {
        return Ok(None);
    };
    check_cancelled(cancellation)?;
    if parse_metadata_kind(&text, &metadata_path)? != Some(MetadataKind::Resource) {
        return Ok(None);
    }
    let file: ResourcePackMetadataFile = toml::from_str(&text).map_err(|error| {
        NwpkgError::at(
            NwpkgErrorKind::Parse,
            &metadata_path,
            format!("failed to parse resource lockfile: {error}"),
        )
    })?;
    let metadata = ResourcePackMetadata::from_file(file, &metadata_path)?;
    check_cancelled(cancellation)?;
    Ok(Some(metadata))
}

/// Returns whether the original ERF is byte-reusable for the current sources.
///
/// # Errors
///
/// Returns an error when source snapshots cannot be hashed.
pub fn should_copy_original_erf(
    metadata: &ErfPackMetadata,
    input: &Path,
) -> Result<bool, NwpkgError> {
    should_copy_original_erf_with_cancellation(metadata, input, &CancellationToken::new())
}

/// Cancellation-aware form of [`should_copy_original_erf`].
pub fn should_copy_original_erf_with_cancellation(
    metadata: &ErfPackMetadata,
    input: &Path,
    cancellation: &CancellationToken,
) -> Result<bool, NwpkgError> {
    check_cancelled(cancellation)?;
    if !path_is_file(&metadata.source, "original ERF")? {
        return Ok(false);
    }
    let current = snapshot_packable_files(input, cancellation)?;
    if current != metadata.file_sha256s {
        return Ok(false);
    }
    let source_sha256 = hash_file(&metadata.source, cancellation)?;
    Ok(source_sha256 == metadata.source_sha256)
}

/// Returns whether the original KEY/BIF set is reusable for current sources.
///
/// # Errors
///
/// Returns an error when referenced files cannot be resolved or hashed.
pub fn should_copy_original_key(
    metadata: &KeyPackMetadata,
    input: &Path,
) -> Result<bool, NwpkgError> {
    should_copy_original_key_with_cancellation(metadata, input, &CancellationToken::new())
}

/// Cancellation-aware form of [`should_copy_original_key`].
pub fn should_copy_original_key_with_cancellation(
    metadata: &KeyPackMetadata,
    input: &Path,
    cancellation: &CancellationToken,
) -> Result<bool, NwpkgError> {
    check_cancelled(cancellation)?;
    if !path_is_file(&metadata.source_key, "original KEY")? {
        return Ok(false);
    }
    let current = snapshot_packable_files(input, cancellation)?;
    if current != metadata.file_sha256s {
        return Ok(false);
    }
    let source_sha256 = hash_file(&metadata.source_key, cancellation)?;
    if source_sha256 != metadata.source_key_sha256 {
        return Ok(false);
    }
    for bif in &metadata.bifs {
        check_cancelled(cancellation)?;
        let path = resolve_existing_key_bif_path_with_cancellation(
            &metadata.source_key,
            bif,
            cancellation,
        )?;
        let digest = hash_file(&path, cancellation)?;
        if metadata.bif_sha256s.get(bif) != Some(&digest) {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Returns whether the original single resource is reusable for current
/// sources.
///
/// # Errors
///
/// Returns an error when source snapshots cannot be hashed.
pub fn should_copy_original_resource(
    metadata: &ResourcePackMetadata,
    input: &Path,
) -> Result<bool, NwpkgError> {
    should_copy_original_resource_with_cancellation(metadata, input, &CancellationToken::new())
}

/// Cancellation-aware form of [`should_copy_original_resource`].
pub fn should_copy_original_resource_with_cancellation(
    metadata: &ResourcePackMetadata,
    input: &Path,
    cancellation: &CancellationToken,
) -> Result<bool, NwpkgError> {
    check_cancelled(cancellation)?;
    if !path_is_file(&metadata.source, "original resource")? {
        return Ok(false);
    }
    let current = snapshot_packable_files(input, cancellation)?;
    if current != metadata.file_sha256s {
        return Ok(false);
    }
    let source_sha256 = hash_file(&metadata.source, cancellation)?;
    Ok(source_sha256 == metadata.source_sha256)
}

fn read_metadata_text(
    input: &Path,
    cancellation: &CancellationToken,
) -> Result<Option<(PathBuf, String)>, NwpkgError> {
    check_cancelled(cancellation)?;
    let input_is_dir = match fs::metadata(input) {
        Ok(metadata) => metadata.is_dir(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(error) => {
            return Err(NwpkgError::at(
                NwpkgErrorKind::Io,
                input,
                format!("failed to inspect lockfile input: {error}"),
            ));
        }
    };
    let metadata_path = if input_is_dir {
        input.join(PACKAGE_LOCK_FILENAME)
    } else if input.file_name().and_then(|value| value.to_str()) == Some(PACKAGE_LOCK_FILENAME) {
        input.to_path_buf()
    } else {
        return Ok(None);
    };
    if !path_is_file(&metadata_path, "package lockfile")? {
        return Ok(None);
    }
    const MAX_LOCKFILE_BYTES: usize = 64 * 1024 * 1024;
    let mut file = File::open(&metadata_path).map_err(|error| {
        NwpkgError::at(
            NwpkgErrorKind::Io,
            &metadata_path,
            format!("failed to read lockfile: {error}"),
        )
    })?;
    let declared_len = file.metadata().map_err(|error| {
        NwpkgError::at(
            NwpkgErrorKind::Io,
            &metadata_path,
            format!("failed to inspect lockfile: {error}"),
        )
    })?;
    if declared_len.len() > MAX_LOCKFILE_BYTES as u64 {
        return Err(NwpkgError::at(
            NwpkgErrorKind::Validation,
            &metadata_path,
            format!(
                "lockfile contains {} bytes, exceeding maximum {MAX_LOCKFILE_BYTES}",
                declared_len.len()
            ),
        ));
    }
    let capacity = usize::try_from(declared_len.len()).map_err(|_error| {
        NwpkgError::at(
            NwpkgErrorKind::Validation,
            &metadata_path,
            "lockfile length does not fit in memory",
        )
    })?;
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(capacity).map_err(|error| {
        NwpkgError::at(
            NwpkgErrorKind::Io,
            &metadata_path,
            format!("failed to reserve lockfile buffer: {error}"),
        )
    })?;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        check_cancelled(cancellation)?;
        let count = file.read(&mut buffer).map_err(|error| {
            NwpkgError::at(
                NwpkgErrorKind::Io,
                &metadata_path,
                format!("failed to read lockfile: {error}"),
            )
        })?;
        if count == 0 {
            break;
        }
        if bytes.len().saturating_add(count) > MAX_LOCKFILE_BYTES {
            return Err(NwpkgError::at(
                NwpkgErrorKind::Validation,
                &metadata_path,
                format!("lockfile exceeds maximum {MAX_LOCKFILE_BYTES} while being read"),
            ));
        }
        let chunk = buffer.get(..count).ok_or_else(|| {
            NwpkgError::at(
                NwpkgErrorKind::Io,
                &metadata_path,
                "lockfile reader returned an invalid byte count",
            )
        })?;
        bytes.extend_from_slice(chunk);
    }
    check_cancelled(cancellation)?;
    let text = String::from_utf8(bytes).map_err(|error| {
        NwpkgError::at(
            NwpkgErrorKind::Parse,
            &metadata_path,
            format!("lockfile is not valid UTF-8: {error}"),
        )
    })?;
    Ok(Some((metadata_path, text)))
}

fn path_is_file(path: &Path, label: &str) -> Result<bool, NwpkgError> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(metadata.is_file()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(NwpkgError::at(
            NwpkgErrorKind::Io,
            path,
            format!("failed to inspect {label}: {error}"),
        )),
    }
}

fn parse_metadata_kind(
    text: &str,
    metadata_path: &Path,
) -> Result<Option<MetadataKind>, NwpkgError> {
    toml::from_str::<MetadataKindProbe>(text)
        .map(|probe| probe.kind)
        .map_err(|error| {
            NwpkgError::at(
                NwpkgErrorKind::Parse,
                metadata_path,
                format!("failed to parse lockfile kind: {error}"),
            )
        })
}

fn serialize_entry_order(entry_order: &[resman::ResRef]) -> Vec<String> {
    entry_order.iter().map(ToString::to_string).collect()
}

fn parse_entry_order(
    entries: Vec<String>,
    metadata_path: &Path,
) -> Result<Vec<resman::ResRef>, NwpkgError> {
    entries
        .into_iter()
        .map(|entry| {
            resman::ResolvedResRef::from_filename_preserving_case(&entry)
                .map(Into::into)
                .map_err(|error| {
                    NwpkgError::at(
                        NwpkgErrorKind::Validation,
                        metadata_path,
                        format!("invalid lockfile entry_order value {entry:?}: {error}"),
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
) -> Result<BTreeMap<resman::ResRef, compressedbuf::Algorithm>, NwpkgError> {
    entries
        .into_iter()
        .map(|(entry, algorithm)| {
            let rr = resman::ResolvedResRef::from_filename_preserving_case(&entry)
                .map(Into::into)
                .map_err(|error| {
                    NwpkgError::at(
                        NwpkgErrorKind::Validation,
                        metadata_path,
                        format!("invalid lockfile entry_algorithms key {entry:?}: {error}"),
                    )
                })?;
            let algorithm = compressedbuf::Algorithm::from_u32(algorithm).map_err(|error| {
                NwpkgError::at(
                    NwpkgErrorKind::Validation,
                    metadata_path,
                    format!("invalid lockfile entry_algorithms value for {entry:?}: {error}"),
                )
            })?;
            Ok((rr, algorithm))
        })
        .collect()
}

struct CancellableReader<'a, R> {
    inner:        R,
    cancellation: &'a CancellationToken,
}

impl<R: Read> Read for CancellableReader<'_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.cancellation
            .check()
            .map_err(|error| io::Error::new(io::ErrorKind::Interrupted, error.to_string()))?;
        self.inner.read(buffer)
    }
}

fn hash_file(path: &Path, cancellation: &CancellationToken) -> Result<String, NwpkgError> {
    check_cancelled(cancellation)?;
    let file = File::open(path).map_err(|error| {
        NwpkgError::at(
            NwpkgErrorKind::Io,
            path,
            format!("failed to open source for hashing: {error}"),
        )
    })?;
    checksums::sha256_digest_reader(CancellableReader {
        inner: BufReader::new(file),
        cancellation,
    })
    .map(|digest| digest.to_string())
    .map_err(|error| {
        if cancellation.is_cancelled() {
            return NwpkgError::new(NwpkgErrorKind::Cancelled, error.to_string());
        }
        NwpkgError::at(
            NwpkgErrorKind::Io,
            path,
            format!("failed to hash source: {error}"),
        )
    })
}

fn write_metadata(path: &Path, value: &impl Serialize, replace: bool) -> Result<(), NwpkgError> {
    let encoded = toml::Value::try_from(value).map_err(|error| {
        NwpkgError::at(
            NwpkgErrorKind::Parse,
            path,
            format!("failed to serialize lockfile: {error}"),
        )
    })?;
    let toml::Value::Table(mut document) = encoded else {
        return Err(NwpkgError::at(
            NwpkgErrorKind::Parse,
            path,
            "serialized lockfile root was not a TOML table",
        ));
    };
    if let Ok(existing) = fs::read_to_string(path)
        && let Ok(existing) = toml::from_str::<toml::Table>(&existing)
        && let Some(resolution) = existing.get("dependency_resolution")
    {
        document.insert("dependency_resolution".to_string(), resolution.clone());
    }
    let text = toml::to_string_pretty(&document).map_err(|error| {
        NwpkgError::at(
            NwpkgErrorKind::Parse,
            path,
            format!("failed to serialize lockfile: {error}"),
        )
    })?;
    atomic_write(path, text.as_bytes(), replace)
}

fn validate_sha256(value: &str, label: &str, metadata_path: &Path) -> Result<(), NwpkgError> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(NwpkgError::at(
            NwpkgErrorKind::Validation,
            metadata_path,
            format!("{label} must be exactly 64 hexadecimal characters"),
        ));
    }
    Ok(())
}

fn validate_optional_source_sha256(
    source: &Path,
    value: &str,
    label: &str,
    metadata_path: &Path,
) -> Result<(), NwpkgError> {
    if source.as_os_str().is_empty() && value.is_empty() {
        return Ok(());
    }
    if source.as_os_str().is_empty() || value.is_empty() {
        return Err(NwpkgError::at(
            NwpkgErrorKind::Validation,
            metadata_path,
            format!("{label} and its source path must either both be present or both be empty"),
        ));
    }
    validate_sha256(value, label, metadata_path)
}

fn validate_file_snapshot(
    snapshot: &BTreeMap<String, String>,
    metadata_path: &Path,
) -> Result<(), NwpkgError> {
    let mut logical_paths = BTreeSet::new();
    for (name, digest) in snapshot {
        validate_portable_relative_path(name, "file_sha256s key", metadata_path)?;
        let logical = name.replace('\\', "/").to_lowercase();
        if !logical_paths.insert(logical.clone()) {
            return Err(NwpkgError::at(
                NwpkgErrorKind::Validation,
                metadata_path,
                format!("file_sha256s contains a case-insensitive collision for {logical:?}"),
            ));
        }
        validate_sha256(digest, &format!("file_sha256s[{name:?}]"), metadata_path)?;
    }
    Ok(())
}

fn snapshot_packable_files(
    root: &Path,
    cancellation: &CancellationToken,
) -> Result<BTreeMap<String, String>, NwpkgError> {
    check_cancelled(cancellation)?;
    let mut result = BTreeMap::new();
    let mut pending = vec![(root.to_path_buf(), 0_usize)];
    let mut inspected = 0_usize;
    while let Some((current, depth)) = pending.pop() {
        check_cancelled(cancellation)?;
        if depth > 4_096 {
            return Err(NwpkgError::at(
                NwpkgErrorKind::Validation,
                &current,
                "package snapshot exceeded maximum directory depth 4096",
            ));
        }
        let mut children = Vec::new();
        for entry in sorted_dir_entries_with_cancellation(&current, cancellation)? {
            check_cancelled(cancellation)?;
            inspected = inspected.checked_add(1).ok_or_else(|| {
                NwpkgError::at(
                    NwpkgErrorKind::Validation,
                    &entry.path,
                    "package snapshot entry count overflowed",
                )
            })?;
            if inspected > 5_000_000 {
                return Err(NwpkgError::at(
                    NwpkgErrorKind::Validation,
                    &entry.path,
                    "package snapshot exceeded maximum of 5000000 entries",
                ));
            }
            if should_skip_top_level_dir(&entry.path) {
                continue;
            }
            let metadata = fs::symlink_metadata(&entry.path).map_err(|error| {
                NwpkgError::at(
                    NwpkgErrorKind::Io,
                    &entry.path,
                    format!("failed to inspect package entry: {error}"),
                )
            })?;
            if metadata.file_type().is_symlink() {
                return Err(NwpkgError::at(
                    NwpkgErrorKind::Validation,
                    &entry.path,
                    "package snapshot does not permit symbolic links",
                ));
            }
            if metadata.is_dir() {
                if path_is_file(
                    &entry.path.join(PROJECT_MANIFEST_FILENAME),
                    "nested package manifest",
                )? {
                    continue;
                }
                children.push(entry.path);
                continue;
            }
            if !metadata.is_file() || is_project_control_file(&entry.path) {
                continue;
            }
            let relative_path = entry.path.strip_prefix(root).map_err(|error| {
                NwpkgError::at(
                    NwpkgErrorKind::Validation,
                    &entry.path,
                    format!("failed to relativize package entry: {error}"),
                )
            })?;
            let relative = relative_path
                .to_str()
                .ok_or_else(|| {
                    NwpkgError::at(
                        NwpkgErrorKind::Validation,
                        &entry.path,
                        "package snapshot paths must be valid UTF-8",
                    )
                })?
                .replace('\\', "/");
            let digest = hash_file(&entry.path, cancellation)?;
            result.insert(relative, digest);
        }
        for child in children.into_iter().rev() {
            pending.push((child, depth + 1));
        }
    }
    Ok(result)
}

fn check_cancelled(cancellation: &CancellationToken) -> Result<(), NwpkgError> {
    cancellation
        .check()
        .map_err(|error| NwpkgError::new(NwpkgErrorKind::Cancelled, error.to_string()))
}

#[cfg(test)]
mod tests;
