use std::{
    fs::File,
    path::{Path, PathBuf},
    sync::Arc,
};

use nwnrs_key::BifResolver;
use nwnrs_resman::ResMan;
use tracing::{info, instrument, warn};

use crate::{InstallResult, normalize_relative_path, read_key_table, shared_stream};

#[instrument(
    level = "info",
    skip(into),
    err,
    fields(root = %root.display(), language_root = %resolved_language_root.display(), key_name = key)
)]
pub(crate) fn load_key(
    into: &mut ResMan,
    root: &Path,
    resolved_language_root: &Path,
    key: &str,
) -> InstallResult<()> {
    let key_file = Path::new("data").join(format!("{key}.key"));
    let key_path = if resolved_language_root.join(&key_file).is_file() {
        resolved_language_root.join(&key_file)
    } else {
        root.join(&key_file)
    };

    if !key_path.is_file() {
        if !key.ends_with("_loc") {
            warn!(path = %key_path.display(), "key not found, skipping");
        }
        return Ok(());
    }

    let lang_root = resolved_language_root.to_path_buf();
    let base_root = root.to_path_buf();
    let resolver: BifResolver = Arc::new(move |filename: &str| {
        let normalized = normalize_relative_path(filename);
        let basename = normalized
            .file_name()
            .map(PathBuf::from)
            .unwrap_or_else(|| normalized.clone());
        let language_candidate = lang_root.join("data").join(basename);
        let candidate = if language_candidate.is_file() {
            language_candidate
        } else {
            base_root.join(normalized)
        };

        if candidate.is_file() {
            Ok(Some(shared_stream(File::open(candidate)?)))
        } else {
            Ok(None)
        }
    });

    let file = File::open(&key_path)?;
    let key_table = read_key_table(file, key_path.display().to_string(), resolver)?;
    into.add(Arc::new(key_table));
    info!(path = %key_path.display(), "loaded key table");
    Ok(())
}

#[allow(clippy::panic)]
#[cfg(test)]
mod tests {
    use std::{fs, time::SystemTime};

    use nwnrs_resman::ResMan;

    use super::load_key;

    fn unique_test_dir(prefix: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_else(|error| panic!("clock drift: {error}"))
            .as_nanos();
        std::env::temp_dir().join(format!("nwnrs-install-keyload-{prefix}-{nanos}"))
    }

    #[test]
    fn missing_key_file_is_skipped_without_modifying_manager() {
        let root = unique_test_dir("root");
        let lang_root = root.join("lang").join("english");
        fs::create_dir_all(root.join("data"))
            .unwrap_or_else(|error| panic!("create data: {error}"));
        fs::create_dir_all(&lang_root).unwrap_or_else(|error| panic!("create lang root: {error}"));
        let mut manager = ResMan::new(0);

        load_key(&mut manager, &root, &lang_root, "missing")
            .unwrap_or_else(|error| panic!("load missing key: {error}"));

        assert!(manager.containers().is_empty());
    }
}
