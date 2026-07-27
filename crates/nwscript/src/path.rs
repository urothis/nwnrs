#[cfg(windows)]
use std::sync::Arc;
use std::{
    ffi::OsString,
    fs,
    hash::{Hash, Hasher},
    io,
    path::{Component, Path, PathBuf},
};

/// Stable identity for one physical source file or not-yet-created path.
///
/// Existing files use the platform filesystem identity so symbolic and hard
/// links compare as the same source. Missing files use a canonical existing
/// ancestor plus their exact-case suffix.
#[derive(Clone, Debug)]
pub struct PhysicalSourceId {
    identity: PhysicalFileIdentity,
    path:     PathBuf,
}

impl PartialEq for PhysicalSourceId {
    fn eq(&self, other: &Self) -> bool {
        self.identity == other.identity
    }
}

impl Eq for PhysicalSourceId {}

impl Hash for PhysicalSourceId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.identity.hash(state);
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum PhysicalFileIdentity {
    #[cfg(unix)]
    Unix {
        device: u64,
        inode:  u64,
    },
    #[cfg(windows)]
    Windows(Arc<same_file::Handle>),
    #[cfg(not(any(unix, windows)))]
    ExistingPath(PathBuf),
    MissingPath(PathBuf),
}

impl PhysicalSourceId {
    /// Resolves a physical identity for an existing or missing path.
    ///
    /// # Errors
    ///
    /// Returns an error when the path cannot be inspected or its nearest
    /// existing ancestor cannot be canonicalized.
    pub fn resolve(path: &Path) -> io::Result<Self> {
        match fs::metadata(path) {
            Ok(metadata) => {
                let canonical = fs::canonicalize(path)?;
                Ok(Self {
                    identity: existing_file_identity(&metadata, &canonical)?,
                    path:     canonical,
                })
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let canonical = canonicalize_with_missing_suffix(path)?;
                Ok(Self {
                    identity: PhysicalFileIdentity::MissingPath(canonical.clone()),
                    path:     canonical,
                })
            }
            Err(error) => Err(error),
        }
    }

    /// Returns the canonical path retained for diagnostics and file access.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Compares two paths using physical filesystem identity.
///
/// If either path cannot be resolved, comparison falls back to exact path
/// equality and never applies case folding.
#[must_use]
pub fn physical_sources_equal(left: &Path, right: &Path) -> bool {
    try_physical_sources_equal(left, right).unwrap_or_else(|_| left == right)
}

/// Returns a reversible display identity for a native filesystem path.
///
/// Valid UTF-8 paths retain their normal spelling. Paths that cannot be
/// represented as UTF-8 use a platform-tagged hexadecimal native encoding
/// instead of replacing bytes or UTF-16 code units.
#[must_use]
pub fn physical_path_display(path: &Path) -> String {
    nwnrs_types::native_path::display(path)
}

/// Returns an exact native path key with ASCII case folding.
///
/// This is reserved for NWN logical resource matching and collision checks;
/// physical source identity must use [`PhysicalSourceId`] instead.
#[must_use]
pub fn logical_path_key(path: &Path) -> Vec<u8> {
    nwnrs_types::native_path::ascii_folded_key(path)
}

/// Compares two paths using physical filesystem identity and preserves lookup
/// failures for callers making destructive or overwrite decisions.
///
/// # Errors
///
/// Returns the first filesystem identity error for either path.
pub fn try_physical_sources_equal(left: &Path, right: &Path) -> io::Result<bool> {
    Ok(PhysicalSourceId::resolve(left)? == PhysicalSourceId::resolve(right)?)
}

#[cfg(unix)]
fn existing_file_identity(
    metadata: &fs::Metadata,
    _canonical: &Path,
) -> io::Result<PhysicalFileIdentity> {
    use std::os::unix::fs::MetadataExt as _;

    Ok(PhysicalFileIdentity::Unix {
        device: metadata.dev(),
        inode:  metadata.ino(),
    })
}

#[cfg(windows)]
fn existing_file_identity(
    _metadata: &fs::Metadata,
    canonical: &Path,
) -> io::Result<PhysicalFileIdentity> {
    same_file::Handle::from_path(canonical)
        .map(Arc::new)
        .map(PhysicalFileIdentity::Windows)
}

#[cfg(not(any(unix, windows)))]
fn existing_file_identity(
    _metadata: &fs::Metadata,
    canonical: &Path,
) -> io::Result<PhysicalFileIdentity> {
    Ok(PhysicalFileIdentity::ExistingPath(canonical.to_path_buf()))
}

/// Case-insensitive, slash-normalized identity used for NWN script and
/// resource resolution.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct LogicalScriptName(String);

impl LogicalScriptName {
    /// Parses and normalizes an NWN logical script or resource name.
    ///
    /// # Errors
    ///
    /// Returns an error for empty names and parent-directory traversal.
    pub fn parse(name: &str) -> io::Result<Self> {
        let replaced = name.replace('\\', "/");
        let mut components = Vec::new();
        for component in replaced.split('/') {
            match component {
                "" | "." => {}
                ".." => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "logical script name contains parent-directory traversal",
                    ));
                }
                value => components.push(value.to_ascii_lowercase()),
            }
        }
        if components.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "logical script name is empty",
            ));
        }
        Ok(Self(components.join("/")))
    }

    /// Returns the normalized logical name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Canonicalizes the nearest existing ancestor of `path` while preserving a
/// not-yet-created suffix.
///
/// Parent-directory components are rejected so validation and later file
/// creation cannot resolve to different locations.
///
/// # Errors
///
/// Returns an error when `path` contains a parent-directory component, has no
/// existing ancestor, or that ancestor cannot be canonicalized.
pub fn canonicalize_with_missing_suffix(path: &Path) -> io::Result<PathBuf> {
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path contains a parent-directory component",
        ));
    }

    let absolute;
    let mut existing = if path.is_absolute() {
        path
    } else {
        absolute = std::env::current_dir()?.join(path);
        absolute.as_path()
    };
    let mut suffix = Vec::<OsString>::new();
    loop {
        match existing.try_exists() {
            Ok(true) => break,
            Ok(false) => {}
            Err(error) => return Err(error),
        }
        suffix.push(
            existing
                .file_name()
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::NotFound,
                        "path has no existing filesystem ancestor",
                    )
                })?
                .to_os_string(),
        );
        existing = existing.parent().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "path has no existing filesystem ancestor",
            )
        })?;
    }
    let mut canonical = fs::canonicalize(existing)?;
    for component in suffix.into_iter().rev() {
        canonical.push(component);
    }
    Ok(canonical)
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::Path,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{LogicalScriptName, PhysicalSourceId, canonicalize_with_missing_suffix};

    fn unique_test_dir() -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock before epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("nwnrs-canonical-path-{nonce}"))
    }

    #[test]
    fn canonicalizes_existing_ancestor_and_preserves_missing_suffix() {
        let root = unique_test_dir();
        fs::create_dir_all(root.join("existing")).expect("create existing ancestor");
        let actual = canonicalize_with_missing_suffix(
            &root.join("existing").join("missing").join("source.nss"),
        )
        .expect("canonicalize missing suffix");
        let expected = fs::canonicalize(root.join("existing"))
            .expect("canonical existing ancestor")
            .join("missing")
            .join("source.nss");
        assert_eq!(actual, expected);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn canonicalizes_relative_paths_from_the_current_directory() {
        let relative = Path::new("target")
            .join("nwnrs-path-test")
            .join("missing")
            .join("source.nss");
        let actual =
            canonicalize_with_missing_suffix(&relative).expect("canonicalize relative suffix");
        let expected = std::env::current_dir()
            .expect("current directory")
            .canonicalize()
            .expect("canonical current directory")
            .join(relative);
        assert_eq!(actual, expected);
    }

    #[test]
    fn rejects_parent_directory_components() {
        let error = canonicalize_with_missing_suffix(Path::new("source/../escape.nss"))
            .expect_err("reject parent directory");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[cfg(unix)]
    #[test]
    fn resolves_symbolic_alias_before_appending_suffix() {
        use std::os::unix::fs::symlink;

        let root = unique_test_dir();
        fs::create_dir_all(root.join("real")).expect("create target");
        symlink(root.join("real"), root.join("alias")).expect("create alias");
        let actual = canonicalize_with_missing_suffix(&root.join("alias/new/source.nss"))
            .expect("canonicalize through alias");
        assert_eq!(
            actual,
            fs::canonicalize(root.join("real"))
                .expect("canonical target")
                .join("new/source.nss")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn physical_identity_resolves_symbolic_and_hard_links() {
        use std::os::unix::fs::symlink;

        let root = unique_test_dir();
        fs::create_dir_all(&root).expect("create root");
        let source = root.join("source.nss");
        let hard_link = root.join("hard-link.nss");
        let symbolic_link = root.join("symbolic-link.nss");
        fs::write(&source, "void main() {}\n").expect("write source");
        fs::hard_link(&source, &hard_link).expect("create hard link");
        symlink(&source, &symbolic_link).expect("create symbolic link");

        let expected = PhysicalSourceId::resolve(&source).expect("source identity");
        assert_eq!(
            expected,
            PhysicalSourceId::resolve(&hard_link).expect("hard-link identity")
        );
        assert_eq!(
            expected,
            PhysicalSourceId::resolve(&symbolic_link).expect("symbolic-link identity")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn missing_physical_identity_preserves_exact_case() {
        let root = unique_test_dir();
        fs::create_dir_all(&root).expect("create root");
        let upper =
            PhysicalSourceId::resolve(&root.join("Missing.nss")).expect("upper-case identity");
        let lower =
            PhysicalSourceId::resolve(&root.join("missing.nss")).expect("lower-case identity");
        assert_ne!(upper, lower);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn logical_names_are_case_insensitive_and_slash_normalized() {
        assert_eq!(
            LogicalScriptName::parse(r".\Scripts\My_File.NSS").expect("windows-style name"),
            LogicalScriptName::parse("scripts/my_file.nss").expect("slash-style name")
        );
        assert!(LogicalScriptName::parse("../escape.nss").is_err());
    }
}
