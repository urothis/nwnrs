#![forbid(unsafe_code)]
//! Shared install-backed test helpers for workspace crates.

use std::{
    error::Error,
    fmt, fs, io,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::SystemTime,
};

use crate::{
    install::prelude::*,
    resman::{
        CachePolicy, Res, ResMan, ResManError, ResRef, ResRefError, ResType, ResolvedResRef,
        lookup_res_ext,
    },
};

const TEST_LANGUAGE: &str = "english";
const TEST_CACHE_SIZE_MB: usize = 64;

static INSTALL_CONTEXT: OnceLock<Result<InstallContext, TestResourceError>> = OnceLock::new();

struct InstallContext {
    root:   PathBuf,
    resman: Mutex<ResMan>,
}

/// Errors returned by install-backed test resource helpers.
///
/// # Examples
///
/// ```rust,no_run
/// let _ = std::mem::size_of::<nwnrs_types::test_support::TestResourceError>();
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

/// Preserves one install-backed resource error as a test failure.
///
/// # Examples
///
/// ```rust,no_run
/// let _ = nwnrs_types::test_support::skip_if_game_resources_unavailable;
/// ```
pub fn skip_if_game_resources_unavailable(error: Box<dyn Error>) -> Result<(), Box<dyn Error>> {
    Err(error)
}

/// Demands one shipped resource from the cached install-backed [`ResMan`].
///
/// # Examples
///
/// ```rust,no_run
/// let _ = nwnrs_types::test_support::demand_resource;
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
/// let _ = nwnrs_types::test_support::read_resource_bytes;
/// ```
pub fn read_resource_bytes(resref: &str, res_type: ResType) -> Result<Vec<u8>, TestResourceError> {
    demand_resource(resref, res_type)?
        .read_all(CachePolicy::Bypass)
        .map_err(Into::into)
}

/// Returns deterministic shipped resource candidates for one resource type.
///
/// # Examples
///
/// ```rust,no_run
/// let _ = nwnrs_types::test_support::shipped_resource_candidates;
/// ```
pub fn shipped_resource_candidates(
    res_type: ResType,
) -> Result<Vec<ResolvedResRef>, TestResourceError> {
    let context = install_context()?;
    let guard = context.resman.lock().map_err(|error| {
        TestResourceError::install_unavailable(format!("test resman lock poisoned: {error}"))
    })?;
    let mut result = guard
        .contents()
        .into_iter()
        .filter(|candidate| candidate.res_type() == res_type)
        .filter_map(|candidate| candidate.resolve())
        .collect::<Vec<_>>();
    result.sort_by_key(|candidate| candidate.to_file().to_ascii_lowercase());
    Ok(result)
}

/// Reads the first deterministic shipped resource of the requested type.
///
/// # Examples
///
/// ```rust,no_run
/// let _ = nwnrs_types::test_support::read_first_shipped_resource_bytes;
/// ```
pub fn read_first_shipped_resource_bytes(
    res_type: ResType,
) -> Result<(ResolvedResRef, Vec<u8>), TestResourceError> {
    read_first_shipped_resource_bytes_matching(res_type, |_resref, _bytes| true)
}

/// Reads the first deterministic shipped resource of the requested type whose
/// bytes satisfy `predicate`.
///
/// # Examples
///
/// ```rust,no_run
/// let _ = nwnrs_types::test_support::read_first_shipped_resource_bytes_matching::<
///     fn(&nwnrs_types::resman::ResolvedResRef, &[u8]) -> bool,
/// >;
/// ```
pub fn read_first_shipped_resource_bytes_matching<F>(
    res_type: ResType,
    mut predicate: F,
) -> Result<(ResolvedResRef, Vec<u8>), TestResourceError>
where
    F: FnMut(&ResolvedResRef, &[u8]) -> bool,
{
    let candidates = shipped_resource_candidates(res_type)?;
    for candidate in candidates {
        let bytes = read_resolved_resource_bytes(candidate.base())?;
        if predicate(&candidate, &bytes) {
            return Ok((candidate, bytes));
        }
    }

    let extension = lookup_res_ext(res_type).unwrap_or_else(|| format!("type-{}", res_type.0));
    Err(TestResourceError::resource_unavailable(format!(
        "no shipped .{extension} resource matched the requested predicate"
    )))
}

/// Materializes one shipped resource to a temporary file using its registered
/// extension.
///
/// # Examples
///
/// ```rust,no_run
/// let _ = nwnrs_types::test_support::materialize_resource_to_temp_file;
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
/// let _ = nwnrs_types::test_support::materialize_bytes_to_temp_file;
/// ```
pub fn materialize_bytes_to_temp_file(
    bytes: &[u8],
    filename: &str,
) -> Result<PathBuf, TestResourceError> {
    let path = std::env::temp_dir().join(unique_temp_name(filename));
    fs::write(&path, bytes)?;
    Ok(path)
}

/// Reads the canonical shipped English `dialog.tlk`.
///
/// # Examples
///
/// ```rust,no_run
/// let _ = nwnrs_types::test_support::read_shipped_dialog_tlk_bytes;
/// ```
pub fn read_shipped_dialog_tlk_bytes() -> Result<Vec<u8>, TestResourceError> {
    let context = install_context()?;
    let path = resolve_language_root(&context.root, TEST_LANGUAGE)?.join("data/dialog.tlk");
    fs::read(&path).map_err(Into::into)
}

/// Finds the first deterministic shipped loose file by extension under the
/// install root.
///
/// # Examples
///
/// ```rust,no_run
/// let _ = nwnrs_types::test_support::find_first_shipped_file;
/// ```
pub fn find_first_shipped_file(extension: &str) -> Result<PathBuf, TestResourceError> {
    read_first_shipped_file_bytes_matching(extension, |_path, _bytes| true).map(|(path, _)| path)
}

/// Reads the first deterministic shipped loose file by extension whose contents
/// satisfy `predicate`.
///
/// # Examples
///
/// ```rust,no_run
/// let _ = nwnrs_types::test_support::read_first_shipped_file_bytes_matching::<
///     fn(&std::path::Path, &[u8]) -> bool,
/// >;
/// ```
pub fn read_first_shipped_file_bytes_matching<F>(
    extension: &str,
    mut predicate: F,
) -> Result<(PathBuf, Vec<u8>), TestResourceError>
where
    F: FnMut(&Path, &[u8]) -> bool,
{
    let context = install_context()?;
    let normalized = extension.trim_start_matches('.').to_ascii_lowercase();
    let mut candidates = Vec::new();
    collect_matching_files(&context.root, &normalized, &mut candidates)?;
    candidates.sort_by_key(|path| path.display().to_string().to_ascii_lowercase());

    for candidate in candidates {
        let bytes = fs::read(&candidate)?;
        if predicate(&candidate, &bytes) {
            return Ok((candidate, bytes));
        }
    }

    Err(TestResourceError::resource_unavailable(format!(
        "no shipped .{normalized} file matched the requested predicate under {}",
        context.root.display()
    )))
}

/// Finds a deterministic shipped archive path by extension.
///
/// # Examples
///
/// ```rust,no_run
/// let _ = nwnrs_types::test_support::find_shipped_archive;
/// ```
pub fn find_shipped_archive(extension: &str) -> Result<PathBuf, TestResourceError> {
    shipped_archive_candidates(extension)?
        .into_iter()
        .next()
        .ok_or_else(|| {
            TestResourceError::resource_unavailable(format!(
                "no shipped .{} archive found",
                extension.trim_start_matches('.').to_ascii_lowercase()
            ))
        })
}

/// Returns deterministic shipped archive candidates by extension.
///
/// # Examples
///
/// ```rust,no_run
/// let _ = nwnrs_types::test_support::shipped_archive_candidates;
/// ```
pub fn shipped_archive_candidates(extension: &str) -> Result<Vec<PathBuf>, TestResourceError> {
    let context = install_context()?;
    let normalized = extension.trim_start_matches('.').to_ascii_lowercase();
    let roots = archive_search_roots(&context.root, &normalized)?;
    let mut matches = Vec::new();
    for root in roots {
        collect_matching_files(&root, &normalized, &mut matches)?;
    }
    matches.sort_by_key(|path| path.display().to_string().to_ascii_lowercase());
    Ok(matches)
}

/// Boxes install-backed resource errors for test functions that return
/// `Result<_, Box<dyn Error>>`.
///
/// # Examples
///
/// ```rust,no_run
/// let _ = nwnrs_types::test_support::require_game_resource::<()>;
/// ```
pub fn require_game_resource<T>(result: Result<T, TestResourceError>) -> Result<T, Box<dyn Error>> {
    result.map_err(|error| Box::new(error) as Box<dyn Error>)
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
            false,
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
        resman: Mutex::new(resman),
    })
}

fn archive_search_roots(root: &Path, extension: &str) -> Result<Vec<PathBuf>, TestResourceError> {
    match extension {
        "mod" => Ok(vec![root.join("data/mod")]),
        "hak" => Ok(vec![root.join("data/hk")]),
        "nwm" => Ok(vec![root.join("data/nwm")]),
        "erf" => Ok(vec![
            root.join("data/txpk"),
            resolve_language_root(root, TEST_LANGUAGE)?.join("data"),
        ]),
        other => Err(TestResourceError::resource_unavailable(format!(
            "unsupported shipped archive extension: {other}"
        ))),
    }
}

#[cfg(test)]
fn find_first_archive_match(roots: &[PathBuf], extension: &str) -> Option<PathBuf> {
    let mut matches = Vec::new();
    for root in roots {
        if collect_matching_files(root, extension, &mut matches).is_err() {
            continue;
        }
    }
    matches.sort_by_key(|path| path.display().to_string().to_ascii_lowercase());
    matches.into_iter().next()
}

fn read_resolved_resource_bytes(resref: &ResRef) -> Result<Vec<u8>, TestResourceError> {
    let context = install_context()?;
    let mut guard = context.resman.lock().map_err(|error| {
        TestResourceError::install_unavailable(format!("test resman lock poisoned: {error}"))
    })?;
    let res = guard.demand(resref, CachePolicy::Use).map_err(|error| {
        TestResourceError::resource_unavailable(format!(
            "missing shipped resource {resref}: {error}"
        ))
    })?;
    res.read_all(CachePolicy::Bypass).map_err(Into::into)
}

fn collect_matching_files(
    root: &Path,
    extension: &str,
    out: &mut Vec<PathBuf>,
) -> Result<(), TestResourceError> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries.collect::<Result<Vec<_>, io::Error>>()?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    let mut entries = entries;
    entries.sort_by_key(|entry| entry.file_name().to_string_lossy().to_ascii_lowercase());

    for entry in entries {
        let file_type = entry.file_type()?;
        let path = entry.path();
        if file_type.is_dir() {
            collect_matching_files(&path, extension, out)?;
        } else if file_type.is_file()
            && path
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case(extension))
        {
            out.push(path);
        }
    }

    Ok(())
}

fn unique_temp_name(filename: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("nwnrs-types-{nanos}-{filename}")
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

#[cfg(test)]
mod tests {
    use std::{error::Error, fs, io};

    use super::{
        TEST_LANGUAGE, archive_search_roots, discover_install_context_with,
        find_first_archive_match, materialize_bytes_to_temp_file, require_game_resource,
    };
    use crate::resman::lookup_res_type;

    fn unique_test_dir(prefix: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap_or_else(|error| panic!("clock drift: {error}"))
            .as_nanos();
        std::env::temp_dir().join(format!("nwnrs-types-{prefix}-{nanos}"))
    }

    #[test]
    fn install_discovery_failure_is_reported() {
        let result = require_game_resource(discover_install_context_with(
            |_override_dir| Err(io::Error::other("missing root").into()),
            |_override_dir| Err(io::Error::other("missing user").into()),
            |_root, _user| unreachable!("builder should not run"),
        ));
        let error = match result {
            Ok(_context) => panic!("discovery should not succeed"),
            Err(error) => error,
        };
        assert!(
            error
                .to_string()
                .contains("NWN install not available for install-backed tests"),
            "{error}"
        );
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
        fs::create_dir_all(root.join("data").join("mod").join("nested"))
            .unwrap_or_else(|error| panic!("create root mod dir: {error}"));
        fs::write(root.join("data").join("mod").join("b.mod"), [])
            .unwrap_or_else(|error| panic!("write b.mod: {error}"));
        fs::write(root.join("data").join("mod").join("a.mod"), [])
            .unwrap_or_else(|error| panic!("write a.mod: {error}"));
        fs::write(
            root.join("data").join("mod").join("nested").join("c.mod"),
            [],
        )
        .unwrap_or_else(|error| panic!("write c.mod: {error}"));

        let roots = archive_search_roots(&root, "mod")
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
        fs::create_dir_all(root.join("lang").join("en").join("data"))
            .unwrap_or_else(|error| panic!("create language data: {error}"));

        let roots = archive_search_roots(&root, "erf")
            .unwrap_or_else(|error| panic!("archive roots: {error}"));
        assert_eq!(roots.first(), Some(&root.join("data/txpk")));
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
        let res = require_game_resource(super::demand_resource("cloak_001", plt_type))?;
        assert_eq!(res.resref().res_ref(), "cloak_001");
        Ok(())
    }
}
