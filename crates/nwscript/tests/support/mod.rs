//! Shared test helpers for NWScript integration tests.

use std::{
    collections::HashMap,
    error::Error,
    fs,
    io,
    path::Path,
    sync::{Mutex, OnceLock},
};

const NWSCRIPT_ASSETS_BASE_URL: &str =
    "https://github.com/nwn-rs/assets/raw/refs/heads/build8193.37/nss";

static ASSET_CACHE: OnceLock<Mutex<HashMap<String, Vec<u8>>>> = OnceLock::new();

/// Builds one test-friendly `io::Error`.
pub fn test_error(message: impl Into<String>) -> io::Error {
    io::Error::other(message.into())
}

/// Returns the NWScript testing asset root without requiring the source files to be checked in.
pub fn assets_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/testing/nwscript")
}

/// Loads one NWScript source file from disk when present, otherwise fetches it from the pinned remote assets.
pub fn load_nss_bytes(assets: &Path, path: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut candidates = vec![path.to_string()];
    let lowercase = path.to_ascii_lowercase();
    if lowercase != path {
        candidates.push(lowercase);
    }

    for candidate in &candidates {
        let local_path = assets.join("nss").join(candidate);
        if local_path.exists() {
            return Ok(fs::read(local_path)?);
        }
    }

    let cache = ASSET_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    {
        let guard = cache
            .lock()
            .map_err(|error| test_error(format!("nwscript asset cache lock poisoned: {error}")))?;
        if let Some(bytes) = guard.get(path) {
            return Ok(bytes.clone());
        }
        for candidate in &candidates {
            if let Some(bytes) = guard.get(candidate) {
                return Ok(bytes.clone());
            }
        }
    }

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    let mut last_error: Option<Box<dyn Error>> = None;
    for candidate in &candidates {
        let url = format!("{NWSCRIPT_ASSETS_BASE_URL}/{candidate}");
        match runtime.block_on(async {
            let response = reqwest::get(&url).await?.error_for_status()?;
            Ok::<Vec<u8>, reqwest::Error>(response.bytes().await?.to_vec())
        }) {
            Ok(bytes) => {
                let mut guard = cache.lock().map_err(|error| {
                    test_error(format!("nwscript asset cache lock poisoned: {error}"))
                })?;
                guard.insert(path.to_string(), bytes.clone());
                guard.insert(candidate.clone(), bytes.clone());
                return Ok(bytes);
            }
            Err(error) => last_error = Some(Box::new(error)),
        }
    }

    Err(last_error.unwrap_or_else(|| test_error(format!("missing nwscript asset: {path}")).into()))
}

/// Returns whether the pinned remote NWScript assets are temporarily unavailable in this environment.
pub fn remote_assets_unavailable(error: &(dyn Error + 'static)) -> bool {
    error
        .downcast_ref::<reqwest::Error>()
        .is_some_and(|error| error.is_connect() || error.is_timeout())
}

/// Converts transient remote asset failures into a skipped test.
pub fn skip_if_remote_assets_unavailable(
    error: Box<dyn Error>,
) -> Result<(), Box<dyn Error>> {
    if remote_assets_unavailable(error.as_ref()) {
        tracing::warn!("skipping nwscript remote asset test: {error}");
        return Ok(());
    }

    Err(error)
}
