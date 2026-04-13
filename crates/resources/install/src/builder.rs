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
/// directories, override directories, `NWSync` manifests, additional ERFs, and
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
) -> InstallResult<ResMan> {
    info!("building default resource manager");
    let root = root.as_ref();
    let user_directory = user_directory.as_ref();
    let resolved_language_root = crate::resolve_language_root(root, language)?;

    let autodetect_keys = keys.is_empty() || matches!(keys, [single] if single == "autodetect");
    let actual_keys = if autodetect_keys {
        DEFAULT_KEYFILES.join(",")
    } else {
        keys.join(",")
    };
    let keys = actual_keys
        .split(',')
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();

    for erf in additional_erfs {
        if !erf.is_file() {
            return Err(InstallError::msg(format!(
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
            return Err(InstallError::msg(format!(
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
            return Err(InstallError::msg(format!(
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

    for dir in additional_dirs {
        debug!(path = %dir.display(), "loading additional directory");
        result.add(Arc::new(nwnrs_resdir::read_resdir(dir)?));
    }

    info!("built default resource manager");
    Ok(result)
}

#[allow(clippy::panic)]
#[cfg(test)]
mod tests {
    use std::{fs, time::SystemTime};

    use nwnrs_checksums::EMPTY_SECURE_HASH;

    use super::new_default_resman;
    use crate::resolve_language_root;

    fn unique_test_dir(prefix: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_else(|error| panic!("clock drift: {error}"))
            .as_nanos();
        std::env::temp_dir().join(format!("nwnrs-install-builder-{prefix}-{nanos}"))
    }

    #[test]
    fn rejects_missing_language_directory() {
        let root = unique_test_dir("missing-lang-root");
        let user = unique_test_dir("missing-lang-user");
        fs::create_dir_all(&root).unwrap_or_else(|error| panic!("create root: {error}"));
        fs::create_dir_all(&user).unwrap_or_else(|error| panic!("create user: {error}"));

        let err = new_default_resman(&root, &user, "english", 0, false, false, &[], &[], &[], &[])
            .expect_err("builder should fail");
        assert!(err.to_string().contains("language"));
    }

    #[test]
    fn language_aliases_resolve_short_folder_names() {
        let root = unique_test_dir("lang-alias-root");
        let alias_root = root.join("lang").join("en");
        fs::create_dir_all(&alias_root)
            .unwrap_or_else(|error| panic!("create alias root: {error}"));

        let resolved = resolve_language_root(&root, "english")
            .unwrap_or_else(|error| panic!("resolve language alias: {error}"));
        assert_eq!(resolved, alias_root);
    }

    #[test]
    fn rejects_missing_additional_directories() {
        let root = unique_test_dir("dirs-root");
        let user = unique_test_dir("dirs-user");
        let lang_root = root.join("lang").join("en");
        fs::create_dir_all(&lang_root).unwrap_or_else(|error| panic!("create lang root: {error}"));
        fs::create_dir_all(&user).unwrap_or_else(|error| panic!("create user: {error}"));
        let missing = root.join("does-not-exist");

        let err = new_default_resman(
            &root,
            &user,
            "english",
            0,
            false,
            false,
            &[],
            &[],
            &[missing],
            &[],
        )
        .expect_err("builder should fail");
        assert!(err.to_string().contains("requested --dirs not found"));
    }

    #[test]
    fn manifests_require_existing_user_directory() {
        let root = unique_test_dir("manifest-root");
        let user = unique_test_dir("manifest-user-missing");
        let lang_root = root.join("lang").join("en");
        fs::create_dir_all(&lang_root).unwrap_or_else(|error| panic!("create lang root: {error}"));

        let err = new_default_resman(
            &root,
            &user,
            "english",
            0,
            false,
            false,
            &[],
            &[],
            &[],
            &[EMPTY_SECURE_HASH],
        )
        .expect_err("builder should fail");
        assert!(err.to_string().contains("is not a directory"));
    }

    #[test]
    fn load_ovr_uses_root_override_directory() {
        let root = unique_test_dir("ovr-root");
        let user = unique_test_dir("ovr-user");
        let lang_root = root.join("lang").join("en");
        let ovr_root = root.join("ovr");
        fs::create_dir_all(&lang_root).unwrap_or_else(|error| panic!("create lang root: {error}"));
        fs::create_dir_all(&ovr_root).unwrap_or_else(|error| panic!("create ovr root: {error}"));
        fs::create_dir_all(&user).unwrap_or_else(|error| panic!("create user: {error}"));

        let result =
            new_default_resman(&root, &user, "english", 0, false, true, &[], &[], &[], &[]);

        assert!(result.is_ok(), "load_ovr should use root-level ovr");
    }
}
