use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

use nwnrs_nwscript::CancellationToken;
use nwnrs_types::checksums;

use crate::{
    DependencySpec, GitDependency, NwpkgError, NwpkgErrorKind, PROJECT_MANIFEST_FILENAME,
    ProjectKind, ProjectManifest, fs::collect_nss_paths_with_cancellation,
    read_project_manifest_with_cancellation,
};

mod git;
mod lockfile;
mod model;

pub use git::{
    DependencyOperation, DependencyOperationKind, DependencyProgressEvent,
    DependencyProgressReporter, DependencyResolverConfiguration, clean_dependency_cache,
    clean_dependency_cache_with_cancellation, normalize_git_source, plan_dependency_operation,
    plan_dependency_operation_with_progress, register_dependency_lock,
    resolve_dependency_operation, resolve_dependency_operation_with_progress,
    write_registered_dependency_lock, write_registered_dependency_lock_with_cancellation,
};
pub use lockfile::{read_dependency_lock, write_dependency_lock};
pub use model::{
    DEPENDENCY_LOCK_FORMAT, DependencyLock, GitSelector, LockedDependency, LockedDependencySource,
};

/// One resolved include-library dependency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedIncludeDependency {
    /// Dependency name from the declaring manifest.
    pub name:         String,
    /// Canonical directory containing the dependency manifest.
    pub package_root: PathBuf,
    /// Canonical directory exposed to the NWScript include resolver.
    pub source_root:  PathBuf,
}

/// Resolves all transitive local include dependencies for the nearest project.
///
/// Paths are relative to the manifest that declares them. Dependency cycles,
/// missing manifests, non-include projects, escaped source roots, and
/// case-insensitive include-name collisions are rejected.
pub fn resolve_include_dependencies(
    input: &Path,
) -> Result<Vec<ResolvedIncludeDependency>, NwpkgError> {
    resolve_include_dependencies_with_cancellation(input, &CancellationToken::new())
}

/// Resolves all transitive local include dependencies and remains interruptible
/// throughout manifest and source traversal.
///
/// # Errors
///
/// Returns the same errors as [`resolve_include_dependencies`], plus a typed
/// cancellation error when cancellation is requested.
pub fn resolve_include_dependencies_with_cancellation(
    input: &Path,
    cancellation: &CancellationToken,
) -> Result<Vec<ResolvedIncludeDependency>, NwpkgError> {
    check_cancelled(cancellation)?;
    let Some(project_root) = crate::find_project_root_with_cancellation(input, cancellation)?
    else {
        return Ok(Vec::new());
    };
    let mut state = ResolverState::new(
        cancellation,
        DependencyResolverConfiguration::from_environment(),
    );
    state.visit_manifest(&project_root)?;
    validate_include_names(&state.resolved, cancellation)?;
    Ok(state.resolved)
}

/// Resolves the complete dependency graph with an explicit Git/cache
/// configuration and no network access.
///
/// Git packages must already be pinned and materialized by an explicit fetch
/// operation.
///
/// # Errors
///
/// Returns an error for invalid manifests, stale or missing locks, unavailable
/// cache entries, dependency graph violations, or cancellation.
pub fn resolve_include_dependencies_with_configuration(
    input: &Path,
    configuration: &DependencyResolverConfiguration,
    cancellation: &CancellationToken,
) -> Result<Vec<ResolvedIncludeDependency>, NwpkgError> {
    check_cancelled(cancellation)?;
    let Some(project_root) = crate::find_project_root_with_cancellation(input, cancellation)?
    else {
        return Ok(Vec::new());
    };
    let mut state = ResolverState::new(cancellation, configuration.clone());
    state.visit_manifest(&project_root)?;
    validate_include_names(&state.resolved, cancellation)?;
    Ok(state.resolved)
}

/// Resolves include dependencies from an already parsed root manifest.
///
/// This is the canonical entry point for editor buffers whose current
/// `nwpkg.toml` contents may not have been saved yet. Dependency manifests are
/// still read from their declared package directories.
///
/// # Errors
///
/// Returns an error when the supplied manifest, source layout, dependency
/// graph, or include namespace is invalid.
pub fn resolve_include_dependencies_from_manifest(
    project_root: &Path,
    manifest: &ProjectManifest,
) -> Result<Vec<ResolvedIncludeDependency>, NwpkgError> {
    resolve_include_dependencies_from_manifest_with_cancellation(
        project_root,
        manifest,
        &CancellationToken::new(),
    )
}

/// Resolves include dependencies from an already parsed root manifest with
/// cooperative cancellation.
///
/// # Errors
///
/// Returns the same errors as
/// [`resolve_include_dependencies_from_manifest`], plus a typed cancellation
/// error.
pub fn resolve_include_dependencies_from_manifest_with_cancellation(
    project_root: &Path,
    manifest: &ProjectManifest,
    cancellation: &CancellationToken,
) -> Result<Vec<ResolvedIncludeDependency>, NwpkgError> {
    check_cancelled(cancellation)?;
    manifest.validate(project_root)?;
    let mut state = ResolverState::new(
        cancellation,
        DependencyResolverConfiguration::from_environment(),
    );
    state.visit_root_manifest(project_root, manifest)?;
    validate_include_names(&state.resolved, cancellation)?;
    Ok(state.resolved)
}

/// Resolves dependencies from an already parsed root manifest with explicit
/// offline Git/cache configuration.
///
/// # Errors
///
/// Returns an error for invalid manifest contents, stale or missing Git pins,
/// dependency graph violations, or cancellation.
pub fn resolve_include_dependencies_from_manifest_with_configuration(
    project_root: &Path,
    manifest: &ProjectManifest,
    configuration: &DependencyResolverConfiguration,
    cancellation: &CancellationToken,
) -> Result<Vec<ResolvedIncludeDependency>, NwpkgError> {
    check_cancelled(cancellation)?;
    manifest.validate(project_root)?;
    let mut state = ResolverState::new(cancellation, configuration.clone());
    state.visit_root_manifest(project_root, manifest)?;
    validate_include_names(&state.resolved, cancellation)?;
    Ok(state.resolved)
}

struct ResolverState<'a> {
    emitted:      HashSet<PathBuf>,
    resolved:     Vec<ResolvedIncludeDependency>,
    cancellation: &'a CancellationToken,
    lock:         Option<DependencyLock>,
    cache_root:   PathBuf,
}

impl<'a> ResolverState<'a> {
    fn new(
        cancellation: &'a CancellationToken,
        configuration: DependencyResolverConfiguration,
    ) -> Self {
        Self {
            emitted: HashSet::new(),
            resolved: Vec::new(),
            cancellation,
            lock: None,
            cache_root: configuration.cache_root,
        }
    }

    fn visit_manifest(&mut self, project_root: &Path) -> Result<(), NwpkgError> {
        self.visit_manifest_impl(project_root, None)
    }

    fn visit_root_manifest(
        &mut self,
        project_root: &Path,
        manifest: &ProjectManifest,
    ) -> Result<(), NwpkgError> {
        self.visit_manifest_impl(project_root, Some(manifest.clone()))
    }

    fn visit_manifest_impl(
        &mut self,
        project_root: &Path,
        root_manifest: Option<ProjectManifest>,
    ) -> Result<(), NwpkgError> {
        const MAX_DEPENDENCY_PACKAGES: usize = 100_000;
        const MAX_DEPENDENCY_DEPTH: usize = 4_096;

        enum Action {
            Enter {
                root:     PathBuf,
                edge:     Option<ResolvedIncludeDependency>,
                manifest: Option<ProjectManifest>,
                route:    String,
            },
            Exit {
                root: PathBuf,
                edge: Option<ResolvedIncludeDependency>,
            },
        }

        let project_root = canonical_directory(project_root, "project")?;
        let manifest_is_overlay = root_manifest.is_some();
        self.lock = read_dependency_lock(&project_root)?;
        if !manifest_is_overlay && let Some(lock) = &self.lock {
            let manifest_path = project_root.join(PROJECT_MANIFEST_FILENAME);
            let digest = fs::read(&manifest_path)
                .map(|bytes| checksums::sha256_digest(bytes).to_string())
                .map_err(|error| {
                    NwpkgError::at(
                        NwpkgErrorKind::Io,
                        &manifest_path,
                        format!("failed to fingerprint project manifest: {error}"),
                    )
                })?;
            if digest != lock.manifest_sha256 {
                return Err(NwpkgError::at(
                    NwpkgErrorKind::Validation,
                    &manifest_path,
                    "nwpkg.toml differs from nwpkg.lock; run `nwnrs nwpkg fetch` explicitly",
                ));
            }
        }
        let mut actions = vec![Action::Enter {
            root:     project_root,
            edge:     None,
            manifest: root_manifest,
            route:    String::new(),
        }];
        let mut active = Vec::<PathBuf>::new();
        let mut visited = HashSet::<PathBuf>::new();
        while let Some(action) = actions.pop() {
            check_cancelled(self.cancellation)?;
            match action {
                Action::Enter {
                    root,
                    edge,
                    manifest,
                    route,
                } => {
                    if visited.contains(&root) {
                        if let Some(edge) = edge
                            && self.emitted.insert(edge.package_root.clone())
                        {
                            self.resolved.push(edge);
                        }
                        continue;
                    }
                    if let Some(index) = active.iter().position(|candidate| candidate == &root) {
                        let cycle_paths = active.get(index..).ok_or_else(|| {
                            NwpkgError::at(
                                NwpkgErrorKind::Validation,
                                &root,
                                "dependency cycle index exceeded the active traversal stack",
                            )
                        })?;
                        let mut cycle = cycle_paths
                            .iter()
                            .map(|path| path.display().to_string())
                            .collect::<Vec<_>>();
                        cycle.push(root.display().to_string());
                        return Err(NwpkgError::at(
                            NwpkgErrorKind::Validation,
                            &root,
                            format!("nwpkg dependency cycle: {}", cycle.join(" -> ")),
                        ));
                    }
                    if active.len() >= MAX_DEPENDENCY_DEPTH {
                        return Err(NwpkgError::at(
                            NwpkgErrorKind::Validation,
                            &root,
                            format!(
                                "nwpkg dependency graph exceeded maximum depth \
                                 {MAX_DEPENDENCY_DEPTH}"
                            ),
                        ));
                    }
                    if visited.len().saturating_add(active.len()) >= MAX_DEPENDENCY_PACKAGES {
                        return Err(NwpkgError::at(
                            NwpkgErrorKind::Validation,
                            &root,
                            format!(
                                "nwpkg dependency graph exceeded maximum of \
                                 {MAX_DEPENDENCY_PACKAGES} packages"
                            ),
                        ));
                    }
                    let manifest = match manifest {
                        Some(manifest) => manifest,
                        None => read_required_manifest(&root, self.cancellation)?,
                    };
                    active.push(root.clone());
                    actions.push(Action::Exit {
                        root: root.clone(),
                        edge,
                    });
                    let mut dependencies = Vec::new();
                    for (name, dependency) in manifest.dependencies {
                        check_cancelled(self.cancellation)?;
                        let dependency_route = if route.is_empty() {
                            name.clone()
                        } else {
                            format!("{route}/{name}")
                        };
                        let dependency_root = match dependency {
                            DependencySpec::Path(dependency) => {
                                if dependency.path.as_os_str().is_empty() {
                                    return Err(NwpkgError::at(
                                        NwpkgErrorKind::Validation,
                                        root.join(PROJECT_MANIFEST_FILENAME),
                                        format!("dependency {name:?} has an empty path"),
                                    ));
                                }
                                canonical_directory(
                                    &root.join(&dependency.path),
                                    &format!("dependency {name:?}"),
                                )?
                            }
                            DependencySpec::Git(dependency) => locked_git_dependency_root(
                                self.lock.as_ref(),
                                &self.cache_root,
                                &dependency_route,
                                &dependency,
                                &root,
                                self.cancellation,
                            )?,
                        };
                        let dependency_manifest =
                            read_required_manifest(&dependency_root, self.cancellation)?;
                        if dependency_manifest.project.kind != ProjectKind::Include {
                            return Err(NwpkgError::at(
                                NwpkgErrorKind::Validation,
                                &dependency_root,
                                format!(
                                    "dependency {name:?} is kind {:?}; only include packages may \
                                     be dependencies",
                                    dependency_manifest.project.kind
                                ),
                            ));
                        }
                        let source_root = canonical_directory(
                            &dependency_root.join(&dependency_manifest.source.path),
                            &format!("source for dependency {name:?}"),
                        )?;
                        if !source_root.starts_with(&dependency_root) {
                            return Err(NwpkgError::at(
                                NwpkgErrorKind::Validation,
                                &source_root,
                                format!(
                                    "dependency {name:?} source escapes package root {}",
                                    dependency_root.display()
                                ),
                            ));
                        }
                        dependencies.push(Action::Enter {
                            root:     dependency_root.clone(),
                            edge:     Some(ResolvedIncludeDependency {
                                name,
                                package_root: dependency_root,
                                source_root,
                            }),
                            manifest: None,
                            route:    dependency_route,
                        });
                    }
                    actions.extend(dependencies.into_iter().rev());
                }
                Action::Exit {
                    root,
                    edge,
                } => {
                    let popped = active.pop();
                    if popped.as_ref() != Some(&root) {
                        return Err(NwpkgError::at(
                            NwpkgErrorKind::Validation,
                            &root,
                            "dependency traversal stack became inconsistent",
                        ));
                    }
                    visited.insert(root);
                    if let Some(edge) = edge
                        && self.emitted.insert(edge.package_root.clone())
                    {
                        self.resolved.push(edge);
                    }
                }
            }
        }
        Ok(())
    }
}

fn read_required_manifest(
    root: &Path,
    cancellation: &CancellationToken,
) -> Result<ProjectManifest, NwpkgError> {
    read_project_manifest_with_cancellation(root, cancellation)?.ok_or_else(|| {
        NwpkgError::at(
            NwpkgErrorKind::NotFound,
            root.join(PROJECT_MANIFEST_FILENAME),
            "nwpkg dependency does not contain a manifest",
        )
    })
}

fn locked_git_dependency_root(
    lock: Option<&DependencyLock>,
    cache_root: &Path,
    route: &str,
    dependency: &GitDependency,
    declaring_root: &Path,
    cancellation: &CancellationToken,
) -> Result<PathBuf, NwpkgError> {
    let lock_path = declaring_root.join(crate::PACKAGE_LOCK_FILENAME);
    let lock = lock.ok_or_else(|| {
        NwpkgError::at_manifest_field(
            NwpkgErrorKind::NotFound,
            &lock_path,
            &[
                "dependencies",
                route.split('/').next_back().unwrap_or(route),
            ],
            format!("Git dependency {route:?} is not locked; run `nwnrs nwpkg fetch` explicitly"),
        )
    })?;
    let package = lock.packages.get(route).ok_or_else(|| {
        NwpkgError::at(
            NwpkgErrorKind::Validation,
            &lock_path,
            format!("Git dependency {route:?} is missing from nwpkg.lock; run `nwnrs nwpkg fetch`"),
        )
    })?;
    let LockedDependencySource::Git {
        git,
        selector,
        commit,
        subdir,
        cache,
    } = &package.source
    else {
        return Err(NwpkgError::at(
            NwpkgErrorKind::Validation,
            &lock_path,
            format!("dependency {route:?} changed source kind; refresh nwpkg.lock"),
        ));
    };
    let normalized_git = normalize_git_source(&dependency.git, declaring_root)?;
    if git != &normalized_git
        || selector != &GitSelector::from_dependency(dependency)
        || subdir != &dependency.subdir
    {
        return Err(NwpkgError::at(
            NwpkgErrorKind::Validation,
            &lock_path,
            format!(
                "Git dependency {route:?} no longer matches nwpkg.toml; run `nwnrs nwpkg fetch`"
            ),
        ));
    }
    if !git::validate_locked_cache_entry(
        cache_root,
        git,
        commit,
        subdir.as_deref(),
        cache,
        &package.manifest_sha256,
        cancellation,
    )? {
        return Err(NwpkgError::at(
            NwpkgErrorKind::Validation,
            cache_root.join(cache),
            format!(
                "locked Git dependency {route:?} cache failed integrity validation; run `nwnrs \
                 nwpkg fetch`"
            ),
        ));
    }
    let repository = cache_root.join(cache).join("repository");
    let root = subdir
        .as_ref()
        .map_or_else(|| repository.clone(), |subdir| repository.join(subdir));
    if !root.join(PROJECT_MANIFEST_FILENAME).is_file() {
        return Err(NwpkgError::at(
            NwpkgErrorKind::NotFound,
            &root,
            format!("locked Git dependency {route:?} is not cached; run `nwnrs nwpkg fetch`"),
        ));
    }
    canonical_directory(&root, &format!("locked Git dependency {route:?}"))
}

fn canonical_directory(path: &Path, label: &str) -> Result<PathBuf, NwpkgError> {
    let canonical = fs::canonicalize(path).map_err(|error| {
        NwpkgError::at(
            NwpkgErrorKind::Io,
            path,
            format!("failed to resolve {label}: {error}"),
        )
    })?;
    let metadata = fs::metadata(&canonical).map_err(|error| {
        NwpkgError::at(
            NwpkgErrorKind::Io,
            &canonical,
            format!("failed to inspect {label}: {error}"),
        )
    })?;
    if !metadata.is_dir() {
        return Err(NwpkgError::at(
            NwpkgErrorKind::Validation,
            &canonical,
            format!("{label} is not a directory"),
        ));
    }
    Ok(canonical)
}

fn validate_include_names(
    dependencies: &[ResolvedIncludeDependency],
    cancellation: &CancellationToken,
) -> Result<(), NwpkgError> {
    let mut names = BTreeMap::<String, PathBuf>::new();
    for dependency in dependencies {
        for path in collect_nss_paths_with_cancellation(&dependency.source_root, cancellation)? {
            check_cancelled(cancellation)?;
            let relative = path
                .strip_prefix(&dependency.source_root)
                .map_err(|error| {
                    NwpkgError::at(
                        NwpkgErrorKind::Validation,
                        &path,
                        format!(
                            "failed to relativize include against {}: {error}",
                            dependency.source_root.display()
                        ),
                    )
                })?;
            let name = relative
                .to_str()
                .ok_or_else(|| {
                    NwpkgError::at(
                        NwpkgErrorKind::Validation,
                        &path,
                        "logical include paths must be valid UTF-8",
                    )
                })?
                .replace('\\', "/")
                .to_ascii_lowercase();
            if let Some(existing) = names.insert(name.clone(), path.clone()) {
                return Err(NwpkgError::at(
                    NwpkgErrorKind::Validation,
                    &path,
                    format!(
                        "include dependency collision for {name:?}: {} and {}",
                        existing.display(),
                        path.display()
                    ),
                ));
            }
        }
    }
    Ok(())
}

fn check_cancelled(cancellation: &CancellationToken) -> Result<(), NwpkgError> {
    cancellation
        .check()
        .map_err(|error| NwpkgError::new(NwpkgErrorKind::Cancelled, error.to_string()))
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    use nwnrs_nwscript::CancellationToken;

    use super::{
        resolve_include_dependencies, resolve_include_dependencies_from_manifest,
        resolve_include_dependencies_with_cancellation,
    };
    use crate::{DependencySpec, PathDependency, ProjectKind, ProjectManifest};

    #[test]
    fn dependency_resolution_honors_cancellation_before_discovery() {
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        let error = resolve_include_dependencies_with_cancellation(
            Path::new("path-that-must-not-be-read"),
            &cancellation,
        )
        .expect_err("cancel dependency resolution");
        assert_eq!(error.kind(), crate::NwpkgErrorKind::Cancelled);
    }

    fn unique_test_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock drift before unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("nwnrs-nwpkg-{prefix}-{nanos}"))
    }

    fn write_manifest(root: &Path, name: &str, kind: &str, dependencies: &str) {
        fs::create_dir_all(root).expect("create package root");
        fs::write(
            root.join("nwpkg.toml"),
            format!(
                "[project]\nname = {name:?}\nkind = {kind:?}\n\n[source]\npath = \
                 \".\"\n{dependencies}"
            ),
        )
        .expect("write manifest");
    }

    #[test]
    fn resolves_transitive_local_include_dependencies() {
        let root = unique_test_dir("dependency-resolution");
        let app = root.join("app");
        let direct = root.join("direct");
        let transitive = root.join("transitive");
        write_manifest(
            &app,
            "app",
            "mod",
            "\n[dependencies]\ndirect = { path = \"../direct\" }\n",
        );
        write_manifest(
            &direct,
            "direct",
            "include",
            "\n[dependencies]\ntransitive = { path = \"../transitive\" }\n",
        );
        write_manifest(&transitive, "transitive", "include", "");
        fs::write(direct.join("direct.nss"), "int Direct();\n").expect("write direct include");
        fs::write(transitive.join("transitive.nss"), "int Transitive();\n")
            .expect("write transitive include");

        let resolved = resolve_include_dependencies(&app).expect("resolve dependencies");
        assert_eq!(resolved.len(), 2);
        assert_eq!(
            resolved.first().map(|item| item.name.as_str()),
            Some("transitive")
        );
        assert_eq!(
            resolved.get(1).map(|item| item.name.as_str()),
            Some("direct")
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn resolves_dependencies_from_an_unsaved_root_manifest() {
        let root = unique_test_dir("unsaved-root-manifest");
        let app = root.join("app");
        let dependency = root.join("dependency");
        fs::create_dir_all(&app).expect("create app package");
        write_manifest(&dependency, "dependency", "include", "");
        fs::write(dependency.join("dependency.nss"), "int Dependency();\n")
            .expect("write dependency include");

        let mut manifest = ProjectManifest::new("app", ProjectKind::Mod, ".");
        manifest.dependencies.insert(
            "dependency".to_string(),
            DependencySpec::Path(PathDependency {
                path: PathBuf::from("../dependency"),
            }),
        );

        let resolved = resolve_include_dependencies_from_manifest(&app, &manifest)
            .expect("resolve unsaved root manifest");
        assert_eq!(resolved.len(), 1);
        assert_eq!(
            resolved.first().map(|dependency| dependency.name.as_str()),
            Some("dependency")
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_dependency_cycles() {
        let root = unique_test_dir("dependency-cycle");
        let left = root.join("left");
        let right = root.join("right");
        write_manifest(
            &left,
            "left",
            "include",
            "\n[dependencies]\nright = { path = \"../right\" }\n",
        );
        write_manifest(
            &right,
            "right",
            "include",
            "\n[dependencies]\nleft = { path = \"../left\" }\n",
        );

        let error = resolve_include_dependencies(&left).expect_err("reject cycle");
        assert!(error.to_string().contains("dependency cycle"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_case_insensitive_include_collisions() {
        let root = unique_test_dir("dependency-collision");
        let app = root.join("app");
        let left = root.join("left");
        let right = root.join("right");
        write_manifest(
            &app,
            "app",
            "mod",
            "\n[dependencies]\nleft = { path = \"../left\" }\nright = { path = \"../right\" }\n",
        );
        write_manifest(&left, "left", "include", "");
        write_manifest(&right, "right", "include", "");
        fs::create_dir_all(left.join("nested")).expect("create left nested source");
        fs::create_dir_all(right.join("NESTED")).expect("create right nested source");
        fs::write(left.join("nested/Shared.nss"), "int Left();\n").expect("write left include");
        fs::write(right.join("NESTED/shared.NSS"), "int Right();\n").expect("write right include");

        let error = resolve_include_dependencies(&app).expect_err("reject collision");
        assert!(error.to_string().contains("include dependency collision"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_non_include_dependencies() {
        let root = unique_test_dir("dependency-kind");
        let app = root.join("app");
        let dependency = root.join("dependency");
        write_manifest(
            &app,
            "app",
            "mod",
            "\n[dependencies]\ndependency = { path = \"../dependency\" }\n",
        );
        write_manifest(&dependency, "dependency", "hak", "");

        let error = resolve_include_dependencies(&app).expect_err("reject non-include package");
        assert!(
            error
                .to_string()
                .contains("only include packages may be dependencies")
        );

        let _ = fs::remove_dir_all(root);
    }
}
