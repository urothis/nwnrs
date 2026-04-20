#![forbid(unsafe_code)]
//! Shared install-backed test helpers for workspace crates.

use std::{
    error::Error,
    fmt, fs, io,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::SystemTime,
};

use nwnrs_install::prelude::*;
use nwnrs_resman::prelude::*;
use nwnrs_resref::prelude::*;
use nwnrs_restype::prelude::*;

const TEST_LANGUAGE: &str = "english";
const TEST_CACHE_SIZE_MB: usize = 64;

static INSTALL_CONTEXT: OnceLock<Result<InstallContext, TestResourceError>> = OnceLock::new();

struct InstallContext {
    root:   PathBuf,
    user:   PathBuf,
    resman: Mutex<ResMan>,
}

/// Errors returned by install-backed test resource helpers.
///
/// # Examples
///
/// ```rust,no_run
/// let _ = std::mem::size_of::<nwnrs_test_support::TestResourceError>();
/// ```
#[derive(Debug)]
pub enum TestResourceError {
    /// The local Neverwinter Nights install or user directory could not be
    /// discovered.
    InstallUnavailable(String),
    /// One required shipped resource could not be found in the discovered
    /// install.
    ResourceUnavailable(String),
    /// An underlying IO operation failed.
    Io(io::Error),
    /// Install discovery or manager construction failed.
    Install(InstallError),
    /// Resource-manager access failed.
    ResMan(ResManError),
    /// Resource-reference validation failed.
    ResRef(ResRefError),
}

impl TestResourceError {
    fn install_unavailable(message: impl Into<String>) -> Self {
        Self::InstallUnavailable(message.into())
    }

    fn resource_unavailable(message: impl Into<String>) -> Self {
        Self::ResourceUnavailable(message.into())
    }
}

impl fmt::Display for TestResourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InstallUnavailable(message) | Self::ResourceUnavailable(message) => {
                f.write_str(message)
            }
            Self::Io(error) => error.fmt(f),
            Self::Install(error) => error.fmt(f),
            Self::ResMan(error) => error.fmt(f),
            Self::ResRef(error) => error.fmt(f),
        }
    }
}

impl Error for TestResourceError {}

impl From<io::Error> for TestResourceError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<InstallError> for TestResourceError {
    fn from(value: InstallError) -> Self {
        Self::Install(value)
    }
}

impl From<ResManError> for TestResourceError {
    fn from(value: ResManError) -> Self {
        Self::ResMan(value)
    }
}

impl From<ResRefError> for TestResourceError {
    fn from(value: ResRefError) -> Self {
        Self::ResRef(value)
    }
}

/// Marker error used to convert unavailable game resources into skipped tests.
///
/// # Examples
///
/// ```rust,no_run
/// let _ = std::mem::size_of::<nwnrs_test_support::SkippedTestError>();
/// ```
#[derive(Debug)]
pub struct SkippedTestError {
    message: String,
}

impl fmt::Display for SkippedTestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for SkippedTestError {}

/// Returns `true` when `error` indicates that install-backed game resources are
/// unavailable in the current environment.
///
/// # Examples
///
/// ```rust,no_run
/// let _ = nwnrs_test_support::game_resources_unavailable;
/// ```
pub fn game_resources_unavailable(error: &(dyn Error + 'static)) -> bool {
    error.downcast_ref::<SkippedTestError>().is_some()
}

/// Converts an unavailable-game-resources error into a cleanly skipped test.
///
/// # Examples
///
/// ```rust,no_run
/// let _ = nwnrs_test_support::skip_if_game_resources_unavailable;
/// ```
pub fn skip_if_game_resources_unavailable(error: Box<dyn Error>) -> Result<(), Box<dyn Error>> {
    if game_resources_unavailable(error.as_ref()) {
        tracing::warn!("skipping install-backed test: {error}");
        return Ok(());
    }

    Err(error)
}

/// Demands one shipped resource from the cached install-backed [`ResMan`].
///
/// # Examples
///
/// ```rust,no_run
/// let _ = nwnrs_test_support::demand_resource;
/// ```
pub fn demand_resource(resref: &str, res_type: ResType) -> Result<Res, TestResourceError> {
    let context = install_context()?;
    let rr = ResRef::new(resref.to_string(), res_type)?;
    let mut guard = context.resman.lock().map_err(|error| {
        TestResourceError::install_unavailable(format!("test resman lock poisoned: {error}"))
    })?;
    guard.demand(&rr, CachePolicy::Use).map_err(|error| {
        TestResourceError::resource_unavailable(format!("missing shipped resource {rr}: {error}"))
    })
}

/// Reads the raw bytes for one shipped resource from the cached install-backed
/// [`ResMan`].
///
/// # Examples
///
/// ```rust,no_run
/// let _ = nwnrs_test_support::read_resource_bytes;
/// ```
pub fn read_resource_bytes(resref: &str, res_type: ResType) -> Result<Vec<u8>, TestResourceError> {
    demand_resource(resref, res_type)?
        .read_all(CachePolicy::Bypass)
        .map_err(Into::into)
}

/// Materializes one shipped resource to a temporary file using its registered
/// extension.
///
/// # Examples
///
/// ```rust,no_run
/// let _ = nwnrs_test_support::materialize_resource_to_temp_file;
/// ```
pub fn materialize_resource_to_temp_file(
    resref: &str,
    res_type: ResType,
) -> Result<PathBuf, TestResourceError> {
    let bytes = read_resource_bytes(resref, res_type)?;
    let extension = lookup_res_ext(res_type).ok_or_else(|| {
        TestResourceError::resource_unavailable(format!(
            "resource type {res_type} has no registered extension"
        ))
    })?;
    materialize_bytes_to_temp_file(&bytes, &format!("{resref}.{extension}"))
}

/// Writes `bytes` to a uniquely named file under the process temp directory.
///
/// # Examples
///
/// ```rust,no_run
/// let _ = nwnrs_test_support::materialize_bytes_to_temp_file;
/// ```
pub fn materialize_bytes_to_temp_file(
    bytes: &[u8],
    filename: &str,
) -> Result<PathBuf, TestResourceError> {
    let path = std::env::temp_dir().join(unique_temp_name(filename));
    fs::write(&path, bytes)?;
    Ok(path)
}

/// Finds a deterministic shipped archive path by extension.
///
/// # Examples
///
/// ```rust,no_run
/// let _ = nwnrs_test_support::find_shipped_archive;
/// ```
pub fn find_shipped_archive(extension: &str) -> Result<PathBuf, TestResourceError> {
    let context = install_context()?;
    find_shipped_archive_in_roots(&context.root, &context.user, extension)
}

/// Converts unavailable install-backed resources into a boxed skip marker while
/// preserving all other errors.
///
/// # Examples
///
/// ```rust,no_run
/// let _ = nwnrs_test_support::require_game_resource::<()>;
/// ```
pub fn require_game_resource<T>(result: Result<T, TestResourceError>) -> Result<T, Box<dyn Error>> {
    result.map_err(|error| match error {
        TestResourceError::InstallUnavailable(message)
        | TestResourceError::ResourceUnavailable(message) => Box::new(SkippedTestError {
            message,
        }) as Box<dyn Error>,
        other => Box::new(other) as Box<dyn Error>,
    })
}

fn install_context() -> Result<&'static InstallContext, TestResourceError> {
    match INSTALL_CONTEXT.get_or_init(discover_install_context) {
        Ok(context) => Ok(context),
        Err(error) => Err(error.clone_for_cache()),
    }
}

fn discover_install_context() -> Result<InstallContext, TestResourceError> {
    discover_install_context_with(find_nwnrs_root, find_user_root, |root, user| {
        new_default_resman(
            root,
            user,
            TEST_LANGUAGE,
            TEST_CACHE_SIZE_MB,
            true,
            true,
            &[],
            &[],
            &[],
            &[],
        )
    })
}

fn discover_install_context_with<FR, FU, FB>(
    find_root: FR,
    find_user: FU,
    build_resman: FB,
) -> Result<InstallContext, TestResourceError>
where
    FR: Fn(&str) -> InstallResult<PathBuf>,
    FU: Fn(&str) -> InstallResult<PathBuf>,
    FB: Fn(&Path, &Path) -> InstallResult<ResMan>,
{
    let root = match find_root("") {
        Ok(root) => root,
        Err(error) => {
            return Err(TestResourceError::install_unavailable(format!(
                "NWN install not available for install-backed tests: {error}"
            )));
        }
    };
    let user = match find_user("") {
        Ok(user) => user,
        Err(error) => {
            return Err(TestResourceError::install_unavailable(format!(
                "NWN user directory not available for install-backed tests: {error}"
            )));
        }
    };
    let resman = build_resman(&root, &user).map_err(|error| {
        TestResourceError::install_unavailable(format!(
            "failed to build install-backed resource manager for tests: {error}"
        ))
    })?;
    Ok(InstallContext {
        root,
        user,
        resman: Mutex::new(resman),
    })
}

fn find_shipped_archive_in_roots(
    root: &Path,
    user: &Path,
    extension: &str,
) -> Result<PathBuf, TestResourceError> {
    let normalized = extension.trim_start_matches('.').to_ascii_lowercase();
    let roots = archive_search_roots(root, user, &normalized)?;
    find_first_archive_match(&roots, &normalized).ok_or_else(|| {
        TestResourceError::resource_unavailable(format!(
            "no shipped .{normalized} archive found under {}",
            roots
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ))
    })
}

fn archive_search_roots(
    root: &Path,
    user: &Path,
    extension: &str,
) -> Result<Vec<PathBuf>, TestResourceError> {
    match extension {
        "mod" => Ok(vec![root.join("modules"), user.join("modules")]),
        "hak" => Ok(vec![root.join("hak"), user.join("hak")]),
        "erf" => Ok(vec![
            root.join("data"),
            resolve_language_root(root, TEST_LANGUAGE)?.join("data"),
        ]),
        other => Err(TestResourceError::resource_unavailable(format!(
            "unsupported shipped archive extension: {other}"
        ))),
    }
}

fn find_first_archive_match(roots: &[PathBuf], extension: &str) -> Option<PathBuf> {
    for root in roots {
        let Ok(entries) = fs::read_dir(root) else {
            continue;
        };
        let mut matches = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.is_file())
            .filter(|path| {
                path.extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case(extension))
            })
            .collect::<Vec<_>>();
        matches.sort_by_key(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().to_ascii_lowercase())
        });
        if let Some(first) = matches.into_iter().next() {
            return Some(first);
        }
    }
    None
}

fn unique_temp_name(filename: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("nwnrs-test-support-{nanos}-{filename}")
}

impl TestResourceError {
    fn clone_for_cache(&self) -> Self {
        match self {
            Self::InstallUnavailable(message) => Self::InstallUnavailable(message.clone()),
            Self::ResourceUnavailable(message) => Self::ResourceUnavailable(message.clone()),
            Self::Io(error) => Self::Io(io::Error::new(error.kind(), error.to_string())),
            Self::Install(error) => Self::InstallUnavailable(error.to_string()),
            Self::ResMan(error) => Self::ResourceUnavailable(error.to_string()),
            Self::ResRef(error) => Self::ResourceUnavailable(error.to_string()),
        }
    }
}

#[allow(clippy::panic)]
#[cfg(test)]
mod tests {
    use std::{error::Error, fs, io};

    use nwnrs_restype::lookup_res_type;

    use super::{
        TEST_LANGUAGE, archive_search_roots, discover_install_context_with,
        find_first_archive_match, materialize_bytes_to_temp_file, require_game_resource,
        skip_if_game_resources_unavailable,
    };

    fn unique_test_dir(prefix: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap_or_else(|error| panic!("clock drift: {error}"))
            .as_nanos();
        std::env::temp_dir().join(format!("nwnrs-test-support-{prefix}-{nanos}"))
    }

    #[test]
    fn install_discovery_failure_becomes_skip() -> Result<(), Box<dyn Error>> {
        let result = require_game_resource(discover_install_context_with(
            |_override_dir| Err(io::Error::other("missing root").into()),
            |_override_dir| Err(io::Error::other("missing user").into()),
            |_root, _user| unreachable!("builder should not run"),
        ));
        match result {
            Ok(_context) => panic!("discovery should not succeed"),
            Err(error) => skip_if_game_resources_unavailable(error),
        }
    }

    #[test]
    fn temp_file_materialization_preserves_bytes() {
        let path = materialize_bytes_to_temp_file(b"hello", "test.bin")
            .unwrap_or_else(|error| panic!("materialize bytes: {error}"));
        let bytes = fs::read(&path).unwrap_or_else(|error| panic!("read temp bytes: {error}"));
        assert_eq!(bytes, b"hello");
    }

    #[test]
    fn archive_lookup_uses_lexicographic_order_per_root() {
        let root = unique_test_dir("archive-root");
        let user = unique_test_dir("archive-user");
        fs::create_dir_all(root.join("modules"))
            .unwrap_or_else(|error| panic!("create root modules: {error}"));
        fs::create_dir_all(user.join("modules"))
            .unwrap_or_else(|error| panic!("create user modules: {error}"));
        fs::write(root.join("modules").join("b.mod"), [])
            .unwrap_or_else(|error| panic!("write b.mod: {error}"));
        fs::write(root.join("modules").join("a.mod"), [])
            .unwrap_or_else(|error| panic!("write a.mod: {error}"));
        fs::write(user.join("modules").join("c.mod"), [])
            .unwrap_or_else(|error| panic!("write c.mod: {error}"));

        let roots = archive_search_roots(&root, &user, "mod")
            .unwrap_or_else(|error| panic!("archive roots: {error}"));
        let result = find_first_archive_match(&roots, "mod")
            .unwrap_or_else(|| panic!("missing archive match"));
        assert_eq!(
            result.file_name().and_then(|name| name.to_str()),
            Some("a.mod")
        );
    }

    #[test]
    fn archive_roots_include_language_data_for_erf() {
        let root = unique_test_dir("erf-root");
        let user = unique_test_dir("erf-user");
        fs::create_dir_all(root.join("lang").join("en").join("data"))
            .unwrap_or_else(|error| panic!("create language data: {error}"));

        let roots = archive_search_roots(&root, &user, "erf")
            .unwrap_or_else(|error| panic!("archive roots: {error}"));
        assert_eq!(roots.first(), Some(&root.join("data")));
        assert_eq!(
            roots.get(1),
            Some(&root.join("lang").join("en").join("data"))
        );
        assert_eq!(TEST_LANGUAGE, "english");
    }

    #[test]
    fn shipped_resource_demand_succeeds_when_game_is_available() -> Result<(), Box<dyn Error>> {
        let Some(plt_type) = lookup_res_type("plt") else {
            panic!("plt res type should be registered");
        };
        let result = require_game_resource(super::demand_resource("cloak_001", plt_type));
        match result {
            Ok(res) => {
                assert_eq!(res.resref().res_ref(), "cloak_001");
                Ok(())
            }
            Err(error) => skip_if_game_resources_unavailable(error),
        }
    }
}
