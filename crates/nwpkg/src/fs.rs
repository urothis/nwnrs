use std::{
    ffi::{OsStr, OsString},
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
};

use nwnrs_nwscript::CancellationToken;
use tempfile::NamedTempFile;

use crate::{NwpkgError, NwpkgErrorKind, PACKAGE_LOCK_FILENAME, PROJECT_MANIFEST_FILENAME};

pub(crate) struct DirEntryInfo {
    pub(crate) file_name: String,
    pub(crate) path:      PathBuf,
}

/// Deliberately generous safety bounds for package filesystem traversal.
///
/// These defaults are intended to reject pathological or hostile directory
/// trees, not ordinary large modules. Callers that legitimately exceed them
/// can opt into larger values explicitly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackageTraversalLimits {
    /// Maximum number of filesystem entries inspected by one traversal.
    pub max_entries: usize,
    /// Maximum directory nesting below the requested root.
    pub max_depth:   usize,
}

impl Default for PackageTraversalLimits {
    fn default() -> Self {
        Self {
            max_entries: 5_000_000,
            max_depth:   4_096,
        }
    }
}

pub(crate) fn ensure_output_file_ready(path: &Path, force: bool) -> Result<(), NwpkgError> {
    match fs::metadata(path) {
        Ok(_) if !force => {
            return Err(NwpkgError::at(
                NwpkgErrorKind::Validation,
                path,
                "output file exists; use --force to overwrite",
            ));
        }
        Ok(metadata) if !metadata.is_file() => {
            return Err(NwpkgError::at(
                NwpkgErrorKind::Validation,
                path,
                "output target exists but is not a file",
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(NwpkgError::at(
                NwpkgErrorKind::Io,
                path,
                format!("failed to inspect output target: {error}"),
            ));
        }
    }
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|error| {
            NwpkgError::at(
                NwpkgErrorKind::Io,
                parent,
                format!("failed to create output directory: {error}"),
            )
        })?;
    }
    Ok(())
}

pub(crate) fn atomic_write(path: &Path, contents: &[u8], replace: bool) -> Result<(), NwpkgError> {
    let parent = path.parent().filter(|value| !value.as_os_str().is_empty());
    let directory = parent.unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(directory).map_err(|error| {
        NwpkgError::at(
            NwpkgErrorKind::Io,
            directory,
            format!("failed to create parent directory: {error}"),
        )
    })?;
    let mut temporary = NamedTempFile::new_in(directory).map_err(|error| {
        NwpkgError::at(
            NwpkgErrorKind::Io,
            directory,
            format!("failed to create temporary output: {error}"),
        )
    })?;
    temporary.write_all(contents).map_err(|error| {
        NwpkgError::at(
            NwpkgErrorKind::Io,
            path,
            format!("failed to write temporary output: {error}"),
        )
    })?;
    temporary.as_file().sync_all().map_err(|error| {
        NwpkgError::at(
            NwpkgErrorKind::Io,
            path,
            format!("failed to synchronize temporary output: {error}"),
        )
    })?;
    let persisted = if replace {
        temporary.persist(path)
    } else {
        temporary.persist_noclobber(path)
    }
    .map_err(|error| {
        let kind = if error.error.kind() == std::io::ErrorKind::AlreadyExists {
            NwpkgErrorKind::Validation
        } else {
            NwpkgErrorKind::Io
        };
        NwpkgError::at(
            kind,
            path,
            format!("failed to persist output atomically: {}", error.error),
        )
    })?;
    persisted.sync_all().map_err(|error| {
        NwpkgError::at(
            NwpkgErrorKind::Io,
            path,
            format!("failed to synchronize output: {error}"),
        )
    })?;
    #[cfg(unix)]
    {
        fs::File::open(directory)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| {
                NwpkgError::at(
                    NwpkgErrorKind::Io,
                    directory,
                    format!("failed to synchronize output directory: {error}"),
                )
            })?;
    }
    Ok(())
}

pub(crate) fn sorted_dir_entries_with_cancellation(
    dir: &Path,
    cancellation: &CancellationToken,
) -> Result<Vec<DirEntryInfo>, NwpkgError> {
    let mut entries = Vec::new();
    let iterator = fs::read_dir(dir).map_err(|error| {
        NwpkgError::at(
            NwpkgErrorKind::Io,
            dir,
            format!("failed to read directory: {error}"),
        )
    })?;
    for entry in iterator {
        check_cancelled(cancellation)?;
        let entry = entry.map_err(|error| {
            NwpkgError::at(
                NwpkgErrorKind::Io,
                dir,
                format!("failed to read directory entry: {error}"),
            )
        })?;
        let file_name = validated_source_entry_name(entry.file_name(), &entry.path())?;
        entries.push(DirEntryInfo {
            file_name,
            path: entry.path(),
        });
    }
    check_cancelled(cancellation)?;
    entries.sort_by(|lhs, rhs| {
        lhs.file_name
            .to_ascii_lowercase()
            .cmp(&rhs.file_name.to_ascii_lowercase())
            .then_with(|| lhs.file_name.cmp(&rhs.file_name))
    });
    Ok(entries)
}

fn validated_source_entry_name(name: OsString, path: &Path) -> Result<String, NwpkgError> {
    name.into_string().map_err(|_name| {
        NwpkgError::at(
            NwpkgErrorKind::Validation,
            path,
            "package source entry names must be valid UTF-8",
        )
    })
}

pub(crate) fn collect_nss_paths_with_cancellation(
    root: &Path,
    cancellation: &CancellationToken,
) -> Result<Vec<PathBuf>, NwpkgError> {
    collect_nss_paths_with_limits(root, cancellation, PackageTraversalLimits::default())
}

pub(crate) fn collect_nss_paths_with_limits(
    root: &Path,
    cancellation: &CancellationToken,
    limits: PackageTraversalLimits,
) -> Result<Vec<PathBuf>, NwpkgError> {
    check_cancelled(cancellation)?;
    let mut paths = Vec::new();
    let mut pending = vec![(root.to_path_buf(), 0_usize)];
    let mut inspected = 0_usize;
    while let Some((directory, depth)) = pending.pop() {
        check_cancelled(cancellation)?;
        if depth > limits.max_depth {
            return Err(NwpkgError::at(
                NwpkgErrorKind::Validation,
                directory,
                format!(
                    "package source traversal exceeded the configured maximum depth of {}",
                    limits.max_depth
                ),
            ));
        }
        let mut child_directories = Vec::new();
        for entry in sorted_dir_entries_with_cancellation(&directory, cancellation)? {
            inspected = inspected.checked_add(1).ok_or_else(|| {
                NwpkgError::at(
                    NwpkgErrorKind::Validation,
                    &entry.path,
                    "package source traversal entry count overflowed",
                )
            })?;
            if inspected > limits.max_entries {
                return Err(NwpkgError::at(
                    NwpkgErrorKind::Validation,
                    &entry.path,
                    format!(
                        "package source traversal exceeded the configured maximum of {} entries",
                        limits.max_entries
                    ),
                ));
            }
            let metadata = fs::symlink_metadata(&entry.path).map_err(|error| {
                NwpkgError::at(
                    NwpkgErrorKind::Io,
                    &entry.path,
                    format!("failed to inspect source entry: {error}"),
                )
            })?;
            if metadata.file_type().is_symlink() {
                return Err(NwpkgError::at(
                    NwpkgErrorKind::Validation,
                    &entry.path,
                    "package source traversal does not permit symbolic links",
                ));
            }
            if metadata.is_dir() {
                let nested_manifest = entry.path.join(PROJECT_MANIFEST_FILENAME);
                let has_nested_manifest = match fs::metadata(&nested_manifest) {
                    Ok(metadata) => metadata.is_file(),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
                    Err(error) => {
                        return Err(NwpkgError::at(
                            NwpkgErrorKind::Io,
                            &nested_manifest,
                            format!("failed to inspect nested package manifest: {error}"),
                        ));
                    }
                };
                if has_nested_manifest {
                    continue;
                }
                child_directories.push(entry.path);
            } else if metadata.is_file()
                && entry
                    .path
                    .extension()
                    .and_then(OsStr::to_str)
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("nss"))
            {
                paths.push(entry.path);
            }
        }
        for child in child_directories.into_iter().rev() {
            pending.push((child, depth + 1));
        }
    }
    Ok(paths)
}

pub(crate) fn should_skip_top_level_dir(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(OsStr::to_str),
        Some(".git" | ".svn")
    )
}

pub(crate) fn validate_portable_relative_path(
    value: &str,
    label: &str,
    error_path: &Path,
) -> Result<PathBuf, NwpkgError> {
    if value.is_empty() || value.contains('\0') {
        return Err(NwpkgError::at(
            NwpkgErrorKind::Validation,
            error_path,
            format!("{label} must be a non-empty portable relative path"),
        ));
    }
    let normalized = value.replace('\\', "/");
    let path = Path::new(&normalized);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
        || normalized
            .split('/')
            .any(|component| component.is_empty() || component == "." || component.contains(':'))
    {
        return Err(NwpkgError::at(
            NwpkgErrorKind::Validation,
            error_path,
            format!("{label} {value:?} is not a safe portable relative path"),
        ));
    }
    Ok(path.to_path_buf())
}

pub(crate) fn sort_paths_case_insensitive(
    paths: &mut Vec<PathBuf>,
    label: &str,
) -> Result<(), NwpkgError> {
    let mut keyed = Vec::with_capacity(paths.len());
    for path in paths.drain(..) {
        let key = path
            .to_str()
            .ok_or_else(|| {
                NwpkgError::at(
                    NwpkgErrorKind::Validation,
                    &path,
                    format!("{label} paths must be valid UTF-8"),
                )
            })?
            .replace('\\', "/")
            .to_ascii_lowercase();
        keyed.push((key, path));
    }
    keyed.sort_by(|(left_key, left), (right_key, right)| {
        left_key.cmp(right_key).then_with(|| left.cmp(right))
    });
    paths.extend(keyed.into_iter().map(|(_, path)| path));
    Ok(())
}

fn check_cancelled(cancellation: &CancellationToken) -> Result<(), NwpkgError> {
    cancellation
        .check()
        .map_err(|error| NwpkgError::new(NwpkgErrorKind::Cancelled, error.to_string()))
}

/// Returns whether `path` is an nwpkg manifest or lockfile.
#[must_use]
pub fn is_project_control_file(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(OsStr::to_str),
        Some(PACKAGE_LOCK_FILENAME | PROJECT_MANIFEST_FILENAME)
    )
}

#[cfg(all(test, unix))]
mod tests {
    use std::{
        ffi::OsString,
        fs,
        os::unix::{ffi::OsStringExt as _, fs::symlink},
        path::Path,
        time::{SystemTime, UNIX_EPOCH},
    };

    use nwnrs_nwscript::CancellationToken;

    use super::{collect_nss_paths_with_cancellation, validated_source_entry_name};

    #[test]
    fn nss_traversal_honors_cancellation() {
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        let error = collect_nss_paths_with_cancellation(
            Path::new("path-that-must-not-be-read"),
            &cancellation,
        )
        .expect_err("cancel traversal before filesystem access");
        assert_eq!(error.kind(), crate::NwpkgErrorKind::Cancelled);
    }

    #[test]
    fn nss_traversal_rejects_non_utf8_logical_names() {
        let invalid = OsString::from_vec(vec![
            b'i', b'n', b'v', b'a', b'l', b'i', b'd', 0xff, b'.', b'n', b's', b's',
        ]);
        let error = validated_source_entry_name(invalid, Path::new("invalid-name"))
            .expect_err("reject non-UTF8 logical name");
        assert_eq!(error.kind(), crate::NwpkgErrorKind::Validation);
        assert!(error.message().contains("valid UTF-8"));
    }

    #[test]
    fn nss_traversal_rejects_symlink_cycles_and_escapes() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock before epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("nwnrs-nwpkg-symlink-{nonce}"));
        fs::create_dir_all(root.join("source")).expect("create source");
        fs::write(root.join("outside.nss"), "void Outside();").expect("write outside source");
        symlink(&root, root.join("source/loop")).expect("create cycle");

        let cycle =
            collect_nss_paths_with_cancellation(&root.join("source"), &CancellationToken::new())
                .expect_err("reject symlink cycle");
        assert!(cycle.to_string().contains("does not permit symbolic links"));

        fs::remove_file(root.join("source/loop")).expect("remove cycle");
        symlink(root.join("outside.nss"), root.join("source/escape.nss"))
            .expect("create file escape");
        let escape =
            collect_nss_paths_with_cancellation(&root.join("source"), &CancellationToken::new())
                .expect_err("reject symlink escape");
        assert!(
            escape
                .to_string()
                .contains("does not permit symbolic links")
        );

        let _ = fs::remove_dir_all(root);
    }
}
