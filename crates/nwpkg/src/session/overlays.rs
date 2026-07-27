use std::{
    collections::BTreeMap,
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use nwnrs_nwscript::{
    CancellationToken, CompilerResourceLimits, LogicalScriptName, canonicalize_with_missing_suffix,
    physical_sources_equal,
};

use super::{check_cancelled, copy_snapshot_bytes};
use crate::{NwpkgError, NwpkgErrorKind, ResolvedIncludeDependency};

pub(super) fn source_belongs_to_root(path: &Path, root: &Path, root_is_file: bool) -> bool {
    if root_is_file {
        physical_sources_equal(path, root)
    } else {
        path.starts_with(root)
    }
}

fn logical_overlay_name(
    path: &Path,
    source_root: &Path,
    source_root_is_file: bool,
    dependencies: &[ResolvedIncludeDependency],
) -> Result<LogicalScriptName, NwpkgError> {
    let relative = if source_root_is_file && physical_sources_equal(path, source_root) {
        path.file_name().map(PathBuf::from)
    } else if let Ok(relative) = path.strip_prefix(source_root) {
        Some(relative.to_path_buf())
    } else {
        dependencies.iter().find_map(|dependency| {
            path.strip_prefix(&dependency.source_root)
                .ok()
                .map(Path::to_path_buf)
        })
    }
    .ok_or_else(|| {
        NwpkgError::at(
            NwpkgErrorKind::Validation,
            path,
            "source overlay has no logical package-relative name",
        )
    })?;
    let relative = relative.to_str().ok_or_else(|| {
        NwpkgError::at(
            NwpkgErrorKind::Validation,
            path,
            "logical NWScript source paths must be valid UTF-8",
        )
    })?;
    LogicalScriptName::parse(relative).map_err(|error| {
        NwpkgError::at(
            NwpkgErrorKind::Validation,
            path,
            format!("invalid logical source name: {error}"),
        )
    })
}

pub(super) fn normalize_overlays(
    project_root: &Path,
    source_root: &Path,
    dependencies: &[ResolvedIncludeDependency],
    overlays: &BTreeMap<PathBuf, Vec<u8>>,
    ignore_unrelated_absolute_overlays: bool,
    cancellation: &CancellationToken,
) -> Result<BTreeMap<PathBuf, Arc<[u8]>>, NwpkgError> {
    let mut normalized = BTreeMap::new();
    let mut logical_paths = BTreeMap::<LogicalScriptName, PathBuf>::new();
    let limits = CompilerResourceLimits::default();
    let source_root_is_file = std::fs::metadata(source_root)
        .map_err(|error| {
            NwpkgError::at(
                NwpkgErrorKind::Io,
                source_root,
                format!("failed to inspect package source root: {error}"),
            )
        })?
        .is_file();
    let mut total_bytes = 0_usize;
    for (path, contents) in overlays {
        check_cancelled(cancellation)?;
        let was_absolute = path.is_absolute();
        let path = if path.is_absolute() {
            path.clone()
        } else {
            project_root.join(path)
        };
        if path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        {
            return Err(NwpkgError::at(
                NwpkgErrorKind::Validation,
                path,
                "source overlay escapes the resolved package graph",
            ));
        }
        let path = canonicalize_with_missing_suffix(&path).map_err(|error| {
            NwpkgError::at(
                NwpkgErrorKind::Io,
                &path,
                format!("failed to resolve source overlay ancestor: {error}"),
            )
        })?;
        if !source_belongs_to_root(&path, source_root, source_root_is_file)
            && !dependencies
                .iter()
                .any(|dependency| source_belongs_to_root(&path, &dependency.source_root, false))
        {
            if ignore_unrelated_absolute_overlays && was_absolute {
                continue;
            }
            return Err(NwpkgError::at(
                NwpkgErrorKind::Validation,
                path,
                "source overlay escapes the resolved package graph",
            ));
        }
        let logical = logical_overlay_name(&path, source_root, source_root_is_file, dependencies)?;
        if let Some(existing) = logical_paths.insert(logical, path.clone()) {
            return Err(NwpkgError::at(
                NwpkgErrorKind::Validation,
                &path,
                format!(
                    "source overlays collide case-insensitively with {}",
                    existing.display()
                ),
            ));
        }
        total_bytes = total_bytes.checked_add(contents.len()).ok_or_else(|| {
            NwpkgError::at(
                NwpkgErrorKind::Validation,
                &path,
                "source overlay byte count overflowed",
            )
        })?;
        if contents.len() > limits.max_source_file_bytes
            || total_bytes > limits.max_total_source_bytes
        {
            return Err(NwpkgError::at(
                NwpkgErrorKind::Validation,
                &path,
                "source overlays exceed configured compiler source bounds",
            ));
        }
        let copied =
            copy_snapshot_bytes(&path, contents, limits.max_source_file_bytes, cancellation)?;
        normalized.insert(path, Arc::<[u8]>::from(copied));
    }
    Ok(normalized)
}
