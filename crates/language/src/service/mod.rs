//! Persistent workspace, package-session, and document lifecycle service.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicU64, Ordering},
    },
};

use nwnrs_nwpkg::{
    DependencyProgressReporter, DependencyResolverConfiguration, EventCompletionAnalysis,
    EventProjectPlan, EventRegistration, ResolvedPackageSession,
    analyze_event_completion_from_session_with_cancellation,
    generate_event_dispatcher_from_session_with_cancellation,
};
use nwnrs_nwscript::{
    BatchCompileReport, BatchCompileStatus, CancellationToken, CompilerDiagnostic,
    LogicalScriptName, PhysicalSourceId, physical_sources_equal,
};

use crate::{
    LanguageCheckReport, LanguageCheckSummary, LanguageDiagnostic, LanguageDiagnosticCode,
    LanguageDiagnosticSeverity, LanguageOperationError, LanguageRange, LanguageSource,
    ManifestCompletion, ManifestDefinition, ManifestHover, ManifestSymbol, NwScriptCheckOptions,
    NwScriptCompletion, NwScriptDefinitionQuery, NwScriptDocumentSymbol, NwScriptIncludeCandidate,
    NwScriptInlayHint, NwScriptOutgoingCall, NwScriptProjectDocuments, NwScriptProjectIndex,
    NwScriptReference, NwScriptSemanticToken, NwScriptSignatureHelp, NwScriptSymbolDefinition,
    PackageDependencyPlan, PackageDependencySource, PackageDependencySourceContents,
    PackageDependencyStatus, analyze_manifest_diagnostics, analyze_nwscript_document,
    check_nwscript_with_cancellation,
    dependencies::{
        PendingDependencyPlan, apply_update_plan, clean_cache, create_update_plan,
        dependency_status, execute_fetch, list_dependency_sources, read_dependency_source,
    },
    find_nwscript_definitions, find_nwscript_outgoing_calls, find_nwscript_references,
    list_nwscript_document_symbols, manifest_completions, manifest_definition, manifest_hover,
    manifest_symbols,
    manifests::manifest_dependency_range,
    nwscript_utf16_range,
};

/// Compiler and installation configuration shared by one package session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageConfiguration {
    /// Source used to discover the deepest owning package.
    pub source_path:         PathBuf,
    /// Optional explicit package root used only when discovery finds none.
    pub project_root:        Option<PathBuf>,
    /// Additional include directories.
    pub include_directories: Vec<PathBuf>,
    /// Optional explicit language specification.
    pub langspec:            Option<PathBuf>,
    /// Maximum recursive include depth.
    pub max_include_depth:   usize,
    /// Optional NWN installation root.
    pub root:                Option<PathBuf>,
    /// Optional NWN user-directory root.
    pub user:                Option<PathBuf>,
    /// Installation language.
    pub language:            String,
    /// Whether installation override resources participate.
    pub load_ovr:            bool,
    /// Optional system Git executable for package dependency resolution.
    pub git_executable:      Option<PathBuf>,
    /// Optional immutable Git dependency cache root.
    pub package_cache:       Option<PathBuf>,
}

impl Default for PackageConfiguration {
    fn default() -> Self {
        Self {
            source_path:         PathBuf::new(),
            project_root:        None,
            include_directories: Vec::new(),
            langspec:            None,
            max_include_depth:   nwnrs_nwscript::DEFAULT_MAX_INCLUDE_DEPTH,
            root:                None,
            user:                None,
            language:            "english".to_string(),
            load_ovr:            false,
            git_executable:      None,
            package_cache:       None,
        }
    }
}

impl PackageConfiguration {
    fn query(
        &self,
        source_path: PathBuf,
        symbol: String,
        qualifier: Option<String>,
        overlays: BTreeMap<PathBuf, Vec<u8>>,
    ) -> NwScriptDefinitionQuery {
        NwScriptDefinitionQuery {
            source_path,
            symbol,
            qualifier,
            project_root: self.project_root.clone(),
            include_directories: self.include_directories.clone(),
            source_overlays: overlays,
            langspec: self.langspec.clone(),
            max_include_depth: self.max_include_depth,
            root: self.root.clone(),
            user: self.user.clone(),
            language: self.language.clone(),
            load_ovr: self.load_ovr,
        }
    }
}

fn logical_document(
    configuration: &PackageConfiguration,
) -> Result<Option<LogicalScriptName>, String> {
    if configuration.source_path.is_absolute() {
        return Ok(None);
    }
    let source = configuration
        .source_path
        .to_str()
        .ok_or_else(|| "logical script identities must be valid UTF-8".to_string())?;
    LogicalScriptName::parse(source)
        .map(Some)
        .map_err(|error| format!("invalid logical script identity {source:?}: {error}"))
}

/// Immutable snapshot of one synchronized open editor document.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenDocument {
    /// Exact resolved source path.
    pub path:    PathBuf,
    /// Monotonically increasing editor document version.
    pub version: i64,
    /// Current authoritative source bytes, or `None` when the editor rejected
    /// this version because it is not representable in the required encoding.
    pub bytes:   Option<Arc<[u8]>>,
}

/// One package discovered from the native workspace registry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspacePackage {
    /// Canonical package root.
    pub root:          PathBuf,
    /// Canonical manifest path.
    pub manifest_path: PathBuf,
}

/// One indexed source symbol associated with its package and source path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceSymbol {
    /// Canonical package root.
    pub package_root: PathBuf,
    /// Physical source path.
    pub source_path:  PathBuf,
    /// Compiler-owned symbol.
    pub symbol:       NwScriptDocumentSymbol,
}

struct RegisteredDocument {
    document: OpenDocument,
    physical: PhysicalSourceId,
    sequence: u64,
}

#[derive(Default)]
struct DocumentRegistry {
    // Editor document versions belong to a stable canonical path, not to an
    // inode/file ID. Editors commonly save by atomically replacing a file,
    // which changes its physical identity without closing the document.
    documents:     HashMap<PathBuf, RegisteredDocument>,
    next_sequence: u64,
}

#[derive(Default)]
struct PendingInvalidations {
    clear_all: bool,
    paths:     HashSet<PhysicalSourceId>,
}

struct RecoveryPlanState {
    version:         i64,
    attribute_start: usize,
    plan:            Arc<EventProjectPlan>,
}

struct EventPackageState {
    generation: u64,
    session:    ResolvedPackageSession,
    strict:     Option<Result<Option<Arc<EventProjectPlan>>, String>>,
    recovery:   HashMap<PhysicalSourceId, RecoveryPlanState>,
}

struct PackageSession {
    root:             PathBuf,
    configuration:    Mutex<PackageConfiguration>,
    index:            Mutex<NwScriptProjectIndex>,
    pending:          Mutex<PendingInvalidations>,
    event:            Mutex<Option<EventPackageState>>,
    last_used:        AtomicU64,
    dependency_roots: Mutex<Vec<PathBuf>>,
}

impl PackageSession {
    fn new(root: PathBuf, configuration: PackageConfiguration) -> Self {
        Self {
            root,
            configuration: Mutex::new(configuration),
            index: Mutex::new(NwScriptProjectIndex::new()),
            pending: Mutex::new(PendingInvalidations::default()),
            event: Mutex::new(None),
            last_used: AtomicU64::new(0),
            dependency_roots: Mutex::new(Vec::new()),
        }
    }

    fn update_configuration(&self, configuration: &PackageConfiguration) -> Result<(), String> {
        let mut current = lock(&self.configuration, "package configuration")?;
        // `source_path` identifies the document being queried, not package
        // compiler configuration. Definition and relationship resolution
        // routinely cross source files inside one package; treating that as a
        // configuration change discarded the package index and exact-version
        // recovery plan during the query itself.
        if !same_package_configuration(&current, configuration) {
            *current = configuration.clone();
            drop(current);
            self.queue_clear()?;
        }
        Ok(())
    }

    fn queue_clear(&self) -> Result<(), String> {
        let mut pending = lock(&self.pending, "package invalidation queue")?;
        pending.clear_all = true;
        pending.paths.clear();
        drop(pending);
        *lock(&self.event, "package event cache")? = None;
        Ok(())
    }

    fn queue_path(&self, path: &Path) -> Result<(), String> {
        let identity = PhysicalSourceId::resolve(path).map_err(|error| {
            format!(
                "failed to resolve invalidated source {}: {error}",
                path.display()
            )
        })?;
        let mut pending = lock(&self.pending, "package invalidation queue")?;
        if !pending.clear_all {
            pending.paths.insert(identity);
        }
        Ok(())
    }

    fn lock_index(&self) -> crate::LanguageResult<MutexGuard<'_, NwScriptProjectIndex>> {
        let mut index = lock(&self.index, "package compiler index")?;
        let mut pending = lock(&self.pending, "package invalidation queue")?;
        let changed = pending.clear_all || !pending.paths.is_empty();
        if pending.clear_all {
            index.clear();
        } else {
            for path in &pending.paths {
                index.invalidate_path(path.path())?;
            }
        }
        *pending = PendingInvalidations::default();
        drop(pending);
        if changed {
            *lock(&self.event, "package event cache")? = None;
        }
        Ok(index)
    }
}

fn same_package_configuration(left: &PackageConfiguration, right: &PackageConfiguration) -> bool {
    left.project_root == right.project_root
        && left.include_directories == right.include_directories
        && left.langspec == right.langspec
        && left.max_include_depth == right.max_include_depth
        && left.root == right.root
        && left.user == right.user
        && left.language == right.language
        && left.load_ovr == right.load_ovr
        && left.git_executable == right.git_executable
        && left.package_cache == right.package_cache
}

#[derive(Default)]
struct LanguageServiceState {
    documents:        Mutex<DocumentRegistry>,
    packages:         Mutex<HashMap<PhysicalSourceId, Arc<PackageSession>>>,
    workspaces:       Mutex<HashMap<PhysicalSourceId, WorkspaceInventory>>,
    access_clock:     AtomicU64,
    dependency_plans: Mutex<HashMap<PhysicalSourceId, (String, PendingDependencyPlan)>>,
}

#[derive(Default)]
struct WorkspaceInventory {
    initialized: bool,
    manifests:   BTreeSet<PathBuf>,
}

/// Persistent compiler-owned language service.
///
/// One instance manages independent indexed sessions per deepest owning
/// `nwpkg.toml` and synchronizes unsaved text through document lifecycle APIs.
#[derive(Clone, Default)]
pub struct LanguageService {
    state: Arc<LanguageServiceState>,
}

impl LanguageService {
    /// Creates an empty persistent language service.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a workspace root for package enumeration.
    pub fn add_workspace(&self, root: &Path) -> Result<(), LanguageOperationError> {
        let identity = PhysicalSourceId::resolve(root)
            .map_err(|error| format!("failed to resolve workspace {}: {error}", root.display()))?;
        lock(&self.state.workspaces, "workspace registry")?
            .entry(identity)
            .or_default();
        Ok(())
    }

    /// Removes a workspace root and every package session owned by it.
    pub fn remove_workspace(&self, root: &Path) -> Result<(), LanguageOperationError> {
        let identity = PhysicalSourceId::resolve(root)
            .map_err(|error| format!("failed to resolve workspace {}: {error}", root.display()))?;
        lock(&self.state.workspaces, "workspace registry")?.remove(&identity);
        lock(&self.state.packages, "package registry")?
            .retain(|_key, package| !package.root.starts_with(identity.path()));
        lock(&self.state.dependency_plans, "dependency update plans")?
            .retain(|package, _| !package.path().starts_with(identity.path()));
        Ok(())
    }

    /// Discovers every package below registered workspace roots.
    pub fn workspace_packages(
        &self,
        cancellation: Option<&CancellationToken>,
    ) -> Result<Vec<WorkspacePackage>, LanguageOperationError> {
        let workspaces = lock(&self.state.workspaces, "workspace registry")?
            .iter()
            .map(|(identity, inventory)| {
                (
                    identity.clone(),
                    inventory.initialized,
                    inventory.manifests.clone(),
                )
            })
            .collect::<Vec<_>>();
        let mut manifests = BTreeSet::new();
        for (identity, initialized, cached) in workspaces {
            if initialized {
                manifests.extend(cached);
                continue;
            }
            let discovered = scan_workspace_manifests(identity.path(), cancellation)?;
            manifests.extend(discovered.iter().cloned());
            if let Some(inventory) =
                lock(&self.state.workspaces, "workspace registry")?.get_mut(&identity)
            {
                inventory.manifests = discovered;
                inventory.initialized = true;
            }
        }
        Ok(manifests
            .into_iter()
            .filter_map(|manifest_path| {
                let root = manifest_path.parent()?.to_path_buf();
                Some(WorkspacePackage {
                    root,
                    manifest_path,
                })
            })
            .collect())
    }

    /// Returns indexed symbols for all packages in the workspace registry.
    pub fn workspace_symbols(
        &self,
        base: &PackageConfiguration,
        cancellation: Option<&CancellationToken>,
    ) -> Result<Vec<WorkspaceSymbol>, LanguageOperationError> {
        let mut output = Vec::new();
        for package in self.workspace_packages(cancellation)? {
            if let Some(cancellation) = cancellation {
                cancellation.check().map_err(|error| error.to_string())?;
            }
            let mut configuration = base.clone();
            configuration.source_path = package.root.clone();
            configuration.project_root = Some(package.root.clone());
            let (documents, _warnings) = self.project_documents(&configuration, cancellation)?;
            for (source_path, symbols) in documents {
                output.extend(symbols.into_iter().map(|symbol| WorkspaceSymbol {
                    package_root: package.root.clone(),
                    source_path: source_path.clone(),
                    symbol,
                }));
            }
        }
        Ok(output)
    }

    /// Opens a document overlay and invalidates every affected package.
    pub fn open_document(
        &self,
        path: &Path,
        version: i64,
        bytes: impl Into<Arc<[u8]>>,
    ) -> Result<(), LanguageOperationError> {
        self.update_document(path, version, bytes)
    }

    /// Replaces one document overlay when `version` is newer.
    pub fn update_document(
        &self,
        path: &Path,
        version: i64,
        bytes: impl Into<Arc<[u8]>>,
    ) -> Result<(), LanguageOperationError> {
        let identity = PhysicalSourceId::resolve(path).map_err(|error| {
            format!(
                "failed to resolve open document {}: {error}",
                path.display()
            )
        })?;
        {
            let mut documents = lock(&self.state.documents, "document registry")?;
            insert_document_version(
                &mut documents,
                identity.clone(),
                version,
                Some(bytes.into()),
            )?;
        }
        self.invalidate_source(identity.path())
    }

    /// Marks one editor document version as encoding-invalid.
    ///
    /// Invalid documents remain present in the registry so queries cannot
    /// silently fall back to older overlays or disk contents.
    pub fn invalidate_document(
        &self,
        path: &Path,
        version: i64,
    ) -> Result<(), LanguageOperationError> {
        let identity = PhysicalSourceId::resolve(path).map_err(|error| {
            format!(
                "failed to resolve invalid editor document {}: {error}",
                path.display()
            )
        })?;
        {
            let mut documents = lock(&self.state.documents, "document registry")?;
            insert_document_version(&mut documents, identity.clone(), version, None)?;
        }
        self.invalidate_source(identity.path())
    }

    /// Closes one document overlay and invalidates its disk-backed source.
    pub fn close_document(&self, path: &Path) -> Result<(), LanguageOperationError> {
        let identity = PhysicalSourceId::resolve(path).map_err(|error| {
            format!(
                "failed to resolve closed document {}: {error}",
                path.display()
            )
        })?;
        let mut documents = lock(&self.state.documents, "document registry")?;
        if documents.documents.remove(identity.path()).is_none() {
            documents
                .documents
                .retain(|_, document| document.physical != identity);
        }
        drop(documents);
        self.invalidate_source(identity.path())
    }

    /// Returns the synchronized document snapshot for one path.
    pub fn document(&self, path: &Path) -> Result<Option<OpenDocument>, LanguageOperationError> {
        let identity = PhysicalSourceId::resolve(path)
            .map_err(|error| format!("failed to resolve document {}: {error}", path.display()))?;
        let documents = lock(&self.state.documents, "document registry")?;
        Ok(documents
            .documents
            .get(identity.path())
            .or_else(|| {
                documents
                    .documents
                    .values()
                    .filter(|document| document.physical == identity)
                    .max_by_key(|document| document.sequence)
            })
            .map(|document| document.document.clone()))
    }

    /// Returns manifest diagnostics from the authoritative open-document
    /// snapshot, falling back to disk only when the manifest is not open.
    pub fn manifest_diagnostics(
        &self,
        path: &Path,
        git_executable: Option<PathBuf>,
        cache_root: Option<PathBuf>,
    ) -> Result<Vec<LanguageDiagnostic>, LanguageOperationError> {
        let source = self.manifest_source(path)?;
        let mut diagnostics = analyze_manifest_diagnostics(path, &source);
        if !diagnostics.iter().any(|diagnostic| {
            diagnostic.severity == LanguageDiagnosticSeverity::Error
                && diagnostic.code.as_ref().is_some_and(|code| {
                    matches!(
                        code,
                        LanguageDiagnosticCode::Named(name)
                            if name == "nwpkg.syntax"
                                || name == "nwpkg.schema"
                                || name == "nwpkg.validation"
                    )
                })
        }) {
            let configuration = dependency_configuration(git_executable, cache_root);
            for status in dependency_status(path, &source, &configuration)
                .map_err(|error| error.to_string())?
            {
                if !matches!(status.state.as_str(), "missing" | "stale" | "invalid") {
                    continue;
                }
                let direct = status.route.split('/').next().unwrap_or(&status.name);
                diagnostics.push(LanguageDiagnostic {
                    severity: LanguageDiagnosticSeverity::Error,
                    code:     Some(LanguageDiagnosticCode::Named(format!(
                        "nwpkg.dependency.{}",
                        status.state
                    ))),
                    message:  status.message.unwrap_or_else(|| {
                        format!("dependency {} is {}", status.route, status.state)
                    }),
                    source:   LanguageSource::Physical(path.to_path_buf()),
                    range:    manifest_dependency_range(&source, direct),
                });
            }
        }
        Ok(diagnostics)
    }

    /// Returns manifest completions at a UTF-16 editor position.
    pub fn manifest_completions(
        &self,
        path: &Path,
        line: u32,
        character: u32,
    ) -> Result<Vec<ManifestCompletion>, LanguageOperationError> {
        let source = self.manifest_source(path)?;
        let offset = utf16_position_to_byte(&source, line, character);
        Ok(manifest_completions(path, &source, offset))
    }

    /// Returns manifest hover information at a UTF-16 editor position.
    pub fn manifest_hover(
        &self,
        path: &Path,
        line: u32,
        character: u32,
    ) -> Result<Option<ManifestHover>, LanguageOperationError> {
        let source = self.manifest_source(path)?;
        let offset = utf16_position_to_byte(&source, line, character);
        Ok(manifest_hover(&source, offset))
    }

    /// Resolves a manifest definition at a UTF-16 editor position.
    pub fn manifest_definition(
        &self,
        path: &Path,
        line: u32,
        character: u32,
    ) -> Result<Option<ManifestDefinition>, LanguageOperationError> {
        let source = self.manifest_source(path)?;
        let offset = utf16_position_to_byte(&source, line, character);
        Ok(manifest_definition(path, &source, offset))
    }

    /// Returns manifest Outline symbols from the authoritative snapshot.
    pub fn manifest_symbols(
        &self,
        path: &Path,
    ) -> Result<Vec<ManifestSymbol>, LanguageOperationError> {
        let source = self.manifest_source(path)?;
        Ok(manifest_symbols(&source))
    }

    /// Returns the current local/Git dependency graph without network access.
    pub fn package_dependencies(
        &self,
        manifest_path: &Path,
        git_executable: Option<PathBuf>,
        cache_root: Option<PathBuf>,
    ) -> Result<Vec<PackageDependencyStatus>, LanguageOperationError> {
        let source = self
            .manifest_source(manifest_path)
            .map_err(LanguageOperationError::from)?;
        dependency_status(
            manifest_path,
            &source,
            &dependency_configuration(git_executable, cache_root),
        )
    }

    /// Explicitly fetches and locks missing package dependencies.
    pub fn fetch_package_dependencies(
        &self,
        manifest_path: &Path,
        git_executable: Option<PathBuf>,
        cache_root: Option<PathBuf>,
        selected: Option<&str>,
        cancellation: &CancellationToken,
        progress: Option<&DependencyProgressReporter>,
    ) -> Result<Vec<PackageDependencyStatus>, LanguageOperationError> {
        self.require_saved_manifest(manifest_path)?;
        let root = manifest_path.parent().ok_or_else(|| {
            LanguageOperationError::new(
                "nwpkg.manifest.noRoot",
                format!("manifest has no package root: {}", manifest_path.display()),
            )
        })?;
        let result = execute_fetch(
            root,
            &dependency_configuration(git_executable, cache_root),
            selected,
            cancellation,
            progress,
        )?;
        self.invalidate_source(&root.join(nwnrs_nwpkg::PACKAGE_LOCK_FILENAME))?;
        Ok(result)
    }

    /// Resolves an update and retains it until an explicit apply call.
    pub fn plan_package_dependency_update(
        &self,
        manifest_path: &Path,
        git_executable: Option<PathBuf>,
        cache_root: Option<PathBuf>,
        selected: Option<&str>,
        cancellation: &CancellationToken,
        progress: Option<&DependencyProgressReporter>,
    ) -> Result<PackageDependencyPlan, LanguageOperationError> {
        self.require_saved_manifest(manifest_path)?;
        let root = manifest_path.parent().ok_or_else(|| {
            LanguageOperationError::new(
                "nwpkg.manifest.noRoot",
                format!("manifest has no package root: {}", manifest_path.display()),
            )
        })?;
        let (plan, pending) = create_update_plan(
            root,
            &dependency_configuration(git_executable, cache_root),
            selected,
            cancellation,
            progress,
        )?;
        let identity = PhysicalSourceId::resolve(root).map_err(|error| {
            LanguageOperationError::new(
                "nwpkg.preview.identity",
                format!("failed to resolve package {}: {error}", root.display()),
            )
        })?;
        lock(&self.state.dependency_plans, "dependency update plans")
            .map_err(LanguageOperationError::from)?
            .insert(identity, (plan.token.clone(), pending));
        Ok(plan)
    }

    /// Applies one exact dependency update preview after revalidating both the
    /// manifest and previous lock.
    pub fn apply_package_dependency_update(
        &self,
        manifest_path: &Path,
        token: &str,
    ) -> Result<(), LanguageOperationError> {
        self.require_saved_manifest(manifest_path)?;
        let expected_root = manifest_path.parent().ok_or_else(|| {
            LanguageOperationError::new(
                "nwpkg.manifest.noRoot",
                format!("manifest has no package root: {}", manifest_path.display()),
            )
        })?;
        let identity = PhysicalSourceId::resolve(expected_root).map_err(|error| {
            LanguageOperationError::new(
                "nwpkg.preview.identity",
                format!(
                    "failed to resolve package {}: {error}",
                    expected_root.display()
                ),
            )
        })?;
        let pending = {
            let mut plans = lock(&self.state.dependency_plans, "dependency update plans")
                .map_err(LanguageOperationError::from)?;
            if !plans
                .get(&identity)
                .is_some_and(|(expected_token, _)| expected_token == token)
            {
                return Err(LanguageOperationError::new(
                    "nwpkg.preview.expired",
                    "dependency update preview expired; create a new preview",
                ));
            }
            plans.remove(&identity).map(|(_, pending)| pending)
        }
        .ok_or_else(|| {
            LanguageOperationError::new(
                "nwpkg.preview.expired",
                "dependency update preview expired; create a new preview",
            )
        })?;
        if !physical_sources_equal(&pending.project_root, expected_root) {
            return Err(LanguageOperationError::new(
                "nwpkg.preview.packageMismatch",
                "dependency update preview belongs to another package",
            ));
        }
        apply_update_plan(pending)?;
        self.invalidate_source(&expected_root.join(nwnrs_nwpkg::PACKAGE_LOCK_FILENAME))
    }

    /// Removes managed Git packages proven unused by the persistent global
    /// lock-owner registry.
    pub fn clean_package_dependency_cache(
        &self,
        git_executable: Option<PathBuf>,
        cache_root: Option<PathBuf>,
        cancellation: &CancellationToken,
    ) -> Result<Vec<PathBuf>, LanguageOperationError> {
        clean_cache(
            &dependency_configuration(git_executable, cache_root),
            cancellation,
        )
    }

    /// Lists manifest and NWScript sources exposed by one materialized
    /// dependency route.
    pub fn package_dependency_sources(
        &self,
        manifest_path: &Path,
        route: &str,
        git_executable: Option<PathBuf>,
        cache_root: Option<PathBuf>,
        cancellation: &CancellationToken,
    ) -> Result<Vec<PackageDependencySource>, LanguageOperationError> {
        let source = self
            .manifest_source(manifest_path)
            .map_err(LanguageOperationError::from)?;
        list_dependency_sources(
            manifest_path,
            &source,
            route,
            &dependency_configuration(git_executable, cache_root),
            cancellation,
        )
    }

    /// Reads exact bytes for one source previously exposed by
    /// [`Self::package_dependency_sources`].
    pub fn read_package_dependency_source(
        &self,
        manifest_path: &Path,
        route: &str,
        relative_path: &str,
        git_executable: Option<PathBuf>,
        cache_root: Option<PathBuf>,
    ) -> Result<PackageDependencySourceContents, LanguageOperationError> {
        let source = self
            .manifest_source(manifest_path)
            .map_err(LanguageOperationError::from)?;
        read_dependency_source(
            manifest_path,
            &source,
            route,
            relative_path,
            &dependency_configuration(git_executable, cache_root),
        )
    }

    fn manifest_source(&self, path: &Path) -> Result<String, String> {
        if let Some(document) = self.document(path).map_err(|error| error.message)? {
            let bytes = document.bytes.ok_or_else(|| {
                format!(
                    "open manifest {} is invalid for its synchronized version",
                    path.display()
                )
            })?;
            return String::from_utf8(bytes.as_ref().to_vec()).map_err(|error| {
                format!(
                    "open manifest {} is not valid UTF-8: {error}",
                    path.display()
                )
            });
        }
        fs::read_to_string(path)
            .map_err(|error| format!("failed to read manifest {}: {error}", path.display()))
    }

    fn manifest_source_if_present(&self, path: &Path) -> Result<Option<String>, String> {
        if self
            .document(path)
            .map_err(|error| error.message)?
            .is_some()
        {
            return self.manifest_source(path).map(Some);
        }
        match fs::read_to_string(path) {
            Ok(source) => Ok(Some(source)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(format!(
                "failed to read manifest {}: {error}",
                path.display()
            )),
        }
    }

    fn require_saved_manifest(&self, path: &Path) -> Result<(), LanguageOperationError> {
        if let Some(document) = self.document(path)? {
            let bytes = document.bytes.ok_or_else(|| {
                LanguageOperationError::new(
                    "nwpkg.manifest.invalidSnapshot",
                    format!(
                        "open manifest {} is invalid for its synchronized version",
                        path.display()
                    ),
                )
                .with_path(path)
            })?;
            let disk = fs::read(path).map_err(|error| {
                LanguageOperationError::new(
                    "nwpkg.manifest.read",
                    format!("failed to read manifest {}: {error}", path.display()),
                )
                .with_path(path)
            })?;
            if bytes.as_ref() != disk {
                return Err(LanguageOperationError::new(
                    "nwpkg.manifest.unsaved",
                    format!(
                        "{} has unsaved changes; save it before resolving dependencies",
                        path.display()
                    ),
                )
                .with_path(path));
            }
        }
        Ok(())
    }

    /// Invalidates packages affected by a physical watcher notification.
    pub fn invalidate_source(&self, path: &Path) -> Result<(), LanguageOperationError> {
        if path
            .file_name()
            .is_some_and(|name| name == "nwpkg.toml" || name == "nwpkg.lock")
        {
            let resolved =
                nwnrs_nwscript::canonicalize_with_missing_suffix(path).map_err(|error| {
                    format!(
                        "failed to resolve dependency plan source {}: {error}",
                        path.display()
                    )
                })?;
            lock(&self.state.dependency_plans, "dependency update plans")?
                .retain(|root, _| !resolved.starts_with(root.path()));
        }
        if path.file_name().is_some_and(|name| name == "nwpkg.toml") {
            let manifest_path =
                nwnrs_nwscript::canonicalize_with_missing_suffix(path).map_err(|error| {
                    format!(
                        "failed to resolve workspace manifest {}: {error}",
                        path.display()
                    )
                })?;
            let exists = path.is_file();
            for (root, inventory) in lock(&self.state.workspaces, "workspace registry")?.iter_mut()
            {
                if manifest_path.starts_with(root.path()) {
                    if exists {
                        inventory.manifests.insert(manifest_path.clone());
                    } else {
                        inventory.manifests.remove(&manifest_path);
                    }
                }
            }
        }
        let packages = lock(&self.state.packages, "package registry")?;
        for package in packages.values() {
            if path
                .file_name()
                .is_some_and(|name| name == "nwpkg.toml" || name == "nwpkg.lock")
                && path.starts_with(&package.root)
            {
                package.queue_clear()?;
            } else if package_contains_source(package, path)? {
                package.queue_path(path)?;
            }
        }
        Ok(())
    }

    /// Invalidates every known package session.
    pub fn invalidate_all(&self) -> Result<(), LanguageOperationError> {
        for package in lock(&self.state.packages, "package registry")?.values() {
            package.queue_clear()?;
        }
        for inventory in lock(&self.state.workspaces, "workspace registry")?.values_mut() {
            inventory.initialized = false;
            inventory.manifests.clear();
        }
        lock(&self.state.dependency_plans, "dependency update plans")?.clear();
        Ok(())
    }

    /// Resolves the deepest package owning a physical source, or the canonical
    /// non-package directory fallback used by compiler sessions.
    pub fn resolve_package_root(&self, path: &Path) -> Result<PathBuf, LanguageOperationError> {
        discover_package_root(path, path.parent()).map_err(|message| {
            LanguageOperationError::new("nwpkg.ownership", message).with_path(path)
        })
    }

    /// Releases the deepest package owning `path`.
    pub fn release_package(&self, path: &Path) -> Result<(), LanguageOperationError> {
        let root = discover_package_root(path, None)?;
        let identity = PhysicalSourceId::resolve(&root)
            .map_err(|error| format!("failed to resolve package {}: {error}", root.display()))?;
        lock(&self.state.packages, "package registry")?.remove(&identity);
        lock(&self.state.dependency_plans, "dependency update plans")?.remove(&identity);
        Ok(())
    }

    /// Checks NWScript inputs using only synchronized overlays belonging to
    /// their actual package graphs.
    pub fn check(
        &self,
        mut options: NwScriptCheckOptions,
        cancellation: &CancellationToken,
    ) -> Result<LanguageCheckReport, LanguageOperationError> {
        let documents = lock(&self.state.documents, "document registry")?;
        let mut overlays = BTreeMap::new();
        for registered in documents.documents.values() {
            let document = &registered.document;
            let relevant = options.paths.iter().any(|input| {
                discover_package_root(input, input.parent())
                    .is_ok_and(|root| document.path.starts_with(root))
            });
            if relevant {
                let bytes = document.bytes.as_ref().ok_or_else(|| {
                    format!(
                        "open NWScript document {} is not representable as Windows-1252",
                        document.path.display()
                    )
                })?;
                overlays.insert(document.path.clone(), bytes.to_vec());
            }
        }
        drop(documents);
        options.source_overlays = overlays;
        let report = check_nwscript_with_cancellation(&options, cancellation)?;
        Ok(language_check_report(report))
    }

    /// Finds compiler definitions in the owning persistent package session.
    pub fn definitions(
        &self,
        configuration: &PackageConfiguration,
        symbol: String,
        qualifier: Option<String>,
        cancellation: Option<&CancellationToken>,
    ) -> Result<Vec<NwScriptSymbolDefinition>, LanguageOperationError> {
        let package = self.package(configuration)?;
        let query = self.query(configuration, symbol, qualifier)?;
        if logical_document(configuration)?.is_some() {
            return find_nwscript_definitions(&query);
        }
        package.lock_index()?.definitions(&query, cancellation)
    }

    /// Returns compiler-owned ordinary NWScript completions.
    pub fn completions(
        &self,
        configuration: &PackageConfiguration,
        line: usize,
        column: usize,
        cancellation: Option<&CancellationToken>,
    ) -> Result<Vec<NwScriptCompletion>, LanguageOperationError> {
        let package = self.package(configuration)?;
        let query = self.query(configuration, String::new(), None)?;
        package
            .lock_index()?
            .completions(&query, line, column, cancellation)
    }

    /// Returns compiler-owned signature help for the active call.
    pub fn signature_help(
        &self,
        configuration: &PackageConfiguration,
        line: usize,
        column: usize,
        cancellation: Option<&CancellationToken>,
    ) -> Result<Option<NwScriptSignatureHelp>, LanguageOperationError> {
        let package = self.package(configuration)?;
        let query = self.query(configuration, String::new(), None)?;
        package
            .lock_index()?
            .signature_help(&query, line, column, cancellation)
    }

    /// Finds include files that provide an unresolved symbol.
    pub fn include_candidates(
        &self,
        configuration: &PackageConfiguration,
        symbol: String,
        cancellation: Option<&CancellationToken>,
    ) -> Result<Vec<NwScriptIncludeCandidate>, LanguageOperationError> {
        let package = self.package(configuration)?;
        let query = self.query(configuration, symbol, None)?;
        package
            .lock_index()?
            .include_candidates(&query, cancellation)
    }

    /// Finds references to the symbol at a source position.
    pub fn references(
        &self,
        configuration: &PackageConfiguration,
        line: usize,
        column: usize,
        cancellation: Option<&CancellationToken>,
    ) -> Result<Vec<NwScriptReference>, LanguageOperationError> {
        let package = self.package(configuration)?;
        let query = self.query(configuration, String::new(), None)?;
        if logical_document(configuration)?.is_some() {
            return find_nwscript_references(&query, line, column);
        }
        package
            .lock_index()?
            .find_references(&query, line, column, cancellation)
    }

    /// Produces a compiler-owned prepare-rename result or complete validated
    /// edit plan.
    pub fn rename_plan(
        &self,
        configuration: &PackageConfiguration,
        line: usize,
        column: usize,
        new_name: Option<&str>,
        cancellation: Option<&CancellationToken>,
    ) -> Result<crate::NwScriptRenamePlan, LanguageOperationError> {
        if logical_document(configuration)
            .map_err(LanguageOperationError::from)?
            .is_some()
        {
            return Err(LanguageOperationError::new(
                "nwscript.rename.readOnly",
                "packed game and generated symbols are read-only and cannot be renamed",
            ));
        }
        let package = self.package(configuration)?;
        let query = self.query(configuration, String::new(), None)?;
        package
            .lock_index()?
            .rename_plan(&query, line, column, new_name, cancellation)
    }

    /// Returns quick fixes derived from compiler tokens, symbols, and source
    /// spans without parsing diagnostic prose in the editor.
    pub fn code_actions(
        &self,
        configuration: &PackageConfiguration,
        diagnostics: &[crate::NwScriptCodeActionDiagnostic],
        cancellation: Option<&CancellationToken>,
    ) -> Result<Vec<crate::NwScriptCodeAction>, LanguageOperationError> {
        if logical_document(configuration)
            .map_err(LanguageOperationError::from)?
            .is_some()
        {
            return Ok(Vec::new());
        }
        let package = self.package(configuration)?;
        let query = self.query(configuration, String::new(), None)?;
        let mut index = package.lock_index()?;
        crate::code_actions::code_actions(&mut index, &query, diagnostics, cancellation)
    }

    /// Finds outgoing calls from the function containing a source line.
    pub fn outgoing_calls(
        &self,
        configuration: &PackageConfiguration,
        line: usize,
        cancellation: Option<&CancellationToken>,
    ) -> Result<Vec<NwScriptOutgoingCall>, LanguageOperationError> {
        let package = self.package(configuration)?;
        let query = self.query(configuration, String::new(), None)?;
        if logical_document(configuration)?.is_some() {
            return find_nwscript_outgoing_calls(&query, line);
        }
        package
            .lock_index()?
            .outgoing_calls(&query, line, cancellation)
    }

    /// Returns compiler-backed document symbols.
    pub fn document_symbols(
        &self,
        configuration: &PackageConfiguration,
        cancellation: Option<&CancellationToken>,
    ) -> Result<Vec<NwScriptDocumentSymbol>, LanguageOperationError> {
        let package = self.package(configuration)?;
        let query = self.query(configuration, String::new(), None)?;
        if let Some(resource) = logical_document(configuration)? {
            return list_nwscript_document_symbols(&query, Some(resource.as_str()));
        }
        let recovery = if cancellation.is_some() {
            self.current_event_recovery_plan(configuration)?
        } else {
            None
        };
        let mut symbols = if recovery.is_some() {
            // The project index performs strict macro analysis. For the exact
            // synchronized document version represented by a recovery plan,
            // use the compiler's tolerant authored-source Outline instead;
            // the canonical recovery plan below supplies event semantics.
            list_nwscript_document_symbols(&query, None)?
        } else {
            package
                .lock_index()?
                .document_symbols(&query, cancellation)?
        };
        if let Some(cancellation) = cancellation {
            if let Some(plan) = recovery {
                decorate_event_outline(&mut symbols, &plan, &configuration.source_path);
            } else if let Some(plan) = self.strict_event_plan(configuration, cancellation)? {
                decorate_event_outline(&mut symbols, &plan, &configuration.source_path);
            }
        }
        Ok(symbols)
    }

    /// Returns semantic tokens and inlay hints.
    pub fn analyze_document(
        &self,
        configuration: &PackageConfiguration,
        cancellation: Option<&CancellationToken>,
    ) -> Result<(Vec<NwScriptSemanticToken>, Vec<NwScriptInlayHint>), LanguageOperationError> {
        let package = self.package(configuration)?;
        let query = self.query(configuration, String::new(), None)?;
        if let Some(resource) = logical_document(configuration)? {
            return analyze_nwscript_document(&query, Some(resource.as_str()));
        }
        package.lock_index()?.analyze_document(&query, cancellation)
    }

    /// Returns every indexed source and Outline symbol in one package.
    pub fn project_documents(
        &self,
        configuration: &PackageConfiguration,
        cancellation: Option<&CancellationToken>,
    ) -> Result<NwScriptProjectDocuments, LanguageOperationError> {
        let package = self.package(configuration)?;
        let query = self.query(configuration, String::new(), None)?;
        let (mut documents, warnings) = package
            .lock_index()?
            .project_documents(&query, cancellation)?;
        if let Some(cancellation) = cancellation
            && let Some(plan) = self.strict_event_plan(configuration, cancellation)?
        {
            for (path, symbols) in &mut documents {
                decorate_event_outline(symbols, &plan, path);
            }
        }
        Ok((documents, warnings))
    }

    /// Returns the strict event plan cached for the current package generation.
    pub fn strict_event_plan(
        &self,
        configuration: &PackageConfiguration,
        cancellation: &CancellationToken,
    ) -> Result<Option<Arc<EventProjectPlan>>, LanguageOperationError> {
        let package = self.package(configuration)?;
        drop(package.lock_index()?);
        self.synchronize_event_package(&package, cancellation)?;
        let mut event = lock(&package.event, "package event cache")?;
        let Some(state) = event.as_mut() else {
            return Ok(None);
        };
        if let Some(cached) = &state.strict {
            return Ok(cached.clone()?);
        }
        let result =
            generate_event_dispatcher_from_session_with_cancellation(&state.session, cancellation)
                .map(|generated| generated.map(|dispatcher| Arc::new(dispatcher.plan)))
                .map_err(|diagnostic| diagnostic.message);
        state.strict = Some(result.clone());
        Ok(result?)
    }

    /// Produces an editor-only event recovery analysis for the current open
    /// document version. Recovered plans are never returned to build callers.
    pub fn event_recovery(
        &self,
        configuration: &PackageConfiguration,
        cursor_offset: usize,
        cancellation: &CancellationToken,
    ) -> Result<Option<EventCompletionAnalysis>, LanguageOperationError> {
        let package = self.package(configuration)?;
        drop(package.lock_index()?);
        self.synchronize_event_package(&package, cancellation)?;
        let document = self
            .document(&configuration.source_path)?
            .ok_or_else(|| "event recovery requires an open synchronized document".to_string())?;
        let source_id = PhysicalSourceId::resolve(&document.path)
            .map_err(|error| format!("failed to resolve recovery document: {error}"))?;
        let mut event = lock(&package.event, "package event cache")?;
        let Some(state) = event.as_mut() else {
            return Ok(None);
        };
        let analysis = analyze_event_completion_from_session_with_cancellation(
            &state.session,
            &configuration.source_path,
            cursor_offset,
            cancellation,
        )
        .map_err(|diagnostic| diagnostic.message)?;
        if let Some(plan) = analysis
            .as_ref()
            .and_then(|analysis| analysis.plan.as_ref())
        {
            state.recovery.insert(
                source_id,
                RecoveryPlanState {
                    version:         document.version,
                    attribute_start: analysis
                        .as_ref()
                        .map_or(cursor_offset, |analysis| analysis.context.attribute_start),
                    plan:            Arc::new(plan.clone()),
                },
            );
        }
        Ok(analysis)
    }

    /// Returns an exact recovery plan only when the synchronized document
    /// version and active attribute match.
    pub fn cached_event_recovery_plan(
        &self,
        configuration: &PackageConfiguration,
        document_version: i64,
        attribute_start: usize,
    ) -> Result<Option<Arc<EventProjectPlan>>, LanguageOperationError> {
        let package = self.package(configuration)?;
        let source = PhysicalSourceId::resolve(&configuration.source_path)
            .map_err(|error| format!("failed to resolve recovery source: {error}"))?;
        let event = lock(&package.event, "package event cache")?;
        Ok(event
            .as_ref()
            .and_then(|state| state.recovery.get(&source))
            .filter(|cached| {
                cached.version == document_version && cached.attribute_start == attribute_start
            })
            .map(|cached| Arc::clone(&cached.plan)))
    }

    fn current_event_recovery_plan(
        &self,
        configuration: &PackageConfiguration,
    ) -> crate::LanguageResult<Option<Arc<EventProjectPlan>>> {
        let Some(document) = self.document(&configuration.source_path)? else {
            return Ok(None);
        };
        let package = self.package(configuration)?;
        let source = PhysicalSourceId::resolve(&document.path)
            .map_err(|error| format!("failed to resolve recovery source: {error}"))?;
        let event = lock(&package.event, "package event cache")?;
        Ok(event
            .as_ref()
            .and_then(|state| state.recovery.get(&source))
            .filter(|cached| cached.version == document.version)
            .map(|cached| Arc::clone(&cached.plan)))
    }

    fn package(
        &self,
        configuration: &PackageConfiguration,
    ) -> crate::LanguageResult<Arc<PackageSession>> {
        let root = discover_package_root(
            &configuration.source_path,
            configuration.project_root.as_deref(),
        )?;
        let identity = PhysicalSourceId::resolve(&root).map_err(|error| {
            format!("failed to resolve package root {}: {error}", root.display())
        })?;
        let mut effective_configuration = configuration.clone();
        let manifest_path = root.join(nwnrs_nwpkg::PROJECT_MANIFEST_FILENAME);
        let manifest = self
            .manifest_source_if_present(&manifest_path)?
            .map(|manifest_source| {
                let manifest = toml::from_str::<nwnrs_nwpkg::ProjectManifest>(&manifest_source)
                    .map_err(|error| {
                        format!("failed to parse {}: {error}", manifest_path.display())
                    })?;
                manifest
                    .validate(&root)
                    .map_err(|error| error.to_string())?;
                Ok::<_, String>(manifest)
            })
            .transpose()?;
        if let Some(manifest) = &manifest {
            effective_configuration.language = manifest.project.language.as_str().to_string();
        }
        let package = {
            let mut packages = lock(&self.state.packages, "package registry")?;
            Arc::clone(packages.entry(identity).or_insert_with(|| {
                Arc::new(PackageSession::new(root, effective_configuration.clone()))
            }))
        };
        package.update_configuration(&effective_configuration)?;
        let dependency_roots = if let Some(manifest) = &manifest {
            let dependency_configuration = dependency_configuration(
                effective_configuration.git_executable.clone(),
                effective_configuration.package_cache.clone(),
            );
            nwnrs_nwpkg::resolve_include_dependencies_from_manifest_with_configuration(
                &package.root,
                manifest,
                &dependency_configuration,
                &CancellationToken::new(),
            )
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(|dependency| dependency.source_root)
            .collect()
        } else {
            Vec::new()
        };
        *lock(&package.dependency_roots, "package dependency roots")? = dependency_roots;
        package.last_used.store(
            self.state.access_clock.fetch_add(1, Ordering::Relaxed),
            Ordering::Relaxed,
        );
        self.enforce_global_analysis_budget(&package)?;
        Ok(package)
    }

    fn enforce_global_analysis_budget(
        &self,
        protected: &Arc<PackageSession>,
    ) -> crate::LanguageResult<()> {
        const MAX_LANGUAGE_SERVICE_ANALYSIS_BYTES: usize = 512 * 1024 * 1024;

        let packages = lock(&self.state.packages, "package registry")?
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let mut retained = Vec::with_capacity(packages.len());
        let mut total = 0_usize;
        for package in packages {
            let weight = package.lock_index()?.estimated_memory_bytes();
            total = total.saturating_add(weight);
            retained.push((package.last_used.load(Ordering::Relaxed), weight, package));
        }
        if total <= MAX_LANGUAGE_SERVICE_ANALYSIS_BYTES {
            return Ok(());
        }
        retained.sort_by_key(|(last_used, _, _)| *last_used);
        for (_, weight, package) in retained {
            if total <= MAX_LANGUAGE_SERVICE_ANALYSIS_BYTES {
                break;
            }
            if Arc::ptr_eq(&package, protected) {
                continue;
            }
            package.lock_index()?.clear();
            *lock(&package.event, "package event cache")? = None;
            total = total.saturating_sub(weight);
        }
        Ok(())
    }

    fn query(
        &self,
        configuration: &PackageConfiguration,
        symbol: String,
        qualifier: Option<String>,
    ) -> crate::LanguageResult<NwScriptDefinitionQuery> {
        let package = self.package(configuration)?;
        let overlays = self.package_overlays(&package)?;
        let effective = lock(&package.configuration, "package configuration")?.clone();
        Ok(effective.query(
            configuration.source_path.clone(),
            symbol,
            qualifier,
            overlays,
        ))
    }

    fn package_overlays(
        &self,
        package: &PackageSession,
    ) -> Result<BTreeMap<PathBuf, Vec<u8>>, String> {
        let documents = lock(&self.state.documents, "document registry")?;
        let mut overlays = BTreeMap::new();
        let mut selected = HashMap::<PhysicalSourceId, &RegisteredDocument>::new();
        for registered in documents.documents.values() {
            let document = &registered.document;
            if package_contains_source(package, &document.path)? {
                match selected.get(&registered.physical) {
                    Some(existing) if existing.sequence >= registered.sequence => {}
                    _ => {
                        selected.insert(registered.physical.clone(), registered);
                    }
                }
            }
        }
        for registered in selected.into_values() {
            let document = &registered.document;
            let bytes = document.bytes.as_ref().ok_or_else(|| {
                format!(
                    "open NWScript document {} is not representable as Windows-1252",
                    document.path.display()
                )
            })?;
            overlays.insert(document.path.clone(), bytes.to_vec());
        }
        Ok(overlays)
    }

    fn synchronize_event_package(
        &self,
        package: &PackageSession,
        cancellation: &CancellationToken,
    ) -> Result<(), String> {
        let overlays = self.package_overlays(package)?;
        let mut event = lock(&package.event, "package event cache")?;
        if let Some(state) = event.as_mut() {
            if state
                .session
                .refresh_if_changed(&overlays, cancellation)
                .map_err(|error| error.to_string())?
            {
                state.generation = state.generation.wrapping_add(1);
                state.strict = None;
                state.recovery.clear();
            }
            return Ok(());
        }
        let configuration = lock(&package.configuration, "package configuration")?.clone();
        let dependency_configuration =
            dependency_configuration(configuration.git_executable, configuration.package_cache);
        let manifest_path = package.root.join(nwnrs_nwpkg::PROJECT_MANIFEST_FILENAME);
        let Some(manifest_source) = self.manifest_source_if_present(&manifest_path)? else {
            return Ok(());
        };
        let manifest = toml::from_str::<nwnrs_nwpkg::ProjectManifest>(&manifest_source)
            .map_err(|error| format!("failed to parse {}: {error}", manifest_path.display()))?;
        let resolved = ResolvedPackageSession::open_with_manifest_and_workspace_overlays_and_dependency_configuration(
            &package.root,
            &manifest,
            &overlays,
            cancellation,
            &dependency_configuration,
        )
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("{} is not an nwpkg package", package.root.display()))?;
        *event = Some(EventPackageState {
            generation: 0,
            session:    resolved,
            strict:     None,
            recovery:   HashMap::new(),
        });
        Ok(())
    }
}

fn utf16_position_to_byte(source: &str, target_line: u32, target_character: u32) -> usize {
    let mut line = 0_u32;
    let mut line_start = 0_usize;
    for (index, character) in source.char_indices() {
        if line == target_line {
            line_start = index;
            break;
        }
        if character == '\n' {
            line = line.saturating_add(1);
            line_start = index.saturating_add(1);
        }
    }
    if line < target_line {
        return source.len();
    }
    let mut utf16 = 0_u32;
    for (relative, character) in source[line_start..].char_indices() {
        if character == '\n' || utf16 >= target_character {
            return line_start.saturating_add(relative);
        }
        let width = u32::try_from(character.len_utf16()).unwrap_or(u32::MAX);
        let next = utf16.saturating_add(width);
        if next > target_character {
            return line_start.saturating_add(relative);
        }
        utf16 = next;
    }
    source.len()
}

fn dependency_configuration(
    git_executable: Option<PathBuf>,
    cache_root: Option<PathBuf>,
) -> DependencyResolverConfiguration {
    let mut configuration = DependencyResolverConfiguration::from_environment();
    if let Some(git_executable) = git_executable {
        configuration.git_executable = git_executable;
    }
    if let Some(cache_root) = cache_root {
        configuration.cache_root = cache_root;
    }
    configuration
}

fn insert_document_version(
    documents: &mut DocumentRegistry,
    identity: PhysicalSourceId,
    version: i64,
    bytes: Option<Arc<[u8]>>,
) -> crate::LanguageResult<()> {
    if documents
        .documents
        .get(identity.path())
        .is_some_and(|document| document.document.version > version)
    {
        return Err(LanguageOperationError::new(
            "language.document.staleVersion",
            format!("document version {version} is older than the synchronized version"),
        )
        .with_path(identity.path()));
    }
    documents.next_sequence = documents.next_sequence.wrapping_add(1);
    documents.documents.insert(
        identity.path().to_path_buf(),
        RegisteredDocument {
            document: OpenDocument {
                path: identity.path().to_path_buf(),
                version,
                bytes,
            },
            physical: identity,
            sequence: documents.next_sequence,
        },
    );
    Ok(())
}

fn language_check_report(report: BatchCompileReport) -> LanguageCheckReport {
    let mut diagnostics = Vec::new();
    for entry in &report.entries {
        if entry.status != BatchCompileStatus::Error {
            continue;
        }
        diagnostics.extend(
            entry
                .diagnostic
                .iter()
                .chain(&entry.additional_diagnostics)
                .map(language_compiler_diagnostic),
        );
    }
    LanguageCheckReport {
        diagnostics,
        summary: LanguageCheckSummary {
            compiled: report.successes,
            skipped:  report.skips,
            failed:   report.errors,
        },
    }
}

fn language_compiler_diagnostic(diagnostic: &CompilerDiagnostic) -> LanguageDiagnostic {
    let source = match &diagnostic.source {
        Some(nwnrs_nwscript::ScriptSourceOrigin::Physical(path)) => LanguageSource::Physical(
            PhysicalSourceId::resolve(path)
                .map_or_else(|_| path.clone(), |identity| identity.path().to_path_buf()),
        ),
        Some(nwnrs_nwscript::ScriptSourceOrigin::Virtual(uri)) => {
            LanguageSource::Virtual(uri.clone())
        }
        None => LanguageSource::Virtual(format!(
            "nwnrs-source:{}",
            diagnostic.file.as_deref().unwrap_or("<compiler>")
        )),
    };
    let bytes = diagnostic.source_bytes.as_deref().unwrap_or_default();
    let range = match (diagnostic.start_offset, diagnostic.end_offset) {
        (Some(start), Some(end)) => nwscript_utf16_range(bytes, start, end),
        _ => fallback_diagnostic_range(diagnostic),
    };
    LanguageDiagnostic {
        severity: LanguageDiagnosticSeverity::Error,
        code: diagnostic.code.map(LanguageDiagnosticCode::Numeric),
        message: diagnostic.message.clone(),
        source,
        range,
    }
}

fn fallback_diagnostic_range(diagnostic: &CompilerDiagnostic) -> LanguageRange {
    let line = diagnostic.start_line.unwrap_or(1).saturating_sub(1);
    let character = diagnostic.start_column.unwrap_or(1).saturating_sub(1);
    let end_line = diagnostic.end_line.unwrap_or(line + 1).saturating_sub(1);
    let end_character = diagnostic
        .end_column
        .unwrap_or(character.saturating_add(2))
        .saturating_sub(1);
    LanguageRange {
        start: crate::LanguagePosition {
            line:      u32::try_from(line).unwrap_or(u32::MAX),
            character: u32::try_from(character).unwrap_or(u32::MAX),
        },
        end:   crate::LanguagePosition {
            line:      u32::try_from(end_line).unwrap_or(u32::MAX),
            character: u32::try_from(end_character.max(character + 1)).unwrap_or(u32::MAX),
        },
    }
}

fn discover_package_root(path: &Path, fallback: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(root) = nwnrs_nwpkg::find_project_root(path).map_err(|error| error.to_string())? {
        return Ok(root);
    }
    let selected = fallback.unwrap_or(path);
    nwnrs_nwscript::canonicalize_with_missing_suffix(selected).map_err(|error| {
        format!(
            "failed to resolve non-package source root {}: {error}",
            selected.display()
        )
    })
}

fn package_contains_source(package: &PackageSession, path: &Path) -> Result<bool, String> {
    if path.starts_with(&package.root) {
        return Ok(true);
    }
    if lock(&package.dependency_roots, "package dependency roots")?
        .iter()
        .any(|root| path.starts_with(root))
    {
        return Ok(true);
    }
    let event = lock(&package.event, "package event cache")?;
    let Some(state) = event.as_ref() else {
        return Ok(false);
    };
    if path.starts_with(state.session.source_root()) {
        return Ok(true);
    }
    Ok(state
        .session
        .dependencies()
        .iter()
        .any(|dependency| path.starts_with(&dependency.source_root)))
}

fn decorate_event_outline(
    symbols: &mut [NwScriptDocumentSymbol],
    plan: &EventProjectPlan,
    source_path: &Path,
) {
    for symbol in symbols {
        if symbol.kind != crate::NwScriptDocumentSymbolKind::Function {
            continue;
        }
        let routes = plan
            .registrations
            .iter()
            .filter(|registration| {
                registration.handler == symbol.name
                    && nwnrs_nwscript::physical_sources_equal(
                        &registration.handler_range.path,
                        source_path,
                    )
                    && registration.handler_range.start_line == symbol.selection_range.start_line
                    && registration.handler_range.start_column
                        == symbol.selection_range.start_column
            })
            .map(event_outline_route)
            .collect::<Vec<_>>();
        if routes.is_empty() {
            continue;
        }
        let signature = symbol.detail.take().unwrap_or_else(|| symbol.name.clone());
        symbol.detail = Some(format!("event: {} · {signature}", routes.join(", ")));
    }
}

fn event_outline_route(registration: &EventRegistration) -> String {
    let mut clauses = Vec::new();
    if !registration.before.is_empty() {
        clauses.push(format!(
            "before({})",
            registration
                .before
                .iter()
                .map(|relation| relation.target.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !registration.after.is_empty() {
        clauses.push(format!(
            "after({})",
            registration
                .after
                .iter()
                .map(|relation| relation.target.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let mut route = format!("{}::{}", registration.subject, registration.phase);
    if !clauses.is_empty() {
        route.push('(');
        route.push_str(&clauses.join(", "));
        route.push(')');
    }
    route
}

fn lock<'a, T>(mutex: &'a Mutex<T>, label: &str) -> Result<MutexGuard<'a, T>, String> {
    mutex
        .lock()
        .map_err(|error| format!("{label} is poisoned: {error}"))
}

fn scan_workspace_manifests(
    root: &Path,
    cancellation: Option<&CancellationToken>,
) -> Result<BTreeSet<PathBuf>, String> {
    const MAX_WORKSPACE_DIRECTORIES: usize = 100_000;
    const MAX_WORKSPACE_ENTRIES: usize = 1_000_000;
    const MAX_WORKSPACE_PACKAGES: usize = 20_000;
    const MAX_WORKSPACE_DEPTH: usize = 512;

    let mut manifests = BTreeSet::new();
    let mut visited = HashSet::new();
    let mut pending = vec![(root.to_path_buf(), 0_usize)];
    let mut entries_seen = 0_usize;
    while let Some((directory, depth)) = pending.pop() {
        if let Some(cancellation) = cancellation {
            cancellation.check().map_err(|error| error.to_string())?;
        }
        if depth > MAX_WORKSPACE_DEPTH {
            return Err(format!(
                "workspace package discovery exceeded maximum depth {MAX_WORKSPACE_DEPTH} below {}",
                root.display()
            ));
        }
        let identity = PhysicalSourceId::resolve(&directory).map_err(|error| {
            format!(
                "failed to resolve workspace directory {}: {error}",
                directory.display()
            )
        })?;
        if !visited.insert(identity) {
            continue;
        }
        if visited.len() > MAX_WORKSPACE_DIRECTORIES {
            return Err(format!(
                "workspace package discovery exceeded {MAX_WORKSPACE_DIRECTORIES} directories \
                 below {}",
                root.display()
            ));
        }
        let entries = fs::read_dir(&directory).map_err(|error| {
            format!("failed to read workspace {}: {error}", directory.display())
        })?;
        let mut children = Vec::new();
        for entry in entries {
            entries_seen = entries_seen.saturating_add(1);
            if entries_seen > MAX_WORKSPACE_ENTRIES {
                return Err(format!(
                    "workspace package discovery exceeded {MAX_WORKSPACE_ENTRIES} entries below {}",
                    root.display()
                ));
            }
            let entry = entry.map_err(|error| {
                format!(
                    "failed to read an entry below {}: {error}",
                    directory.display()
                )
            })?;
            let file_type = entry.file_type().map_err(|error| {
                format!("failed to inspect {}: {error}", entry.path().display())
            })?;
            if file_type.is_dir() {
                if !matches!(
                    entry.file_name().to_str(),
                    Some(".git" | "node_modules" | "target")
                ) {
                    children.push(entry.path());
                }
            } else if file_type.is_file()
                && entry.file_name() == nwnrs_nwpkg::PROJECT_MANIFEST_FILENAME
            {
                manifests.insert(
                    nwnrs_nwscript::canonicalize_with_missing_suffix(&entry.path())
                        .map_err(|error| error.to_string())?,
                );
                if manifests.len() > MAX_WORKSPACE_PACKAGES {
                    return Err(format!(
                        "workspace package discovery exceeded {MAX_WORKSPACE_PACKAGES} packages \
                         below {}",
                        root.display()
                    ));
                }
            }
        }
        children.sort();
        pending.extend(
            children
                .into_iter()
                .rev()
                .map(|child| (child, depth.saturating_add(1))),
        );
    }
    Ok(manifests)
}

#[cfg(test)]
mod tests {
    use std::{fs, sync::Arc};

    use nwnrs_nwscript::{CancellationToken, CompilerDiagnostic, ScriptSourceOrigin};
    use tempfile::tempdir;

    use super::{
        LanguageDiagnosticCode, LanguageService, LanguageSource, PackageConfiguration,
        PhysicalSourceId, language_compiler_diagnostic, lock,
    };

    #[test]
    fn document_versions_never_move_backwards() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("main.nss");
        fs::write(&path, "void main() {}\n").expect("write source");
        let service = LanguageService::new();
        service
            .open_document(&path, 2, Arc::<[u8]>::from(b"void main() {}\n".as_slice()))
            .expect("open document");
        assert!(
            service
                .update_document(&path, 1, Arc::<[u8]>::from(b"void main() { }\n".as_slice()),)
                .is_err()
        );
    }

    #[test]
    fn unsaved_manifest_overlay_drives_analysis_but_cannot_fetch() {
        let directory = tempdir().expect("temporary directory");
        let manifest_path = directory.path().join("nwpkg.toml");
        let saved = "[project]\nname = \"saved\"\nkind = \"include\"\n\n[source]\npath = \"src\"\n";
        let overlay = "[project]\nname = \"overlay\"\nkind = \"include\"\n\n[source]\npath = \
                       \"src\"\n\n[dependencies]\nmissing = { path = \"../missing\" }\n";
        fs::create_dir_all(directory.path().join("src")).expect("create source directory");
        fs::write(&manifest_path, saved).expect("write saved manifest");
        let service = LanguageService::new();
        service
            .open_document(&manifest_path, 1, Arc::<[u8]>::from(overlay.as_bytes()))
            .expect("open manifest overlay");

        let diagnostics = service
            .manifest_diagnostics(&manifest_path, None, Some(directory.path().join("cache")))
            .expect("analyze manifest overlay");
        assert!(diagnostics.iter().any(|diagnostic| {
            matches!(
                diagnostic.code.as_ref(),
                Some(LanguageDiagnosticCode::Named(code))
                    if code == "nwpkg.dependency.missing"
            )
        }));
        let error = service
            .fetch_package_dependencies(
                &manifest_path,
                None,
                Some(directory.path().join("cache")),
                None,
                &nwnrs_nwscript::CancellationToken::new(),
                None,
            )
            .expect_err("unsaved manifest must not produce a lock");
        assert!(error.message.contains("unsaved changes"));
        assert!(!directory.path().join("nwpkg.lock").exists());
    }

    #[test]
    fn dependency_update_previews_are_one_per_package_and_invalidate_with_the_package() {
        let directory = tempfile::tempdir().expect("create dependency preview fixture");
        let app = directory.path().join("app");
        let shared = directory.path().join("shared");
        fs::create_dir_all(app.join("src")).expect("create app source");
        fs::create_dir_all(shared.join("src")).expect("create dependency source");
        fs::write(
            app.join("nwpkg.toml"),
            "[project]\nname = \"app\"\nkind = \"include\"\n\n[source]\npath = \
             \"src\"\n\n[dependencies]\nshared = { path = \"../shared\" }\n",
        )
        .expect("write app manifest");
        fs::write(
            shared.join("nwpkg.toml"),
            "[project]\nname = \"shared\"\nkind = \"include\"\n\n[source]\npath = \"src\"\n",
        )
        .expect("write dependency manifest");
        let service = LanguageService::new();
        let manifest = app.join("nwpkg.toml");
        for _ in 0..2 {
            service
                .plan_package_dependency_update(
                    &manifest,
                    None,
                    Some(directory.path().join("cache")),
                    None,
                    &CancellationToken::new(),
                    None,
                )
                .expect("create dependency preview");
        }
        assert_eq!(
            lock(&service.state.dependency_plans, "dependency preview test")
                .expect("lock dependency previews")
                .len(),
            1
        );
        service
            .invalidate_source(&manifest)
            .expect("invalidate package manifest");
        assert!(
            lock(&service.state.dependency_plans, "dependency preview test")
                .expect("lock dependency previews")
                .is_empty()
        );

        service
            .plan_package_dependency_update(
                &manifest,
                None,
                Some(directory.path().join("cache")),
                None,
                &CancellationToken::new(),
                None,
            )
            .expect("recreate dependency preview");
        service.release_package(&app).expect("release package");
        assert!(
            lock(&service.state.dependency_plans, "dependency preview test")
                .expect("lock dependency previews")
                .is_empty()
        );
    }

    #[test]
    fn encoding_invalid_versions_never_fall_back_to_stale_or_disk_source() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("main.nss");
        fs::write(
            directory.path().join("nwpkg.toml"),
            "[project]\nname = \"encoding\"\nkind = \"include\"\n[source]\npath = \".\"\n",
        )
        .expect("write manifest");
        fs::write(&path, "void DiskVersion() {}\n").expect("write source");
        let service = LanguageService::new();
        service
            .open_document(
                &path,
                1,
                Arc::<[u8]>::from(b"void OverlayVersion() {}\n".as_slice()),
            )
            .expect("open overlay");
        let configuration = PackageConfiguration {
            source_path: path.clone(),
            ..PackageConfiguration::default()
        };
        let symbols = service
            .document_symbols(&configuration, None)
            .expect("overlay symbols");
        assert!(symbols.iter().any(|symbol| symbol.name == "OverlayVersion"));
        assert!(!symbols.iter().any(|symbol| symbol.name == "DiskVersion"));

        service
            .invalidate_document(&path, 2)
            .expect("invalidate encoding");
        let error = service
            .document_symbols(&configuration, None)
            .expect_err("invalid encoding must suspend package analysis");
        assert!(error.message.contains("not representable as Windows-1252"));

        service
            .update_document(
                &path,
                3,
                Arc::<[u8]>::from(b"void RecoveredVersion() {}\n".as_slice()),
            )
            .expect("restore valid overlay");
        let symbols = service
            .document_symbols(&configuration, None)
            .expect("recovered symbols");
        assert!(
            symbols
                .iter()
                .any(|symbol| symbol.name == "RecoveredVersion")
        );
        assert!(!symbols.iter().any(|symbol| symbol.name == "OverlayVersion"));
    }

    #[test]
    fn atomic_file_replacement_cannot_leave_a_stale_overlay() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("main.nss");
        fs::write(
            directory.path().join("nwpkg.toml"),
            "[project]\nname = \"atomic\"\nkind = \"include\"\n[source]\npath = \".\"\n",
        )
        .expect("write manifest");
        fs::write(&path, "void DiskVersion() {}\n").expect("write source");
        let service = LanguageService::new();
        service
            .open_document(
                &path,
                1,
                Arc::<[u8]>::from(b"void OldOverlay() {}\n".as_slice()),
            )
            .expect("open old overlay");

        // Model editors which save through a temporary file followed by an
        // atomic replacement: the stable path remains the same while its
        // physical file identity changes.
        fs::remove_file(&path).expect("remove old inode");
        fs::write(&path, "void SavedVersion() {}\n").expect("replace source");
        service
            .update_document(
                &path,
                2,
                Arc::<[u8]>::from(b"void NewOverlay() {}\n".as_slice()),
            )
            .expect("update replacement overlay");

        assert_eq!(
            service
                .state
                .documents
                .lock()
                .expect("document registry")
                .documents
                .len(),
            1
        );
        let symbols = service
            .document_symbols(
                &PackageConfiguration {
                    source_path: path.clone(),
                    ..PackageConfiguration::default()
                },
                None,
            )
            .expect("replacement symbols");
        assert!(symbols.iter().any(|symbol| symbol.name == "NewOverlay"));
        assert!(!symbols.iter().any(|symbol| symbol.name == "OldOverlay"));
        assert!(!symbols.iter().any(|symbol| symbol.name == "SavedVersion"));

        service.close_document(&path).expect("close replacement");
        assert!(
            service
                .state
                .documents
                .lock()
                .expect("document registry")
                .documents
                .is_empty()
        );
    }

    #[test]
    fn creating_a_previously_missing_document_replaces_its_registry_entry() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("new_file.nss");
        let service = LanguageService::new();
        service
            .open_document(
                &path,
                1,
                Arc::<[u8]>::from(b"void BeforeSave() {}\n".as_slice()),
            )
            .expect("open missing document");
        fs::write(&path, "void Saved() {}\n").expect("create source");
        service
            .update_document(
                &path,
                2,
                Arc::<[u8]>::from(b"void AfterSave() {}\n".as_slice()),
            )
            .expect("update created document");
        let documents = service.state.documents.lock().expect("document registry");
        assert_eq!(documents.documents.len(), 1);
        assert_eq!(
            documents
                .documents
                .values()
                .next()
                .expect("registered document")
                .document
                .version,
            2
        );
    }

    #[test]
    fn deepest_manifest_owns_nested_source() {
        let directory = tempdir().expect("temporary directory");
        let outer = directory.path();
        let inner = outer.join("nested");
        fs::create_dir_all(&inner).expect("create nested package");
        for (root, name) in [(outer, "outer"), (inner.as_path(), "inner")] {
            fs::write(
                root.join("nwpkg.toml"),
                format!(
                    "[project]\nname = \"{name}\"\nkind = \"include\"\n[source]\npath = \".\"\n"
                ),
            )
            .expect("write manifest");
        }
        let source = inner.join("main.nss");
        fs::write(&source, "void main() {}\n").expect("write source");
        let service = LanguageService::new();
        let symbols = service
            .document_symbols(
                &PackageConfiguration {
                    source_path: source,
                    ..PackageConfiguration::default()
                },
                None,
            )
            .expect("nested package symbols");
        assert!(symbols.iter().any(|symbol| symbol.name == "main"));
    }

    #[test]
    fn physical_aliases_share_one_document_overlay() {
        let directory = tempdir().expect("temporary directory");
        let source = directory.path().join("main.nss");
        let alias = directory.path().join("alias.nss");
        fs::write(&source, "void main() {}\n").expect("write source");
        fs::hard_link(&source, &alias).expect("create hard link");
        let service = LanguageService::new();
        service
            .open_document(
                &source,
                1,
                Arc::<[u8]>::from(b"void changed() {}\n".as_slice()),
            )
            .expect("open source");
        assert_eq!(
            service
                .document(&alias)
                .expect("alias lookup")
                .expect("overlay")
                .version,
            1
        );
    }

    #[test]
    fn workspace_package_inventory_updates_from_exact_manifest_events() {
        let directory = tempdir().expect("temporary directory");
        let first = directory.path().join("first");
        let second = directory.path().join("second");
        fs::create_dir_all(&first).expect("create first package");
        fs::create_dir_all(&second).expect("create second package");
        let first_manifest = first.join("nwpkg.toml");
        let second_manifest = second.join("nwpkg.toml");
        fs::write(
            &first_manifest,
            "[project]\nname = \"first\"\nkind = \"include\"\n[source]\npath = \".\"\n",
        )
        .expect("write first manifest");
        let service = LanguageService::new();
        service
            .add_workspace(directory.path())
            .expect("add workspace");
        assert_eq!(
            service
                .workspace_packages(None)
                .expect("initial packages")
                .len(),
            1
        );

        fs::write(
            &second_manifest,
            "[project]\nname = \"second\"\nkind = \"include\"\n[source]\npath = \".\"\n",
        )
        .expect("write second manifest");
        service
            .invalidate_source(&second_manifest)
            .expect("notify manifest creation");
        assert_eq!(
            service
                .workspace_packages(None)
                .expect("created package")
                .len(),
            2
        );

        fs::remove_file(&first_manifest).expect("remove first manifest");
        service
            .invalidate_source(&first_manifest)
            .expect("notify manifest deletion");
        let packages = service.workspace_packages(None).expect("remaining package");
        assert_eq!(packages.len(), 1);
        assert_eq!(
            packages.first().expect("remaining package").root,
            fs::canonicalize(second).expect("canonical second package")
        );
    }

    #[test]
    fn compiler_diagnostics_use_the_compiler_origin_without_basename_discovery() {
        let directory = tempdir().expect("temporary directory");
        let first = directory.path().join("first").join("shared.nss");
        let second = directory.path().join("second").join("shared.nss");
        fs::create_dir_all(first.parent().expect("first parent")).expect("create first");
        fs::create_dir_all(second.parent().expect("second parent")).expect("create second");
        fs::write(&first, "void first() {}\n").expect("write first");
        fs::write(&second, "void second() {}\n").expect("write second");
        let diagnostic = CompilerDiagnostic {
            code:         Some(-1),
            message:      "test".to_string(),
            file:         Some("shared.nss".to_string()),
            source:       Some(ScriptSourceOrigin::Physical(second.clone())),
            start_line:   Some(1),
            start_column: Some(1),
            end_line:     Some(1),
            end_column:   Some(2),
            start_offset: Some(0),
            end_offset:   Some(1),
            source_bytes: Some(b"void second() {}\n".to_vec()),
        };
        assert_eq!(
            language_compiler_diagnostic(&diagnostic).source,
            LanguageSource::Physical(
                PhysicalSourceId::resolve(&second)
                    .expect("second identity")
                    .path()
                    .to_path_buf()
            )
        );
    }
}
