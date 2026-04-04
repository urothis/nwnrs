use std::{
    fs::File,
    path::{Path, PathBuf},
    sync::Arc,
};

use nwnrs_key::BifResolver;
use nwnrs_resman::ResMan;
use tracing::{info, instrument, warn};

use crate::{GameResult, normalize_relative_path, read_key_table, shared_stream};

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
) -> GameResult<()> {
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
