use std::{
    fs,
    path::{Path, PathBuf},
};

use nwnrs::prelude::{resman::ResContainer, *};
use tracing::{debug, info, instrument};

use crate::{
    args::{NwsyncFetchCmd, NwsyncPrintCmd, NwsyncPruneCmd, NwsyncWriteCmd},
    util::write_stdout_line,
};

#[instrument(
    level = "info",
    skip_all,
    err,
    fields(
        input = %cmd.input.display(),
        manifest_sha1 = cmd.manifest.as_deref().unwrap_or("")
    )
)]
pub(crate) fn run_nwsync_print(cmd: NwsyncPrintCmd) -> Result<(), String> {
    if cmd.input.is_dir() {
        info!("printing nwsync repository");
        let nwsync = resnwsync::open_nwsync(&cmd.input).map_err(|error| {
            format!(
                "failed to open nwsync repo {}: {error}",
                cmd.input.display()
            )
        })?;

        if let Some(manifest) = cmd.manifest {
            debug!("printing specific manifest");
            let manifest_sha1: checksums::SecureHash = manifest
                .parse()
                .map_err(|error| format!("invalid manifest sha1 {manifest}: {error}"))?;
            let manifest =
                resnwsync::new_resnwsync_manifest(&nwsync, manifest_sha1).map_err(|error| {
                    format!(
                        "failed to load manifest {} from {}: {}",
                        manifest,
                        cmd.input.display(),
                        error
                    )
                })?;

            let mut contents = manifest.contents();
            contents.sort();
            for rr in contents {
                let sha1 = manifest.sha1_for(&rr).ok_or_else(|| {
                    format!("missing sha1 for {} in {}", rr, manifest.manifest_sha1())
                })?;
                write_stdout_line(&format!("{sha1} {rr}"))?;
            }
            return Ok(());
        }

        debug!("printing repository summary");
        let mut manifests = nwsync.get_all_manifests().map_err(|error| {
            format!(
                "failed to list manifests from {}: {}",
                cmd.input.display(),
                error
            )
        })?;
        manifests.sort_by_key(|sha1| sha1.to_string());
        write_stdout_line(&format!("root {}", nwsync.root().display()))?;
        write_stdout_line(&format!("manifests {}", manifests.len()))?;
        for sha1 in manifests {
            write_stdout_line(&format!("manifest {sha1}"))?;
        }
        let mut resrefs = nwsync.get_all_resrefs();
        resrefs.sort_by_key(|sha1| sha1.to_string());
        write_stdout_line(&format!("resrefs {}", resrefs.len()))?;
        return Ok(());
    }

    if cmd.manifest.is_some() {
        return Err("--manifest is only valid when INPUT is a nwsync repository".to_string());
    }

    info!("printing standalone manifest");
    let manifest = nwsync::read_manifest_file(&cmd.input)
        .map_err(|error| format!("failed to read manifest {}: {error}", cmd.input.display()))?;
    for entry in manifest.entries() {
        write_stdout_line(&format!("{} {} {}", entry.sha1, entry.size, entry.resref))?;
    }
    Ok(())
}

#[instrument(
    level = "info",
    skip_all,
    err,
    fields(
        input = %cmd.input.display(),
        output = %cmd.output.display(),
        force = cmd.force
    )
)]
pub(crate) fn run_nwsync_write(cmd: NwsyncWriteCmd) -> Result<(), String> {
    info!("generating nwsync manifest");

    if !cmd.input.is_dir() {
        return Err(format!(
            "input must be a directory: {}",
            cmd.input.display()
        ));
    }

    // Collect all files recursively
    let mut entries = Vec::new();
    collect_files_recursively(&cmd.input, &mut entries)?;

    if entries.is_empty() {
        return Err(format!(
            "no valid resource files found in {}",
            cmd.input.display()
        ));
    }

    // Create manifest
    let mut manifest = nwsync::Manifest::default();
    for (path, resref) in entries {
        debug!(path = %path.display(), resref = %resref, "processing file");

        let data = fs::read(&path)
            .map_err(|error| format!("failed to read {}: {error}", path.display()))?;

        let sha1 = checksums::secure_hash(&data);
        let size = u32::try_from(data.len()).map_err(|_error| {
            format!(
                "file {} is too large ({} bytes > u32::MAX)",
                path.display(),
                data.len()
            )
        })?;

        manifest.add_entry(nwsync::ManifestEntry::new(sha1, size, resref));
    }

    // Ensure output directory exists
    if let Some(parent) = cmd.output.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create output directory: {error}"))?;
    }

    // Check if output exists and handle force flag
    if cmd.output.exists() && !cmd.force {
        return Err(format!(
            "output file exists, use -f to overwrite: {}",
            cmd.output.display()
        ));
    }

    // Write manifest
    nwsync::write_manifest_file(&cmd.output, &manifest)
        .map_err(|error| format!("failed to write manifest {}: {error}", cmd.output.display()))?;

    info!(
        entry_count = manifest.entries().len(),
        "manifest written successfully"
    );
    Ok(())
}

fn collect_files_recursively(
    dir: &Path,
    entries: &mut Vec<(PathBuf, resref::ResRef)>,
) -> Result<(), String> {
    for entry in fs::read_dir(dir)
        .map_err(|error| format!("failed to read directory {}: {error}", dir.display()))?
    {
        let entry = entry.map_err(|error| format!("failed to read directory entry: {error}"))?;
        let path = entry.path();

        if path.is_dir() {
            // Skip common directories
            if matches!(
                path.file_name().and_then(|n| n.to_str()),
                Some(".git") | Some(".svn")
            ) {
                continue;
            }
            collect_files_recursively(&path, entries)?;
        } else if path.is_file() {
            // Try to parse as resref
            if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
                match resref::new_resolved_res_ref_from_filename(filename) {
                    Ok(resolved) => {
                        entries.push((path, resolved.base().clone()));
                    }
                    Err(_) => {
                        // Skip files that don't have valid resref names
                        debug!(path = %path.display(), "skipping file with invalid resref name");
                    }
                }
            }
        }
    }
    Ok(())
}

#[instrument(
    level = "info",
    skip_all,
    err,
    fields(repository = %cmd.repository.display(), dry_run = cmd.dry_run)
)]
pub(crate) fn run_nwsync_prune(cmd: NwsyncPruneCmd) -> Result<(), String> {
    info!("pruning nwsync repository");

    let nwsync = resnwsync::open_nwsync(&cmd.repository).map_err(|error| {
        format!(
            "failed to open nwsync repo {}: {error}",
            cmd.repository.display()
        )
    })?;

    // Get all manifests
    let manifests = nwsync
        .get_all_manifests()
        .map_err(|error| format!("failed to list manifests: {error}"))?;

    if manifests.is_empty() {
        info!("no manifests found, nothing to prune");
        return Ok(());
    }

    // Collect all referenced SHA-1s
    let mut referenced_sha1s = std::collections::HashSet::new();
    for manifest_sha1 in &manifests {
        let manifest = resnwsync::new_resnwsync_manifest(&nwsync, *manifest_sha1)
            .map_err(|error| format!("failed to load manifest {}: {error}", manifest_sha1))?;

        for resref in manifest.contents() {
            if let Some(sha1) = manifest.sha1_for(&resref) {
                referenced_sha1s.insert(sha1);
            }
        }
    }

    // Get all stored SHA-1s
    let all_sha1s = nwsync.get_all_resrefs();
    let mut unreferenced_sha1s = Vec::new();

    for sha1 in &all_sha1s {
        if !referenced_sha1s.contains(sha1) {
            unreferenced_sha1s.push(*sha1);
        }
    }

    if unreferenced_sha1s.is_empty() {
        info!("no unreferenced data found");
        return Ok(());
    }

    info!(
        total_manifests = manifests.len(),
        total_data = all_sha1s.len(),
        unreferenced = unreferenced_sha1s.len(),
        "found unreferenced data"
    );

    if cmd.dry_run {
        for sha1 in &unreferenced_sha1s {
            write_stdout_line(&format!("would remove: {sha1}"))?;
        }
        return Ok(());
    }

    // TODO: Implement actual removal from database
    // This would require adding removal methods to nwnrs_resnwsync
    // For now, just report what would be removed
    for sha1 in &unreferenced_sha1s {
        write_stdout_line(&format!("would remove: {sha1}"))?;
    }

    info!("prune operation not yet fully implemented - dry run shown above");
    Ok(())
}

#[instrument(
    level = "info",
    skip_all,
    err,
    fields(url = %cmd.url, output = cmd.output.as_ref().map(|p| p.display().to_string()).unwrap_or_default())
)]
pub(crate) fn run_nwsync_fetch(cmd: NwsyncFetchCmd) -> Result<(), String> {
    info!("fetching nwsync manifest");

    // For now, just show what would be done
    // TODO: Implement HTTP download and aria2c integration

    write_stdout_line(&format!("would fetch manifest from: {}", cmd.url))?;
    if let Some(output) = &cmd.output {
        write_stdout_line(&format!("would save to: {}", output.display()))?;
    }

    info!("fetch operation not yet implemented");
    Ok(())
}
