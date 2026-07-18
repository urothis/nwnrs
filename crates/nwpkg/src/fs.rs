#![allow(missing_docs)]

use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};

use crate::{PACKAGE_LOCK_FILENAME, PROJECT_MANIFEST_FILENAME};

pub(crate) struct DirEntryInfo {
    pub(crate) file_name: String,
    pub(crate) path:      PathBuf,
}

pub(crate) fn ensure_output_file_ready(path: &Path, force: bool) -> Result<(), String> {
    if path.exists() && !force {
        return Err(format!(
            "output file exists; use --force to overwrite: {}",
            path.display()
        ));
    }
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
    }
    Ok(())
}

pub(crate) fn sorted_dir_entries(dir: &Path) -> Result<Vec<DirEntryInfo>, String> {
    let mut entries = fs::read_dir(dir)
        .map_err(|error| format!("failed to read {}: {error}", dir.display()))?
        .map(|entry| {
            let entry =
                entry.map_err(|error| format!("failed to read {}: {error}", dir.display()))?;
            let file_name = entry.file_name().to_string_lossy().into_owned();
            Ok(DirEntryInfo {
                file_name,
                path: entry.path(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    entries.sort_by(|lhs, rhs| {
        lhs.file_name
            .to_ascii_lowercase()
            .cmp(&rhs.file_name.to_ascii_lowercase())
    });
    Ok(entries)
}

pub(crate) fn should_skip_top_level_dir(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(OsStr::to_str),
        Some(".git" | ".svn")
    )
}

pub(crate) fn entry_is_dir(path: &Path, no_symlinks: bool) -> Result<bool, String> {
    let meta = fs::symlink_metadata(path)
        .map_err(|error| format!("failed to stat {}: {error}", path.display()))?;
    if meta.file_type().is_symlink() {
        if no_symlinks {
            return Ok(false);
        }
        return Ok(path.is_dir());
    }
    Ok(meta.is_dir())
}

pub(crate) fn entry_is_file(path: &Path, no_symlinks: bool) -> Result<bool, String> {
    let meta = fs::symlink_metadata(path)
        .map_err(|error| format!("failed to stat {}: {error}", path.display()))?;
    if meta.file_type().is_symlink() {
        if no_symlinks {
            return Ok(false);
        }
        return Ok(path.is_file());
    }
    Ok(meta.is_file())
}

pub(crate) fn normalize_key_bif_filename(filename: &str) -> String {
    filename.replace('\\', "/")
}

pub fn is_project_control_file(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(OsStr::to_str),
        Some(PACKAGE_LOCK_FILENAME | PROJECT_MANIFEST_FILENAME)
    )
}
