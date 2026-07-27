use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use nwnrs_nwpkg::{
    DependencyLock, DependencyOperation, DependencyOperationKind, DependencyProgressReporter,
    DependencyResolverConfiguration, DependencySpec, GitSelector, LockedDependencySource,
    ProjectManifest, clean_dependency_cache_with_cancellation, normalize_git_source,
    plan_dependency_operation_with_progress, read_dependency_lock,
    resolve_dependency_operation_with_progress, write_registered_dependency_lock,
};
use nwnrs_nwscript::CancellationToken;
use nwnrs_types::checksums;

use crate::LanguageOperationError;

const MAX_DEPENDENCY_SOURCE_DEPTH: usize = 512;
const MAX_DEPENDENCY_SOURCE_ENTRIES: usize = 1_000_000;

/// Dependency source/status shown by language clients.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageDependencyStatus {
    /// Deterministic graph route.
    pub route:         String,
    /// Alias in the declaring package.
    pub name:          String,
    /// Parent route, or `None` for a direct dependency.
    pub parent:        Option<String>,
    /// `path` or `git`.
    pub source_kind:   String,
    /// Human-readable selector.
    pub selector:      Option<String>,
    /// Exact locked Git commit.
    pub commit:        Option<String>,
    /// `live`, `cached`, `missing`, `stale`, or `invalid`.
    pub state:         String,
    /// Materialized package root when available.
    pub root:          Option<PathBuf>,
    /// Materialized package manifest when available.
    pub manifest_path: Option<PathBuf>,
    /// Diagnostic detail for unavailable dependencies.
    pub message:       Option<String>,
}

/// One commit transition in an explicit dependency update.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageDependencyChange {
    /// Deterministic graph route.
    pub route:  String,
    /// Previously locked commit.
    pub before: Option<String>,
    /// Newly resolved commit.
    pub after:  String,
}

/// Preview returned before an update lock is applied.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageDependencyPlan {
    /// Content-derived token accepted by the apply operation.
    pub token:   String,
    /// Exact commit changes.
    pub changes: Vec<PackageDependencyChange>,
    /// Resulting dependency statuses.
    pub status:  Vec<PackageDependencyStatus>,
}

/// One read-only source exposed from a resolved dependency package.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageDependencySource {
    /// Dependency graph route.
    pub route:         String,
    /// Portable path relative to the dependency package root.
    pub relative_path: String,
    /// Exact physical cache or local path.
    pub physical_path: PathBuf,
    /// `utf8` for the manifest or `windows1252` for NWScript.
    pub encoding:      String,
}

/// Exact bytes and encoding for a read-only dependency source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageDependencySourceContents {
    /// Source bytes without an intermediate string conversion.
    pub bytes:    Vec<u8>,
    /// `utf8` or `windows1252`.
    pub encoding: String,
}

#[derive(Clone)]
pub(crate) struct PendingDependencyPlan {
    pub project_root:         PathBuf,
    pub manifest_sha256:      String,
    pub previous_lock_sha256: Option<String>,
    pub operation:            DependencyOperation,
    pub configuration:        DependencyResolverConfiguration,
}

pub(crate) fn dependency_status(
    manifest_path: &Path,
    source: &str,
    configuration: &DependencyResolverConfiguration,
) -> Result<Vec<PackageDependencyStatus>, LanguageOperationError> {
    let manifest = toml::from_str::<ProjectManifest>(source).map_err(|error| error.to_string())?;
    let root = manifest_path
        .parent()
        .ok_or_else(|| format!("manifest has no package root: {}", manifest_path.display()))?;
    let lock = read_dependency_lock(root).map_err(String::from)?;
    let mut output = Vec::new();
    let mut emitted = BTreeSet::new();
    for (name, dependency) in &manifest.dependencies {
        let route = name.clone();
        push_manifest_status(
            &mut output,
            &mut emitted,
            root,
            &route,
            name,
            dependency,
            lock.as_ref(),
            configuration,
        );
    }
    if let Some(lock) = lock {
        for (route, package) in &lock.packages {
            if emitted.contains(route) {
                continue;
            }
            push_locked_status(
                &mut output,
                route,
                &package.name,
                Some(parent_route(route)),
                &package.source,
                configuration,
                locked_package_root(&lock, route, root, configuration),
                None,
            );
        }
    }
    output.sort_by(|left, right| left.route.cmp(&right.route));
    Ok(output)
}

pub(crate) fn execute_fetch(
    project_root: &Path,
    configuration: &DependencyResolverConfiguration,
    selected: Option<&str>,
    cancellation: &CancellationToken,
    progress: Option<&DependencyProgressReporter>,
) -> Result<Vec<PackageDependencyStatus>, LanguageOperationError> {
    resolve_dependency_operation_with_progress(
        project_root,
        configuration,
        DependencyOperationKind::Fetch,
        selected,
        cancellation,
        progress,
    )
    .map_err(LanguageOperationError::from)?;
    let manifest_path = project_root.join(nwnrs_nwpkg::PROJECT_MANIFEST_FILENAME);
    let source = fs::read_to_string(&manifest_path)
        .map_err(|error| format!("failed to read {}: {error}", manifest_path.display()))?;
    dependency_status(&manifest_path, &source, configuration)
}

pub(crate) fn create_update_plan(
    project_root: &Path,
    configuration: &DependencyResolverConfiguration,
    selected: Option<&str>,
    cancellation: &CancellationToken,
    progress: Option<&DependencyProgressReporter>,
) -> Result<(PackageDependencyPlan, PendingDependencyPlan), LanguageOperationError> {
    let operation = plan_dependency_operation_with_progress(
        project_root,
        configuration,
        DependencyOperationKind::Update,
        selected,
        cancellation,
        progress,
    )
    .map_err(LanguageOperationError::from)?;
    let manifest_path = project_root.join(nwnrs_nwpkg::PROJECT_MANIFEST_FILENAME);
    let source = fs::read_to_string(&manifest_path)
        .map_err(|error| format!("failed to read {}: {error}", manifest_path.display()))?;
    let manifest_sha256 = checksums::sha256_digest(source.as_bytes()).to_string();
    let lock_path = project_root.join(nwnrs_nwpkg::PACKAGE_LOCK_FILENAME);
    let previous_lock_sha256 = fs::read(&lock_path)
        .ok()
        .map(|bytes| checksums::sha256_digest(bytes).to_string());
    let encoded = toml::to_string(&operation.lock)
        .map_err(|error| format!("failed to fingerprint dependency plan: {error}"))?;
    let token = checksums::sha256_digest(
        format!(
            "{}\0{}\0{}",
            project_root.display(),
            manifest_sha256,
            encoded
        )
        .as_bytes(),
    )
    .to_string();
    let changes = operation
        .changes
        .iter()
        .map(|(route, (before, after))| PackageDependencyChange {
            route:  route.clone(),
            before: before.clone(),
            after:  after.clone(),
        })
        .collect();
    let status = statuses_from_lock(&operation.lock, project_root, configuration);
    let pending = PendingDependencyPlan {
        project_root: project_root.to_path_buf(),
        manifest_sha256,
        previous_lock_sha256,
        operation,
        configuration: configuration.clone(),
    };
    Ok((
        PackageDependencyPlan {
            token,
            changes,
            status,
        },
        pending,
    ))
}

pub(crate) fn apply_update_plan(plan: PendingDependencyPlan) -> Result<(), LanguageOperationError> {
    let manifest_path = plan
        .project_root
        .join(nwnrs_nwpkg::PROJECT_MANIFEST_FILENAME);
    let current_manifest = fs::read(&manifest_path)
        .map_err(|error| format!("failed to read {}: {error}", manifest_path.display()))?;
    let current_manifest_sha256 = checksums::sha256_digest(current_manifest).to_string();
    if current_manifest_sha256 != plan.manifest_sha256 {
        return Err(LanguageOperationError::new(
            "nwpkg.preview.manifestChanged",
            "nwpkg.toml changed after the dependency update preview; create a new preview",
        ));
    }
    let lock_path = plan.project_root.join(nwnrs_nwpkg::PACKAGE_LOCK_FILENAME);
    let current_lock_sha256 = fs::read(&lock_path)
        .ok()
        .map(|bytes| checksums::sha256_digest(bytes).to_string());
    if current_lock_sha256 != plan.previous_lock_sha256 {
        return Err(LanguageOperationError::new(
            "nwpkg.preview.lockChanged",
            "nwpkg.lock changed after the dependency update preview; create a new preview",
        ));
    }
    write_registered_dependency_lock(
        &plan.configuration,
        &plan.project_root,
        plan.operation.lock(),
    )
    .map_err(LanguageOperationError::from)
}

pub(crate) fn clean_cache(
    configuration: &DependencyResolverConfiguration,
    cancellation: &CancellationToken,
) -> Result<Vec<PathBuf>, LanguageOperationError> {
    clean_dependency_cache_with_cancellation(configuration, cancellation)
        .map_err(LanguageOperationError::from)
}

pub(crate) fn list_dependency_sources(
    manifest_path: &Path,
    source: &str,
    route: &str,
    configuration: &DependencyResolverConfiguration,
    cancellation: &CancellationToken,
) -> Result<Vec<PackageDependencySource>, LanguageOperationError> {
    cancellation.check()?;
    let status = dependency_status(manifest_path, source, configuration)?;
    let dependency = status
        .into_iter()
        .find(|dependency| dependency.route == route)
        .ok_or_else(|| format!("dependency route {route:?} is not resolved"))?;
    let root = dependency
        .root
        .ok_or_else(|| format!("dependency route {route:?} is not materialized"))?;
    let root = fs::canonicalize(&root).map_err(|error| {
        format!(
            "failed to resolve dependency root {}: {error}",
            root.display()
        )
    })?;
    let manifest_path = root.join(nwnrs_nwpkg::PROJECT_MANIFEST_FILENAME);
    let manifest = nwnrs_nwpkg::read_project_manifest(&manifest_path)
        .map_err(String::from)?
        .ok_or_else(|| format!("dependency route {route:?} has no manifest"))?;
    let source_root = fs::canonicalize(root.join(&manifest.source.path)).map_err(|error| {
        format!(
            "failed to resolve dependency source {}: {error}",
            root.join(&manifest.source.path).display()
        )
    })?;
    if !source_root.starts_with(&root) {
        return Err(format!("dependency route {route:?} source escapes its package root").into());
    }
    let mut output = vec![PackageDependencySource {
        route:         route.to_string(),
        relative_path: nwnrs_nwpkg::PROJECT_MANIFEST_FILENAME.to_string(),
        physical_path: manifest_path,
        encoding:      "utf8".to_string(),
    }];
    let mut pending = vec![(source_root, 0_usize)];
    let mut visited = 0_usize;
    while let Some((directory, depth)) = pending.pop() {
        cancellation.check()?;
        if depth > MAX_DEPENDENCY_SOURCE_DEPTH {
            return Err(LanguageOperationError::new(
                "nwpkg.source.depthLimit",
                format!(
                    "dependency source traversal exceeded maximum depth \
                     {MAX_DEPENDENCY_SOURCE_DEPTH}"
                ),
            )
            .with_path(directory));
        }
        let mut entries = fs::read_dir(&directory)
            .map_err(|error| format!("failed to read {}: {error}", directory.display()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("failed to read {}: {error}", directory.display()))?;
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries.into_iter().rev() {
            cancellation.check()?;
            visited = visited.saturating_add(1);
            if visited > MAX_DEPENDENCY_SOURCE_ENTRIES {
                return Err(LanguageOperationError::new(
                    "nwpkg.source.entryLimit",
                    format!(
                        "dependency source traversal exceeded maximum of \
                         {MAX_DEPENDENCY_SOURCE_ENTRIES} entries"
                    ),
                )
                .with_path(directory));
            }
            let file_type = entry.file_type().map_err(|error| {
                format!("failed to inspect {}: {error}", entry.path().display())
            })?;
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                pending.push((entry.path(), depth.saturating_add(1)));
                continue;
            }
            if !file_type.is_file()
                || !entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("nss"))
            {
                continue;
            }
            let physical_path = fs::canonicalize(entry.path()).map_err(|error| {
                format!("failed to resolve {}: {error}", entry.path().display())
            })?;
            if !physical_path.starts_with(&root) {
                return Err(format!(
                    "dependency source {} escapes package root {}",
                    physical_path.display(),
                    root.display()
                )
                .into());
            }
            let relative = physical_path.strip_prefix(&root).map_err(|error| {
                format!(
                    "failed to relativize {} against {}: {error}",
                    physical_path.display(),
                    root.display()
                )
            })?;
            let relative_path = relative
                .to_str()
                .ok_or_else(|| {
                    LanguageOperationError::new(
                        "nwpkg.source.invalidPathEncoding",
                        "dependency source paths must be valid UTF-8",
                    )
                    .with_path(&physical_path)
                })?
                .replace('\\', "/");
            output.push(PackageDependencySource {
                route: route.to_string(),
                relative_path,
                physical_path,
                encoding: "windows1252".to_string(),
            });
        }
    }
    output.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(output)
}

pub(crate) fn read_dependency_source(
    manifest_path: &Path,
    source: &str,
    route: &str,
    relative_path: &str,
    configuration: &DependencyResolverConfiguration,
) -> Result<PackageDependencySourceContents, LanguageOperationError> {
    let available = list_dependency_sources(
        manifest_path,
        source,
        route,
        configuration,
        &CancellationToken::new(),
    )?;
    let selected = available
        .into_iter()
        .find(|candidate| candidate.relative_path == relative_path)
        .ok_or_else(|| {
            format!("dependency source {relative_path:?} is not exposed by route {route:?}")
        })?;
    let bytes = fs::read(&selected.physical_path).map_err(|error| {
        format!(
            "failed to read dependency source {}: {error}",
            selected.physical_path.display()
        )
    })?;
    Ok(PackageDependencySourceContents {
        bytes,
        encoding: selected.encoding,
    })
}

fn push_manifest_status(
    output: &mut Vec<PackageDependencyStatus>,
    emitted: &mut BTreeSet<String>,
    declaring_root: &Path,
    route: &str,
    name: &str,
    dependency: &DependencySpec,
    lock: Option<&DependencyLock>,
    configuration: &DependencyResolverConfiguration,
) {
    emitted.insert(route.to_string());
    match dependency {
        DependencySpec::Path(dependency) => {
            let root = declaring_root.join(&dependency.path);
            let manifest_path = root.join(nwnrs_nwpkg::PROJECT_MANIFEST_FILENAME);
            let available = manifest_path.is_file();
            output.push(PackageDependencyStatus {
                route:         route.to_string(),
                name:          name.to_string(),
                parent:        route_parent(route),
                source_kind:   "path".to_string(),
                selector:      Some(nwnrs_nwscript::physical_path_display(&dependency.path)),
                commit:        None,
                state:         if available { "live" } else { "missing" }.to_string(),
                root:          Some(root),
                manifest_path: available.then_some(manifest_path),
                message:       (!available)
                    .then(|| "local dependency manifest is unavailable".to_string()),
            });
        }
        DependencySpec::Git(dependency) => {
            let locked = lock.and_then(|lock| lock.packages.get(route));
            if let Some(locked) = locked {
                let expected = GitSelector::from_dependency(dependency);
                let expected_git = normalize_git_source(&dependency.git, declaring_root)
                    .unwrap_or_else(|_| dependency.git.clone());
                let stale = !matches!(
                    &locked.source,
                    LockedDependencySource::Git {
                        git,
                        selector,
                        subdir,
                        ..
                    } if git == &expected_git
                        && selector == &expected
                        && subdir == &dependency.subdir
                );
                push_locked_status(
                    output,
                    route,
                    name,
                    route_parent(route),
                    &locked.source,
                    configuration,
                    None,
                    stale.then(|| {
                        "manifest selector differs from the locked Git dependency".to_string()
                    }),
                );
            } else {
                output.push(PackageDependencyStatus {
                    route:         route.to_string(),
                    name:          name.to_string(),
                    parent:        route_parent(route),
                    source_kind:   "git".to_string(),
                    selector:      Some(selector_label(&GitSelector::from_dependency(dependency))),
                    commit:        None,
                    state:         "missing".to_string(),
                    root:          None,
                    manifest_path: None,
                    message:       Some(
                        "Git dependency is not locked; fetch dependencies explicitly".to_string(),
                    ),
                });
            }
        }
    }
}

fn statuses_from_lock(
    lock: &DependencyLock,
    project_root: &Path,
    configuration: &DependencyResolverConfiguration,
) -> Vec<PackageDependencyStatus> {
    let mut output = lock
        .packages
        .iter()
        .map(|(route, package)| {
            let mut status = Vec::new();
            push_locked_status(
                &mut status,
                route,
                &package.name,
                route_parent(route),
                &package.source,
                configuration,
                locked_package_root(lock, route, project_root, configuration),
                None,
            );
            status
                .into_iter()
                .next()
                .unwrap_or_else(|| PackageDependencyStatus {
                    route:         route.clone(),
                    name:          package.name.clone(),
                    parent:        route_parent(route),
                    source_kind:   "invalid".to_string(),
                    selector:      None,
                    commit:        None,
                    state:         "invalid".to_string(),
                    root:          None,
                    manifest_path: None,
                    message:       Some("dependency status could not be constructed".to_string()),
                })
        })
        .collect::<Vec<_>>();
    output.sort_by(|left, right| left.route.cmp(&right.route));
    output
}

fn push_locked_status(
    output: &mut Vec<PackageDependencyStatus>,
    route: &str,
    name: &str,
    parent: Option<String>,
    source: &LockedDependencySource,
    configuration: &DependencyResolverConfiguration,
    resolved_root: Option<PathBuf>,
    stale: Option<String>,
) {
    match source {
        LockedDependencySource::Path {
            path,
        } => {
            let manifest_path = resolved_root
                .as_ref()
                .map(|root| root.join(nwnrs_nwpkg::PROJECT_MANIFEST_FILENAME));
            let available = manifest_path.as_ref().is_some_and(|path| path.is_file());
            output.push(PackageDependencyStatus {
                route: route.to_string(),
                name: name.to_string(),
                parent,
                source_kind: "path".to_string(),
                selector: Some(nwnrs_nwscript::physical_path_display(path)),
                commit: None,
                state: if stale.is_some() {
                    "stale"
                } else if available {
                    "live"
                } else {
                    "missing"
                }
                .to_string(),
                root: available.then_some(resolved_root).flatten(),
                manifest_path: available.then_some(manifest_path).flatten(),
                message: stale.or_else(|| {
                    (!available).then(|| "local dependency manifest is unavailable".to_string())
                }),
            });
        }
        LockedDependencySource::Git {
            selector,
            commit,
            subdir,
            cache,
            ..
        } => {
            let repository = configuration.cache_root.join(cache).join("repository");
            let root = subdir
                .as_ref()
                .map_or_else(|| repository.clone(), |subdir| repository.join(subdir));
            let manifest = root.join(nwnrs_nwpkg::PROJECT_MANIFEST_FILENAME);
            let cached = manifest.is_file();
            output.push(PackageDependencyStatus {
                route: route.to_string(),
                name: name.to_string(),
                parent,
                source_kind: "git".to_string(),
                selector: Some(selector_label(selector)),
                commit: Some(commit.clone()),
                state: if stale.is_some() {
                    "stale"
                } else if cached {
                    "cached"
                } else {
                    "missing"
                }
                .to_string(),
                root: cached.then_some(root),
                manifest_path: cached.then_some(manifest),
                message: stale.or_else(|| {
                    (!cached).then(|| {
                        "locked Git dependency is not present in the local cache".to_string()
                    })
                }),
            });
        }
    }
}

fn locked_package_root(
    lock: &DependencyLock,
    route: &str,
    project_root: &Path,
    configuration: &DependencyResolverConfiguration,
) -> Option<PathBuf> {
    let mut root = project_root.to_path_buf();
    let mut current_route = String::new();
    for segment in route.split('/') {
        if !current_route.is_empty() {
            current_route.push('/');
        }
        current_route.push_str(segment);
        let package = lock.packages.get(&current_route)?;
        root = match &package.source {
            LockedDependencySource::Path {
                path,
            } => root.join(path),
            LockedDependencySource::Git {
                subdir,
                cache,
                ..
            } => {
                let repository = configuration.cache_root.join(cache).join("repository");
                subdir
                    .as_ref()
                    .map_or(repository.clone(), |subdir| repository.join(subdir))
            }
        };
    }
    Some(root)
}

fn selector_label(selector: &GitSelector) -> String {
    match selector {
        GitSelector::Rev(value) => format!("rev {value}"),
        GitSelector::Tag(value) => format!("tag {value}"),
        GitSelector::Branch(value) => format!("branch {value}"),
        GitSelector::Default => "default branch".to_string(),
    }
}

fn parent_route(route: &str) -> String {
    route
        .rsplit_once('/')
        .map_or_else(String::new, |(parent, _)| parent.to_string())
}

fn route_parent(route: &str) -> Option<String> {
    route.rsplit_once('/').map(|(parent, _)| parent.to_string())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use nwnrs_nwpkg::DependencyResolverConfiguration;
    use nwnrs_nwscript::CancellationToken;
    use tempfile::TempDir;

    use super::{dependency_status, execute_fetch};

    #[test]
    fn transitive_local_dependency_status_retains_its_physical_root() {
        let fixture = TempDir::new().expect("create dependency fixture");
        let app = fixture.path().join("app");
        let direct = fixture.path().join("direct");
        let transitive = fixture.path().join("transitive");
        for root in [&app, &direct, &transitive] {
            fs::create_dir_all(root.join("src")).expect("create package source");
        }
        fs::write(
            transitive.join("nwpkg.toml"),
            "[project]\nname = \"transitive\"\nkind = \"include\"\n\n[source]\npath = \"src\"\n",
        )
        .expect("write transitive manifest");
        fs::write(transitive.join("src/transitive.nss"), "int Transitive;\n")
            .expect("write transitive source");
        fs::write(
            direct.join("nwpkg.toml"),
            "[project]\nname = \"direct\"\nkind = \"include\"\n\n[source]\npath = \
             \"src\"\n\n[dependencies]\ntransitive = { path = \"../transitive\" }\n",
        )
        .expect("write direct manifest");
        fs::write(direct.join("src/direct.nss"), "int Direct;\n").expect("write direct source");
        let manifest = "[project]\nname = \"app\"\nkind = \"include\"\n\n[source]\npath = \
                        \"src\"\n\n[dependencies]\ndirect = { path = \"../direct\" }\n";
        fs::write(app.join("nwpkg.toml"), manifest).expect("write app manifest");
        fs::write(app.join("src/main.nss"), "#include \"transitive\"\n").expect("write app source");
        let configuration = DependencyResolverConfiguration {
            git_executable: "git".into(),
            cache_root:     fixture.path().join("cache"),
        };

        execute_fetch(&app, &configuration, None, &CancellationToken::new(), None)
            .expect("lock local dependency graph");
        let status = dependency_status(&app.join("nwpkg.toml"), manifest, &configuration)
            .expect("read dependency status");
        let transitive_status = status
            .iter()
            .find(|dependency| dependency.route == "direct/transitive")
            .expect("transitive status");
        assert_eq!(
            transitive_status
                .root
                .as_ref()
                .and_then(|root| fs::canonicalize(root).ok()),
            fs::canonicalize(&transitive).ok()
        );
        assert_eq!(transitive_status.state, "live");
    }
}
