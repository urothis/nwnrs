use std::{
    collections::HashSet,
    fs,
    io::Cursor,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use nwnrs::prelude::{resman::ResContainer, *};
use reqwest::Url;
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
        manifests.sort_by_key(std::string::ToString::to_string);
        write_stdout_line(&format!("root {}", nwsync.root().display()))?;
        write_stdout_line(&format!("manifests {}", manifests.len()))?;
        for sha1 in manifests {
            write_stdout_line(&format!("manifest {sha1}"))?;
        }
        let mut resrefs = nwsync.get_all_resrefs();
        resrefs.sort_by_key(std::string::ToString::to_string);
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
                Some(".git" | ".svn")
            ) {
                continue;
            }
            collect_files_recursively(&path, entries)?;
        } else if path.is_file() {
            // Try to parse as resref
            if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
                match resref::ResolvedResRef::from_filename(filename) {
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

    let mut nwsync = resnwsync::open_nwsync(&cmd.repository).map_err(|error| {
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
            .map_err(|error| format!("failed to load manifest {manifest_sha1}: {error}"))?;

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

    let deleted = nwsync
        .delete_resref_data(&unreferenced_sha1s)
        .map_err(|error| format!("failed to prune {}: {error}", cmd.repository.display()))?;
    for sha1 in &unreferenced_sha1s {
        write_stdout_line(&format!("removed: {sha1}"))?;
    }

    info!(deleted, "prune completed");
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

    let output = cmd.output.unwrap_or_else(|| PathBuf::from("."));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("failed to initialize async runtime: {error}"))?;
    let summary = runtime.block_on(fetch_manifest_repository(&cmd.url, &output))?;

    write_stdout_line(&format!("manifest {}", summary.manifest_sha1))?;
    write_stdout_line(&format!("downloaded {}", summary.downloaded))?;
    write_stdout_line(&format!("skipped {}", summary.skipped))?;
    write_stdout_line(&format!("output {}", output.display()))?;
    Ok(())
}

struct FetchSummary {
    manifest_sha1: checksums::SecureHash,
    downloaded:    usize,
    skipped:       usize,
}

async fn fetch_manifest_repository(url: &str, output: &Path) -> Result<FetchSummary, String> {
    let manifest_url = Url::parse(url).map_err(|error| format!("invalid url {url}: {error}"))?;
    let base_url = manifest_repository_base_url(&manifest_url)?;
    let client = reqwest::Client::builder()
        .build()
        .map_err(|error| format!("failed to build http client: {error}"))?;

    let manifest_bytes = fetch_bytes(&client, &manifest_url).await?;
    let manifest_sha1 = checksums::secure_hash(&manifest_bytes);
    if let Some(url_sha1) = manifest_sha1_from_url(&manifest_url)
        && url_sha1 != manifest_sha1
    {
        return Err(format!(
            "manifest sha1 in url ({url_sha1}) does not match downloaded payload ({manifest_sha1})"
        ));
    }

    let manifest = nwsync::read_manifest(&mut Cursor::new(&manifest_bytes))
        .map_err(|error| format!("failed to parse manifest {manifest_url}: {error}"))?;
    let hash_tree_depth = manifest
        .hash_tree_depth()
        .map_err(|error| format!("failed to read manifest hash tree depth: {error}"))?;

    let mut repo = resnwsync::open_or_create_nwsync(output)
        .map_err(|error| format!("failed to open output repo {}: {error}", output.display()))?;

    let mut seen = HashSet::new();
    let mut downloaded = 0_usize;
    let mut skipped = 0_usize;
    for entry in manifest.entries() {
        if !seen.insert(entry.sha1) {
            continue;
        }

        if repo.contains_resref_data(entry.sha1) {
            skipped += 1;
            continue;
        }

        let blob_url = manifest_entry_data_url(&base_url, entry.sha1, hash_tree_depth)?;
        let blob = fetch_bytes(&client, &blob_url).await?;
        let inserted = repo
            .put_resref_data(entry.sha1, &blob)
            .map_err(|error| format!("failed to store {blob_url}: {error}"))?;
        if inserted {
            downloaded += 1;
        } else {
            skipped += 1;
        }
    }

    let created_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let created_at = i64::try_from(created_at).unwrap_or(i64::MAX);
    repo.put_manifest(manifest_sha1, &manifest, created_at)
        .map_err(|error| format!("failed to store manifest {manifest_sha1}: {error}"))?;

    Ok(FetchSummary {
        manifest_sha1,
        downloaded,
        skipped,
    })
}

async fn fetch_bytes(client: &reqwest::Client, url: &Url) -> Result<Vec<u8>, String> {
    client
        .get(url.clone())
        .send()
        .await
        .map_err(|error| format!("failed to fetch {url}: {error}"))?
        .error_for_status()
        .map_err(|error| format!("request failed for {url}: {error}"))?
        .bytes()
        .await
        .map(|bytes| bytes.to_vec())
        .map_err(|error| format!("failed to read response body for {url}: {error}"))
}

fn manifest_repository_base_url(manifest_url: &Url) -> Result<Url, String> {
    let segments = manifest_url
        .path_segments()
        .ok_or_else(|| format!("url has no path segments: {manifest_url}"))?
        .filter(|segment| !segment.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    if segments.is_empty() {
        return Err(format!("url has no manifest path: {manifest_url}"));
    }

    let trim = if segments.len() >= 2
        && matches!(
            segments.get(segments.len() - 2).map(String::as_str),
            Some("manifest" | "manifests")
        ) {
        2
    } else {
        1
    };

    let mut base = manifest_url.clone();
    base.set_query(None);
    base.set_fragment(None);
    {
        let mut parts = base
            .path_segments_mut()
            .map_err(|error| format!("cannot derive base path from {manifest_url}: {error:?}"))?;
        parts.clear();
        let keep = segments.len() - trim;
        for segment in segments.iter().take(keep) {
            parts.push(segment);
        }
    }
    Ok(base)
}

fn manifest_sha1_from_url(url: &Url) -> Option<checksums::SecureHash> {
    let last = url
        .path_segments()
        .and_then(|mut segments| segments.rfind(|segment| !segment.is_empty()))?;

    last.parse().ok()
}

fn manifest_entry_data_url(
    base_url: &Url,
    sha1: checksums::SecureHash,
    hash_tree_depth: usize,
) -> Result<Url, String> {
    let sha1_hex = sha1.to_string();
    let mut url = base_url.clone();
    let base_segments = base_url
        .path_segments()
        .ok_or_else(|| format!("cannot build data url from {base_url}"))?
        .filter(|segment| !segment.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    {
        let mut parts = url
            .path_segments_mut()
            .map_err(|error| format!("cannot build data url from {base_url}: {error:?}"))?;
        parts.clear();
        for segment in &base_segments {
            parts.push(segment);
        }
        parts.push("data");
        parts.push("sha1");
        for index in 0..hash_tree_depth {
            let start = index * 2;
            let prefix = sha1_hex.get(start..start + 2).ok_or_else(|| {
                format!("sha1 too short for hash tree depth {hash_tree_depth}: {sha1_hex}")
            })?;
            parts.push(prefix);
        }
        parts.push(&sha1_hex);
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use nwnrs::prelude::checksums::parse_secure_hash;
    use reqwest::Url;

    use super::{manifest_entry_data_url, manifest_repository_base_url};

    #[test]
    fn manifest_base_url_strips_manifest_path() {
        let url = match Url::parse(
            "https://example.com/nwsync/manifests/0123456789012345678901234567890123456789",
        ) {
            Ok(value) => value,
            Err(error) => panic!("parse url: {error}"),
        };
        let base = match manifest_repository_base_url(&url) {
            Ok(value) => value,
            Err(error) => panic!("base url: {error}"),
        };
        assert_eq!(base.as_str(), "https://example.com/nwsync");
    }

    #[test]
    fn manifest_data_url_uses_hash_tree_layout() {
        let base = match Url::parse("https://example.com/nwsync/") {
            Ok(value) => value,
            Err(error) => panic!("parse base: {error}"),
        };
        let sha1 = match parse_secure_hash("0123456789012345678901234567890123456789") {
            Ok(value) => value,
            Err(error) => panic!("parse sha1: {error}"),
        };
        let url = match manifest_entry_data_url(&base, sha1, 2) {
            Ok(value) => value,
            Err(error) => panic!("data url: {error}"),
        };
        assert_eq!(
            url.as_str(),
            "https://example.com/nwsync/data/sha1/01/23/0123456789012345678901234567890123456789"
        );
    }
}
