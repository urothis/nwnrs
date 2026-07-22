#![allow(missing_docs)]

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{PROJECT_MANIFEST_FILENAME, ProjectKind, fs::ensure_output_file_ready};

/// Typed `nwpkg.toml` contents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectManifest {
    pub project:      ProjectSection,
    pub source:       SourceSection,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dependencies: BTreeMap<String, DependencySpec>,
}

/// One project dependency. Only local paths are supported until the Git
/// resolver and lock semantics are implemented.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DependencySpec {
    Path(PathDependency),
}

/// A dependency resolved relative to the declaring `nwpkg.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathDependency {
    pub path: PathBuf,
}

/// Top-level project metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectSection {
    pub name: String,
    pub kind: ProjectKind,
}

/// Source-root metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSection {
    pub path: PathBuf,
}

impl ProjectManifest {
    /// Creates one manifest from explicit values.
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        kind: ProjectKind,
        source_path: impl Into<PathBuf>,
    ) -> Self {
        Self {
            project:      ProjectSection {
                name: name.into(),
                kind,
            },
            source:       SourceSection {
                path: source_path.into(),
            },
            dependencies: BTreeMap::new(),
        }
    }
}

pub fn write_project_manifest(
    destination: &Path,
    kind: ProjectKind,
    name: &str,
    source_path: &str,
    force: bool,
) -> Result<(), String> {
    let manifest_path = destination.join(PROJECT_MANIFEST_FILENAME);
    ensure_output_file_ready(&manifest_path, force)?;
    let manifest = ProjectManifest::new(name, kind, source_path);
    let text = toml::to_string_pretty(&manifest)
        .map_err(|error| format!("failed to serialize {}: {error}", manifest_path.display()))?;
    fs::write(&manifest_path, text)
        .map_err(|error| format!("failed to write {}: {error}", manifest_path.display()))
}

pub fn read_project_manifest(input: &Path) -> Result<Option<ProjectManifest>, String> {
    let manifest_path = if input.is_dir() {
        input.join(PROJECT_MANIFEST_FILENAME)
    } else if input.file_name().and_then(|value| value.to_str()) == Some(PROJECT_MANIFEST_FILENAME)
    {
        input.to_path_buf()
    } else {
        return Ok(None);
    };
    if !manifest_path.is_file() {
        return Ok(None);
    }
    let text = fs::read_to_string(&manifest_path)
        .map_err(|error| format!("failed to read {}: {error}", manifest_path.display()))?;
    let manifest = toml::from_str::<ProjectManifest>(&text)
        .map_err(|error| format!("failed to parse {}: {error}", manifest_path.display()))?;
    Ok(Some(manifest))
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    use crate::{
        PROJECT_MANIFEST_FILENAME, ProjectKind, read_project_manifest, write_project_manifest,
    };

    fn unique_test_dir(prefix: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock drift before unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("nwnrs-nwpkg-{prefix}-{nanos}"))
    }

    #[test]
    fn manifest_writes_and_reads_as_toml() {
        let root = unique_test_dir("manifest-roundtrip");
        fs::create_dir_all(&root).expect("create temp dir");

        write_project_manifest(&root, ProjectKind::Mod, "example", "src", false)
            .expect("write manifest");
        let manifest = read_project_manifest(&root)
            .expect("read manifest")
            .expect("manifest exists");

        assert_eq!(manifest.project.name, "example");
        assert_eq!(manifest.project.kind, ProjectKind::Mod);
        assert_eq!(manifest.source.path, std::path::PathBuf::from("src"));
        assert!(root.join(PROJECT_MANIFEST_FILENAME).is_file());

        let _ = fs::remove_dir_all(root);
    }
}
