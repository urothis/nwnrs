use crate::args::NwsyncPrintCmd;
use crate::util::write_stdout_line;
use nwn_nwsync::prelude::*;
use nwn_resman::prelude::*;
use nwn_resnwsync::prelude::*;
use tracing::{debug, info, instrument};

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
        let nwsync = open_nwsync(&cmd.input).map_err(|error| {
            format!(
                "failed to open nwsync repo {}: {error}",
                cmd.input.display()
            )
        })?;

        if let Some(manifest) = cmd.manifest {
            debug!("printing specific manifest");
            let manifest_sha1 = manifest
                .parse()
                .map_err(|error| format!("invalid manifest sha1 {manifest}: {error}"))?;
            let manifest = new_resnwsync_manifest(&nwsync, manifest_sha1).map_err(|error| {
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
    let manifest = read_manifest_file(&cmd.input)
        .map_err(|error| format!("failed to read manifest {}: {error}", cmd.input.display()))?;
    for entry in manifest.entries() {
        write_stdout_line(&format!("{} {} {}", entry.sha1, entry.size, entry.resref))?;
    }
    Ok(())
}
