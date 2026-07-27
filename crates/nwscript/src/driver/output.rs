use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
};

use super::CompilerHostError;
use crate::{CancellationToken, canonicalize_with_missing_suffix};

#[derive(Clone)]
pub(super) struct PendingOutput {
    pub(super) path: PathBuf,
    pub(super) data: Option<Vec<u8>>,
}

pub(super) fn validate_logical_output_alias(alias: &str) -> Result<(), CompilerHostError> {
    let path = Path::new(alias);
    if alias.is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
        || alias
            .replace('\\', "/")
            .split('/')
            .any(|component| component.is_empty() || component == "." || component.contains(':'))
    {
        return Err(CompilerHostError::new(format!(
            "compiler output alias is not a safe portable relative path: {alias:?}"
        )));
    }
    Ok(())
}

pub(super) fn validate_output_target(base: &Path, target: &Path) -> Result<(), CompilerHostError> {
    let canonical_base = canonicalize_with_missing_suffix(base).map_err(|error| {
        CompilerHostError::new(format!(
            "failed to resolve compiler output directory {}: {error}",
            base.display()
        ))
    })?;
    let canonical_target = canonicalize_with_missing_suffix(target).map_err(|error| {
        CompilerHostError::new(format!(
            "failed to resolve compiler output target {}: {error}",
            target.display()
        ))
    })?;
    if !canonical_target.starts_with(&canonical_base) {
        return Err(CompilerHostError::new(format!(
            "compiler output target escapes configured directory {}: {}",
            canonical_base.display(),
            canonical_target.display()
        )));
    }
    Ok(())
}

pub(super) fn publish_outputs_transactionally(
    outputs: Vec<PendingOutput>,
    overwrite_existing: bool,
    cancellation: Option<&CancellationToken>,
) -> Result<(), CompilerHostError> {
    validate_pending_outputs(&outputs, overwrite_existing)?;

    let mut staged = Vec::with_capacity(outputs.len());
    for output in outputs {
        if let Some(cancellation) = cancellation {
            cancellation
                .check()
                .map_err(|error| CompilerHostError::new(error.to_string()))?;
        }
        let parent = output
            .path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let file = if let Some(data) = output.data {
            fs::create_dir_all(parent)?;
            let mut file = tempfile::NamedTempFile::new_in(parent)?;
            for chunk in data.chunks(1024 * 1024) {
                if let Some(cancellation) = cancellation {
                    cancellation
                        .check()
                        .map_err(|error| CompilerHostError::new(error.to_string()))?;
                }
                file.write_all(chunk)?;
            }
            file.as_file().sync_all()?;
            Some(file)
        } else {
            None
        };
        staged.push((file, output.path));
    }

    let mut published = Vec::<(PathBuf, Option<tempfile::TempPath>)>::new();
    for (staged_file, target) in staged {
        if let Some(cancellation) = cancellation
            && let Err(error) = cancellation.check()
        {
            return Err(with_output_rollback(
                CompilerHostError::new(error.to_string()),
                rollback_published_outputs(&published),
            ));
        }
        let target_exists = target.try_exists().map_err(|error| {
            with_output_rollback(
                CompilerHostError::new(format!(
                    "failed to inspect compiler output {}: {error}",
                    target.display()
                )),
                rollback_published_outputs(&published),
            )
        })?;
        let backup = if target_exists {
            let parent = target
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."));
            let placeholder = match tempfile::NamedTempFile::new_in(parent) {
                Ok(placeholder) => placeholder,
                Err(error) => {
                    return Err(with_output_rollback(
                        CompilerHostError::new(format!(
                            "failed to create backup for compiler output {}: {error}",
                            target.display()
                        )),
                        rollback_published_outputs(&published),
                    ));
                }
            };
            let backup = placeholder.into_temp_path();
            if let Err(error) = fs::remove_file(&backup) {
                return Err(with_output_rollback(
                    CompilerHostError::new(format!(
                        "failed to prepare backup for compiler output {}: {error}",
                        target.display()
                    )),
                    rollback_published_outputs(&published),
                ));
            }
            if let Err(error) = fs::rename(&target, &backup) {
                return Err(with_output_rollback(
                    CompilerHostError::new(format!(
                        "failed to stage existing output {} for replacement: {error}",
                        target.display()
                    )),
                    rollback_published_outputs(&published),
                ));
            }
            Some(backup)
        } else {
            None
        };
        if let Some(staged_file) = staged_file
            && let Err(error) = staged_file.persist_noclobber(&target)
        {
            let mut rollback_errors = Vec::new();
            if let Err(rollback_error) = rollback_output(&target, backup.as_ref()) {
                rollback_errors.push(rollback_error.message);
            }
            if let Err(rollback_error) = rollback_published_outputs(&published) {
                rollback_errors.push(rollback_error.message);
            }
            let primary = CompilerHostError::new(format!(
                "failed to publish compiler output {}: {}",
                target.display(),
                error.error
            ));
            return Err(with_rollback_messages(primary, rollback_errors));
        }
        published.push((target, backup));
    }
    if let Some(cancellation) = cancellation
        && let Err(error) = cancellation.check()
    {
        return Err(with_output_rollback(
            CompilerHostError::new(error.to_string()),
            rollback_published_outputs(&published),
        ));
    }
    finalize_published_outputs(published)
}

fn validate_pending_outputs(
    outputs: &[PendingOutput],
    overwrite_existing: bool,
) -> Result<(), CompilerHostError> {
    validate_output_entries(
        outputs
            .iter()
            .map(|output| (output.path.as_path(), output.data.is_some())),
        overwrite_existing,
    )
}

pub(super) fn validate_output_paths<'a>(
    paths: impl IntoIterator<Item = &'a Path>,
    overwrite_existing: bool,
) -> Result<(), CompilerHostError> {
    validate_output_entries(
        paths.into_iter().map(|path| (path, true)),
        overwrite_existing,
    )
}

fn validate_output_entries<'a>(
    entries: impl IntoIterator<Item = (&'a Path, bool)>,
    overwrite_existing: bool,
) -> Result<(), CompilerHostError> {
    let mut logical_targets = BTreeMap::<Vec<u8>, PathBuf>::new();
    for (path, is_write) in entries {
        let logical = crate::logical_path_key(path);
        if let Some(existing) = logical_targets.insert(logical.clone(), path.to_path_buf()) {
            return Err(CompilerHostError::new(format!(
                "compiler artifacts have ambiguous case-insensitive targets: {} and {}",
                existing.display(),
                path.display()
            )));
        }
        match fs::metadata(path) {
            Ok(_) if is_write && !overwrite_existing => {
                return Err(CompilerHostError::new(format!(
                    "output already exists; use overwrite to replace {}",
                    path.display()
                )));
            }
            Ok(metadata) if !metadata.is_file() => {
                return Err(CompilerHostError::new(format!(
                    "compiler output target is not a file: {}",
                    path.display()
                )));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(CompilerHostError::new(format!(
                    "failed to inspect compiler output target {}: {error}",
                    path.display()
                )));
            }
        }
    }
    Ok(())
}

fn finalize_published_outputs(
    published: Vec<(PathBuf, Option<tempfile::TempPath>)>,
) -> Result<(), CompilerHostError> {
    let mut directories = BTreeMap::<PathBuf, ()>::new();
    let mut errors = Vec::new();
    for (target, backup) in published {
        let parent = target
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        directories.insert(parent, ());
        if let Some(backup) = backup {
            let backup_path = backup.to_path_buf();
            if let Err(error) = backup.close() {
                errors.push(format!(
                    "failed to remove committed compiler output backup {}: {error}",
                    backup_path.display()
                ));
            }
        }
    }
    #[cfg(unix)]
    for directory in directories.into_keys() {
        if let Err(error) = fs::File::open(&directory).and_then(|directory| directory.sync_all()) {
            errors.push(format!(
                "failed to synchronize compiler output directory {}: {error}",
                directory.display()
            ));
        }
    }
    #[cfg(not(unix))]
    let _directories = directories;
    if errors.is_empty() {
        Ok(())
    } else {
        Err(CompilerHostError::new(errors.join("; ")))
    }
}

fn rollback_published_outputs(
    published: &[(PathBuf, Option<tempfile::TempPath>)],
) -> Result<(), CompilerHostError> {
    let mut errors = Vec::new();
    for (target, backup) in published.iter().rev() {
        if let Err(error) = rollback_output(target, backup.as_ref()) {
            errors.push(error.message);
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(CompilerHostError::new(errors.join("; ")))
    }
}

fn rollback_output(
    target: &Path,
    backup: Option<&tempfile::TempPath>,
) -> Result<(), CompilerHostError> {
    let mut errors = Vec::new();
    match target.try_exists() {
        Ok(true) => {
            if let Err(error) = fs::remove_file(target) {
                errors.push(format!(
                    "failed to remove partially published output {}: {error}",
                    target.display()
                ));
            }
        }
        Ok(false) => {}
        Err(error) => {
            errors.push(format!(
                "failed to inspect partially published output {}: {error}",
                target.display()
            ));
        }
    }
    if let Some(backup) = backup
        && let Err(error) = fs::rename(backup, target)
    {
        errors.push(format!(
            "failed to restore compiler output backup {} to {}: {error}",
            backup.display(),
            target.display()
        ));
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(CompilerHostError::new(errors.join("; ")))
    }
}

fn with_output_rollback(
    primary: CompilerHostError,
    rollback: Result<(), CompilerHostError>,
) -> CompilerHostError {
    match rollback {
        Ok(()) => primary,
        Err(rollback) => with_rollback_messages(primary, vec![rollback.message]),
    }
}

fn with_rollback_messages(
    primary: CompilerHostError,
    rollback_errors: Vec<String>,
) -> CompilerHostError {
    if rollback_errors.is_empty() {
        primary
    } else {
        CompilerHostError::new(format!(
            "{}; output rollback also failed: {}",
            primary.message,
            rollback_errors.join("; ")
        ))
    }
}
