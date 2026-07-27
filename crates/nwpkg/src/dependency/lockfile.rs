use std::{fs, path::Path};

use serde::{Deserialize, Serialize};

use crate::{
    NwpkgError, NwpkgErrorKind, PACKAGE_LOCK_FILENAME, dependency::model::DependencyLock,
    fs::atomic_write,
};

#[derive(Debug, Default, Serialize, Deserialize)]
struct DependencyLockEnvelope {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dependency_resolution: Option<DependencyLock>,
}

/// Reads and validates the dependency-resolution section of `nwpkg.lock`.
///
/// # Errors
///
/// Returns an error when the lock cannot be read, decoded, or validated.
pub fn read_dependency_lock(project_root: &Path) -> Result<Option<DependencyLock>, NwpkgError> {
    let path = project_root.join(PACKAGE_LOCK_FILENAME);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(NwpkgError::at(
                NwpkgErrorKind::Io,
                &path,
                format!("failed to read dependency lock: {error}"),
            ));
        }
    };
    let envelope = toml::from_str::<DependencyLockEnvelope>(&text).map_err(|error| {
        NwpkgError::at(
            NwpkgErrorKind::Parse,
            &path,
            format!("failed to parse dependency lock: {error}"),
        )
    })?;
    if let Some(lock) = &envelope.dependency_resolution {
        lock.validate(&path)?;
    }
    Ok(envelope.dependency_resolution)
}

/// Atomically replaces the dependency-resolution section while retaining pack
/// provenance stored in the same lock document.
///
/// # Errors
///
/// Returns an error when either section is invalid or cannot be persisted.
pub fn write_dependency_lock(project_root: &Path, lock: &DependencyLock) -> Result<(), NwpkgError> {
    let path = project_root.join(PACKAGE_LOCK_FILENAME);
    lock.validate(&path)?;
    let mut document = match fs::read_to_string(&path) {
        Ok(text) => toml::from_str::<toml::Table>(&text).map_err(|error| {
            NwpkgError::at(
                NwpkgErrorKind::Parse,
                &path,
                format!("failed to parse existing lock before update: {error}"),
            )
        })?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => toml::Table::new(),
        Err(error) => {
            return Err(NwpkgError::at(
                NwpkgErrorKind::Io,
                &path,
                format!("failed to read existing lock before update: {error}"),
            ));
        }
    };
    let encoded = toml::Value::try_from(lock).map_err(|error| {
        NwpkgError::at(
            NwpkgErrorKind::Parse,
            &path,
            format!("failed to encode dependency lock: {error}"),
        )
    })?;
    document.insert("dependency_resolution".to_string(), encoded);
    let text = toml::to_string_pretty(&document).map_err(|error| {
        NwpkgError::at(
            NwpkgErrorKind::Parse,
            &path,
            format!("failed to serialize dependency lock: {error}"),
        )
    })?;
    atomic_write(&path, text.as_bytes(), true)
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{read_dependency_lock, write_dependency_lock};
    use crate::{
        DEPENDENCY_LOCK_FORMAT, DependencyLock, LockedDependency, LockedDependencySource,
        PACKAGE_LOCK_FILENAME,
    };

    fn unique_test_dir() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock drift before unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("nwnrs-dependency-lock-{nanos}"))
    }

    #[test]
    fn dependency_lock_update_preserves_pack_provenance() {
        let root = unique_test_dir();
        fs::create_dir_all(&root).expect("create lock fixture");
        fs::write(
            root.join(PACKAGE_LOCK_FILENAME),
            "format_version = 1\nkind = \"mod\"\nproject_name = \"fixture\"\n",
        )
        .expect("write pack lock section");
        let dependency = LockedDependency {
            name:            "shared".to_string(),
            source:          LockedDependencySource::Path {
                path: "../shared".into(),
            },
            manifest_sha256: "b".repeat(64),
            dependencies:    BTreeMap::new(),
        };
        let lock = DependencyLock {
            format:          DEPENDENCY_LOCK_FORMAT,
            manifest_sha256: "a".repeat(64),
            root:            BTreeMap::from([("shared".to_string(), "shared".to_string())]),
            packages:        BTreeMap::from([("shared".to_string(), dependency)]),
        };

        write_dependency_lock(&root, &lock).expect("write dependency section");
        assert_eq!(
            read_dependency_lock(&root).expect("read dependency section"),
            Some(lock)
        );
        let document = fs::read_to_string(root.join(PACKAGE_LOCK_FILENAME))
            .expect("read unified lock document");
        assert!(document.contains("format_version = 1"));
        assert!(document.contains("project_name = \"fixture\""));
        assert!(document.contains("[dependency_resolution]"));

        let _ = fs::remove_dir_all(root);
    }
}
