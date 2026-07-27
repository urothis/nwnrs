use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::Read,
    path::{Component, Path, PathBuf},
};

use nwnrs_nwscript::CancellationToken;
use serde::{Deserialize, Serialize};

use crate::{
    NwpkgError, NwpkgErrorKind, PROJECT_MANIFEST_FILENAME, ProjectKind, ProjectLayout,
    fs::{atomic_write, ensure_output_file_ready},
};

/// Editor-facing value category for one manifest field.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManifestValueKind {
    /// Arbitrary TOML string.
    String,
    /// One of [`ProjectKind::all`].
    ProjectKind,
    /// One of [`ProjectLanguage::all`].
    ProjectLanguage,
    /// Filesystem path relative to the manifest.
    Path,
    /// Inline dependency table containing a local path.
    Dependency,
}

/// Authoritative editor schema for one manifest field.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ManifestFieldSchema {
    /// TOML field name.
    pub name:          &'static str,
    /// Human-readable field documentation.
    pub documentation: &'static str,
    /// Expected value category.
    pub value_kind:    ManifestValueKind,
    /// Whether strict deserialization requires this field.
    pub required:      bool,
}

/// Authoritative editor schema for one manifest table.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ManifestSectionSchema {
    /// Dotted TOML table path.
    pub path:          &'static [&'static str],
    /// Human-readable section documentation.
    pub documentation: &'static str,
    /// Fixed fields supported by the section.
    pub fields:        &'static [ManifestFieldSchema],
    /// Whether arbitrary dependency names are accepted as keys.
    pub dynamic_keys:  bool,
}

macro_rules! manifest_section {
    (
        $schema:ident,
        $(#[$section_meta:meta])*
        pub struct $section:ident {
            $(
                $(#[$field_meta:meta])*
                $field:ident: $type:ty => {
                    documentation: $documentation:literal,
                    value_kind: $value_kind:expr,
                    required: $required:literal
                }
            ),* $(,)?
        }
    ) => {
        const $schema: &[ManifestFieldSchema] = &[
            $(
                ManifestFieldSchema {
                    name: stringify!($field),
                    documentation: $documentation,
                    value_kind: $value_kind,
                    required: $required,
                },
            )*
        ];

        $(#[$section_meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(deny_unknown_fields)]
        pub struct $section {
            $(
                #[doc = $documentation]
                $(#[$field_meta])*
                pub $field: $type,
            )*
        }
    };
}

manifest_section! {
    PROJECT_FIELDS,
    /// Top-level project metadata.
    pub struct ProjectSection {
        name: String => {
            documentation: "Stable human-readable package name.",
            value_kind: ManifestValueKind::String,
            required: true
        },
        kind: ProjectKind => {
            documentation: "Resource or archive kind produced by this package.",
            value_kind: ManifestValueKind::ProjectKind,
            required: true
        },
        #[serde(default)]
        language: ProjectLanguage => {
            documentation: "Language policy for native package resources. Defaults to English.",
            value_kind: ManifestValueKind::ProjectLanguage,
            required: false
        }
    }
}

manifest_section! {
    SOURCE_FIELDS,
    /// Source-root metadata.
    pub struct SourceSection {
        path: PathBuf => {
            documentation: "Source file or directory relative to nwpkg.toml.",
            value_kind: ManifestValueKind::Path,
            required: true
        }
    }
}
const MANIFEST_SECTIONS: &[ManifestSectionSchema] = &[
    ManifestSectionSchema {
        path:          &["project"],
        documentation: "Package identity and output kind.",
        fields:        PROJECT_FIELDS,
        dynamic_keys:  false,
    },
    ManifestSectionSchema {
        path:          &["source"],
        documentation: "Package source-root configuration.",
        fields:        SOURCE_FIELDS,
        dynamic_keys:  false,
    },
    ManifestSectionSchema {
        path:          &["dependencies"],
        documentation: "Named local-path or Git include-package dependencies. Git sources are \
                        resolved explicitly and pinned in nwpkg.lock.",
        fields:        &[],
        dynamic_keys:  true,
    },
];

/// Returns the editor schema derived from the strict manifest model.
#[must_use]
pub fn manifest_schema() -> &'static [ManifestSectionSchema] {
    MANIFEST_SECTIONS
}

/// Typed `nwpkg.toml` contents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectManifest {
    /// Project identity and output kind.
    pub project:      ProjectSection,
    /// Source path relative to the manifest directory.
    pub source:       SourceSection,
    /// Named local dependencies.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dependencies: BTreeMap<String, DependencySpec>,
}

/// One project dependency. Only local paths are supported until the Git
/// resolver and lock semantics are implemented.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DependencySpec {
    /// A package located at a filesystem path.
    Path(PathDependency),
    /// A package resolved from a Git repository and pinned by `nwpkg.lock`.
    Git(GitDependency),
}

/// A dependency resolved relative to the declaring `nwpkg.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathDependency {
    /// Path relative to the manifest that declares the dependency.
    pub path: PathBuf,
}

/// A dependency resolved from a Git repository.
///
/// At most one selector may be supplied. Omitting every selector follows the
/// repository's default branch, while the lockfile always records the exact
/// resolved commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitDependency {
    /// Repository URL accepted by the configured system Git executable.
    pub git:    String,
    /// Exact commit-ish to resolve.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rev:    Option<String>,
    /// Tag to resolve.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag:    Option<String>,
    /// Branch to resolve.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// Optional package directory within the repository.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subdir: Option<PathBuf>,
}

impl GitDependency {
    /// Validates selector exclusivity and repository-relative paths.
    ///
    /// # Errors
    ///
    /// Returns a field-associated validation error for ambiguous selectors,
    /// embedded URL credentials, or an unsafe subdirectory.
    pub fn validate(&self, name: &str) -> Result<(), NwpkgError> {
        let field = ["dependencies", name];
        if self.git.trim().is_empty() {
            return Err(NwpkgError::manifest_field(
                NwpkgErrorKind::Validation,
                &field,
                format!("dependency {name:?} has an empty git URL"),
            ));
        }
        if contains_url_credentials(&self.git) {
            return Err(NwpkgError::manifest_field(
                NwpkgErrorKind::Validation,
                &field,
                format!(
                    "dependency {name:?} embeds credentials in its git URL; use an SSH agent or \
                     system credential helper"
                ),
            ));
        }
        let selectors = [self.rev.as_ref(), self.tag.as_ref(), self.branch.as_ref()]
            .into_iter()
            .flatten()
            .count();
        if selectors > 1 {
            return Err(NwpkgError::manifest_field(
                NwpkgErrorKind::Validation,
                &field,
                format!("dependency {name:?} may define only one of rev, tag, or branch"),
            ));
        }
        for (selector_name, selector) in [
            ("rev", self.rev.as_deref()),
            ("tag", self.tag.as_deref()),
            ("branch", self.branch.as_deref()),
        ] {
            if selector.is_some_and(|value| value.trim().is_empty()) {
                return Err(NwpkgError::manifest_field(
                    NwpkgErrorKind::Validation,
                    &field,
                    format!("dependency {name:?} has an empty {selector_name} selector"),
                ));
            }
        }
        if let Some(subdir) = &self.subdir
            && (subdir.as_os_str().is_empty()
                || subdir.is_absolute()
                || subdir.components().any(|component| {
                    matches!(
                        component,
                        Component::ParentDir | Component::RootDir | Component::Prefix(_)
                    )
                }))
        {
            return Err(NwpkgError::manifest_field(
                NwpkgErrorKind::Validation,
                &field,
                format!("dependency {name:?} subdir must be a non-empty repository-relative path"),
            ));
        }
        Ok(())
    }
}

fn contains_url_credentials(url: &str) -> bool {
    let Some((scheme, remainder)) = url.split_once("://") else {
        return false;
    };
    remainder
        .split(['/', '?', '#'])
        .next()
        .and_then(|authority| authority.rsplit_once('@').map(|(user_info, _)| user_info))
        .is_some_and(|user_info| {
            user_info.contains(':')
                || !matches!(scheme.to_ascii_lowercase().as_str(), "ssh" | "git+ssh")
        })
}

/// Language policy for native resources in an nwpkg project.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProjectLanguage {
    /// English resources encoded with Windows-1252.
    #[default]
    English,
    /// French resources encoded with Windows-1252.
    French,
    /// German resources encoded with Windows-1252.
    German,
    /// Italian resources encoded with Windows-1252.
    Italian,
    /// Spanish resources encoded with Windows-1252.
    Spanish,
    /// Polish resources encoded with Windows-1250.
    Polish,
}

impl ProjectLanguage {
    /// Returns every supported package language in stable presentation order.
    #[must_use]
    pub const fn all() -> &'static [Self] {
        &[
            Self::English,
            Self::French,
            Self::German,
            Self::Italian,
            Self::Spanish,
            Self::Polish,
        ]
    }

    /// Returns the canonical manifest spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::English => "english",
            Self::French => "french",
            Self::German => "german",
            Self::Italian => "italian",
            Self::Spanish => "spanish",
            Self::Polish => "polish",
        }
    }

    /// Returns the numeric Neverwinter Nights language identifier.
    #[must_use]
    pub const fn id(self) -> u32 {
        match self {
            Self::English => 0,
            Self::French => 1,
            Self::German => 2,
            Self::Italian => 3,
            Self::Spanish => 4,
            Self::Polish => 5,
        }
    }
}

impl ProjectManifest {
    /// Creates one manifest from explicit values.
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        kind: ProjectKind,
        source_path: impl Into<PathBuf>,
    ) -> Self {
        Self::new_with_language(name, kind, source_path, ProjectLanguage::English)
    }

    /// Creates one manifest with an explicit native-resource language policy.
    #[must_use]
    pub fn new_with_language(
        name: impl Into<String>,
        kind: ProjectKind,
        source_path: impl Into<PathBuf>,
        language: ProjectLanguage,
    ) -> Self {
        Self {
            project:      ProjectSection {
                name: name.into(),
                kind,
                language,
            },
            source:       SourceSection {
                path: source_path.into(),
            },
            dependencies: BTreeMap::new(),
        }
    }

    /// Validates package identity, source confinement, and layout.
    ///
    /// `project_root` is the directory containing `nwpkg.toml`.
    ///
    /// # Errors
    ///
    /// Returns a validation error for empty names, unsafe source paths,
    /// missing sources, or a file/directory mismatch for the project kind.
    pub fn validate(&self, project_root: &Path) -> Result<(), NwpkgError> {
        if self.project.name.trim().is_empty() {
            return Err(NwpkgError::manifest_field(
                NwpkgErrorKind::Validation,
                &["project", "name"],
                "project.name must not be empty",
            ));
        }
        if self.source.path.as_os_str().is_empty() {
            return Err(NwpkgError::manifest_field(
                NwpkgErrorKind::Validation,
                &["source", "path"],
                "source.path must not be empty",
            ));
        }
        if self.source.path.is_absolute()
            || self
                .source
                .path
                .components()
                .any(|component| matches!(component, Component::ParentDir | Component::Prefix(_)))
        {
            return Err(NwpkgError::at_manifest_field(
                NwpkgErrorKind::Validation,
                &self.source.path,
                &["source", "path"],
                "source.path must remain relative to the package root",
            ));
        }

        let canonical_root = fs::canonicalize(project_root).map_err(|error| {
            NwpkgError::at(
                NwpkgErrorKind::Validation,
                project_root,
                format!("package root is unavailable: {error}"),
            )
        })?;
        let source = project_root.join(&self.source.path);
        let canonical_source = fs::canonicalize(&source).map_err(|error| {
            NwpkgError::at_manifest_field(
                NwpkgErrorKind::Validation,
                &source,
                &["source", "path"],
                format!("package source is unavailable: {error}"),
            )
        })?;
        if !canonical_source.starts_with(&canonical_root) {
            return Err(NwpkgError::at_manifest_field(
                NwpkgErrorKind::Validation,
                &source,
                &["source", "path"],
                format!(
                    "source.path resolves outside package root {}",
                    canonical_root.display()
                ),
            ));
        }
        let expects_directory = !matches!(self.project.kind.layout(), ProjectLayout::Resource);
        let source_metadata = fs::metadata(&canonical_source).map_err(|error| {
            NwpkgError::at_manifest_field(
                NwpkgErrorKind::Io,
                &canonical_source,
                &["source", "path"],
                format!("failed to inspect package source: {error}"),
            )
        })?;
        if expects_directory != source_metadata.is_dir() {
            let expected = if expects_directory {
                "directory"
            } else {
                "file"
            };
            return Err(NwpkgError::at_manifest_field(
                NwpkgErrorKind::Validation,
                source,
                &["source", "path"],
                format!(
                    "{} packages require source.path to identify a {expected}",
                    self.project.kind
                ),
            ));
        }
        for (name, dependency) in &self.dependencies {
            if name.trim().is_empty() {
                return Err(NwpkgError::manifest_field(
                    NwpkgErrorKind::Validation,
                    &["dependencies"],
                    "dependency names must not be empty",
                ));
            }
            match dependency {
                DependencySpec::Path(dependency) => {
                    if dependency.path.as_os_str().is_empty() {
                        return Err(NwpkgError::manifest_field(
                            NwpkgErrorKind::Validation,
                            &["dependencies", name],
                            format!("dependency {name:?} has an empty path"),
                        ));
                    }
                }
                DependencySpec::Git(dependency) => dependency.validate(name)?,
            }
        }
        Ok(())
    }
}

/// Writes a validated project manifest using atomic replacement.
///
/// # Errors
///
/// Returns an error when the requested manifest is invalid or cannot be
/// serialized or persisted.
pub fn write_project_manifest(
    destination: &Path,
    kind: ProjectKind,
    name: &str,
    source_path: &str,
    force: bool,
) -> Result<(), NwpkgError> {
    write_project_manifest_with_language(
        destination,
        kind,
        name,
        source_path,
        ProjectLanguage::English,
        force,
    )
}

/// Writes a validated project manifest with an explicit resource language.
///
/// # Errors
///
/// Returns an error when the requested manifest is invalid or cannot be
/// serialized or persisted.
pub fn write_project_manifest_with_language(
    destination: &Path,
    kind: ProjectKind,
    name: &str,
    source_path: &str,
    language: ProjectLanguage,
    force: bool,
) -> Result<(), NwpkgError> {
    let manifest_path = destination.join(PROJECT_MANIFEST_FILENAME);
    ensure_output_file_ready(&manifest_path, force)?;
    let manifest = ProjectManifest::new_with_language(name, kind, source_path, language);
    manifest.validate(destination)?;
    let text = toml::to_string_pretty(&manifest).map_err(|error| {
        NwpkgError::at(
            NwpkgErrorKind::Parse,
            &manifest_path,
            format!("failed to serialize manifest: {error}"),
        )
    })?;
    atomic_write(&manifest_path, text.as_bytes(), force)
}

/// Reads and validates a project manifest from a directory or manifest path.
///
/// # Errors
///
/// Returns an error when the manifest cannot be read, parsed, or validated.
pub fn read_project_manifest(input: &Path) -> Result<Option<ProjectManifest>, NwpkgError> {
    read_project_manifest_with_cancellation(input, &CancellationToken::new())
}

/// Reads and validates a project manifest with bounded, cancellable I/O.
///
/// # Errors
///
/// Returns an error when the manifest cannot be read, is larger than the
/// manifest policy, is not UTF-8, cannot be parsed, fails validation, or the
/// operation is cancelled.
pub fn read_project_manifest_with_cancellation(
    input: &Path,
    cancellation: &CancellationToken,
) -> Result<Option<ProjectManifest>, NwpkgError> {
    check_cancelled(cancellation)?;
    let input_is_dir = match fs::metadata(input) {
        Ok(metadata) => metadata.is_dir(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => {
            return Err(NwpkgError::at(
                NwpkgErrorKind::Io,
                input,
                format!("failed to inspect manifest input: {error}"),
            ));
        }
    };
    let manifest_path = if input_is_dir {
        input.join(PROJECT_MANIFEST_FILENAME)
    } else if input.file_name().and_then(|value| value.to_str()) == Some(PROJECT_MANIFEST_FILENAME)
    {
        input.to_path_buf()
    } else {
        return Ok(None);
    };
    match fs::metadata(&manifest_path) {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => return Ok(None),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(NwpkgError::at(
                NwpkgErrorKind::Io,
                &manifest_path,
                format!("failed to inspect manifest: {error}"),
            ));
        }
    }
    const MAX_MANIFEST_BYTES: usize = 16 * 1024 * 1024;
    let mut file = File::open(&manifest_path).map_err(|error| {
        NwpkgError::at(
            NwpkgErrorKind::Io,
            &manifest_path,
            format!("failed to read manifest: {error}"),
        )
    })?;
    let declared_len = file
        .metadata()
        .map_err(|error| {
            NwpkgError::at(
                NwpkgErrorKind::Io,
                &manifest_path,
                format!("failed to inspect manifest: {error}"),
            )
        })?
        .len();
    if declared_len > MAX_MANIFEST_BYTES as u64 {
        return Err(NwpkgError::at(
            NwpkgErrorKind::Validation,
            &manifest_path,
            format!(
                "manifest contains {declared_len} bytes, exceeding maximum {MAX_MANIFEST_BYTES}"
            ),
        ));
    }
    let mut bytes = Vec::new();
    let capacity = usize::try_from(declared_len).map_err(|_error| {
        NwpkgError::at(
            NwpkgErrorKind::Validation,
            &manifest_path,
            "manifest length does not fit in memory",
        )
    })?;
    bytes.try_reserve_exact(capacity).map_err(|error| {
        NwpkgError::at(
            NwpkgErrorKind::Io,
            &manifest_path,
            format!("failed to reserve manifest buffer: {error}"),
        )
    })?;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        check_cancelled(cancellation)?;
        let count = file.read(&mut buffer).map_err(|error| {
            NwpkgError::at(
                NwpkgErrorKind::Io,
                &manifest_path,
                format!("failed to read manifest: {error}"),
            )
        })?;
        if count == 0 {
            break;
        }
        if bytes.len().saturating_add(count) > MAX_MANIFEST_BYTES {
            return Err(NwpkgError::at(
                NwpkgErrorKind::Validation,
                &manifest_path,
                format!("manifest exceeds maximum {MAX_MANIFEST_BYTES} while being read"),
            ));
        }
        let chunk = buffer.get(..count).ok_or_else(|| {
            NwpkgError::at(
                NwpkgErrorKind::Io,
                &manifest_path,
                "manifest reader returned an invalid byte count",
            )
        })?;
        bytes.extend_from_slice(chunk);
    }
    check_cancelled(cancellation)?;
    let text = std::str::from_utf8(&bytes).map_err(|error| {
        NwpkgError::at(
            NwpkgErrorKind::Parse,
            &manifest_path,
            format!("manifest is not valid UTF-8: {error}"),
        )
    })?;
    let manifest = toml::from_str::<ProjectManifest>(text).map_err(|error| {
        NwpkgError::at(
            NwpkgErrorKind::Parse,
            &manifest_path,
            format!("failed to parse manifest: {error}"),
        )
    })?;
    let project_root = manifest_path.parent().unwrap_or_else(|| Path::new("."));
    manifest.validate(project_root)?;
    check_cancelled(cancellation)?;
    Ok(Some(manifest))
}

fn check_cancelled(cancellation: &CancellationToken) -> Result<(), NwpkgError> {
    cancellation
        .check()
        .map_err(|error| NwpkgError::new(NwpkgErrorKind::Cancelled, error.to_string()))
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    use nwnrs_nwscript::CancellationToken;

    use crate::{
        PROJECT_MANIFEST_FILENAME, ProjectKind, ProjectLanguage, read_project_manifest,
        read_project_manifest_with_cancellation, write_project_manifest,
        write_project_manifest_with_language,
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
        fs::create_dir_all(root.join("src")).expect("create source dir");

        write_project_manifest(&root, ProjectKind::Mod, "example", "src", false)
            .expect("write manifest");
        let manifest = read_project_manifest(&root)
            .expect("read manifest")
            .expect("manifest exists");

        assert_eq!(manifest.project.name, "example");
        assert_eq!(manifest.project.kind, ProjectKind::Mod);
        assert_eq!(manifest.project.language, ProjectLanguage::English);
        assert_eq!(manifest.source.path, std::path::PathBuf::from("src"));
        assert!(root.join(PROJECT_MANIFEST_FILENAME).is_file());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn manifest_persists_explicit_native_resource_language() {
        let root = unique_test_dir("manifest-language");
        fs::create_dir_all(root.join("src")).expect("create source dir");

        write_project_manifest_with_language(
            &root,
            ProjectKind::Mod,
            "polish",
            "src",
            ProjectLanguage::Polish,
            false,
        )
        .expect("write manifest");
        let manifest = read_project_manifest(&root)
            .expect("read manifest")
            .expect("manifest exists");

        assert_eq!(manifest.project.language, ProjectLanguage::Polish);
        assert_eq!(manifest.project.language.id(), 5);
        let source =
            fs::read_to_string(root.join(PROJECT_MANIFEST_FILENAME)).expect("read manifest source");
        assert!(source.contains("language = \"polish\""));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn manifest_without_language_defaults_to_english() {
        let root = unique_test_dir("manifest-default-language");
        fs::create_dir_all(root.join("src")).expect("create source dir");
        fs::write(
            root.join(PROJECT_MANIFEST_FILENAME),
            "[project]\nname = \"legacy\"\nkind = \"mod\"\n\n[source]\npath = \"src\"\n",
        )
        .expect("write manifest");

        let manifest = read_project_manifest(&root)
            .expect("read manifest")
            .expect("manifest exists");
        assert_eq!(manifest.project.language, ProjectLanguage::English);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn manifest_rejects_unknown_fields_and_unsafe_sources() {
        let root = unique_test_dir("manifest-strict");
        fs::create_dir_all(root.join("src")).expect("create source dir");
        fs::write(
            root.join(PROJECT_MANIFEST_FILENAME),
            r#"
[project]
name = "strict"
kind = "mod"
unexpected = true

[source]
path = "src"
"#,
        )
        .expect("write invalid manifest");
        let unknown = read_project_manifest(&root).expect_err("reject unknown field");
        assert_eq!(unknown.kind(), crate::NwpkgErrorKind::Parse);

        fs::write(
            root.join(PROJECT_MANIFEST_FILENAME),
            r#"
[project]
name = "strict"
kind = "mod"

[source]
path = "../outside"
"#,
        )
        .expect("write escaping manifest");
        let escaped = read_project_manifest(&root).expect_err("reject escaping source");
        assert_eq!(escaped.kind(), crate::NwpkgErrorKind::Validation);
        assert_eq!(
            escaped.manifest_field_path(),
            Some(&["source".to_string(), "path".to_string()][..])
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn manifest_rejects_ambiguous_or_unsafe_git_dependencies() {
        let root = unique_test_dir("manifest-git-validation");
        fs::create_dir_all(root.join("src")).expect("create source dir");
        let path = root.join(PROJECT_MANIFEST_FILENAME);
        let prefix = "[project]\nname = \"git\"\nkind = \"include\"\n\n[source]\npath = \
                      \"src\"\n\n[dependencies]\n";

        fs::write(
            &path,
            format!(
                "{prefix}shared = {{ git = \"https://example.invalid/shared.git\", tag = \"v1\", \
                 branch = \"main\" }}\n"
            ),
        )
        .expect("write ambiguous selector");
        let ambiguous = read_project_manifest(&root).expect_err("reject conflicting selectors");
        assert_eq!(ambiguous.kind(), crate::NwpkgErrorKind::Validation);

        fs::write(
            &path,
            format!(
                "{prefix}shared = {{ git = \"https://user:secret@example.invalid/shared.git\" }}\n"
            ),
        )
        .expect("write credential-bearing URL");
        let credentials =
            read_project_manifest(&root).expect_err("reject embedded Git credentials");
        assert_eq!(credentials.kind(), crate::NwpkgErrorKind::Validation);
        assert!(!credentials.to_string().contains("secret"));

        fs::write(
            &path,
            format!(
                "{prefix}shared = {{ git = \"https://example.invalid/shared.git\", subdir = \
                 \"../escape\" }}\n"
            ),
        )
        .expect("write unsafe subdirectory");
        let escaped = read_project_manifest(&root).expect_err("reject escaping Git subdirectory");
        assert_eq!(escaped.kind(), crate::NwpkgErrorKind::Validation);

        fs::write(
            &path,
            format!("{prefix}shared = {{ git = \"ssh://git@example.invalid/shared.git\" }}\n"),
        )
        .expect("write SSH-agent URL");
        read_project_manifest(&root).expect("allow a non-secret SSH username");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn manifest_read_observes_precancellation() {
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        let error = read_project_manifest_with_cancellation(
            std::path::Path::new("path-that-must-not-be-read"),
            &cancellation,
        )
        .expect_err("cancelled manifest read must fail");
        assert_eq!(error.kind(), crate::NwpkgErrorKind::Cancelled);
    }

    #[test]
    fn manifest_rejects_empty_names_and_layout_mismatches() {
        let root = unique_test_dir("manifest-validation");
        fs::create_dir_all(&root).expect("create root");
        fs::write(root.join("resource.utc"), b"{}").expect("write resource");

        let empty = write_project_manifest(&root, ProjectKind::Utc, " ", "resource.utc", false)
            .expect_err("reject empty project name");
        assert_eq!(empty.kind(), crate::NwpkgErrorKind::Validation);
        assert_eq!(
            empty.manifest_field_path(),
            Some(&["project".to_string(), "name".to_string()][..])
        );

        fs::create_dir_all(root.join("directory")).expect("create directory source");
        let mismatch =
            write_project_manifest(&root, ProjectKind::Utc, "resource", "directory", false)
                .expect_err("resource packages require a file");
        assert_eq!(mismatch.kind(), crate::NwpkgErrorKind::Validation);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn manifest_write_preserves_existing_file_without_force_and_replaces_with_force() {
        let root = unique_test_dir("manifest-atomic-replace");
        fs::create_dir_all(root.join("src")).expect("create source dir");
        write_project_manifest(&root, ProjectKind::Mod, "first", "src", false)
            .expect("write initial manifest");
        let path = root.join(PROJECT_MANIFEST_FILENAME);
        let original = fs::read(&path).expect("read initial manifest");

        let conflict = write_project_manifest(&root, ProjectKind::Mod, "second", "src", false)
            .expect_err("refuse replacement without force");
        assert_eq!(conflict.kind(), crate::NwpkgErrorKind::Validation);
        assert_eq!(
            fs::read(&path).expect("read preserved manifest"),
            original,
            "a failed no-clobber write must preserve every original byte"
        );

        write_project_manifest(&root, ProjectKind::Mod, "second", "src", true)
            .expect("replace manifest with force");
        let manifest = read_project_manifest(&root)
            .expect("read replacement")
            .expect("replacement exists");
        assert_eq!(manifest.project.name, "second");

        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn manifest_rejects_source_symlink_escaping_package_root() {
        use std::os::unix::fs::symlink;

        let root = unique_test_dir("manifest-symlink-escape");
        let outside = unique_test_dir("manifest-symlink-outside");
        fs::create_dir_all(&root).expect("create package root");
        fs::create_dir_all(&outside).expect("create outside source");
        symlink(&outside, root.join("src")).expect("create escaping source symlink");
        fs::write(
            root.join(PROJECT_MANIFEST_FILENAME),
            "[project]\nname = \"escape\"\nkind = \"include\"\n\n[source]\npath = \"src\"\n",
        )
        .expect("write manifest");

        let error = read_project_manifest(&root).expect_err("reject escaping source symlink");
        assert_eq!(error.kind(), crate::NwpkgErrorKind::Validation);
        assert!(error.to_string().contains("outside package root"));

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(outside);
    }
}
