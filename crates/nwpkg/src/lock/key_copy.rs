use std::path::Component;

use super::*;

/// Copies the original KEY and every referenced BIF to a destination.
///
/// # Errors
///
/// Returns an error when targets conflict or any copy fails.
pub fn copy_original_key_set(
    metadata: &KeyPackMetadata,
    output_key: &Path,
    force: bool,
) -> Result<(), NwpkgError> {
    copy_original_key_set_with_cancellation(metadata, output_key, force, &CancellationToken::new())
}

/// Cancellation-aware, transactional form of [`copy_original_key_set`].
///
/// Every source and target is validated before copying begins. Files are
/// copied into a staging directory on the destination filesystem, then
/// published with rollback protection so an interrupted or failed operation
/// does not leave a mixed KEY/BIF set.
///
/// # Errors
///
/// Returns an error for unsafe lockfile paths, conflicting targets, copy or
/// publication failures, or caller cancellation.
pub fn copy_original_key_set_with_cancellation(
    metadata: &KeyPackMetadata,
    output_key: &Path,
    force: bool,
    cancellation: &CancellationToken,
) -> Result<(), NwpkgError> {
    check_cancelled(cancellation)?;
    let output_root = output_key.parent().unwrap_or_else(|| Path::new("."));
    let mut copies = vec![(metadata.source_key.clone(), output_key.to_path_buf())];
    let mut logical_targets = BTreeMap::<String, PathBuf>::new();
    logical_targets.insert(logical_output_path(output_key)?, output_key.to_path_buf());
    for bif in &metadata.bifs {
        let source = resolve_existing_key_bif_path_with_cancellation(
            &metadata.source_key,
            bif,
            cancellation,
        )?;
        let relative = validate_portable_relative_path(bif, "BIF filename", &metadata.source_key)?;
        let target = output_root.join(relative);
        let logical = logical_output_path(&target)?;
        if let Some(existing) = logical_targets.insert(logical.clone(), target.clone()) {
            return Err(NwpkgError::at(
                NwpkgErrorKind::Validation,
                &target,
                format!(
                    "KEY/BIF output targets collide case-insensitively for {logical:?}: {} and {}",
                    existing.display(),
                    target.display()
                ),
            ));
        }
        copies.push((source, target));
    }
    for (source, target) in &copies {
        check_cancelled(cancellation)?;
        if !path_is_file(source)? {
            return Err(NwpkgError::at(
                NwpkgErrorKind::NotFound,
                source,
                "original KEY/BIF source is not a file",
            ));
        }
        match fs::metadata(target) {
            Ok(_) if !force => {
                return Err(NwpkgError::at(
                    NwpkgErrorKind::Validation,
                    target,
                    "output file exists; use --force to overwrite",
                ));
            }
            Ok(metadata) if !metadata.is_file() => {
                return Err(NwpkgError::at(
                    NwpkgErrorKind::Validation,
                    target,
                    "output target exists but is not a file",
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(NwpkgError::at(
                    NwpkgErrorKind::Io,
                    target,
                    format!("failed to inspect KEY/BIF output target: {error}"),
                ));
            }
        }
    }

    fs::create_dir_all(output_root).map_err(|error| {
        NwpkgError::at(
            NwpkgErrorKind::Io,
            output_root,
            format!("failed to create KEY/BIF output directory: {error}"),
        )
    })?;
    let transaction = tempfile::Builder::new()
        .prefix(".nwpkg-key-transaction-")
        .tempdir_in(output_root)
        .map_err(|error| {
            NwpkgError::at(
                NwpkgErrorKind::Io,
                output_root,
                format!("failed to create KEY/BIF transaction directory: {error}"),
            )
        })?;
    let staging = transaction.path().join("staging");
    let backups = transaction.path().join("backups");
    fs::create_dir_all(&staging).map_err(|error| {
        NwpkgError::at(
            NwpkgErrorKind::Io,
            &staging,
            format!("failed to create KEY/BIF staging directory: {error}"),
        )
    })?;

    let mut staged = Vec::with_capacity(copies.len());
    for (index, (source, target)) in copies.iter().enumerate() {
        check_cancelled(cancellation)?;
        let staged_path = staging.join(index.to_string());
        copy_file_with_cancellation(source, &staged_path, cancellation)?;
        staged.push((staged_path, target.clone()));
    }

    let mut published = Vec::<(PathBuf, Option<PathBuf>)>::new();
    for (index, (staged_path, target)) in staged.iter().enumerate() {
        if let Err(error) = check_cancelled(cancellation) {
            return Err(with_key_rollback(error, rollback_key_set(&published)));
        }
        if let Some(parent) = target.parent()
            && let Err(error) = fs::create_dir_all(parent)
        {
            return Err(with_key_rollback(
                NwpkgError::at(
                    NwpkgErrorKind::Io,
                    parent,
                    format!("failed to create KEY/BIF target directory: {error}"),
                ),
                rollback_key_set(&published),
            ));
        }
        let target_exists = target.try_exists().map_err(|error| {
            with_key_rollback(
                NwpkgError::at(
                    NwpkgErrorKind::Io,
                    target,
                    format!("failed to inspect KEY/BIF output target: {error}"),
                ),
                rollback_key_set(&published),
            )
        })?;
        let backup = if target_exists {
            if let Err(error) = fs::create_dir_all(&backups) {
                return Err(with_key_rollback(
                    NwpkgError::at(
                        NwpkgErrorKind::Io,
                        &backups,
                        format!("failed to create KEY/BIF backup directory: {error}"),
                    ),
                    rollback_key_set(&published),
                ));
            }
            let backup = backups.join(index.to_string());
            if let Err(error) = fs::rename(target, &backup) {
                return Err(with_key_rollback(
                    NwpkgError::at(
                        NwpkgErrorKind::Io,
                        target,
                        format!("failed to stage existing KEY/BIF output for replacement: {error}"),
                    ),
                    rollback_key_set(&published),
                ));
            }
            Some(backup)
        } else {
            None
        };
        if let Err(error) = fs::rename(staged_path, target) {
            let mut rollback_errors = Vec::new();
            if let Err(rollback_error) = rollback_key_entry(target, backup.as_deref()) {
                rollback_errors.push(rollback_error.to_string());
            }
            if let Err(rollback_error) = rollback_key_set(&published) {
                rollback_errors.push(rollback_error.to_string());
            }
            let primary = NwpkgError::at(
                NwpkgErrorKind::Io,
                target,
                format!("failed to publish staged KEY/BIF output: {error}"),
            );
            return Err(with_key_rollback_messages(primary, rollback_errors));
        }
        published.push((target.clone(), backup));
    }
    let synchronize_targets = || -> Result<(), NwpkgError> {
        #[cfg(unix)]
        {
            let mut target_directories = BTreeSet::new();
            for (target, _) in &published {
                target_directories.insert(
                    target
                        .parent()
                        .filter(|parent| !parent.as_os_str().is_empty())
                        .unwrap_or_else(|| Path::new("."))
                        .to_path_buf(),
                );
            }
            for directory in target_directories {
                File::open(&directory)
                    .and_then(|directory| directory.sync_all())
                    .map_err(|error| {
                        NwpkgError::at(
                            NwpkgErrorKind::Io,
                            &directory,
                            format!("failed to synchronize KEY/BIF output directory: {error}"),
                        )
                    })?;
            }
        }
        Ok(())
    };
    let synchronization = synchronize_targets();
    let cleanup = transaction.close().map_err(|error| {
        NwpkgError::at(
            NwpkgErrorKind::Io,
            output_root,
            format!("failed to remove completed KEY/BIF transaction state: {error}"),
        )
    });
    let root_synchronization = || -> Result<(), NwpkgError> {
        #[cfg(unix)]
        File::open(output_root)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| {
                NwpkgError::at(
                    NwpkgErrorKind::Io,
                    output_root,
                    format!("failed to synchronize KEY/BIF output root: {error}"),
                )
            })?;
        Ok(())
    }();
    let mut finalization_errors = Vec::new();
    for result in [synchronization, cleanup, root_synchronization] {
        if let Err(error) = result {
            finalization_errors.push(error.to_string());
        }
    }
    if finalization_errors.is_empty() {
        Ok(())
    } else {
        Err(NwpkgError::new(
            NwpkgErrorKind::Io,
            finalization_errors.join("; "),
        ))
    }
}

/// Resolves a BIF filename referenced by a KEY beside that KEY.
///
/// # Errors
///
/// Returns an error when no matching BIF exists.
pub fn resolve_existing_key_bif_path(
    key_path: &Path,
    filename: &str,
) -> Result<PathBuf, NwpkgError> {
    resolve_existing_key_bif_path_with_cancellation(key_path, filename, &CancellationToken::new())
}

/// Resolves one KEY-referenced BIF with portable, case-insensitive matching
/// while observing cancellation.
///
/// Exact paths win. If the exact spelling does not exist, each path component
/// is matched case-insensitively and ambiguous filesystem entries are rejected.
/// Legacy KEY files that record a directory prefix are also allowed to resolve
/// the BIF basename beside the KEY, with the same ambiguity checks.
///
/// # Errors
///
/// Returns an error for unsafe paths, ambiguous matches, filesystem failures,
/// escaped symlinks, cancellation, or a missing BIF.
pub fn resolve_existing_key_bif_path_with_cancellation(
    key_path: &Path,
    filename: &str,
    cancellation: &CancellationToken,
) -> Result<PathBuf, NwpkgError> {
    check_cancelled(cancellation)?;
    let parent = key_path.parent().unwrap_or_else(|| Path::new("."));
    let canonical_parent = fs::canonicalize(parent).map_err(|error| {
        NwpkgError::at(
            NwpkgErrorKind::Io,
            parent,
            format!("failed to resolve KEY directory: {error}"),
        )
    })?;
    let relative = validate_portable_relative_path(filename, "BIF filename", key_path)?;
    if let Some(candidate) =
        resolve_relative_case_insensitively(&canonical_parent, &relative, cancellation)?
        && path_is_file(&candidate)?
    {
        return canonicalize_confined_bif(&canonical_parent, &candidate);
    }
    if let Some(basename) = relative.file_name() {
        let basename_relative = Path::new(basename);
        if let Some(candidate) =
            resolve_relative_case_insensitively(&canonical_parent, basename_relative, cancellation)?
            && path_is_file(&candidate)?
        {
            return canonicalize_confined_bif(&canonical_parent, &candidate);
        }
    }
    Err(NwpkgError::at(
        NwpkgErrorKind::NotFound,
        key_path,
        format!("referenced BIF {filename:?} cannot be found beside the KEY"),
    ))
}

fn resolve_relative_case_insensitively(
    root: &Path,
    relative: &Path,
    cancellation: &CancellationToken,
) -> Result<Option<PathBuf>, NwpkgError> {
    const MAX_BIF_PATH_COMPONENTS: usize = 4_096;
    const MAX_BIF_DIRECTORY_ENTRIES: usize = 5_000_000;

    let components = relative.components().collect::<Vec<_>>();
    if components.len() > MAX_BIF_PATH_COMPONENTS {
        return Err(NwpkgError::at(
            NwpkgErrorKind::Validation,
            relative,
            format!("referenced BIF path exceeds maximum depth {MAX_BIF_PATH_COMPONENTS}"),
        ));
    }
    let mut current = root.to_path_buf();
    let mut inspected = 0_usize;
    for component in components {
        check_cancelled(cancellation)?;
        let Component::Normal(expected) = component else {
            return Err(NwpkgError::at(
                NwpkgErrorKind::Validation,
                relative,
                "referenced BIF path is not a portable relative path",
            ));
        };
        let expected = expected.to_str().ok_or_else(|| {
            NwpkgError::at(
                NwpkgErrorKind::Validation,
                relative,
                "referenced BIF path components must be valid UTF-8",
            )
        })?;
        let mut matches = Vec::new();
        for entry in sorted_dir_entries_with_cancellation(&current, cancellation)? {
            inspected = inspected.checked_add(1).ok_or_else(|| {
                NwpkgError::at(
                    NwpkgErrorKind::Validation,
                    &current,
                    "referenced BIF directory entry count overflowed",
                )
            })?;
            if inspected > MAX_BIF_DIRECTORY_ENTRIES {
                return Err(NwpkgError::at(
                    NwpkgErrorKind::Validation,
                    &current,
                    format!(
                        "referenced BIF resolution exceeded maximum of \
                         {MAX_BIF_DIRECTORY_ENTRIES} directory entries"
                    ),
                ));
            }
            if entry.file_name.eq_ignore_ascii_case(expected) {
                matches.push(entry.path);
            }
        }
        match matches.as_slice() {
            [] => return Ok(None),
            [matched] => current = matched.clone(),
            _ => {
                return Err(NwpkgError::at(
                    NwpkgErrorKind::Validation,
                    &current,
                    format!(
                        "referenced BIF component {expected:?} is ambiguous case-insensitively: {}",
                        matches
                            .iter()
                            .map(|path| path.display().to_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                ));
            }
        }
    }
    Ok(Some(current))
}

fn logical_output_path(path: &Path) -> Result<String, NwpkgError> {
    path.to_str()
        .map(|value| value.replace('\\', "/").to_lowercase())
        .ok_or_else(|| {
            NwpkgError::at(
                NwpkgErrorKind::Validation,
                path,
                "KEY/BIF output paths must be valid UTF-8",
            )
        })
}

fn path_is_file(path: &Path) -> Result<bool, NwpkgError> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(metadata.is_file()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(NwpkgError::at(
            NwpkgErrorKind::Io,
            path,
            format!("failed to inspect referenced BIF: {error}"),
        )),
    }
}

fn canonicalize_confined_bif(root: &Path, candidate: &Path) -> Result<PathBuf, NwpkgError> {
    let canonical = fs::canonicalize(candidate).map_err(|error| {
        NwpkgError::at(
            NwpkgErrorKind::Io,
            candidate,
            format!("failed to resolve referenced BIF: {error}"),
        )
    })?;
    if !canonical.starts_with(root) {
        return Err(NwpkgError::at(
            NwpkgErrorKind::Validation,
            canonical,
            "referenced BIF resolves outside the KEY directory",
        ));
    }
    Ok(canonical)
}

fn copy_file_with_cancellation(
    source: &Path,
    target: &Path,
    cancellation: &CancellationToken,
) -> Result<(), NwpkgError> {
    let mut input = File::open(source).map_err(|error| {
        NwpkgError::at(
            NwpkgErrorKind::Io,
            source,
            format!("failed to open original KEY/BIF source: {error}"),
        )
    })?;
    let mut output = File::create(target).map_err(|error| {
        NwpkgError::at(
            NwpkgErrorKind::Io,
            target,
            format!("failed to create staged KEY/BIF output: {error}"),
        )
    })?;
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        check_cancelled(cancellation)?;
        let count = input.read(&mut buffer).map_err(|error| {
            NwpkgError::at(
                NwpkgErrorKind::Io,
                source,
                format!("failed to read original KEY/BIF source: {error}"),
            )
        })?;
        if count == 0 {
            break;
        }
        let chunk = buffer.get(..count).ok_or_else(|| {
            NwpkgError::at(
                NwpkgErrorKind::Io,
                source,
                "original KEY/BIF reader returned an invalid byte count",
            )
        })?;
        output.write_all(chunk).map_err(|error| {
            NwpkgError::at(
                NwpkgErrorKind::Io,
                target,
                format!("failed to write staged KEY/BIF output: {error}"),
            )
        })?;
    }
    output.sync_all().map_err(|error| {
        NwpkgError::at(
            NwpkgErrorKind::Io,
            target,
            format!("failed to synchronize staged KEY/BIF output: {error}"),
        )
    })
}

fn rollback_key_set(published: &[(PathBuf, Option<PathBuf>)]) -> Result<(), NwpkgError> {
    let mut errors = Vec::new();
    for (target, backup) in published.iter().rev() {
        if let Err(error) = rollback_key_entry(target, backup.as_deref()) {
            errors.push(error.to_string());
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(NwpkgError::new(NwpkgErrorKind::Io, errors.join("; ")))
    }
}

fn rollback_key_entry(target: &Path, backup: Option<&Path>) -> Result<(), NwpkgError> {
    let mut errors = Vec::new();
    match target.try_exists() {
        Ok(true) => {
            if let Err(error) = fs::remove_file(target) {
                errors.push(format!(
                    "failed to remove partially published KEY/BIF output {}: {error}",
                    target.display()
                ));
            }
        }
        Ok(false) => {}
        Err(error) => errors.push(format!(
            "failed to inspect partially published KEY/BIF output {}: {error}",
            target.display()
        )),
    }
    if let Some(backup) = backup
        && let Err(error) = fs::rename(backup, target)
    {
        errors.push(format!(
            "failed to restore KEY/BIF backup {} to {}: {error}",
            backup.display(),
            target.display()
        ));
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(NwpkgError::new(NwpkgErrorKind::Io, errors.join("; ")))
    }
}

fn with_key_rollback(primary: NwpkgError, rollback: Result<(), NwpkgError>) -> NwpkgError {
    match rollback {
        Ok(()) => primary,
        Err(rollback) => with_key_rollback_messages(primary, vec![rollback.to_string()]),
    }
}

fn with_key_rollback_messages(primary: NwpkgError, rollback_errors: Vec<String>) -> NwpkgError {
    if rollback_errors.is_empty() {
        primary
    } else {
        NwpkgError::new(
            primary.kind(),
            format!(
                "{primary}; KEY/BIF rollback also failed: {}",
                rollback_errors.join("; ")
            ),
        )
    }
}
