use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    env, fs,
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::{Command, ExitStatus, Stdio},
    sync::Arc,
    thread,
    time::Duration,
};

use nwnrs_nwscript::CancellationToken;
use nwnrs_types::checksums;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use tempfile::{Builder, NamedTempFile};
use url::Url;

mod cache_lock;

use cache_lock::{CacheLeases, CacheLock};

use super::{
    ResolvedIncludeDependency, canonical_directory,
    lockfile::{read_dependency_lock, write_dependency_lock},
    model::{
        DEPENDENCY_LOCK_FORMAT, DependencyLock, GitSelector, LockedDependency,
        LockedDependencySource,
    },
    read_required_manifest,
};
use crate::{
    DependencySpec, GitDependency, NwpkgError, NwpkgErrorKind, PROJECT_MANIFEST_FILENAME,
    ProjectKind, ProjectManifest, fs::atomic_write,
};

const MAX_DEPENDENCY_DEPTH: usize = 256;
const CACHE_OWNERS_DIRECTORY: &str = ".owners";
const CACHE_MANAGED_DIRECTORY: &str = ".managed";
const CACHE_METADATA_FORMAT: u32 = 1;
const MAX_CACHE_TREE_DEPTH: usize = 512;
const MAX_CACHE_TREE_ENTRIES: usize = 1_000_000;

/// One sanitized, user-presentable dependency resolver progress event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyProgressEvent {
    /// Stable operation phase suitable for editor presentation.
    pub phase:   &'static str,
    /// Sanitized progress text containing no embedded repository credentials.
    pub message: String,
}

/// Thread-safe progress sink used by explicit dependency operations.
#[derive(Clone)]
pub struct DependencyProgressReporter(Arc<dyn Fn(DependencyProgressEvent) + Send + Sync + 'static>);

impl DependencyProgressReporter {
    /// Creates a progress reporter from a thread-safe callback.
    #[must_use]
    pub fn new(callback: impl Fn(DependencyProgressEvent) + Send + Sync + 'static) -> Self {
        Self(Arc::new(callback))
    }

    fn report(&self, phase: &'static str, message: impl Into<String>) {
        (self.0)(DependencyProgressEvent {
            phase,
            message: message.into(),
        });
    }
}

/// Configuration for the explicit system-Git dependency resolver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyResolverConfiguration {
    /// System Git executable.
    pub git_executable: PathBuf,
    /// Content-addressed immutable package cache.
    pub cache_root:     PathBuf,
}

impl DependencyResolverConfiguration {
    /// Resolves platform defaults and supported environment overrides.
    ///
    /// `NWNRS_GIT` selects the executable and `NWNRS_PACKAGE_CACHE` selects the
    /// cache directory.
    #[must_use]
    pub fn from_environment() -> Self {
        let git_executable = env::var_os("NWNRS_GIT")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("git"));
        let cache_root = env::var_os("NWNRS_PACKAGE_CACHE")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(default_cache_root);
        Self {
            git_executable,
            cache_root,
        }
    }
}

impl Default for DependencyResolverConfiguration {
    fn default() -> Self {
        Self::from_environment()
    }
}

/// Explicit network/cache operation requested by a user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DependencyOperationKind {
    /// Materialize existing pins and create missing initial pins.
    Fetch,
    /// Re-resolve movable selectors before writing new pins.
    Update,
}

/// Produces the stable source identity stored in dependency locks.
///
/// Filesystem repositories are canonicalized relative to their declaring
/// package. Remote URLs use a parsed identity with normalized scheme, host,
/// default port, dot segments, and trailing separators. SCP-style sources are
/// converted to their equivalent canonical SSH URL.
///
/// # Errors
///
/// Returns an error when a filesystem source cannot be resolved or represented
/// by the UTF-8 lockfile format.
pub fn normalize_git_source(source: &str, declaring_root: &Path) -> Result<String, NwpkgError> {
    let source = source.trim();
    if source.contains("://") {
        return normalize_remote_git_url(source);
    }
    if let Some((authority, path)) = split_scp_git_source(source) {
        let (user, host) = authority.split_once('@').ok_or_else(|| {
            NwpkgError::new(
                NwpkgErrorKind::Validation,
                "invalid SCP-style Git source authority",
            )
        })?;
        let host = host.to_ascii_lowercase();
        let path = path.trim_start_matches('/').trim_end_matches('/');
        let canonical = format!("ssh://{user}@{host}/{path}");
        return normalize_remote_git_url(&canonical);
    }
    let path = Path::new(source);
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        declaring_root.join(path)
    };
    let canonical = fs::canonicalize(&path).map_err(|error| {
        NwpkgError::at(
            NwpkgErrorKind::Io,
            &path,
            format!("failed to resolve Git dependency source: {error}"),
        )
    })?;
    canonical.to_str().map(str::to_string).ok_or_else(|| {
        NwpkgError::at(
            NwpkgErrorKind::Validation,
            &canonical,
            "Git dependency filesystem paths must be valid UTF-8",
        )
    })
}

fn normalize_remote_git_url(source: &str) -> Result<String, NwpkgError> {
    let mut parsed = Url::parse(source).map_err(|error| {
        NwpkgError::new(
            NwpkgErrorKind::Validation,
            format!("invalid Git dependency URL: {error}"),
        )
    })?;
    if parsed.password().is_some()
        || (!parsed.username().is_empty() && !matches!(parsed.scheme(), "ssh" | "git+ssh"))
    {
        return Err(NwpkgError::new(
            NwpkgErrorKind::Validation,
            "Git dependency URLs cannot contain embedded credentials",
        ));
    }
    if parsed.query().is_some() || parsed.fragment().is_some() {
        return Err(NwpkgError::new(
            NwpkgErrorKind::Validation,
            "Git dependency URLs cannot contain query strings or fragments",
        ));
    }
    if !matches!(
        parsed.scheme(),
        "file" | "git" | "git+ssh" | "http" | "https" | "ssh"
    ) {
        return Err(NwpkgError::new(
            NwpkgErrorKind::Validation,
            format!(
                "unsupported Git dependency URL scheme {:?}",
                parsed.scheme()
            ),
        ));
    }
    if parsed.scheme() == "file" {
        let path = parsed.to_file_path().map_err(|()| {
            NwpkgError::new(
                NwpkgErrorKind::Validation,
                "Git file URL does not contain a valid native path",
            )
        })?;
        let canonical = fs::canonicalize(&path).map_err(|error| {
            NwpkgError::at(
                NwpkgErrorKind::Io,
                &path,
                format!("failed to resolve Git file URL: {error}"),
            )
        })?;
        return Url::from_file_path(&canonical)
            .map(|url| url.to_string())
            .map_err(|()| {
                NwpkgError::at(
                    NwpkgErrorKind::Validation,
                    canonical,
                    "Git file URL path cannot be represented canonically",
                )
            });
    }
    if parsed.cannot_be_a_base() || parsed.host_str().is_none() {
        return Err(NwpkgError::new(
            NwpkgErrorKind::Validation,
            "Git dependency URL must contain a hierarchical host",
        ));
    }
    if parsed.scheme() == "git+ssh" {
        parsed.set_scheme("ssh").map_err(|()| {
            NwpkgError::new(
                NwpkgErrorKind::Validation,
                "failed to canonicalize Git SSH URL scheme",
            )
        })?;
    }
    let default_port = match parsed.scheme() {
        "http" => Some(80),
        "https" => Some(443),
        "ssh" => Some(22),
        "git" => Some(9418),
        _ => None,
    };
    if parsed.port().is_some_and(|port| Some(port) == default_port) {
        parsed.set_port(None).map_err(|()| {
            NwpkgError::new(
                NwpkgErrorKind::Validation,
                "failed to canonicalize Git URL port",
            )
        })?;
    }
    let normalized_path = parsed.path().trim_end_matches('/').to_string();
    parsed.set_path(if normalized_path.is_empty() {
        "/"
    } else {
        &normalized_path
    });
    let mut normalized = parsed.to_string();
    if parsed.path() == "/" && parsed.query().is_none() {
        normalized.pop();
    }
    Ok(normalized)
}

/// Result of one atomic dependency operation.
#[derive(Debug, Clone)]
pub struct DependencyOperation {
    /// Newly validated lock graph.
    pub lock:         DependencyLock,
    /// Materialized compiler include dependencies.
    pub dependencies: Vec<ResolvedIncludeDependency>,
    /// Commit changes keyed by deterministic dependency route.
    pub changes:      BTreeMap<String, (Option<String>, String)>,
    _cache_leases:    Option<Arc<CacheLeases>>,
}

impl PartialEq for DependencyOperation {
    fn eq(&self, other: &Self) -> bool {
        self.lock == other.lock
            && self.dependencies == other.dependencies
            && self.changes == other.changes
    }
}

impl Eq for DependencyOperation {}

impl DependencyOperation {
    /// Returns the validated lock graph while retaining all cache leases until
    /// this operation is dropped.
    #[must_use]
    pub fn lock(&self) -> &DependencyLock {
        &self.lock
    }
}

/// Resolves, validates, materializes, and atomically locks one package graph.
///
/// This is the only nwpkg operation that may perform network access.
///
/// # Errors
///
/// Returns a typed error for invalid manifests, Git failures, unsafe paths,
/// dependency cycles, cache corruption, cancellation, or lock persistence.
pub fn resolve_dependency_operation(
    input: &Path,
    configuration: &DependencyResolverConfiguration,
    operation: DependencyOperationKind,
    selected_dependency: Option<&str>,
    cancellation: &CancellationToken,
) -> Result<DependencyOperation, NwpkgError> {
    resolve_dependency_operation_with_progress(
        input,
        configuration,
        operation,
        selected_dependency,
        cancellation,
        None,
    )
}

/// Resolves, validates, and atomically locks a dependency operation while
/// publishing sanitized progress events.
///
/// # Errors
///
/// Returns the same failures as [`resolve_dependency_operation`].
pub fn resolve_dependency_operation_with_progress(
    input: &Path,
    configuration: &DependencyResolverConfiguration,
    operation: DependencyOperationKind,
    selected_dependency: Option<&str>,
    cancellation: &CancellationToken,
    progress: Option<&DependencyProgressReporter>,
) -> Result<DependencyOperation, NwpkgError> {
    let mut result = plan_dependency_operation_with_progress(
        input,
        configuration,
        operation,
        selected_dependency,
        cancellation,
        progress,
    )?;
    let project_root = canonical_directory(input, "project")?;
    write_registered_dependency_lock_with_cancellation(
        configuration,
        &project_root,
        &result.lock,
        cancellation,
    )?;
    result._cache_leases = None;
    Ok(result)
}

/// Resolves and validates an operation without replacing `nwpkg.lock`, while
/// publishing sanitized progress events.
///
/// # Errors
///
/// Returns the same failures as [`plan_dependency_operation`].
pub fn plan_dependency_operation_with_progress(
    input: &Path,
    configuration: &DependencyResolverConfiguration,
    operation: DependencyOperationKind,
    selected_dependency: Option<&str>,
    cancellation: &CancellationToken,
    progress: Option<&DependencyProgressReporter>,
) -> Result<DependencyOperation, NwpkgError> {
    plan_dependency_operation_inner(
        input,
        configuration,
        operation,
        selected_dependency,
        cancellation,
        progress,
    )
}

/// Resolves and validates an operation without replacing `nwpkg.lock`.
///
/// Cache entries may be populated, but the project remains pinned to its
/// current lock until [`resolve_dependency_operation`] or
/// [`write_dependency_lock`] applies the returned plan.
///
/// # Errors
///
/// Returns the same failures as [`resolve_dependency_operation`].
pub fn plan_dependency_operation(
    input: &Path,
    configuration: &DependencyResolverConfiguration,
    operation: DependencyOperationKind,
    selected_dependency: Option<&str>,
    cancellation: &CancellationToken,
) -> Result<DependencyOperation, NwpkgError> {
    plan_dependency_operation_inner(
        input,
        configuration,
        operation,
        selected_dependency,
        cancellation,
        None,
    )
}

fn plan_dependency_operation_inner(
    input: &Path,
    configuration: &DependencyResolverConfiguration,
    operation: DependencyOperationKind,
    selected_dependency: Option<&str>,
    cancellation: &CancellationToken,
    progress: Option<&DependencyProgressReporter>,
) -> Result<DependencyOperation, NwpkgError> {
    check_cancelled(cancellation)?;
    let project_root = canonical_directory(input, "project")?;
    let manifest = read_required_manifest(&project_root, cancellation)?;
    let previous = read_dependency_lock(&project_root)?;
    let manifest_sha256 = hash_manifest(&project_root)?;
    let mut builder = GraphBuilder {
        configuration,
        cancellation,
        operation,
        selected_dependency,
        previous: previous.as_ref(),
        packages: BTreeMap::new(),
        dependencies: Vec::new(),
        emitted_roots: HashSet::new(),
        active_roots: vec![project_root.clone()],
        changes: BTreeMap::new(),
        cache_leases: CacheLeases::default(),
        progress,
    };
    let root = builder.visit_dependencies(&project_root, &manifest, "")?;
    let lock = DependencyLock {
        format: DEPENDENCY_LOCK_FORMAT,
        manifest_sha256,
        root,
        packages: builder.packages,
    };
    lock.validate(&project_root.join(crate::PACKAGE_LOCK_FILENAME))?;
    Ok(DependencyOperation {
        lock,
        dependencies: builder.dependencies,
        changes: builder.changes,
        _cache_leases: Some(Arc::new(builder.cache_leases)),
    })
}

/// Registers the cache entries referenced by one exact package lock.
///
/// The registry is external to immutable checkouts and keyed by the exact
/// native lockfile path. It lets cleanup consider packages outside the current
/// workspace without scanning arbitrary user directories.
///
/// # Errors
///
/// Returns an error if the registry cannot be updated atomically.
pub fn register_dependency_lock(
    configuration: &DependencyResolverConfiguration,
    project_root: &Path,
    lock: &DependencyLock,
) -> Result<(), NwpkgError> {
    register_dependency_lock_with_cancellation(
        configuration,
        project_root,
        lock,
        &CancellationToken::new(),
    )
}

/// Atomically replaces `nwpkg.lock` and its global cache-owner record.
///
/// Cleanup shares the registry lock with this operation, so it can never
/// observe a newly applied lock without its cache ownership. If lock
/// persistence fails, the previous owner record is restored; a restoration
/// failure can only over-retain cache entries and can never make a referenced
/// entry eligible for deletion.
///
/// # Errors
///
/// Returns an error if the owner record or package lock cannot be persisted.
pub fn write_registered_dependency_lock(
    configuration: &DependencyResolverConfiguration,
    project_root: &Path,
    lock: &DependencyLock,
) -> Result<(), NwpkgError> {
    write_registered_dependency_lock_with_cancellation(
        configuration,
        project_root,
        lock,
        &CancellationToken::new(),
    )
}

/// Cancellation-aware form of [`write_registered_dependency_lock`].
///
/// # Errors
///
/// Returns an error if cancellation is requested or either persistent record
/// cannot be updated safely.
pub fn write_registered_dependency_lock_with_cancellation(
    configuration: &DependencyResolverConfiguration,
    project_root: &Path,
    lock: &DependencyLock,
    cancellation: &CancellationToken,
) -> Result<(), NwpkgError> {
    let (owner_path, record) = dependency_owner_record(configuration, project_root, lock)?;
    let _guard = CacheLock::acquire(&configuration.cache_root, "registry", cancellation)?;
    let previous_owner = match fs::read(&owner_path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(NwpkgError::at(
                NwpkgErrorKind::Io,
                &owner_path,
                format!("failed to read the previous dependency owner record: {error}"),
            ));
        }
    };
    write_toml_atomically(&owner_path, &record)?;
    if let Err(lock_error) = write_dependency_lock(project_root, lock) {
        let restore_result = match previous_owner {
            Some(bytes) => atomic_write(&owner_path, &bytes, true),
            None => fs::remove_file(&owner_path).map_err(|error| {
                NwpkgError::at(
                    NwpkgErrorKind::Io,
                    &owner_path,
                    format!("failed to remove the staged dependency owner record: {error}"),
                )
            }),
        };
        if let Err(restore_error) = restore_result {
            return Err(NwpkgError::at(
                NwpkgErrorKind::Io,
                &owner_path,
                format!(
                    "{lock_error}; additionally failed to restore the dependency owner record: \
                     {restore_error}"
                ),
            ));
        }
        return Err(lock_error);
    }
    Ok(())
}

fn register_dependency_lock_with_cancellation(
    configuration: &DependencyResolverConfiguration,
    project_root: &Path,
    lock: &DependencyLock,
    cancellation: &CancellationToken,
) -> Result<(), NwpkgError> {
    let (owner_path, record) = dependency_owner_record(configuration, project_root, lock)?;
    let _guard = CacheLock::acquire(&configuration.cache_root, "registry", cancellation)?;
    write_toml_atomically(&owner_path, &record)
}

fn dependency_owner_record(
    configuration: &DependencyResolverConfiguration,
    project_root: &Path,
    lock: &DependencyLock,
) -> Result<(PathBuf, CacheOwnerRecord), NwpkgError> {
    let lock_path = fs::canonicalize(project_root)
        .map_err(|error| {
            NwpkgError::at(
                NwpkgErrorKind::Io,
                project_root,
                format!("failed to resolve dependency lock owner: {error}"),
            )
        })?
        .join(crate::PACKAGE_LOCK_FILENAME);
    let encoded_path = encode_native_path(&lock_path);
    let owner_key = checksums::sha256_digest(encoded_path.as_bytes()).to_string();
    let record = CacheOwnerRecord {
        path:   encoded_path,
        caches: lock_cache_keys(lock),
    };
    let owners = configuration.cache_root.join(CACHE_OWNERS_DIRECTORY);
    Ok((owners.join(format!("{owner_key}.toml")), record))
}

/// Removes managed cache entries proven unreferenced by every registered lock.
///
/// # Errors
///
/// Returns an error if the global owner registry cannot be validated or an
/// unused immutable entry cannot be removed. Unregistered legacy entries are
/// preserved because they cannot safely be proven unused.
pub fn clean_dependency_cache(
    configuration: &DependencyResolverConfiguration,
) -> Result<Vec<PathBuf>, NwpkgError> {
    clean_dependency_cache_with_cancellation(configuration, &CancellationToken::new())
}

/// Cancellable form of [`clean_dependency_cache`].
///
/// # Errors
///
/// Returns the same failures as [`clean_dependency_cache`] or a cancellation
/// failure before mutating the next cache entry.
pub fn clean_dependency_cache_with_cancellation(
    configuration: &DependencyResolverConfiguration,
    cancellation: &CancellationToken,
) -> Result<Vec<PathBuf>, NwpkgError> {
    let _guard = CacheLock::acquire(&configuration.cache_root, "registry", cancellation)?;
    let used = refresh_cache_owners(configuration, cancellation)?;
    let managed_root = configuration.cache_root.join(CACHE_MANAGED_DIRECTORY);
    let entries = match fs::read_dir(&managed_root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(NwpkgError::at(
                NwpkgErrorKind::Io,
                &managed_root,
                format!("failed to inspect managed dependency cache entries: {error}"),
            ));
        }
    };
    let mut removed = Vec::new();
    for entry in entries {
        check_cancelled(cancellation)?;
        let entry = entry.map_err(|error| {
            NwpkgError::at(
                NwpkgErrorKind::Io,
                &managed_root,
                format!("failed to inspect managed dependency cache marker: {error}"),
            )
        })?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name.len() != 64
            || !name.bytes().all(|byte| byte.is_ascii_hexdigit())
            || used.contains(name)
        {
            continue;
        }
        let cache_path = configuration.cache_root.join(name);
        let Some(_cache_guard) = CacheLock::try_acquire(&configuration.cache_root, name)? else {
            continue;
        };
        if cache_path.exists() {
            make_writable_recursive(&cache_path, cancellation)?;
            fs::remove_dir_all(&cache_path).map_err(|error| {
                NwpkgError::at(
                    NwpkgErrorKind::Io,
                    &cache_path,
                    format!("failed to remove unused dependency cache entry: {error}"),
                )
            })?;
            removed.push(cache_path);
        }
        fs::remove_file(entry.path()).map_err(|error| {
            NwpkgError::at(
                NwpkgErrorKind::Io,
                entry.path(),
                format!("failed to remove dependency cache ownership marker: {error}"),
            )
        })?;
    }
    removed.sort();
    Ok(removed)
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CacheOwnerRecord {
    path:   String,
    caches: BTreeSet<String>,
}

fn lock_cache_keys(lock: &DependencyLock) -> BTreeSet<String> {
    lock.packages
        .values()
        .filter_map(|package| match &package.source {
            LockedDependencySource::Git {
                cache, ..
            } => Some(cache.clone()),
            LockedDependencySource::Path {
                ..
            } => None,
        })
        .collect()
}

fn refresh_cache_owners(
    configuration: &DependencyResolverConfiguration,
    cancellation: &CancellationToken,
) -> Result<BTreeSet<String>, NwpkgError> {
    let owners = configuration.cache_root.join(CACHE_OWNERS_DIRECTORY);
    let entries = match fs::read_dir(&owners) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeSet::new()),
        Err(error) => {
            return Err(NwpkgError::at(
                NwpkgErrorKind::Io,
                &owners,
                format!("failed to inspect dependency owner registry: {error}"),
            ));
        }
    };
    let mut used = BTreeSet::new();
    for entry in entries {
        check_cancelled(cancellation)?;
        let entry = entry.map_err(|error| {
            NwpkgError::at(
                NwpkgErrorKind::Io,
                &owners,
                format!("failed to inspect dependency owner record: {error}"),
            )
        })?;
        let text = fs::read_to_string(entry.path()).map_err(|error| {
            NwpkgError::at(
                NwpkgErrorKind::Io,
                entry.path(),
                format!("failed to read dependency owner record: {error}"),
            )
        })?;
        let mut record = toml::from_str::<CacheOwnerRecord>(&text).map_err(|error| {
            NwpkgError::at(
                NwpkgErrorKind::Parse,
                entry.path(),
                format!("failed to parse dependency owner record: {error}"),
            )
        })?;
        let lock_path = decode_native_path(&record.path)
            .map_err(|message| NwpkgError::at(NwpkgErrorKind::Parse, entry.path(), message))?;
        let project_root = lock_path.parent().ok_or_else(|| {
            NwpkgError::at(
                NwpkgErrorKind::Validation,
                &lock_path,
                "dependency owner lock path has no project root",
            )
        })?;
        match read_dependency_lock(project_root)? {
            Some(lock) => {
                let caches = lock_cache_keys(&lock);
                if caches != record.caches {
                    record.caches.clone_from(&caches);
                    write_toml_atomically(&entry.path(), &record)?;
                }
                used.extend(caches);
            }
            None => {
                // A missing owner may live on an unmounted volume or have been
                // moved without nwnrs observing it. Preserve its references;
                // absence alone is not proof that its immutable pins are dead.
                used.extend(record.caches);
            }
        }
    }
    Ok(used)
}

#[cfg(unix)]
fn encode_native_path(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt as _;

    format!("unix:{}", encode_hex(path.as_os_str().as_bytes()))
}

#[cfg(windows)]
fn encode_native_path(path: &Path) -> String {
    use std::os::windows::ffi::OsStrExt as _;

    let mut bytes = Vec::new();
    for unit in path.as_os_str().encode_wide() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    format!("windows:{}", encode_hex(&bytes))
}

#[cfg(not(any(unix, windows)))]
compile_error!("nwnrs dependency cache identity requires Unix or Windows native paths");

#[cfg(unix)]
fn decode_native_path(encoded: &str) -> Result<PathBuf, String> {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt as _};

    let bytes = decode_prefixed_hex(encoded, "unix:")?;
    Ok(PathBuf::from(OsString::from_vec(bytes)))
}

#[cfg(windows)]
fn decode_native_path(encoded: &str) -> Result<PathBuf, String> {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt as _};

    let bytes = decode_prefixed_hex(encoded, "windows:")?;
    if bytes.len() % 2 != 0 {
        return Err("dependency owner path contains an incomplete UTF-16 unit".to_string());
    }
    let units = bytes
        .chunks_exact(2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .collect::<Vec<_>>();
    Ok(PathBuf::from(OsString::from_wide(&units)))
}

#[cfg(not(any(unix, windows)))]
fn decode_native_path(encoded: &str) -> Result<PathBuf, String> {
    let bytes = decode_prefixed_hex(encoded, "utf8:")?;
    String::from_utf8(bytes)
        .map(PathBuf::from)
        .map_err(|error| format!("dependency owner path is not UTF-8: {error}"))
}

fn encode_hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        output.push(char::from_digit(u32::from(byte >> 4), 16).unwrap_or('0'));
        output.push(char::from_digit(u32::from(byte & 0x0f), 16).unwrap_or('0'));
    }
    output
}

fn decode_prefixed_hex(encoded: &str, prefix: &str) -> Result<Vec<u8>, String> {
    let hex = encoded
        .strip_prefix(prefix)
        .ok_or_else(|| "dependency owner path is not encoded for this platform".to_string())?;
    if hex.len() % 2 != 0 {
        return Err("dependency owner path contains incomplete hexadecimal data".to_string());
    }
    let (pairs, remainder) = hex.as_bytes().as_chunks::<2>();
    if !remainder.is_empty() {
        return Err("dependency owner path contains incomplete hexadecimal data".to_string());
    }
    pairs
        .iter()
        .map(|[first, second]| {
            let high = hex_value(*first).ok_or_else(|| {
                "dependency owner path contains invalid hexadecimal data".to_string()
            })?;
            let low = hex_value(*second).ok_or_else(|| {
                "dependency owner path contains invalid hexadecimal data".to_string()
            })?;
            Ok((high << 4) | low)
        })
        .collect()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn write_toml_atomically<T: Serialize>(path: &Path, value: &T) -> Result<(), NwpkgError> {
    let parent = path.parent().ok_or_else(|| {
        NwpkgError::at(
            NwpkgErrorKind::Validation,
            path,
            "dependency registry path has no parent",
        )
    })?;
    fs::create_dir_all(parent).map_err(|error| {
        NwpkgError::at(
            NwpkgErrorKind::Io,
            parent,
            format!("failed to create dependency registry directory: {error}"),
        )
    })?;
    let encoded = toml::to_string(value).map_err(|error| {
        NwpkgError::at(
            NwpkgErrorKind::Parse,
            path,
            format!("failed to encode dependency registry record: {error}"),
        )
    })?;
    let mut temporary = NamedTempFile::new_in(parent).map_err(|error| {
        NwpkgError::at(
            NwpkgErrorKind::Io,
            parent,
            format!("failed to create dependency registry staging file: {error}"),
        )
    })?;
    std::io::Write::write_all(&mut temporary, encoded.as_bytes()).map_err(|error| {
        NwpkgError::at(
            NwpkgErrorKind::Io,
            temporary.path(),
            format!("failed to write dependency registry staging file: {error}"),
        )
    })?;
    temporary.as_file().sync_all().map_err(|error| {
        NwpkgError::at(
            NwpkgErrorKind::Io,
            temporary.path(),
            format!("failed to synchronize dependency registry staging file: {error}"),
        )
    })?;
    temporary.persist(path).map_err(|error| {
        NwpkgError::at(
            NwpkgErrorKind::Io,
            path,
            format!(
                "failed to publish dependency registry record: {}",
                error.error
            ),
        )
    })?;
    Ok(())
}

fn mark_cache_managed(
    configuration: &DependencyResolverConfiguration,
    cache: &str,
) -> Result<(), NwpkgError> {
    let managed = configuration.cache_root.join(CACHE_MANAGED_DIRECTORY);
    fs::create_dir_all(&managed).map_err(|error| {
        NwpkgError::at(
            NwpkgErrorKind::Io,
            &managed,
            format!("failed to create managed cache registry: {error}"),
        )
    })?;
    let marker = managed.join(cache);
    OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&marker)
        .and_then(|file| file.sync_all())
        .map_err(|error| {
            NwpkgError::at(
                NwpkgErrorKind::Io,
                marker,
                format!("failed to register managed cache entry: {error}"),
            )
        })
}

struct GraphBuilder<'a> {
    configuration:       &'a DependencyResolverConfiguration,
    cancellation:        &'a CancellationToken,
    operation:           DependencyOperationKind,
    selected_dependency: Option<&'a str>,
    previous:            Option<&'a DependencyLock>,
    packages:            BTreeMap<String, LockedDependency>,
    dependencies:        Vec<ResolvedIncludeDependency>,
    emitted_roots:       HashSet<PathBuf>,
    active_roots:        Vec<PathBuf>,
    changes:             BTreeMap<String, (Option<String>, String)>,
    cache_leases:        CacheLeases,
    progress:            Option<&'a DependencyProgressReporter>,
}

impl GraphBuilder<'_> {
    fn visit_dependencies(
        &mut self,
        declaring_root: &Path,
        manifest: &ProjectManifest,
        parent_route: &str,
    ) -> Result<BTreeMap<String, String>, NwpkgError> {
        if self.active_roots.len() >= MAX_DEPENDENCY_DEPTH {
            return Err(NwpkgError::at(
                NwpkgErrorKind::Validation,
                declaring_root,
                format!("dependency graph exceeded maximum depth {MAX_DEPENDENCY_DEPTH}"),
            ));
        }
        let mut edges = BTreeMap::new();
        for (name, specification) in &manifest.dependencies {
            check_cancelled(self.cancellation)?;
            let route = if parent_route.is_empty() {
                name.clone()
            } else {
                format!("{parent_route}/{name}")
            };
            let (package_root, source) = match specification {
                DependencySpec::Path(dependency) => {
                    let root = canonical_directory(
                        &declaring_root.join(&dependency.path),
                        &format!("dependency {name:?}"),
                    )?;
                    (
                        root,
                        LockedDependencySource::Path {
                            path: normalize_local_dependency_path(&dependency.path),
                        },
                    )
                }
                DependencySpec::Git(dependency) => {
                    dependency.validate(name)?;
                    self.materialize_git(&route, declaring_root, dependency)?
                }
            };
            let package_root = fs::canonicalize(&package_root).map_err(|error| {
                NwpkgError::at(
                    NwpkgErrorKind::Io,
                    &package_root,
                    format!("failed to canonicalize dependency {name:?}: {error}"),
                )
            })?;
            if let Some(index) = self
                .active_roots
                .iter()
                .position(|candidate| candidate == &package_root)
            {
                let cycle_roots = self.active_roots.get(index..).ok_or_else(|| {
                    NwpkgError::at(
                        NwpkgErrorKind::Validation,
                        &package_root,
                        "dependency traversal cycle index became inconsistent",
                    )
                })?;
                let mut cycle = cycle_roots
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>();
                cycle.push(package_root.display().to_string());
                return Err(NwpkgError::at(
                    NwpkgErrorKind::Validation,
                    &package_root,
                    format!("nwpkg dependency cycle: {}", cycle.join(" -> ")),
                ));
            }
            let dependency_manifest = read_required_manifest(&package_root, self.cancellation)?;
            if dependency_manifest.project.kind != ProjectKind::Include {
                return Err(NwpkgError::at(
                    NwpkgErrorKind::Validation,
                    &package_root,
                    format!(
                        "dependency {name:?} is kind {:?}; only include packages may be \
                         dependencies",
                        dependency_manifest.project.kind
                    ),
                ));
            }
            let source_root = canonical_directory(
                &package_root.join(&dependency_manifest.source.path),
                &format!("source for dependency {name:?}"),
            )?;
            if !source_root.starts_with(&package_root) {
                return Err(NwpkgError::at(
                    NwpkgErrorKind::Validation,
                    &source_root,
                    format!(
                        "dependency {name:?} source escapes package root {}",
                        package_root.display()
                    ),
                ));
            }
            self.active_roots.push(package_root.clone());
            let dependency_edges =
                self.visit_dependencies(&package_root, &dependency_manifest, &route)?;
            let popped = self.active_roots.pop();
            if popped.as_ref() != Some(&package_root) {
                return Err(NwpkgError::at(
                    NwpkgErrorKind::Validation,
                    &package_root,
                    "dependency traversal stack became inconsistent",
                ));
            }
            let locked = LockedDependency {
                name: name.clone(),
                source,
                manifest_sha256: hash_manifest(&package_root)?,
                dependencies: dependency_edges,
            };
            if let Some(existing) = self.packages.get(&route) {
                if existing != &locked {
                    return Err(NwpkgError::at(
                        NwpkgErrorKind::Validation,
                        &package_root,
                        format!("dependency route {route:?} resolved inconsistently"),
                    ));
                }
            } else {
                self.packages.insert(route.clone(), locked);
            }
            if self.emitted_roots.insert(package_root.clone()) {
                self.dependencies.push(ResolvedIncludeDependency {
                    name: name.clone(),
                    package_root,
                    source_root,
                });
            }
            edges.insert(name.clone(), route);
        }
        Ok(edges)
    }

    fn materialize_git(
        &mut self,
        route: &str,
        declaring_root: &Path,
        dependency: &GitDependency,
    ) -> Result<(PathBuf, LockedDependencySource), NwpkgError> {
        let mut dependency = dependency.clone();
        dependency.git = normalize_git_source(&dependency.git, declaring_root)?;
        let selector = GitSelector::from_dependency(&dependency);
        let previous = self.previous.and_then(|lock| lock.packages.get(route));
        let previous_git = previous.and_then(|package| match &package.source {
            LockedDependencySource::Git {
                git,
                selector,
                commit,
                subdir,
                cache,
            } if git == &dependency.git
                && selector == &GitSelector::from_dependency(&dependency)
                && subdir == &dependency.subdir =>
            {
                Some((commit.clone(), cache.clone()))
            }
            _ => None,
        });
        let selected = self
            .selected_dependency
            .is_none_or(|selected| route == selected || route.starts_with(&format!("{selected}/")));
        let required_to_reach_selection = self
            .selected_dependency
            .is_some_and(|selected| selected.starts_with(&format!("{route}/")));
        let network_allowed =
            self.selected_dependency.is_none() || selected || required_to_reach_selection;
        let should_update = self.operation == DependencyOperationKind::Update && selected;
        let (commit, cache) = if !network_allowed {
            let (commit, cache) = previous_git.ok_or_else(|| {
                NwpkgError::at(
                    NwpkgErrorKind::Validation,
                    &self.configuration.cache_root,
                    format!(
                        "dependency {route:?} is outside the selected operation and has no usable \
                         lock; fetch the complete dependency graph first"
                    ),
                )
            })?;
            reuse_cached_checkout(
                self.configuration,
                &dependency,
                &commit,
                &cache,
                route,
                &mut self.cache_leases,
                self.cancellation,
            )?
        } else if !should_update {
            if let Some((commit, cache)) = previous_git {
                ensure_cached_checkout(
                    self.configuration,
                    &dependency,
                    &selector,
                    Some(&commit),
                    Some(&cache),
                    self.cancellation,
                    self.progress,
                    &mut self.cache_leases,
                )?
            } else {
                ensure_cached_checkout(
                    self.configuration,
                    &dependency,
                    &selector,
                    None,
                    None,
                    self.cancellation,
                    self.progress,
                    &mut self.cache_leases,
                )?
            }
        } else {
            ensure_cached_checkout(
                self.configuration,
                &dependency,
                &selector,
                None,
                None,
                self.cancellation,
                self.progress,
                &mut self.cache_leases,
            )?
        };
        let previous_commit = previous.and_then(|package| match &package.source {
            LockedDependencySource::Git {
                commit, ..
            } => Some(commit.clone()),
            LockedDependencySource::Path {
                ..
            } => None,
        });
        if previous_commit.as_deref() != Some(commit.as_str()) {
            self.changes
                .insert(route.to_string(), (previous_commit, commit.clone()));
        }
        let repository = self
            .configuration
            .cache_root
            .join(&cache)
            .join("repository");
        let package_root = dependency
            .subdir
            .as_ref()
            .map_or_else(|| repository.clone(), |subdir| repository.join(subdir));
        Ok((
            package_root,
            LockedDependencySource::Git {
                git: dependency.git.clone(),
                selector,
                commit,
                subdir: dependency.subdir.clone(),
                cache,
            },
        ))
    }
}

fn normalize_local_dependency_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if normalized
                    .components()
                    .next_back()
                    .is_some_and(|component| matches!(component, std::path::Component::Normal(_)))
                {
                    normalized.pop();
                } else {
                    normalized.push("..");
                }
            }
            std::path::Component::Normal(component) => normalized.push(component),
            std::path::Component::RootDir | std::path::Component::Prefix(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    normalized
}

fn split_scp_git_source(source: &str) -> Option<(&str, &str)> {
    source
        .split_once(':')
        .filter(|(authority, path)| authority.contains('@') && !path.is_empty())
}

fn reuse_cached_checkout(
    configuration: &DependencyResolverConfiguration,
    dependency: &GitDependency,
    commit: &str,
    cache: &str,
    route: &str,
    leases: &mut CacheLeases,
    cancellation: &CancellationToken,
) -> Result<(String, String), NwpkgError> {
    leases.acquire(&configuration.cache_root, cache, cancellation)?;
    let root = configuration.cache_root.join(cache);
    if validate_cache_entry(&root, dependency, commit, None, cancellation)? {
        mark_cache_managed(configuration, cache)?;
        return Ok((commit.to_string(), cache.to_string()));
    }
    leases.release(cache);
    Err(NwpkgError::at(
        NwpkgErrorKind::NotFound,
        &root,
        format!(
            "dependency {route:?} is outside the selected operation and its locked cache entry is \
             unavailable; fetch the complete dependency graph first"
        ),
    ))
}

fn ensure_cached_checkout(
    configuration: &DependencyResolverConfiguration,
    dependency: &GitDependency,
    selector: &GitSelector,
    locked_commit: Option<&str>,
    locked_cache: Option<&str>,
    cancellation: &CancellationToken,
    progress: Option<&DependencyProgressReporter>,
    leases: &mut CacheLeases,
) -> Result<(String, String), NwpkgError> {
    check_cancelled(cancellation)?;
    fs::create_dir_all(&configuration.cache_root).map_err(|error| {
        NwpkgError::at(
            NwpkgErrorKind::Io,
            &configuration.cache_root,
            format!("failed to create dependency cache: {error}"),
        )
    })?;
    if let (Some(commit), Some(cache)) = (locked_commit, locked_cache) {
        leases.acquire(&configuration.cache_root, cache, cancellation)?;
        let root = configuration.cache_root.join(cache);
        if validate_cache_entry(&root, dependency, commit, None, cancellation)? {
            mark_cache_managed(configuration, cache)?;
            return Ok((commit.to_string(), cache.to_string()));
        }
        leases.release(cache);
    }

    let staging = Builder::new()
        .prefix(".stage-")
        .tempdir_in(&configuration.cache_root)
        .map_err(|error| {
            NwpkgError::at(
                NwpkgErrorKind::Io,
                &configuration.cache_root,
                format!("failed to create dependency staging directory: {error}"),
            )
        })?;
    let repository = staging.path().join("repository");
    report_progress(progress, "clone", "Cloning dependency repository");
    run_git(
        configuration,
        None,
        ["clone", "--no-checkout", "--", dependency.git.as_str()],
        Some(&repository),
        cancellation,
        &dependency.git,
        progress,
    )?;
    let commit = if let Some(commit) = locked_commit {
        if run_git(
            configuration,
            Some(&repository),
            ["rev-parse", "--verify", &format!("{commit}^{{commit}}")],
            None,
            cancellation,
            &dependency.git,
            progress,
        )
        .is_err()
        {
            run_git(
                configuration,
                Some(&repository),
                ["fetch", "--no-tags", "origin", commit],
                None,
                cancellation,
                &dependency.git,
                progress,
            )?;
        }
        resolve_commit(
            configuration,
            &repository,
            &GitSelector::Rev(commit.to_string()),
            cancellation,
            &dependency.git,
            progress,
        )?
    } else if let GitSelector::Rev(revision) = selector {
        match resolve_commit(
            configuration,
            &repository,
            selector,
            cancellation,
            &dependency.git,
            progress,
        ) {
            Ok(commit) => commit,
            Err(error) if error.kind() == NwpkgErrorKind::Cancelled => return Err(error),
            Err(_) => {
                run_git(
                    configuration,
                    Some(&repository),
                    ["fetch", "--no-tags", "origin", revision],
                    None,
                    cancellation,
                    &dependency.git,
                    progress,
                )?;
                resolve_commit(
                    configuration,
                    &repository,
                    selector,
                    cancellation,
                    &dependency.git,
                    progress,
                )?
            }
        }
    } else {
        resolve_commit(
            configuration,
            &repository,
            selector,
            cancellation,
            &dependency.git,
            progress,
        )?
    };
    report_progress(
        progress,
        "checkout",
        format!("Checking out commit {commit}"),
    );
    run_git(
        configuration,
        Some(&repository),
        ["checkout", "--detach", "--force", &commit],
        None,
        cancellation,
        &dependency.git,
        progress,
    )?;
    report_progress(progress, "submodules", "Updating pinned Git submodules");
    run_git(
        configuration,
        Some(&repository),
        ["submodule", "update", "--init", "--recursive"],
        None,
        cancellation,
        &dependency.git,
        progress,
    )?;
    let package_root = dependency
        .subdir
        .as_ref()
        .map_or_else(|| repository.clone(), |subdir| repository.join(subdir));
    if !package_root.join(PROJECT_MANIFEST_FILENAME).is_file() {
        return Err(NwpkgError::at(
            NwpkgErrorKind::NotFound,
            &package_root,
            format!(
                "Git dependency does not contain {PROJECT_MANIFEST_FILENAME} at the selected \
                 subdirectory"
            ),
        ));
    }
    let cache = cache_key(&dependency.git, &commit, dependency.subdir.as_deref())?;
    let target = configuration.cache_root.join(&cache);
    leases.acquire(&configuration.cache_root, &cache, cancellation)?;
    if target.is_dir() {
        if validate_cache_entry(&target, dependency, &commit, None, cancellation)? {
            mark_cache_managed(configuration, &cache)?;
            return Ok((commit, cache));
        }
        return Err(NwpkgError::at(
            NwpkgErrorKind::Validation,
            &target,
            "dependency cache entry is corrupt; clean the cache before retrying",
        ));
    }
    let subdir = dependency
        .subdir
        .as_deref()
        .map(portable_dependency_path)
        .transpose()?
        .unwrap_or_default();
    let metadata = CacheMetadata {
        format: CACHE_METADATA_FORMAT,
        git: dependency.git.clone(),
        commit: commit.clone(),
        subdir,
        manifest_sha256: hash_manifest(&package_root)?,
        content_sha256: hash_cache_repository(&repository, cancellation)?,
    };
    let encoded_metadata = toml::to_string(&metadata).map_err(|error| {
        NwpkgError::at(
            NwpkgErrorKind::Validation,
            staging.path(),
            format!("failed to encode dependency cache metadata: {error}"),
        )
    })?;
    fs::write(staging.path().join("cache.toml"), encoded_metadata).map_err(|error| {
        NwpkgError::at(
            NwpkgErrorKind::Io,
            staging.path(),
            format!("failed to write dependency cache metadata: {error}"),
        )
    })?;
    let staged_path = staging.keep();
    if let Err(error) = fs::rename(&staged_path, &target) {
        let _ = fs::remove_dir_all(&staged_path);
        return Err(NwpkgError::at(
            NwpkgErrorKind::Io,
            &target,
            format!("failed to publish dependency cache entry atomically: {error}"),
        ));
    }
    if let Err(error) = make_readonly_recursive(&target, cancellation) {
        let _ = make_writable_recursive(&target, cancellation);
        let _ = fs::remove_dir_all(&target);
        return Err(error);
    }
    mark_cache_managed(configuration, &cache)?;
    Ok((commit, cache))
}

fn resolve_commit(
    configuration: &DependencyResolverConfiguration,
    repository: &Path,
    selector: &GitSelector,
    cancellation: &CancellationToken,
    redacted_url: &str,
    progress: Option<&DependencyProgressReporter>,
) -> Result<String, NwpkgError> {
    let revision = match selector {
        GitSelector::Rev(value) => format!("{value}^{{commit}}"),
        GitSelector::Tag(value) => format!("refs/tags/{value}^{{commit}}"),
        GitSelector::Branch(value) => format!("refs/remotes/origin/{value}^{{commit}}"),
        GitSelector::Default => "HEAD^{commit}".to_string(),
    };
    let output = run_git(
        configuration,
        Some(repository),
        ["rev-parse", "--verify", revision.as_str()],
        None,
        cancellation,
        redacted_url,
        progress,
    )?;
    let commit = output.trim().to_ascii_lowercase();
    if !matches!(commit.len(), 40 | 64) || !commit.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(NwpkgError::at(
            NwpkgErrorKind::Validation,
            repository,
            format!("Git resolved an invalid commit object ID {commit:?}"),
        ));
    }
    Ok(commit)
}

fn run_git<const N: usize>(
    configuration: &DependencyResolverConfiguration,
    current_dir: Option<&Path>,
    arguments: [&str; N],
    clone_destination: Option<&Path>,
    cancellation: &CancellationToken,
    redacted_url: &str,
    progress: Option<&DependencyProgressReporter>,
) -> Result<String, NwpkgError> {
    check_cancelled(cancellation)?;
    let mut stdout = NamedTempFile::new().map_err(|error| {
        NwpkgError::new(
            NwpkgErrorKind::Io,
            format!("failed to create Git output buffer: {error}"),
        )
    })?;
    let mut command = Command::new(&configuration.git_executable);
    command
        .args(arguments)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "Never")
        .stdout(Stdio::from(stdout.reopen().map_err(|error| {
            NwpkgError::new(
                NwpkgErrorKind::Io,
                format!("failed to prepare Git output: {error}"),
            )
        })?))
        .stderr(Stdio::piped());
    if let Some(current_dir) = current_dir {
        command.current_dir(current_dir);
    }
    if let Some(destination) = clone_destination {
        command.arg(destination);
    }
    let mut child = command.spawn().map_err(|error| {
        NwpkgError::new(
            NwpkgErrorKind::Io,
            format!(
                "failed to start system Git executable {}: {error}",
                configuration.git_executable.display()
            ),
        )
    })?;
    let stderr = child.stderr.take().ok_or_else(|| {
        NwpkgError::new(
            NwpkgErrorKind::Io,
            "system Git did not expose its progress stream",
        )
    })?;
    let reporter = progress.cloned();
    let repository = redacted_url.to_string();
    let stderr_reader =
        thread::spawn(move || read_git_progress(stderr, reporter.as_ref(), &repository));
    let status = loop {
        if cancellation.is_cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(NwpkgError::new(
                NwpkgErrorKind::Cancelled,
                "Git dependency operation was cancelled",
            ));
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => thread::sleep(Duration::from_millis(25)),
            Err(error) => {
                let _ = child.kill();
                return Err(NwpkgError::new(
                    NwpkgErrorKind::Io,
                    format!("failed while waiting for system Git: {error}"),
                ));
            }
        }
    };
    let output = read_temp_output(&mut stdout)?;
    let error_bytes = stderr_reader.join().map_err(|_panic| {
        NwpkgError::new(
            NwpkgErrorKind::Io,
            "system Git progress reader terminated unexpectedly",
        )
    })??;
    let error_output = display_process_bytes(&error_bytes);
    if !status.success() {
        return Err(git_failure(status, &error_output, redacted_url));
    }
    Ok(output)
}

fn read_git_progress(
    mut input: impl Read,
    reporter: Option<&DependencyProgressReporter>,
    redacted_url: &str,
) -> Result<Vec<u8>, NwpkgError> {
    let mut output = Vec::new();
    let mut pending = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        let count = input.read(&mut chunk).map_err(|error| {
            NwpkgError::new(
                NwpkgErrorKind::Io,
                format!("failed to read system Git progress: {error}"),
            )
        })?;
        if count == 0 {
            emit_git_progress_line(reporter, &pending, redacted_url);
            return Ok(output);
        }
        let bytes = chunk.get(..count).ok_or_else(|| {
            NwpkgError::new(
                NwpkgErrorKind::Io,
                "system Git progress reader returned an invalid byte count",
            )
        })?;
        output.extend_from_slice(bytes);
        for byte in bytes {
            if matches!(byte, b'\r' | b'\n') {
                emit_git_progress_line(reporter, &pending, redacted_url);
                pending.clear();
            } else {
                pending.push(*byte);
            }
        }
    }
}

fn emit_git_progress_line(
    reporter: Option<&DependencyProgressReporter>,
    line: &[u8],
    redacted_url: &str,
) {
    let Some(reporter) = reporter else {
        return;
    };
    let line = display_process_bytes(line);
    let sanitized = sanitize_git_output(&line, redacted_url).trim().to_string();
    if !sanitized.is_empty() {
        reporter.report("git", sanitized);
    }
}

fn git_failure(status: ExitStatus, stderr: &str, redacted_url: &str) -> NwpkgError {
    let sanitized = sanitize_git_output(stderr, redacted_url);
    let detail = sanitized.trim();
    let message = if detail.is_empty() {
        format!("system Git failed with {status}")
    } else {
        format!("system Git failed with {status}: {detail}")
    };
    NwpkgError::new(NwpkgErrorKind::Io, message)
}

fn sanitize_git_output(input: &str, redacted_url: &str) -> String {
    redact_url_credentials(&input.replace(redacted_url, "<repository>"))
}

fn report_progress(
    progress: Option<&DependencyProgressReporter>,
    phase: &'static str,
    message: impl Into<String>,
) {
    if let Some(progress) = progress {
        progress.report(phase, message);
    }
}

fn redact_url_credentials(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut remaining = input;
    while let Some(scheme) = remaining.find("://") {
        let authority_start = scheme.saturating_add(3);
        output.push_str(remaining.get(..authority_start).unwrap_or(remaining));
        let Some(authority_and_rest) = remaining.get(authority_start..) else {
            break;
        };
        let authority_end = authority_and_rest
            .find(['/', ' ', '\t', '\r', '\n'])
            .unwrap_or(authority_and_rest.len());
        let authority = authority_and_rest
            .get(..authority_end)
            .unwrap_or(authority_and_rest);
        if let Some((_, host)) = authority.rsplit_once('@') {
            output.push_str("<credentials>@");
            output.push_str(host);
        } else {
            output.push_str(authority);
        }
        remaining = authority_and_rest.get(authority_end..).unwrap_or_default();
    }
    output.push_str(remaining);
    output
}

fn read_temp_output(file: &mut NamedTempFile) -> Result<String, NwpkgError> {
    file.seek(SeekFrom::Start(0)).map_err(|error| {
        NwpkgError::new(
            NwpkgErrorKind::Io,
            format!("failed to seek Git output: {error}"),
        )
    })?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|error| {
        NwpkgError::new(
            NwpkgErrorKind::Io,
            format!("failed to read Git output: {error}"),
        )
    })?;
    String::from_utf8(bytes).map_err(|error| {
        NwpkgError::new(
            NwpkgErrorKind::Validation,
            format!("system Git returned non-UTF-8 command output: {error}"),
        )
    })
}

fn display_process_bytes(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(value) => value.to_string(),
        Err(_) => bytes
            .iter()
            .flat_map(|byte| std::ascii::escape_default(*byte))
            .map(char::from)
            .collect(),
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CacheMetadata {
    format:          u32,
    git:             String,
    commit:          String,
    subdir:          String,
    manifest_sha256: String,
    content_sha256:  String,
}

fn validate_cache_entry(
    root: &Path,
    dependency: &GitDependency,
    commit: &str,
    expected_manifest_sha256: Option<&str>,
    cancellation: &CancellationToken,
) -> Result<bool, NwpkgError> {
    if !root.is_dir() {
        return Ok(false);
    }
    let metadata_path = root.join("cache.toml");
    let text = match fs::read_to_string(&metadata_path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(NwpkgError::at(
                NwpkgErrorKind::Io,
                &metadata_path,
                format!("failed to inspect dependency cache metadata: {error}"),
            ));
        }
    };
    let expected_subdir = dependency
        .subdir
        .as_deref()
        .map(portable_dependency_path)
        .transpose()?
        .unwrap_or_default();
    let metadata = toml::from_str::<CacheMetadata>(&text).map_err(|error| {
        NwpkgError::at(
            NwpkgErrorKind::Validation,
            &metadata_path,
            format!("dependency cache metadata is invalid: {error}"),
        )
    })?;
    let package_root = dependency.subdir.as_ref().map_or_else(
        || root.join("repository"),
        |subdir| root.join("repository").join(subdir),
    );
    if metadata.format != CACHE_METADATA_FORMAT
        || metadata.git != dependency.git
        || metadata.commit != commit
        || metadata.subdir != expected_subdir
        || expected_manifest_sha256.is_some_and(|expected| expected != metadata.manifest_sha256)
        || !package_root.join(PROJECT_MANIFEST_FILENAME).is_file()
    {
        return Ok(false);
    }
    let manifest_sha256 = hash_manifest(&package_root)?;
    if manifest_sha256 != metadata.manifest_sha256 {
        return Ok(false);
    }
    let content_sha256 = hash_cache_repository(&root.join("repository"), cancellation)?;
    Ok(content_sha256 == metadata.content_sha256)
}

pub(super) fn validate_locked_cache_entry(
    cache_root: &Path,
    git: &str,
    commit: &str,
    subdir: Option<&Path>,
    cache: &str,
    manifest_sha256: &str,
    cancellation: &CancellationToken,
) -> Result<bool, NwpkgError> {
    if cache_key(git, commit, subdir)? != cache {
        return Ok(false);
    }
    validate_cache_entry(
        &cache_root.join(cache),
        &GitDependency {
            git:    git.to_string(),
            rev:    None,
            tag:    None,
            branch: None,
            subdir: subdir.map(Path::to_path_buf),
        },
        commit,
        Some(manifest_sha256),
        cancellation,
    )
}

fn hash_cache_repository(
    repository: &Path,
    cancellation: &CancellationToken,
) -> Result<String, NwpkgError> {
    let mut paths = bounded_cache_walk(repository, cancellation)?;
    paths.sort_by(|left, right| {
        relative_path_bytes(repository, left).cmp(&relative_path_bytes(repository, right))
    });
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 128 * 1024];
    for path in paths {
        check_cancelled(cancellation)?;
        let relative = relative_path_bytes(repository, &path);
        let metadata = fs::symlink_metadata(&path).map_err(|error| {
            NwpkgError::at(
                NwpkgErrorKind::Io,
                &path,
                format!("failed to inspect cached dependency content: {error}"),
            )
        })?;
        hasher.update((relative.len() as u64).to_le_bytes());
        hasher.update(&relative);
        if metadata.file_type().is_symlink() {
            hasher.update(b"L");
            let target = fs::read_link(&path).map_err(|error| {
                NwpkgError::at(
                    NwpkgErrorKind::Io,
                    &path,
                    format!("failed to read cached dependency symlink: {error}"),
                )
            })?;
            let bytes = path_bytes(&target);
            hasher.update((bytes.len() as u64).to_le_bytes());
            hasher.update(bytes);
        } else if metadata.is_dir() {
            hasher.update(b"D");
        } else if metadata.is_file() {
            hasher.update(b"F");
            hasher.update(metadata.len().to_le_bytes());
            let mut file = File::open(&path).map_err(|error| {
                NwpkgError::at(
                    NwpkgErrorKind::Io,
                    &path,
                    format!("failed to read cached dependency content: {error}"),
                )
            })?;
            loop {
                check_cancelled(cancellation)?;
                let read = file.read(&mut buffer).map_err(|error| {
                    NwpkgError::at(
                        NwpkgErrorKind::Io,
                        &path,
                        format!("failed to hash cached dependency content: {error}"),
                    )
                })?;
                if read == 0 {
                    break;
                }
                let bytes = buffer.get(..read).ok_or_else(|| {
                    NwpkgError::at(
                        NwpkgErrorKind::Io,
                        &path,
                        "dependency cache read exceeded its fixed hash buffer",
                    )
                })?;
                hasher.update(bytes);
            }
        } else {
            return Err(NwpkgError::at(
                NwpkgErrorKind::Validation,
                &path,
                "dependency cache contains an unsupported filesystem entry",
            ));
        }
    }
    let digest = hasher.finalize();
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut encoded, "{byte:02x}").map_err(|error| {
            NwpkgError::new(
                NwpkgErrorKind::Validation,
                format!("failed to encode dependency cache digest: {error}"),
            )
        })?;
    }
    Ok(encoded)
}

fn bounded_cache_walk(
    root: &Path,
    cancellation: &CancellationToken,
) -> Result<Vec<PathBuf>, NwpkgError> {
    let mut pending = vec![(root.to_path_buf(), 0_usize)];
    let mut paths = Vec::new();
    while let Some((path, depth)) = pending.pop() {
        check_cancelled(cancellation)?;
        if paths.len() >= MAX_CACHE_TREE_ENTRIES {
            return Err(NwpkgError::at(
                NwpkgErrorKind::Validation,
                root,
                format!("dependency cache exceeded maximum of {MAX_CACHE_TREE_ENTRIES} entries"),
            ));
        }
        let metadata = fs::symlink_metadata(&path).map_err(|error| {
            NwpkgError::at(
                NwpkgErrorKind::Io,
                &path,
                format!("failed to traverse dependency cache: {error}"),
            )
        })?;
        if metadata.is_dir() {
            if depth >= MAX_CACHE_TREE_DEPTH {
                return Err(NwpkgError::at(
                    NwpkgErrorKind::Validation,
                    &path,
                    format!("dependency cache exceeded maximum depth {MAX_CACHE_TREE_DEPTH}"),
                ));
            }
            for entry in fs::read_dir(&path).map_err(|error| {
                NwpkgError::at(
                    NwpkgErrorKind::Io,
                    &path,
                    format!("failed to traverse dependency cache: {error}"),
                )
            })? {
                let entry = entry.map_err(|error| {
                    NwpkgError::at(
                        NwpkgErrorKind::Io,
                        &path,
                        format!("failed to traverse dependency cache: {error}"),
                    )
                })?;
                if entry.file_name() == ".git" {
                    continue;
                }
                pending.push((entry.path(), depth.saturating_add(1)));
            }
        }
        paths.push(path);
    }
    Ok(paths)
}

fn relative_path_bytes(root: &Path, path: &Path) -> Vec<u8> {
    path.strip_prefix(root)
        .map_or_else(|_| path_bytes(path), path_bytes)
}

#[cfg(unix)]
fn path_bytes(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt as _;
    path.as_os_str().as_bytes().to_vec()
}

#[cfg(windows)]
fn path_bytes(path: &Path) -> Vec<u8> {
    use std::os::windows::ffi::OsStrExt as _;
    path.as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect()
}

fn cache_key(git: &str, commit: &str, subdir: Option<&Path>) -> Result<String, NwpkgError> {
    let subdir = subdir
        .map(portable_dependency_path)
        .transpose()?
        .unwrap_or_default();
    Ok(checksums::sha256_digest(format!("{git}\0{commit}\0{subdir}").as_bytes()).to_string())
}

fn portable_dependency_path(path: &Path) -> Result<String, NwpkgError> {
    path.to_str()
        .map(|path| path.replace('\\', "/"))
        .ok_or_else(|| {
            NwpkgError::at(
                NwpkgErrorKind::Validation,
                path,
                "dependency subdirectories must be valid UTF-8",
            )
        })
}

fn hash_manifest(root: &Path) -> Result<String, NwpkgError> {
    let path = root.join(PROJECT_MANIFEST_FILENAME);
    fs::read(&path)
        .map(|bytes| checksums::sha256_digest(bytes).to_string())
        .map_err(|error| {
            NwpkgError::at(
                NwpkgErrorKind::Io,
                path,
                format!("failed to hash package manifest: {error}"),
            )
        })
}

fn make_readonly_recursive(
    root: &Path,
    cancellation: &CancellationToken,
) -> Result<(), NwpkgError> {
    for entry in permission_walk(root, cancellation)? {
        let metadata = fs::metadata(&entry).map_err(|error| {
            NwpkgError::at(
                NwpkgErrorKind::Io,
                &entry,
                format!("failed to inspect dependency cache entry: {error}"),
            )
        })?;
        let mut permissions = metadata.permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = if metadata.is_dir() { 0o555 } else { 0o444 };
            permissions.set_mode(mode);
        }
        #[cfg(not(unix))]
        permissions.set_readonly(true);
        fs::set_permissions(&entry, permissions).map_err(|error| {
            NwpkgError::at(
                NwpkgErrorKind::Io,
                &entry,
                format!("failed to make dependency cache immutable: {error}"),
            )
        })?;
    }
    Ok(())
}

fn make_writable_recursive(
    root: &Path,
    cancellation: &CancellationToken,
) -> Result<(), NwpkgError> {
    for entry in permission_walk(root, cancellation)? {
        let metadata = fs::metadata(&entry).map_err(|error| {
            NwpkgError::at(
                NwpkgErrorKind::Io,
                &entry,
                format!("failed to inspect dependency cache entry: {error}"),
            )
        })?;
        let mut permissions = metadata.permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = if metadata.is_dir() { 0o700 } else { 0o600 };
            permissions.set_mode(mode);
        }
        #[cfg(not(unix))]
        permissions.set_readonly(false);
        fs::set_permissions(&entry, permissions).map_err(|error| {
            NwpkgError::at(
                NwpkgErrorKind::Io,
                &entry,
                format!("failed to unlock dependency cache entry: {error}"),
            )
        })?;
    }
    Ok(())
}

fn permission_walk(
    root: &Path,
    cancellation: &CancellationToken,
) -> Result<Vec<PathBuf>, NwpkgError> {
    let mut paths = bounded_cache_walk(root, cancellation)?;
    paths.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
    Ok(paths)
}

fn check_cancelled(cancellation: &CancellationToken) -> Result<(), NwpkgError> {
    cancellation
        .check()
        .map_err(|error| NwpkgError::new(NwpkgErrorKind::Cancelled, error.to_string()))
}

fn default_cache_root() -> PathBuf {
    #[cfg(target_os = "windows")]
    if let Some(root) = env::var_os("LOCALAPPDATA") {
        return PathBuf::from(root).join("nwnrs").join("packages");
    }
    #[cfg(target_os = "macos")]
    if let Some(root) = env::var_os("HOME") {
        return PathBuf::from(root)
            .join("Library")
            .join("Caches")
            .join("nwnrs")
            .join("packages");
    }
    if let Some(root) = env::var_os("XDG_CACHE_HOME") {
        return PathBuf::from(root).join("nwnrs").join("packages");
    }
    if let Some(root) = env::var_os("HOME") {
        return PathBuf::from(root)
            .join(".cache")
            .join("nwnrs")
            .join("packages");
    }
    env::temp_dir().join("nwnrs").join("packages")
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        error::Error,
        process::Command,
        sync::{Arc, Mutex},
    };

    use tempfile::TempDir;

    use super::*;
    use crate::resolve_include_dependencies_with_configuration;

    type TestResult = Result<(), Box<dyn Error>>;

    #[test]
    fn remote_git_sources_have_one_canonical_identity() -> TestResult {
        let root = TempDir::new()?;
        assert_eq!(
            normalize_git_source("HTTPS://Example.COM:443/org/./repo/", root.path())?,
            "https://example.com/org/repo"
        );
        assert_eq!(
            normalize_git_source("git@Example.COM:org/repo/", root.path())?,
            "ssh://git@example.com/org/repo"
        );
        for source in [
            "https://token@example.com/org/repo",
            "https://example.com/org/repo?token=secret",
            "https://example.com/org/repo#branch",
            "ftp://example.com/org/repo",
        ] {
            assert!(
                normalize_git_source(source, root.path()).is_err(),
                "{source} must not produce a persistent Git identity"
            );
        }
        Ok(())
    }

    #[test]
    fn cache_cleanup_respects_every_persistent_lock_owner() -> TestResult {
        let fixture = TempDir::new()?;
        let cache_root = fixture.path().join("cache");
        let first = fixture.path().join("first");
        let second = fixture.path().join("second");
        fs::create_dir_all(&first)?;
        fs::create_dir_all(&second)?;
        let configuration = DependencyResolverConfiguration {
            git_executable: PathBuf::from("git"),
            cache_root:     cache_root.clone(),
        };
        let first_cache = "1".repeat(64);
        let second_cache = "2".repeat(64);
        let first_lock = test_dependency_lock("first", &first_cache);
        let second_lock = test_dependency_lock("second", &second_cache);
        for cache in [&first_cache, &second_cache] {
            fs::create_dir_all(cache_root.join(cache))?;
            mark_cache_managed(&configuration, cache)?;
        }
        write_registered_dependency_lock(&configuration, &first, &first_lock)?;
        write_registered_dependency_lock(&configuration, &second, &second_lock)?;

        assert!(clean_dependency_cache(&configuration)?.is_empty());

        let empty = DependencyLock {
            format:          DEPENDENCY_LOCK_FORMAT,
            manifest_sha256: "0".repeat(64),
            root:            BTreeMap::new(),
            packages:        BTreeMap::new(),
        };
        write_registered_dependency_lock(&configuration, &first, &empty)?;
        let removed = clean_dependency_cache(&configuration)?;
        assert_eq!(removed, vec![cache_root.join(&first_cache)]);
        assert!(cache_root.join(&second_cache).is_dir());

        fs::remove_file(second.join(crate::PACKAGE_LOCK_FILENAME))?;
        assert!(
            clean_dependency_cache(&configuration)?.is_empty(),
            "a temporarily missing owner is not proof that its cache is unused"
        );
        assert!(cache_root.join(second_cache).is_dir());
        Ok(())
    }

    #[test]
    fn cache_cleanup_skips_an_active_cross_process_lease() -> TestResult {
        let fixture = TempDir::new()?;
        let cache_root = fixture.path().join("cache");
        let cache = "3".repeat(64);
        let configuration = DependencyResolverConfiguration {
            git_executable: PathBuf::from("git"),
            cache_root:     cache_root.clone(),
        };
        fs::create_dir_all(cache_root.join(&cache))?;
        mark_cache_managed(&configuration, &cache)?;
        let lease = CacheLock::acquire(&cache_root, &cache, &CancellationToken::new())?;
        assert!(clean_dependency_cache(&configuration)?.is_empty());
        assert!(cache_root.join(&cache).is_dir());
        drop(lease);
        assert_eq!(
            clean_dependency_cache(&configuration)?,
            vec![cache_root.join(cache)]
        );
        Ok(())
    }

    #[test]
    fn abandoned_lock_files_do_not_block_cache_operations() -> TestResult {
        let fixture = TempDir::new()?;
        let cache_root = fixture.path().join("cache");
        let lock_root = cache_root.join(".locks");
        fs::create_dir_all(&lock_root)?;
        fs::write(lock_root.join("abandoned.lock"), b"stale sentinel")?;
        let lease = CacheLock::acquire(&cache_root, "abandoned", &CancellationToken::new())?;
        drop(lease);
        Ok(())
    }

    #[test]
    fn git_dependencies_are_pinned_cached_and_consumed_offline() -> TestResult {
        let fixture = TempDir::new()?;
        let repository = fixture.path().join("repository");
        let project = fixture.path().join("project");
        let cache = fixture.path().join("cache");
        fs::create_dir_all(repository.join("src"))?;
        fs::create_dir_all(repository.join("other/src"))?;
        fs::create_dir_all(project.join("src"))?;
        fs::write(
            repository.join(PROJECT_MANIFEST_FILENAME),
            "[project]\nname = \"shared\"\nkind = \"include\"\n\n[source]\npath = \"src\"\n",
        )?;
        fs::write(repository.join("src/shared.nss"), "int Shared = 1;\n")?;
        fs::write(
            repository.join("other").join(PROJECT_MANIFEST_FILENAME),
            "[project]\nname = \"other\"\nkind = \"include\"\n\n[source]\npath = \"src\"\n",
        )?;
        fs::write(repository.join("other/src/other.nss"), "int Other = 2;\n")?;
        git(&repository, ["init", "--initial-branch=main"])?;
        git(&repository, ["config", "user.email", "tests@nwnrs.invalid"])?;
        git(&repository, ["config", "user.name", "nwnrs tests"])?;
        git(&repository, ["add", "."])?;
        git(&repository, ["commit", "-m", "initial"])?;
        fs::write(
            project.join(PROJECT_MANIFEST_FILENAME),
            "[project]\nname = \"module\"\nkind = \"include\"\n\n[source]\npath = \
             \"src\"\n\n[dependencies]\nshared = { git = \"../repository\", branch = \"main\" \
             }\nother = { git = \"../repository\", branch = \"main\", subdir = \"other\" }\n",
        )?;
        fs::write(project.join("src/main.nss"), "#include \"shared\"\n")?;
        let configuration = DependencyResolverConfiguration {
            git_executable: PathBuf::from("git"),
            cache_root:     cache,
        };
        let progress = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&progress);
        let reporter = DependencyProgressReporter::new(move |event| {
            if let Ok(mut events) = captured.lock() {
                events.push(event);
            }
        });
        let operation = resolve_dependency_operation_with_progress(
            &project,
            &configuration,
            DependencyOperationKind::Fetch,
            None,
            &CancellationToken::new(),
            Some(&reporter),
        )?;
        assert_eq!(operation.lock.packages.len(), 2);
        let locked = operation
            .lock
            .packages
            .get("shared")
            .ok_or("missing locked dependency")?;
        let LockedDependencySource::Git {
            cache,
            commit,
            git,
            ..
        } = &locked.source
        else {
            return Err("dependency was not locked as Git".into());
        };
        assert_eq!(
            git,
            fs::canonicalize(&repository)?
                .to_str()
                .ok_or("fixture repository path is not valid UTF-8")?
        );
        assert!(configuration.cache_root.join(cache).is_dir());
        let shared_cache = cache.clone();
        assert!(matches!(commit.len(), 40 | 64));
        let progress = progress
            .lock()
            .map_err(|_poison| "progress lock poisoned")?;
        assert!(progress.iter().any(|event| event.phase == "clone"));
        assert!(progress.iter().any(|event| event.phase == "submodules"));
        drop(progress);

        let resolved = resolve_include_dependencies_with_configuration(
            &project,
            &configuration,
            &CancellationToken::new(),
        )?;
        assert_eq!(resolved.len(), 2);
        assert!(
            resolved
                .iter()
                .any(|dependency| dependency.source_root.join("shared.nss").is_file())
        );

        let other = operation
            .lock
            .packages
            .get("other")
            .ok_or("missing other locked dependency")?;
        let LockedDependencySource::Git {
            cache: other_cache,
            ..
        } = &other.source
        else {
            return Err("other dependency was not locked as Git".into());
        };
        let unavailable = configuration.cache_root.join(other_cache);
        make_writable_recursive(&unavailable, &CancellationToken::new())?;
        fs::remove_dir_all(&unavailable)?;
        let lock_before = fs::read(project.join(crate::PACKAGE_LOCK_FILENAME))?;
        let targeted = resolve_dependency_operation(
            &project,
            &configuration,
            DependencyOperationKind::Fetch,
            Some("shared"),
            &CancellationToken::new(),
        )
        .expect_err("targeted fetch must not fetch an unrelated missing cache entry");
        assert!(
            targeted
                .to_string()
                .contains("outside the selected operation")
        );
        assert_eq!(
            fs::read(project.join(crate::PACKAGE_LOCK_FILENAME))?,
            lock_before,
            "a failed targeted fetch must not replace the lock"
        );
        drop(operation);
        let shared_root = configuration.cache_root.join(shared_cache);
        make_writable_recursive(&shared_root, &CancellationToken::new())?;
        fs::write(
            shared_root.join("repository/src/shared.nss"),
            "int Shared = 999;\n",
        )?;
        let integrity = resolve_include_dependencies_with_configuration(
            &project,
            &configuration,
            &CancellationToken::new(),
        )
        .expect_err("tampered immutable cache content must be rejected");
        assert!(integrity.message().contains("integrity validation"));
        Ok(())
    }

    fn git<const N: usize>(root: &Path, arguments: [&str; N]) -> TestResult {
        let output = Command::new("git")
            .current_dir(root)
            .args(arguments)
            .output()?;
        if !output.status.success() {
            return Err(format!(
                "Git fixture command failed: {}",
                display_process_bytes(&output.stderr)
            )
            .into());
        }
        Ok(())
    }

    fn test_dependency_lock(name: &str, cache: &str) -> DependencyLock {
        let mut root = BTreeMap::new();
        root.insert(name.to_string(), name.to_string());
        let mut packages = BTreeMap::new();
        packages.insert(
            name.to_string(),
            LockedDependency {
                name:            name.to_string(),
                source:          LockedDependencySource::Git {
                    git:      "https://example.invalid/repository".to_string(),
                    selector: GitSelector::Rev("a".repeat(40)),
                    commit:   "a".repeat(40),
                    subdir:   None,
                    cache:    cache.to_string(),
                },
                manifest_sha256: "0".repeat(64),
                dependencies:    BTreeMap::new(),
            },
        );
        DependencyLock {
            format: DEPENDENCY_LOCK_FORMAT,
            manifest_sha256: "0".repeat(64),
            root,
            packages,
        }
    }
}
