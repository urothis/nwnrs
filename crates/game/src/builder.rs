use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use nwnrs_resman::prelude::*;
use nwnrs_resnwsync::prelude::*;
use tracing::{debug, info, instrument};

use crate::prelude::*;

#[allow(clippy::too_many_arguments)]
/// Builds a conventional layered [`nwnrs_resman::ResMan`] for an NWN
/// installation.
///
/// The resulting manager may include, in precedence order, additional
/// directories, override directories, NWSync manifests, additional ERFs, and
/// the selected KEY/BIF sets.
#[instrument(
    level = "info",
    skip(
        root,
        user_directory,
        keys,
        additional_erfs,
        additional_dirs,
        additional_manifests
    ),
    err,
    fields(language, cache_size, load_keys, load_ovr)
)]
pub fn new_default_resman(
    root: impl AsRef<Path>,
    user_directory: impl AsRef<Path>,
    language: &str,
    cache_size: usize,
    load_keys: bool,
    load_ovr: bool,
    keys: &[String],
    additional_erfs: &[PathBuf],
    additional_dirs: &[PathBuf],
    additional_manifests: &[ManifestSha1],
) -> GameResult<ResMan> {
    info!("building default resource manager");
    let root = root.as_ref();
    let user_directory = user_directory.as_ref();
    let resolved_language = language;
    let resolved_language_root = root.join("lang").join(resolved_language);

    if !resolved_language_root.is_dir() {
        return Err(GameError::msg(format!(
            "language {} not found",
            resolved_language_root.display()
        )));
    }

    let autodetect_keys = keys.is_empty() || matches!(keys, [single] if single == "autodetect");
    let actual_keys = if !autodetect_keys {
        keys.join(",")
    } else {
        DEFAULT_KEYFILES.join(",")
    };
    let keys = actual_keys
        .split(',')
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();

    for erf in additional_erfs {
        if !erf.is_file() {
            return Err(GameError::msg(format!(
                "requested --erfs not found: {}",
                erf.display()
            )));
        }
    }

    let additional_dirs = additional_dirs
        .iter()
        .map(|dir| crate::expand_tilde(dir))
        .collect::<Vec<_>>();
    for dir in &additional_dirs {
        if !dir.is_dir() {
            return Err(GameError::msg(format!(
                "requested --dirs not found: {}",
                dir.display()
            )));
        }
    }

    let mut result = ResMan::new(cache_size);

    if load_keys {
        for key in &keys {
            debug!(key, "loading key");
            crate::keyload::load_key(&mut result, root, &resolved_language_root, key)?;
        }
    }

    for erf in additional_erfs {
        debug!(path = %erf.display(), "loading ERF container");
        let erf_container = nwnrs_erf::read_erf_from_file(erf)?;
        result.add(Arc::new(erf_container));
    }

    let mut nwsync = None;
    if !additional_manifests.is_empty() {
        if !user_directory.is_dir() {
            return Err(GameError::msg(format!(
                "{} is not a directory",
                user_directory.display()
            )));
        }
        nwsync = Some(open_nwsync(user_directory.join("nwsync"))?);
    }

    if let Some(nwsync) = &nwsync {
        for manifest_sha1 in additional_manifests {
            debug!(manifest = %manifest_sha1, "loading nwsync manifest");
            let container = new_resnwsync_manifest(nwsync, *manifest_sha1)?;
            result.add(Arc::new(container));
        }
    }

    if load_ovr {
        debug!("loading base override directory");
        result.add(Arc::new(nwnrs_resdir::read_resdir(root.join("ovr"))?));
    }
    if load_ovr {
        debug!("loading language override directory");
        result.add(Arc::new(nwnrs_resdir::read_resdir(
            resolved_language_root.join("data").join("ovr"),
        )?));
    }

    for dir in additional_dirs {
        debug!(path = %dir.display(), "loading additional directory");
        result.add(Arc::new(nwnrs_resdir::read_resdir(dir)?));
    }

    info!("built default resource manager");
    Ok(result)
}
