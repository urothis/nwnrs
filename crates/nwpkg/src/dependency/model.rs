use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{GitDependency, NwpkgError, NwpkgErrorKind};

/// Lockfile format owned by the nwpkg dependency resolver.
pub const DEPENDENCY_LOCK_FORMAT: u32 = 1;

/// Exact dependency graph embedded in `nwpkg.lock`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DependencyLock {
    /// Lockfile schema discriminator.
    pub format:          u32,
    /// SHA-256 of the root manifest used to produce this graph.
    pub manifest_sha256: String,
    /// Direct dependency aliases mapped to package entry identifiers.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub root:            BTreeMap<String, String>,
    /// Every resolved package, keyed by its deterministic graph route.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub packages:        BTreeMap<String, LockedDependency>,
}

impl DependencyLock {
    /// Validates the lock schema and all graph references.
    ///
    /// # Errors
    ///
    /// Returns an error when the lock format, digest, package source, or graph
    /// edge is invalid.
    pub fn validate(&self, path: &Path) -> Result<(), NwpkgError> {
        if self.format != DEPENDENCY_LOCK_FORMAT {
            return Err(NwpkgError::at(
                NwpkgErrorKind::Validation,
                path,
                format!(
                    "unsupported dependency lock format {}; expected {DEPENDENCY_LOCK_FORMAT}",
                    self.format
                ),
            ));
        }
        validate_sha256(&self.manifest_sha256, "manifest_sha256", path)?;
        for (alias, id) in &self.root {
            validate_alias(alias, path)?;
            if !self.packages.contains_key(id) {
                return Err(NwpkgError::at(
                    NwpkgErrorKind::Validation,
                    path,
                    format!("root dependency {alias:?} references missing package {id:?}"),
                ));
            }
            if id != alias {
                return Err(NwpkgError::at(
                    NwpkgErrorKind::Validation,
                    path,
                    format!(
                        "root dependency {alias:?} must use deterministic package route \
                         {alias:?}, not {id:?}"
                    ),
                ));
            }
        }
        for (id, package) in &self.packages {
            if id.is_empty() || id.starts_with('/') || id.ends_with('/') {
                return Err(NwpkgError::at(
                    NwpkgErrorKind::Validation,
                    path,
                    format!("invalid dependency package identifier {id:?}"),
                ));
            }
            validate_alias(&package.name, path)?;
            if id.split('/').next_back() != Some(package.name.as_str()) {
                return Err(NwpkgError::at(
                    NwpkgErrorKind::Validation,
                    path,
                    format!(
                        "dependency package route {id:?} does not end with alias {:?}",
                        package.name
                    ),
                ));
            }
            validate_sha256(&package.manifest_sha256, "package manifest_sha256", path)?;
            package.source.validate(path)?;
            for (alias, target) in &package.dependencies {
                validate_alias(alias, path)?;
                if !self.packages.contains_key(target) {
                    return Err(NwpkgError::at(
                        NwpkgErrorKind::Validation,
                        path,
                        format!(
                            "dependency package {id:?} edge {alias:?} references missing package \
                             {target:?}"
                        ),
                    ));
                }
                let expected = format!("{id}/{alias}");
                if target != &expected {
                    return Err(NwpkgError::at(
                        NwpkgErrorKind::Validation,
                        path,
                        format!(
                            "dependency package {id:?} edge {alias:?} must target deterministic \
                             route {expected:?}, not {target:?}"
                        ),
                    ));
                }
            }
        }
        let mut reachable = self.root.values().cloned().collect::<Vec<_>>();
        let mut visited = std::collections::BTreeSet::new();
        while let Some(route) = reachable.pop() {
            if !visited.insert(route.clone()) {
                continue;
            }
            let package = self.packages.get(&route).ok_or_else(|| {
                NwpkgError::at(
                    NwpkgErrorKind::Validation,
                    path,
                    format!("dependency graph references missing package {route:?}"),
                )
            })?;
            reachable.extend(package.dependencies.values().cloned());
        }
        if visited.len() != self.packages.len() {
            let unreachable = self
                .packages
                .keys()
                .find(|route| !visited.contains(*route))
                .map_or("<unknown>", String::as_str);
            return Err(NwpkgError::at(
                NwpkgErrorKind::Validation,
                path,
                format!("dependency package {unreachable:?} is unreachable from the root graph"),
            ));
        }
        Ok(())
    }
}

/// One package entry in the resolved dependency graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockedDependency {
    /// Alias used by the declaring package.
    pub name:            String,
    /// Exact immutable or live source identity.
    pub source:          LockedDependencySource,
    /// SHA-256 of this package's manifest at resolution time.
    pub manifest_sha256: String,
    /// Dependency aliases declared by this package.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dependencies:    BTreeMap<String, String>,
}

/// Source captured for one locked package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum LockedDependencySource {
    /// Live local filesystem package.
    Path {
        /// Path relative to the package that declares the dependency.
        path: PathBuf,
    },
    /// Immutable Git checkout.
    Git {
        /// Credential-free repository URL.
        git:      String,
        /// Selector originally requested by the manifest.
        selector: GitSelector,
        /// Exact resolved Git commit.
        commit:   String,
        /// Optional repository-relative package directory.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        subdir:   Option<PathBuf>,
        /// Content-addressed cache entry identifier.
        cache:    String,
    },
}

impl LockedDependencySource {
    fn validate(&self, path: &Path) -> Result<(), NwpkgError> {
        match self {
            Self::Path {
                path: source,
            } => validate_local_path(source, path),
            Self::Git {
                git,
                selector,
                commit,
                subdir,
                cache,
            } => {
                if git.trim().is_empty() {
                    return Err(NwpkgError::at(
                        NwpkgErrorKind::Validation,
                        path,
                        "locked Git URL must not be empty",
                    ));
                }
                selector.validate(path)?;
                if !matches!(commit.len(), 40 | 64)
                    || !commit.bytes().all(|byte| byte.is_ascii_hexdigit())
                {
                    return Err(NwpkgError::at(
                        NwpkgErrorKind::Validation,
                        path,
                        format!("locked Git commit {commit:?} is not a full object ID"),
                    ));
                }
                if cache.len() != 64 || !cache.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                    return Err(NwpkgError::at(
                        NwpkgErrorKind::Validation,
                        path,
                        format!("invalid dependency cache key {cache:?}"),
                    ));
                }
                if let Some(subdir) = subdir {
                    validate_relative_path(subdir, "Git dependency subdir", path)?;
                }
                Ok(())
            }
        }
    }
}

/// User-facing selector retained alongside an exact Git commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "lowercase")]
pub enum GitSelector {
    /// Exact revision or commit-ish.
    Rev(String),
    /// Named tag.
    Tag(String),
    /// Named branch.
    Branch(String),
    /// Repository default branch.
    Default,
}

impl GitSelector {
    /// Builds the canonical selector from a validated manifest dependency.
    #[must_use]
    pub fn from_dependency(dependency: &GitDependency) -> Self {
        if let Some(value) = &dependency.rev {
            return Self::Rev(value.clone());
        }
        if let Some(value) = &dependency.tag {
            return Self::Tag(value.clone());
        }
        if let Some(value) = &dependency.branch {
            return Self::Branch(value.clone());
        }
        Self::Default
    }

    fn validate(&self, path: &Path) -> Result<(), NwpkgError> {
        let value = match self {
            Self::Rev(value) | Self::Tag(value) | Self::Branch(value) => Some(value),
            Self::Default => None,
        };
        if value.is_some_and(|value| value.trim().is_empty()) {
            return Err(NwpkgError::at(
                NwpkgErrorKind::Validation,
                path,
                "locked Git selector must not be empty",
            ));
        }
        Ok(())
    }
}

fn validate_alias(alias: &str, path: &Path) -> Result<(), NwpkgError> {
    if alias.trim().is_empty() || alias.contains('/') {
        return Err(NwpkgError::at(
            NwpkgErrorKind::Validation,
            path,
            format!("invalid dependency alias {alias:?}"),
        ));
    }
    Ok(())
}

fn validate_sha256(value: &str, field: &str, path: &Path) -> Result<(), NwpkgError> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(NwpkgError::at(
            NwpkgErrorKind::Validation,
            path,
            format!("{field} must be exactly 64 hexadecimal characters"),
        ));
    }
    Ok(())
}

fn validate_relative_path(value: &Path, label: &str, path: &Path) -> Result<(), NwpkgError> {
    if value.as_os_str().is_empty()
        || value.is_absolute()
        || value.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err(NwpkgError::at(
            NwpkgErrorKind::Validation,
            path,
            format!("{label} must be a safe relative path"),
        ));
    }
    Ok(())
}

fn validate_local_path(value: &Path, path: &Path) -> Result<(), NwpkgError> {
    if value.as_os_str().is_empty()
        || value.is_absolute()
        || value.components().any(|component| {
            matches!(
                component,
                std::path::Component::RootDir | std::path::Component::Prefix(_)
            )
        })
    {
        return Err(NwpkgError::at(
            NwpkgErrorKind::Validation,
            path,
            "path dependency must be a non-empty relative filesystem path",
        ));
    }
    Ok(())
}
