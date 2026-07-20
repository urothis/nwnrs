use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

use crate::{
    DependencySpec, PROJECT_MANIFEST_FILENAME, ProjectKind, ProjectManifest, read_project_manifest,
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
) -> Result<Vec<ResolvedIncludeDependency>, String> {
    let Some(project_root) = find_project_root(input)? else {
        return Ok(Vec::new());
    };
    let mut state = ResolverState::default();
    state.visit_manifest(&project_root)?;
    validate_include_names(&state.resolved)?;
    Ok(state.resolved)
}

#[derive(Default)]
struct ResolverState {
    active:   Vec<PathBuf>,
    visited:  HashSet<PathBuf>,
    emitted:  HashSet<PathBuf>,
    resolved: Vec<ResolvedIncludeDependency>,
}

impl ResolverState {
    fn visit_manifest(&mut self, project_root: &Path) -> Result<(), String> {
        let project_root = canonical_directory(project_root, "project")?;
        if self.visited.contains(&project_root) {
            return Ok(());
        }
        if let Some(index) = self
            .active
            .iter()
            .position(|active| active == &project_root)
        {
            let cycle_paths = self
                .active
                .get(index..)
                .ok_or_else(|| "dependency cycle index is out of bounds".to_string())?;
            let mut cycle = cycle_paths
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>();
            cycle.push(project_root.display().to_string());
            return Err(format!("nwpkg dependency cycle: {}", cycle.join(" -> ")));
        }

        let manifest = read_required_manifest(&project_root)?;
        self.active.push(project_root.clone());
        for (name, dependency) in manifest.dependencies {
            let DependencySpec::Path(dependency) = dependency;
            if dependency.path.as_os_str().is_empty() {
                return Err(format!(
                    "dependency {name:?} in {} has an empty path",
                    project_root.join(PROJECT_MANIFEST_FILENAME).display()
                ));
            }
            let dependency_root = canonical_directory(
                &project_root.join(&dependency.path),
                &format!("dependency {name:?}"),
            )?;
            let dependency_manifest = read_required_manifest(&dependency_root)?;
            if dependency_manifest.project.kind != ProjectKind::Include {
                return Err(format!(
                    "dependency {name:?} at {} is kind {:?}; only include packages may be \
                     dependencies",
                    dependency_root.display(),
                    dependency_manifest.project.kind
                ));
            }

            self.visit_manifest(&dependency_root)?;
            if self.emitted.insert(dependency_root.clone()) {
                let source_root = canonical_directory(
                    &dependency_root.join(&dependency_manifest.source.path),
                    &format!("source for dependency {name:?}"),
                )?;
                if !source_root.starts_with(&dependency_root) {
                    return Err(format!(
                        "dependency {name:?} source {} escapes package root {}",
                        source_root.display(),
                        dependency_root.display()
                    ));
                }
                self.resolved.push(ResolvedIncludeDependency {
                    name,
                    package_root: dependency_root.clone(),
                    source_root,
                });
            }
        }
        self.active.pop();
        self.visited.insert(project_root);
        Ok(())
    }
}

fn find_project_root(input: &Path) -> Result<Option<PathBuf>, String> {
    let absolute = if input.is_absolute() {
        input.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| format!("failed to read current directory: {error}"))?
            .join(input)
    };
    let mut current = if absolute.is_dir() {
        absolute
    } else {
        absolute
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
    };
    loop {
        if current.join(PROJECT_MANIFEST_FILENAME).is_file() {
            return canonical_directory(&current, "project").map(Some);
        }
        if !current.pop() {
            return Ok(None);
        }
    }
}

fn read_required_manifest(root: &Path) -> Result<ProjectManifest, String> {
    read_project_manifest(root)?.ok_or_else(|| {
        format!(
            "nwpkg dependency does not contain {}: {}",
            PROJECT_MANIFEST_FILENAME,
            root.display()
        )
    })
}

fn canonical_directory(path: &Path, label: &str) -> Result<PathBuf, String> {
    let canonical = fs::canonicalize(path)
        .map_err(|error| format!("failed to resolve {label} {}: {error}", path.display()))?;
    if !canonical.is_dir() {
        return Err(format!(
            "{label} is not a directory: {}",
            canonical.display()
        ));
    }
    Ok(canonical)
}

fn validate_include_names(dependencies: &[ResolvedIncludeDependency]) -> Result<(), String> {
    let mut names = BTreeMap::<String, PathBuf>::new();
    for dependency in dependencies {
        for entry in fs::read_dir(&dependency.source_root).map_err(|error| {
            format!(
                "failed to read include dependency source {}: {error}",
                dependency.source_root.display()
            )
        })? {
            let path = entry
                .map_err(|error| {
                    format!(
                        "failed to read include dependency entry in {}: {error}",
                        dependency.source_root.display()
                    )
                })?
                .path();
            if !path.is_file()
                || !path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("nss"))
            {
                continue;
            }
            let name = path
                .file_name()
                .and_then(|file_name| file_name.to_str())
                .ok_or_else(|| format!("include filename is not UTF-8: {}", path.display()))?
                .to_ascii_lowercase();
            if let Some(existing) = names.insert(name.clone(), path.clone()) {
                return Err(format!(
                    "include dependency collision for {name:?}: {} and {}",
                    existing.display(),
                    path.display()
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::resolve_include_dependencies;

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
            root.join("nwproject.toml"),
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
        assert!(error.contains("dependency cycle"));

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
        fs::write(left.join("Shared.nss"), "int Left();\n").expect("write left include");
        fs::write(right.join("shared.NSS"), "int Right();\n").expect("write right include");

        let error = resolve_include_dependencies(&app).expect_err("reject collision");
        assert!(error.contains("include dependency collision"));

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
        assert!(error.contains("only include packages may be dependencies"));

        let _ = fs::remove_dir_all(root);
    }
}
