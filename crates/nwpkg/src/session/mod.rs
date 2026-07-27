use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
};

use nwnrs_nwscript::{
    CancellationToken, CompilerResourceLimits, FileSystemScriptResolver, LogicalScriptName,
    MacroRegistry, canonicalize_with_missing_suffix, physical_sources_equal,
};
use nwnrs_types::{checksums, encoding::NwnTextContext};

use crate::{
    DependencyResolverConfiguration, NwpkgError, NwpkgErrorKind, PROJECT_MANIFEST_FILENAME,
    ProjectLayout, ProjectManifest, ResolvedIncludeDependency,
    compiler_macros::load_dependency_macro_registry_resolved,
    fs::collect_nss_paths_with_cancellation, read_project_manifest_with_cancellation,
    resolve_include_dependencies_from_manifest_with_configuration,
};

mod overlays;

use overlays::{normalize_overlays, source_belongs_to_root};

type SourceSnapshot = BTreeMap<PathBuf, Arc<[u8]>>;
type SourceFingerprint = BTreeMap<PathBuf, String>;

/// Immutable, reusable package-resolution snapshot for compiler and editor
/// work.
///
/// A session resolves the owning manifest, source root, complete include graph,
/// source overlays, include resolver, and compiler macro registry once. Every
/// consumer of the snapshot therefore observes the same package state.
pub struct ResolvedPackageSession {
    project_root:   PathBuf,
    manifest:       ProjectManifest,
    text_context:   NwnTextContext,
    source_root:    PathBuf,
    dependencies:   Vec<ResolvedIncludeDependency>,
    overlays:       SourceSnapshot,
    source_files:   SourceSnapshot,
    fingerprint:    SourceFingerprint,
    resolver:       FileSystemScriptResolver,
    macro_registry: OnceLock<Arc<MacroRegistry>>,
}

impl ResolvedPackageSession {
    /// Resolves the package owning `input`.
    ///
    /// Overlay paths may be absolute or package-relative, but must remain
    /// inside the project or one of its resolved dependency packages.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid manifests, dependency graphs, overlays,
    /// source traversal, macro definitions, or cancellation.
    pub fn open(
        input: &Path,
        overlays: &BTreeMap<PathBuf, Vec<u8>>,
        cancellation: &CancellationToken,
    ) -> Result<Option<Self>, NwpkgError> {
        Self::open_impl(
            input,
            overlays,
            cancellation,
            false,
            &DependencyResolverConfiguration::from_environment(),
            None,
        )
    }

    /// Resolves the package owning `input` from a workspace-wide overlay map.
    ///
    /// Absolute overlays belonging to other package graphs are ignored. All
    /// relative overlays and every overlay selected for this package retain
    /// the same escape and collision validation as [`Self::open`].
    ///
    /// # Errors
    ///
    /// Returns an error for invalid manifests, dependency graphs, relevant
    /// overlays, source traversal, or cancellation.
    pub fn open_with_workspace_overlays(
        input: &Path,
        overlays: &BTreeMap<PathBuf, Vec<u8>>,
        cancellation: &CancellationToken,
    ) -> Result<Option<Self>, NwpkgError> {
        Self::open_impl(
            input,
            overlays,
            cancellation,
            true,
            &DependencyResolverConfiguration::from_environment(),
            None,
        )
    }

    /// Resolves a package with workspace overlays and explicit offline Git
    /// cache configuration.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`Self::open_with_workspace_overlays`].
    pub fn open_with_workspace_overlays_and_dependency_configuration(
        input: &Path,
        overlays: &BTreeMap<PathBuf, Vec<u8>>,
        cancellation: &CancellationToken,
        dependency_configuration: &DependencyResolverConfiguration,
    ) -> Result<Option<Self>, NwpkgError> {
        Self::open_impl(
            input,
            overlays,
            cancellation,
            true,
            dependency_configuration,
            None,
        )
    }

    /// Resolves a package from an authoritative parsed root-manifest overlay,
    /// workspace source overlays, and explicit dependency configuration.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`Self::open_with_workspace_overlays`].
    pub fn open_with_manifest_and_workspace_overlays_and_dependency_configuration(
        input: &Path,
        manifest: &ProjectManifest,
        overlays: &BTreeMap<PathBuf, Vec<u8>>,
        cancellation: &CancellationToken,
        dependency_configuration: &DependencyResolverConfiguration,
    ) -> Result<Option<Self>, NwpkgError> {
        Self::open_impl(
            input,
            overlays,
            cancellation,
            true,
            dependency_configuration,
            Some(manifest.clone()),
        )
    }

    fn open_impl(
        input: &Path,
        overlays: &BTreeMap<PathBuf, Vec<u8>>,
        cancellation: &CancellationToken,
        ignore_unrelated_absolute_overlays: bool,
        dependency_configuration: &DependencyResolverConfiguration,
        manifest_override: Option<ProjectManifest>,
    ) -> Result<Option<Self>, NwpkgError> {
        cancellation
            .check()
            .map_err(|error| NwpkgError::new(NwpkgErrorKind::Cancelled, error.to_string()))?;
        let Some(project_root) = find_project_root_with_cancellation(input, cancellation)? else {
            return Ok(None);
        };
        let manifest = match manifest_override {
            Some(manifest) => manifest,
            None => read_project_manifest_with_cancellation(&project_root, cancellation)?
                .ok_or_else(|| {
                    NwpkgError::at(
                        NwpkgErrorKind::NotFound,
                        project_root.join(PROJECT_MANIFEST_FILENAME),
                        "project manifest disappeared during resolution",
                    )
                })?,
        };
        let source = project_root.join(&manifest.source.path);
        let source_root = fs::canonicalize(&source).map_err(|error| {
            NwpkgError::at(
                NwpkgErrorKind::Io,
                &source,
                format!("failed to resolve package source: {error}"),
            )
        })?;
        if !source_root.starts_with(&project_root) {
            return Err(NwpkgError::at(
                NwpkgErrorKind::Validation,
                source_root,
                "package source escapes the project root",
            ));
        }

        let dependencies = resolve_include_dependencies_from_manifest_with_configuration(
            &project_root,
            &manifest,
            dependency_configuration,
            cancellation,
        )?;
        let normalized_overlays = normalize_overlays(
            &project_root,
            &source_root,
            &dependencies,
            overlays,
            ignore_unrelated_absolute_overlays,
            cancellation,
        )?;
        let (source_files, fingerprint) = build_source_snapshot(
            &project_root,
            &manifest,
            &source_root,
            &dependencies,
            &normalized_overlays,
            cancellation,
        )?;

        let mut resolver = FileSystemScriptResolver::new();
        match manifest.project.kind.layout() {
            ProjectLayout::Resource => {
                if let Some(parent) = source_root.parent() {
                    resolver.add_root(parent).map_err(|error| {
                        NwpkgError::at(NwpkgErrorKind::Validation, parent, error.to_string())
                    })?;
                }
            }
            ProjectLayout::Include | ProjectLayout::Erf | ProjectLayout::Key => {
                resolver.add_root(&source_root).map_err(|error| {
                    NwpkgError::at(NwpkgErrorKind::Validation, &source_root, error.to_string())
                })?;
            }
        }
        for dependency in &dependencies {
            resolver
                .add_root(&dependency.source_root)
                .map_err(|error| {
                    NwpkgError::at(
                        NwpkgErrorKind::Validation,
                        &dependency.source_root,
                        error.to_string(),
                    )
                })?;
        }
        for (path, contents) in &source_files {
            resolver
                .add_shared_overlay(path, Arc::clone(contents))
                .map_err(|error| {
                    NwpkgError::at(NwpkgErrorKind::Validation, path, error.to_string())
                })?;
        }

        Ok(Some(Self {
            project_root,
            text_context: NwnTextContext::for_language_id(manifest.project.language.id()),
            manifest,
            source_root,
            dependencies,
            overlays: normalized_overlays,
            source_files,
            fingerprint,
            resolver,
            macro_registry: OnceLock::new(),
        }))
    }

    /// Canonical directory containing `nwpkg.toml`.
    #[must_use]
    pub fn project_root(&self) -> &Path {
        &self.project_root
    }

    /// Validated project manifest.
    #[must_use]
    pub const fn manifest(&self) -> &ProjectManifest {
        &self.manifest
    }

    /// Immutable native-resource text policy selected by `project.language`.
    #[must_use]
    pub const fn text_context(&self) -> NwnTextContext {
        self.text_context
    }

    /// Canonical package source path.
    #[must_use]
    pub fn source_root(&self) -> &Path {
        &self.source_root
    }

    /// Complete transitive include dependency graph in deterministic order.
    #[must_use]
    pub fn dependencies(&self) -> &[ResolvedIncludeDependency] {
        &self.dependencies
    }

    /// Normalized immutable source overlay snapshot.
    #[must_use]
    pub const fn overlays(&self) -> &BTreeMap<PathBuf, Arc<[u8]>> {
        &self.overlays
    }

    /// Shared scoped resolver containing project, dependency, and overlay data.
    #[must_use]
    pub const fn resolver(&self) -> &FileSystemScriptResolver {
        &self.resolver
    }

    /// Shared compiler macro registry built lazily from the resolved dependency
    /// graph.
    ///
    /// # Errors
    ///
    /// Returns an error when a dependency macro cannot be loaded, compiled, or
    /// registered, or when cancellation is requested.
    pub fn macro_registry(&self) -> Result<Arc<MacroRegistry>, NwpkgError> {
        self.macro_registry_with_cancellation(&CancellationToken::new())
    }

    /// Returns the macro registry with caller-owned cancellation.
    ///
    /// # Errors
    ///
    /// Returns an error when macro discovery fails or the operation is
    /// cancelled. Failed or cancelled builds are never cached.
    pub fn macro_registry_with_cancellation(
        &self,
        cancellation: &CancellationToken,
    ) -> Result<Arc<MacroRegistry>, NwpkgError> {
        check_cancelled(cancellation)?;
        if let Some(registry) = self.macro_registry.get() {
            return Ok(Arc::clone(registry));
        }
        let registry = Arc::new(load_dependency_macro_registry_resolved(
            &self.dependencies,
            &self.source_files,
            cancellation,
        )?);
        match self.macro_registry.set(Arc::clone(&registry)) {
            Ok(()) => Ok(registry),
            Err(_raced_registry) => self.macro_registry.get().map(Arc::clone).ok_or_else(|| {
                NwpkgError::new(
                    NwpkgErrorKind::Compiler,
                    "macro registry initialization raced but no completed registry is available",
                )
            }),
        }
    }

    /// Returns all `.nss` sources, including unsaved newly-created overlays.
    ///
    /// # Errors
    ///
    /// Returns an error for unsafe traversal or cancellation.
    pub fn nss_source_paths(&self) -> Result<Vec<PathBuf>, NwpkgError> {
        self.nss_source_paths_with_cancellation(&CancellationToken::new())
    }

    /// Returns project NSS sources with caller-owned cancellation.
    pub fn nss_source_paths_with_cancellation(
        &self,
        cancellation: &CancellationToken,
    ) -> Result<Vec<PathBuf>, NwpkgError> {
        check_cancelled(cancellation)?;
        let mut paths = self
            .source_files
            .keys()
            .filter(|path| {
                source_belongs_to_root(
                    path,
                    &self.source_root,
                    matches!(self.manifest.project.kind.layout(), ProjectLayout::Resource),
                )
            })
            .cloned()
            .collect::<Vec<_>>();
        crate::fs::sort_paths_case_insensitive(&mut paths, "NWScript source")?;
        paths.dedup();
        Ok(paths)
    }

    /// Returns every `.nss` source visible through the project and its complete
    /// include dependency graph.
    ///
    /// Unsaved overlays are included for both the project and dependencies.
    /// Traversal is deterministic, rejects symbolic links, and remains
    /// cancellable between directory entries.
    ///
    /// # Errors
    ///
    /// Returns an error for unsafe traversal or cancellation.
    pub fn all_nss_source_paths(&self) -> Result<Vec<PathBuf>, NwpkgError> {
        self.all_nss_source_paths_with_cancellation(&CancellationToken::new())
    }

    /// Returns all project and dependency NSS sources with caller cancellation.
    pub fn all_nss_source_paths_with_cancellation(
        &self,
        cancellation: &CancellationToken,
    ) -> Result<Vec<PathBuf>, NwpkgError> {
        check_cancelled(cancellation)?;
        let mut paths = self.source_files.keys().cloned().collect::<Vec<_>>();
        crate::fs::sort_paths_case_insensitive(&mut paths, "NWScript source")?;
        paths.dedup();
        Ok(paths)
    }

    /// Reads one source through the overlay snapshot before consulting disk.
    ///
    /// # Errors
    ///
    /// Returns an error when the source is outside the session, unavailable, or
    /// the request has been cancelled.
    pub fn read_source(&self, path: &Path) -> Result<Vec<u8>, NwpkgError> {
        self.read_source_with_cancellation(path, &CancellationToken::new())
    }

    /// Reads one snapshotted source with caller-owned cancellation.
    pub fn read_source_with_cancellation(
        &self,
        path: &Path,
        cancellation: &CancellationToken,
    ) -> Result<Vec<u8>, NwpkgError> {
        check_cancelled(cancellation)?;
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.project_root.join(path)
        };
        let path = canonicalize_with_missing_suffix(&path).map_err(|error| {
            NwpkgError::at(
                NwpkgErrorKind::Io,
                &path,
                format!("failed to resolve source path: {error}"),
            )
        })?;
        if let Some(contents) = self.source_files.iter().find_map(|(candidate, contents)| {
            physical_sources_equal(candidate, &path).then_some(contents)
        }) {
            return copy_snapshot_bytes(
                &path,
                contents,
                CompilerResourceLimits::default().max_source_file_bytes,
                cancellation,
            );
        }
        Err(NwpkgError::at(
            NwpkgErrorKind::NotFound,
            path,
            "source is not part of the resolved package snapshot",
        ))
    }

    /// Detects filesystem or overlay changes and replaces this session only
    /// after a complete new snapshot has been resolved successfully.
    ///
    /// # Errors
    ///
    /// Returns an error without modifying the current session when rebuilding
    /// fails or is cancelled.
    pub fn refresh_if_changed(
        &mut self,
        overlays: &BTreeMap<PathBuf, Vec<u8>>,
        cancellation: &CancellationToken,
    ) -> Result<bool, NwpkgError> {
        let Some(rebuilt) =
            Self::open_with_workspace_overlays(&self.project_root, overlays, cancellation)?
        else {
            return Err(NwpkgError::at(
                NwpkgErrorKind::NotFound,
                &self.project_root,
                "package disappeared while rebuilding resolved session",
            ));
        };
        if rebuilt.fingerprint == self.fingerprint {
            return Ok(false);
        }
        *self = rebuilt;
        Ok(true)
    }
}

/// Finds the nearest canonical project root owning `input`.
///
/// # Errors
///
/// Returns an error when the current directory or discovered root cannot be
/// resolved.
pub fn find_project_root(input: &Path) -> Result<Option<PathBuf>, NwpkgError> {
    find_project_root_with_cancellation(input, &CancellationToken::new())
}

/// Finds the nearest project root with cooperative cancellation.
///
/// # Errors
///
/// Returns an I/O or cancellation error while walking ancestors.
pub fn find_project_root_with_cancellation(
    input: &Path,
    cancellation: &CancellationToken,
) -> Result<Option<PathBuf>, NwpkgError> {
    check_cancelled(cancellation)?;
    let absolute = if input.is_absolute() {
        input.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| {
                NwpkgError::new(
                    NwpkgErrorKind::Io,
                    format!("failed to read current directory: {error}"),
                )
            })?
            .join(input)
    };
    let mut current = match fs::metadata(&absolute) {
        Ok(metadata) if metadata.is_dir() => absolute,
        Ok(_) => absolute
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => absolute
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf),
        Err(error) => {
            return Err(NwpkgError::at(
                NwpkgErrorKind::Io,
                &absolute,
                format!("failed to inspect project input: {error}"),
            ));
        }
    };
    loop {
        check_cancelled(cancellation)?;
        let manifest_path = current.join(PROJECT_MANIFEST_FILENAME);
        match fs::metadata(&manifest_path) {
            Ok(metadata) if metadata.is_file() => {
                return fs::canonicalize(&current).map(Some).map_err(|error| {
                    NwpkgError::at(
                        NwpkgErrorKind::Io,
                        &current,
                        format!("failed to resolve project root: {error}"),
                    )
                });
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(NwpkgError::at(
                    NwpkgErrorKind::Io,
                    &manifest_path,
                    format!("failed to inspect project manifest: {error}"),
                ));
            }
        }
        if !current.pop() {
            return Ok(None);
        }
    }
}

fn build_source_snapshot(
    project_root: &Path,
    manifest: &ProjectManifest,
    source_root: &Path,
    dependencies: &[ResolvedIncludeDependency],
    overlays: &BTreeMap<PathBuf, Arc<[u8]>>,
    cancellation: &CancellationToken,
) -> Result<(SourceSnapshot, SourceFingerprint), NwpkgError> {
    let mut sources = BTreeMap::new();
    let mut logical_names = BTreeMap::<LogicalScriptName, PathBuf>::new();
    let mut roots = vec![source_root.to_path_buf()];
    let limits = CompilerResourceLimits::default();
    let mut total_source_bytes = 0_usize;
    roots.extend(
        dependencies
            .iter()
            .map(|dependency| dependency.source_root.clone()),
    );
    for root in &roots {
        check_cancelled(cancellation)?;
        let root_metadata = fs::metadata(root).map_err(|error| {
            NwpkgError::at(
                NwpkgErrorKind::Io,
                root,
                format!("failed to inspect package source root: {error}"),
            )
        })?;
        let root_is_file = root_metadata.is_file();
        let paths = if root_is_file {
            if is_nss_path(root) {
                vec![root.clone()]
            } else {
                Vec::new()
            }
        } else {
            collect_nss_paths_with_cancellation(root, cancellation)?
        };
        for path in paths {
            check_cancelled(cancellation)?;
            let logical = logical_source_name(root, &path, root_is_file)?;
            if let Some(existing) = logical_names.insert(logical.clone(), path.clone())
                && existing != path
            {
                return Err(NwpkgError::at(
                    NwpkgErrorKind::Validation,
                    &path,
                    format!(
                        "package sources are ambiguous on case-insensitive filesystems for \
                         {logical:?}: {} and {}",
                        existing.display(),
                        path.display()
                    ),
                ));
            }
            let bytes = match overlay_for_path(overlays, &path) {
                Some(contents) => Arc::clone(contents),
                None => Arc::<[u8]>::from(read_file_with_cancellation(
                    &path,
                    limits.max_source_file_bytes,
                    cancellation,
                )?),
            };
            if bytes.len() > limits.max_source_file_bytes {
                return Err(NwpkgError::at(
                    NwpkgErrorKind::Validation,
                    &path,
                    format!(
                        "package source contains {} bytes, exceeding configured maximum {}",
                        bytes.len(),
                        limits.max_source_file_bytes
                    ),
                ));
            }
            total_source_bytes = total_source_bytes.checked_add(bytes.len()).ok_or_else(|| {
                NwpkgError::at(
                    NwpkgErrorKind::Validation,
                    &path,
                    "package source snapshot byte count overflowed",
                )
            })?;
            if total_source_bytes > limits.max_total_source_bytes {
                return Err(NwpkgError::at(
                    NwpkgErrorKind::Validation,
                    &path,
                    format!(
                        "package source snapshot exceeds configured maximum of {} bytes",
                        limits.max_total_source_bytes
                    ),
                ));
            }
            sources.insert(path, bytes);
        }
    }
    for (path, contents) in overlays {
        check_cancelled(cancellation)?;
        if !is_nss_path(path)
            || sources
                .keys()
                .any(|existing| physical_sources_equal(existing, path))
        {
            continue;
        }
        let root = roots
            .iter()
            .find(|root| {
                source_belongs_to_root(
                    path,
                    root,
                    root.as_path() == source_root
                        && matches!(manifest.project.kind.layout(), ProjectLayout::Resource),
                )
            })
            .ok_or_else(|| {
                NwpkgError::at(
                    NwpkgErrorKind::Validation,
                    path,
                    "NSS overlay is outside every package source root",
                )
            })?;
        let root_is_file = fs::metadata(root)
            .map_err(|error| {
                NwpkgError::at(
                    NwpkgErrorKind::Io,
                    root,
                    format!("failed to inspect package source root: {error}"),
                )
            })?
            .is_file();
        let logical = logical_source_name(root, path, root_is_file)?;
        if let Some(existing) = logical_names.insert(logical.clone(), path.clone()) {
            return Err(NwpkgError::at(
                NwpkgErrorKind::Validation,
                path,
                format!(
                    "package source overlay is ambiguous on case-insensitive filesystems for \
                     {logical:?}: {} and {}",
                    existing.display(),
                    path.display()
                ),
            ));
        }
        total_source_bytes = total_source_bytes
            .checked_add(contents.len())
            .ok_or_else(|| {
                NwpkgError::at(
                    NwpkgErrorKind::Validation,
                    path,
                    "package source snapshot byte count overflowed",
                )
            })?;
        if contents.len() > limits.max_source_file_bytes
            || total_source_bytes > limits.max_total_source_bytes
        {
            return Err(NwpkgError::at(
                NwpkgErrorKind::Validation,
                path,
                "package source overlay exceeds configured compiler source bounds",
            ));
        }
        sources.insert(path.clone(), Arc::clone(contents));
    }

    let mut fingerprint = BTreeMap::new();
    let manifest_path = project_root.join(PROJECT_MANIFEST_FILENAME);
    let manifest_bytes = toml::to_string(manifest).map_err(|error| {
        NwpkgError::at(
            NwpkgErrorKind::Parse,
            &manifest_path,
            format!("failed to fingerprint validated project manifest: {error}"),
        )
    })?;
    fingerprint.insert(
        manifest_path,
        checksums::sha256_digest(manifest_bytes.as_bytes()).to_string(),
    );
    let mut dependency_graph = String::new();
    for dependency in dependencies {
        check_cancelled(cancellation)?;
        use std::fmt::Write as _;
        writeln!(
            dependency_graph,
            "{}\0{}\0{}",
            dependency.name,
            dependency.package_root.display(),
            dependency.source_root.display()
        )
        .map_err(|error| {
            NwpkgError::new(
                NwpkgErrorKind::Io,
                format!("failed to fingerprint dependency graph: {error}"),
            )
        })?;
    }
    fingerprint.insert(
        project_root.join(".nwpkg-session-dependency-graph"),
        checksums::sha256_digest(dependency_graph.as_bytes()).to_string(),
    );
    for (path, contents) in &sources {
        fingerprint.insert(
            path.clone(),
            checksums::sha256_digest(contents.as_ref()).to_string(),
        );
    }
    Ok((sources, fingerprint))
}

fn read_file_with_cancellation(
    path: &Path,
    max_bytes: usize,
    cancellation: &CancellationToken,
) -> Result<Vec<u8>, NwpkgError> {
    let mut file = File::open(path).map_err(|error| {
        NwpkgError::at(
            NwpkgErrorKind::Io,
            path,
            format!("failed to open package snapshot source: {error}"),
        )
    })?;
    let metadata = file.metadata().map_err(|error| {
        NwpkgError::at(
            NwpkgErrorKind::Io,
            path,
            format!("failed to inspect package snapshot source: {error}"),
        )
    })?;
    let expected = usize::try_from(metadata.len()).map_err(|_error| {
        NwpkgError::at(
            NwpkgErrorKind::Validation,
            path,
            "package snapshot file length does not fit in memory",
        )
    })?;
    if expected > max_bytes {
        return Err(NwpkgError::at(
            NwpkgErrorKind::Validation,
            path,
            format!(
                "package snapshot file contains {expected} bytes, exceeding maximum {max_bytes}"
            ),
        ));
    }
    let mut bytes = Vec::new();
    bytes.try_reserve(expected).map_err(|error| {
        NwpkgError::at(
            NwpkgErrorKind::Io,
            path,
            format!("failed to reserve package snapshot buffer: {error}"),
        )
    })?;
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        check_cancelled(cancellation)?;
        let count = file.read(&mut buffer).map_err(|error| {
            NwpkgError::at(
                NwpkgErrorKind::Io,
                path,
                format!("failed to read package snapshot source: {error}"),
            )
        })?;
        if count == 0 {
            break;
        }
        if bytes.len().saturating_add(count) > max_bytes {
            return Err(NwpkgError::at(
                NwpkgErrorKind::Validation,
                path,
                format!("package snapshot file exceeds maximum {max_bytes} while being read"),
            ));
        }
        let chunk = buffer.get(..count).ok_or_else(|| {
            NwpkgError::at(
                NwpkgErrorKind::Io,
                path,
                "package snapshot reader returned an invalid byte count",
            )
        })?;
        bytes.extend_from_slice(chunk);
    }
    check_cancelled(cancellation)?;
    Ok(bytes)
}

fn copy_snapshot_bytes(
    path: &Path,
    contents: &[u8],
    max_bytes: usize,
    cancellation: &CancellationToken,
) -> Result<Vec<u8>, NwpkgError> {
    if contents.len() > max_bytes {
        return Err(NwpkgError::at(
            NwpkgErrorKind::Validation,
            path,
            format!(
                "package snapshot overlay contains {} bytes, exceeding maximum {max_bytes}",
                contents.len()
            ),
        ));
    }
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(contents.len()).map_err(|error| {
        NwpkgError::at(
            NwpkgErrorKind::Io,
            path,
            format!("failed to reserve package snapshot overlay: {error}"),
        )
    })?;
    for chunk in contents.chunks(1024 * 1024) {
        check_cancelled(cancellation)?;
        bytes.extend_from_slice(chunk);
    }
    Ok(bytes)
}

fn logical_source_name(
    root: &Path,
    path: &Path,
    root_is_file: bool,
) -> Result<LogicalScriptName, NwpkgError> {
    let relative = if root_is_file {
        path.file_name().map(PathBuf::from).ok_or_else(|| {
            NwpkgError::at(
                NwpkgErrorKind::Validation,
                path,
                "resource source has no filename",
            )
        })?
    } else {
        path.strip_prefix(root)
            .map(Path::to_path_buf)
            .map_err(|error| {
                NwpkgError::at(
                    NwpkgErrorKind::Validation,
                    path,
                    format!("failed to relativize package source: {error}"),
                )
            })?
    };
    let relative = relative.to_str().ok_or_else(|| {
        NwpkgError::at(
            NwpkgErrorKind::Validation,
            path,
            "logical NWScript source paths must be valid UTF-8",
        )
    })?;
    LogicalScriptName::parse(relative).map_err(|error| {
        NwpkgError::at(
            NwpkgErrorKind::Validation,
            path,
            format!("invalid logical source name: {error}"),
        )
    })
}

fn is_nss_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("nss"))
}

fn overlay_for_path<'a>(
    overlays: &'a BTreeMap<PathBuf, Arc<[u8]>>,
    path: &Path,
) -> Option<&'a Arc<[u8]>> {
    overlays.iter().find_map(|(candidate, contents)| {
        physical_sources_equal(candidate, path).then_some(contents)
    })
}

fn check_cancelled(cancellation: &CancellationToken) -> Result<(), NwpkgError> {
    cancellation
        .check()
        .map_err(|error| NwpkgError::new(NwpkgErrorKind::Cancelled, error.to_string()))
}

#[cfg(all(test, unix))]
mod tests {
    use std::{
        collections::BTreeMap,
        fs,
        os::unix::fs::symlink,
        time::{SystemTime, UNIX_EPOCH},
    };

    use nwnrs_nwscript::CancellationToken;

    use super::ResolvedPackageSession;

    #[test]
    fn unsaved_overlay_under_a_symlinked_root_normalizes_to_the_package() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock before epoch")
            .as_nanos();
        let base = std::env::temp_dir().join(format!("nwnrs-session-alias-{nonce}"));
        let real = base.join("real");
        let alias = base.join("alias");
        fs::create_dir_all(&real).expect("create package");
        fs::write(
            real.join("nwpkg.toml"),
            "[project]\nname = \"alias\"\nkind = \"include\"\n\n[source]\npath = \".\"\n",
        )
        .expect("write manifest");
        symlink(&real, &alias).expect("create package alias");
        let overlay = alias.join("unsaved.nss");
        let overlays = BTreeMap::from([(overlay, b"void Unsaved();".to_vec())]);

        let session = ResolvedPackageSession::open(&alias, &overlays, &CancellationToken::new())
            .expect("resolve aliased package")
            .expect("package exists");
        let source = session
            .nss_source_paths()
            .expect("enumerate overlay sources");
        assert_eq!(
            source,
            vec![
                fs::canonicalize(&real)
                    .expect("canonical package")
                    .join("unsaved.nss")
            ]
        );
        assert_eq!(
            session
                .read_source(&alias.join("unsaved.nss"))
                .expect("read overlay through aliased path"),
            b"void Unsaved();"
        );

        let _ = fs::remove_dir_all(base);
    }
}

#[cfg(test)]
mod workspace_tests {
    use std::{
        collections::BTreeMap,
        fs,
        sync::Arc,
        time::{SystemTime, UNIX_EPOCH},
    };

    use nwnrs_nwscript::CancellationToken;

    use super::ResolvedPackageSession;

    fn write_include_package(root: &std::path::Path, source: &[u8]) {
        fs::create_dir_all(root).expect("create package");
        fs::write(
            root.join("nwpkg.toml"),
            "[project]\nname = \"refresh\"\nkind = \"include\"\n\n[source]\npath = \".\"\n",
        )
        .expect("write manifest");
        fs::write(root.join("library.nss"), source).expect("write package source");
    }

    #[test]
    fn workspace_overlays_are_scoped_to_the_resolved_package() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock before epoch")
            .as_nanos();
        let base = std::env::temp_dir().join(format!("nwnrs-session-workspace-{nonce}"));
        let left = base.join("left");
        let right = base.join("right");
        for (root, name) in [(&left, "left"), (&right, "right")] {
            fs::create_dir_all(root).expect("create package");
            fs::write(
                root.join("nwpkg.toml"),
                format!(
                    "[project]\nname = \"{name}\"\nkind = \"include\"\n\n[source]\npath = \".\"\n"
                ),
            )
            .expect("write manifest");
        }
        let overlays = BTreeMap::from([
            (left.join("left.nss"), b"void Left();".to_vec()),
            (right.join("right.nss"), b"void Right();".to_vec()),
        ]);
        ResolvedPackageSession::open(&left, &overlays, &CancellationToken::new())
            .err()
            .expect("strict session rejects another package overlay");

        let session = ResolvedPackageSession::open_with_workspace_overlays(
            &left,
            &overlays,
            &CancellationToken::new(),
        )
        .expect("resolve workspace session")
        .expect("package exists");
        assert_eq!(session.overlays().len(), 1);
        assert!(
            session
                .overlays()
                .keys()
                .all(|path| path.starts_with(session.project_root()))
        );
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn refresh_atomically_replaces_a_changed_source_snapshot() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock before epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("nwnrs-session-refresh-{nonce}"));
        write_include_package(&root, b"int Version() { return 1; }\n");
        let source_path = root.join("library.nss");
        let mut session =
            ResolvedPackageSession::open(&root, &BTreeMap::new(), &CancellationToken::new())
                .expect("open package")
                .expect("package exists");
        let snapshot = session
            .source_files
            .get(&fs::canonicalize(&source_path).expect("canonical source"))
            .expect("snapshotted source");
        assert!(
            Arc::strong_count(snapshot) >= 2,
            "the package session and resolver should share one immutable source buffer"
        );

        fs::write(&source_path, b"int Version() { return 2; }\n").expect("change package source");
        assert_eq!(
            session
                .read_source(&source_path)
                .expect("read old snapshot"),
            b"int Version() { return 1; }\n"
        );
        assert!(
            session
                .refresh_if_changed(&BTreeMap::new(), &CancellationToken::new())
                .expect("refresh package")
        );
        assert_eq!(
            session
                .read_source(&source_path)
                .expect("read new snapshot"),
            b"int Version() { return 2; }\n"
        );
        assert!(
            !session
                .refresh_if_changed(&BTreeMap::new(), &CancellationToken::new())
                .expect("unchanged refresh")
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn manifest_language_changes_replace_the_session_text_policy() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock before epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("nwnrs-session-language-{nonce}"));
        write_include_package(&root, b"void Library();\n");
        let mut session =
            ResolvedPackageSession::open(&root, &BTreeMap::new(), &CancellationToken::new())
                .expect("open package")
                .expect("package exists");
        assert_eq!(session.text_context().language_id(), 0);

        fs::write(
            root.join("nwpkg.toml"),
            "[project]\nname = \"refresh\"\nkind = \"include\"\nlanguage = \
             \"polish\"\n\n[source]\npath = \".\"\n",
        )
        .expect("change package language");
        assert!(
            session
                .refresh_if_changed(&BTreeMap::new(), &CancellationToken::new())
                .expect("refresh package")
        );
        assert_eq!(session.text_context().language_id(), 5);
        assert_eq!(
            session.text_context().encoding(),
            nwnrs_types::encoding::LegacyEncoding::Windows1250
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn cancelled_refresh_preserves_the_previous_snapshot() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock before epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("nwnrs-session-refresh-cancel-{nonce}"));
        write_include_package(&root, b"int Version() { return 1; }\n");
        let source_path = root.join("library.nss");
        let mut session =
            ResolvedPackageSession::open(&root, &BTreeMap::new(), &CancellationToken::new())
                .expect("open package")
                .expect("package exists");
        fs::write(&source_path, b"int Version() { return 2; }\n").expect("change package source");
        let cancellation = CancellationToken::new();
        cancellation.cancel();

        let error = session
            .refresh_if_changed(&BTreeMap::new(), &cancellation)
            .expect_err("cancel refresh");
        assert_eq!(error.kind(), crate::NwpkgErrorKind::Cancelled);
        assert_eq!(
            session
                .read_source(&source_path)
                .expect("old snapshot remains readable"),
            b"int Version() { return 1; }\n"
        );

        let _ = fs::remove_dir_all(root);
    }
}
