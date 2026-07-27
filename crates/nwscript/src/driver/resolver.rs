use super::*;

/// One filesystem-backed source resolver that searches one or more root
/// directories.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileSystemScriptResolver {
    roots:    Vec<PathBuf>,
    overlays: BTreeMap<PathBuf, Arc<[u8]>>,
}

impl FileSystemScriptResolver {
    /// Creates one empty filesystem resolver.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates one filesystem resolver with an initial validated root.
    ///
    /// # Errors
    ///
    /// Returns a resolver error when the root is unavailable or is not a
    /// directory.
    pub fn with_root(root: impl Into<PathBuf>) -> Result<Self, SourceError> {
        let mut resolver = Self::new();
        resolver.add_root(root)?;
        Ok(resolver)
    }

    /// Adds one validated search root used for relative script names.
    ///
    /// # Errors
    ///
    /// Returns a resolver error instead of retaining an unavailable,
    /// non-directory, or unresolvable root.
    pub fn add_root(&mut self, root: impl Into<PathBuf>) -> Result<(), SourceError> {
        let root = root.into();
        let absolute = if root.is_absolute() {
            root
        } else {
            std::env::current_dir()
                .map_err(|error| {
                    SourceError::resolver(format!(
                        "failed to resolve current directory for script root {}: {error}",
                        root.display()
                    ))
                })?
                .join(&root)
        };
        let canonical = fs::canonicalize(&absolute).map_err(|error| {
            SourceError::resolver(format!(
                "failed to resolve script root {}: {error}",
                absolute.display()
            ))
        })?;
        let metadata = fs::metadata(&canonical).map_err(|error| {
            SourceError::resolver(format!(
                "failed to inspect script root {}: {error}",
                canonical.display()
            ))
        })?;
        if !metadata.is_dir() {
            return Err(SourceError::resolver(format!(
                "script root is not a directory: {}",
                canonical.display()
            )));
        }
        if !self.roots.contains(&canonical) {
            self.roots.push(canonical);
        }
        Ok(())
    }

    /// Adds or replaces an in-memory source file overlay.
    ///
    /// Overlay paths participate in the same ordered, case-insensitive
    /// candidate resolution as files on disk.
    pub fn add_overlay(
        &mut self,
        path: impl Into<PathBuf>,
        contents: impl Into<Vec<u8>>,
    ) -> Result<(), SourceError> {
        self.add_shared_overlay(path, Arc::<[u8]>::from(contents.into()))
    }

    /// Adds or replaces an immutable, reference-counted source overlay.
    ///
    /// This avoids duplicating package snapshots when multiple compiler
    /// components share the same source graph.
    ///
    /// # Errors
    ///
    /// Returns a resolver error under the same conditions as
    /// [`Self::add_overlay`].
    pub fn add_shared_overlay(
        &mut self,
        path: impl Into<PathBuf>,
        contents: Arc<[u8]>,
    ) -> Result<(), SourceError> {
        let path = self.normalize_overlay_path(path.into())?;
        if !self.is_scoped_overlay(&path) {
            return Err(SourceError::resolver(format!(
                "source overlay is outside every configured script root: {}; configured roots: {}",
                path.display(),
                self.roots
                    .iter()
                    .map(|root| root.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }
        self.insert_shared_overlay(path, contents)?;
        Ok(())
    }

    /// Adds an overlay only when it belongs to one of this resolver's roots.
    ///
    /// Workspace-wide editor overlay maps use this to scope unsaved files to
    /// the package currently being compiled without treating files from
    /// sibling packages as resolver errors.
    ///
    /// # Errors
    ///
    /// Returns an error when the path cannot be normalized or conflicts with
    /// another overlay by portable case-insensitive identity.
    pub fn add_overlay_if_in_scope(
        &mut self,
        path: impl Into<PathBuf>,
        contents: impl Into<Vec<u8>>,
    ) -> Result<bool, SourceError> {
        self.add_shared_overlay_if_in_scope(path, Arc::<[u8]>::from(contents.into()))
    }

    /// Shared-buffer form of [`Self::add_overlay_if_in_scope`].
    ///
    /// # Errors
    ///
    /// Returns an error under the same conditions as
    /// [`Self::add_overlay_if_in_scope`].
    pub fn add_shared_overlay_if_in_scope(
        &mut self,
        path: impl Into<PathBuf>,
        contents: Arc<[u8]>,
    ) -> Result<bool, SourceError> {
        let path = self.normalize_overlay_path(path.into())?;
        if !self.is_scoped_overlay(&path) {
            return Ok(false);
        }
        self.insert_shared_overlay(path, contents)?;
        Ok(true)
    }

    fn normalize_overlay_path(&self, path: PathBuf) -> Result<PathBuf, SourceError> {
        let path = if path.is_absolute() {
            canonicalize_with_missing_suffix(&path).map_err(|error| {
                SourceError::resolver(format!(
                    "failed to resolve source overlay {}: {error}",
                    path.display()
                ))
            })?
        } else {
            if path
                .components()
                .any(|component| matches!(component, Component::ParentDir))
            {
                return Err(SourceError::resolver(format!(
                    "source overlay escapes its script root: {}",
                    path.display()
                )));
            }
            let [root] = self.roots.as_slice() else {
                return Err(SourceError::resolver(
                    "relative source overlays require exactly one configured script root",
                ));
            };
            canonicalize_with_missing_suffix(&root.join(path)).map_err(|error| {
                SourceError::resolver(format!(
                    "failed to resolve relative source overlay: {error}"
                ))
            })?
        };
        Ok(path)
    }

    fn insert_shared_overlay(
        &mut self,
        path: PathBuf,
        contents: Arc<[u8]>,
    ) -> Result<(), SourceError> {
        if let Some(existing) = self
            .overlays
            .keys()
            .find(|candidate| paths_match_case_insensitively(candidate, &path))
            && existing != &path
        {
            return Err(SourceError::resolver(format!(
                "source overlays are ambiguous on case-insensitive filesystems: {} and {}",
                existing.display(),
                path.display()
            )));
        }
        self.overlays.insert(path, contents);
        Ok(())
    }

    fn overlay(&self, candidate: &Path) -> Option<&[u8]> {
        self.overlays.iter().find_map(|(path, contents)| {
            (self.is_scoped_overlay(path) && paths_match_case_insensitively(path, candidate))
                .then_some(contents.as_ref())
        })
    }

    fn overlay_path(&self, candidate: &Path) -> Option<&Path> {
        self.overlays
            .keys()
            .find(|path| {
                self.is_scoped_overlay(path) && paths_match_case_insensitively(path, candidate)
            })
            .map(PathBuf::as_path)
    }

    fn is_scoped_overlay(&self, path: &Path) -> bool {
        if path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        {
            return false;
        }
        if !path.is_absolute() {
            return !self.roots.is_empty();
        }
        self.roots.iter().any(|root| path.starts_with(root))
    }

    /// Resolves a logical source name to the filesystem or overlay path that
    /// would supply it.
    pub fn resolve_script_path(&self, script_name: &str) -> Result<Option<PathBuf>, SourceError> {
        for name in logical_script_names(script_name) {
            if let Some(path) = self.overlay_path(&name) {
                return Ok(Some(path.to_path_buf()));
            }
        }
        for candidate in self.candidate_paths(script_name)? {
            if let Some(path) = self.overlay_path(&candidate) {
                return Ok(Some(path.to_path_buf()));
            }
            if let Some(path) = resolve_case_insensitive_file(&candidate, &self.roots, None)?
                && self.is_scoped_file(&path)?
            {
                return Ok(Some(path));
            }
        }
        Ok(None)
    }

    fn candidate_paths(&self, script_name: &str) -> Result<Vec<PathBuf>, SourceError> {
        let path = Path::new(script_name);
        if path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        {
            return Ok(Vec::new());
        }
        let names = logical_script_names(script_name);
        let mut candidates = Vec::new();
        for name in names {
            if name.is_absolute() {
                let name = canonicalize_with_missing_suffix(&name).map_err(|error| {
                    SourceError::resolver(format!(
                        "failed to resolve absolute script candidate {}: {error}",
                        name.display()
                    ))
                })?;
                if self.roots.iter().any(|root| name.starts_with(root)) {
                    candidates.push(name);
                }
            } else {
                for root in &self.roots {
                    candidates.push(root.join(&name));
                }
            }
        }
        Ok(candidates)
    }

    fn is_scoped_file(&self, path: &Path) -> Result<bool, SourceError> {
        let canonical = fs::canonicalize(path).map_err(|error| {
            SourceError::resolver(format!(
                "failed to resolve script candidate {}: {error}",
                path.display()
            ))
        })?;
        Ok(self.roots.iter().any(|root| canonical.starts_with(root)))
    }
}

impl ScriptResolver for FileSystemScriptResolver {
    fn resolve_script_origin(
        &self,
        script_name: &str,
        res_type: ResType,
    ) -> Result<Option<crate::ScriptSourceOrigin>, SourceError> {
        if res_type != NW_SCRIPT_SOURCE_RES_TYPE {
            return Ok(None);
        }
        self.resolve_script_path(script_name)
            .map(|path| path.map(crate::ScriptSourceOrigin::Physical))
    }

    fn resolve_script_bytes(
        &self,
        script_name: &str,
        res_type: ResType,
    ) -> Result<Option<Vec<u8>>, SourceError> {
        self.resolve_script_bytes_with_cancellation(
            script_name,
            res_type,
            crate::CompilerResourceLimits::default().max_source_file_bytes,
            &crate::CancellationToken::new(),
        )
    }

    fn resolve_script_bytes_with_cancellation(
        &self,
        script_name: &str,
        res_type: ResType,
        max_bytes: usize,
        cancellation: &crate::CancellationToken,
    ) -> Result<Option<Vec<u8>>, SourceError> {
        cancellation
            .check()
            .map_err(|_error| SourceError::Cancelled)?;
        if res_type != NW_SCRIPT_SOURCE_RES_TYPE {
            return Ok(None);
        }
        for name in logical_script_names(script_name) {
            cancellation
                .check()
                .map_err(|_error| SourceError::Cancelled)?;
            if let Some(contents) = self.overlay(&name) {
                return copy_overlay_bounded(script_name, contents, max_bytes, cancellation)
                    .map(Some);
            }
        }
        for candidate in self.candidate_paths(script_name)? {
            cancellation
                .check()
                .map_err(|_error| SourceError::Cancelled)?;
            if let Some(contents) = self.overlay(&candidate) {
                return copy_overlay_bounded(script_name, contents, max_bytes, cancellation)
                    .map(Some);
            }
            if let Some(resolved) =
                resolve_case_insensitive_file(&candidate, &self.roots, Some(cancellation))?
                && self.is_scoped_file(&resolved)?
            {
                if let Some(contents) = self.overlay(&resolved) {
                    return copy_overlay_bounded(script_name, contents, max_bytes, cancellation)
                        .map(Some);
                }
                return read_source_file_bounded(script_name, &resolved, max_bytes, cancellation)
                    .map(Some);
            }
        }
        Ok(None)
    }
}

fn copy_overlay_bounded(
    script_name: &str,
    contents: &[u8],
    max_bytes: usize,
    cancellation: &crate::CancellationToken,
) -> Result<Vec<u8>, SourceError> {
    if contents.len() > max_bytes {
        return Err(SourceError::resource_limit(
            script_name,
            format!(
                "source file contains {} bytes, exceeding configured maximum {max_bytes}",
                contents.len()
            ),
        ));
    }
    let mut copied = Vec::new();
    copied.try_reserve_exact(contents.len()).map_err(|error| {
        SourceError::resolver(format!("failed to reserve source buffer: {error}"))
    })?;
    for chunk in contents.chunks(1024 * 1024) {
        cancellation
            .check()
            .map_err(|_error| SourceError::Cancelled)?;
        copied.extend_from_slice(chunk);
    }
    Ok(copied)
}

fn read_source_file_bounded(
    script_name: &str,
    path: &Path,
    max_bytes: usize,
    cancellation: &crate::CancellationToken,
) -> Result<Vec<u8>, SourceError> {
    let mut file = fs::File::open(path).map_err(|error| {
        SourceError::resolver(format!(
            "failed to open source file {}: {error}",
            path.display()
        ))
    })?;
    let metadata = file.metadata().map_err(|error| {
        SourceError::resolver(format!(
            "failed to inspect source file {}: {error}",
            path.display()
        ))
    })?;
    let declared_len = usize::try_from(metadata.len()).map_err(|_error| {
        SourceError::resource_limit(script_name, "source file length does not fit in memory")
    })?;
    if declared_len > max_bytes {
        return Err(SourceError::resource_limit(
            script_name,
            format!(
                "source file contains {declared_len} bytes, exceeding configured maximum \
                 {max_bytes}"
            ),
        ));
    }
    let mut contents = Vec::new();
    contents.try_reserve_exact(declared_len).map_err(|error| {
        SourceError::resolver(format!("failed to reserve source buffer: {error}"))
    })?;
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        cancellation
            .check()
            .map_err(|_error| SourceError::Cancelled)?;
        let read = file.read(&mut buffer).map_err(|error| {
            SourceError::resolver(format!(
                "failed to read source file {}: {error}",
                path.display()
            ))
        })?;
        if read == 0 {
            break;
        }
        let next_len = contents.len().checked_add(read).ok_or_else(|| {
            SourceError::resource_limit(script_name, "source file length overflowed")
        })?;
        if next_len > max_bytes {
            return Err(SourceError::resource_limit(
                script_name,
                format!("source file exceeds configured maximum {max_bytes} while being read"),
            ));
        }
        let chunk = buffer.get(..read).ok_or_else(|| {
            SourceError::resolver(format!(
                "source reader returned an invalid byte count for {}",
                path.display()
            ))
        })?;
        contents.extend_from_slice(chunk);
    }
    cancellation
        .check()
        .map_err(|_error| SourceError::Cancelled)?;
    Ok(contents)
}

fn logical_script_names(script_name: &str) -> Vec<PathBuf> {
    let path = Path::new(script_name);
    let mut names = vec![PathBuf::from(script_name)];
    if path.extension().is_none() {
        names.push(PathBuf::from(format!(
            "{script_name}.{}",
            get_res_ext(NW_SCRIPT_SOURCE_RES_TYPE)
        )));
    }
    names
}

fn paths_match_case_insensitively(left: &Path, right: &Path) -> bool {
    crate::logical_path_key(left) == crate::logical_path_key(right)
}

fn resolve_case_insensitive_file(
    path: &Path,
    roots: &[PathBuf],
    cancellation: Option<&crate::CancellationToken>,
) -> Result<Option<PathBuf>, SourceError> {
    let anchor = roots
        .iter()
        .filter(|root| path.starts_with(root))
        .max_by_key(|root| root.components().count());
    let (mut current, relative) = match anchor {
        Some(root) => (
            root.clone(),
            path.strip_prefix(root).map_err(|error| {
                SourceError::resolver(format!(
                    "failed to relativize script candidate {} against {}: {error}",
                    path.display(),
                    root.display()
                ))
            })?,
        ),
        None => (PathBuf::new(), path),
    };
    for component in relative.components() {
        match component {
            Component::Prefix(prefix) => current.push(prefix.as_os_str()),
            Component::RootDir => current.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => current.push(".."),
            Component::Normal(name) => {
                let search_directory = if current.as_os_str().is_empty() {
                    Path::new(".")
                } else {
                    current.as_path()
                };
                let requested = name.to_os_string();
                let entries = fs::read_dir(search_directory).map_err(|error| {
                    SourceError::resolver(format!(
                        "failed to search script directory {}: {error}",
                        search_directory.display()
                    ))
                })?;
                let mut matches = Vec::new();
                for (index, entry) in entries.enumerate() {
                    if index >= 5_000_000 {
                        return Err(SourceError::resolver(format!(
                            "script directory {} exceeds the maximum of 5000000 entries",
                            search_directory.display()
                        )));
                    }
                    if index & 0x0fff == 0
                        && cancellation.is_some_and(crate::CancellationToken::is_cancelled)
                    {
                        return Err(SourceError::Cancelled);
                    }
                    let entry = entry.map_err(|error| {
                        SourceError::resolver(format!(
                            "failed to read script directory entry in {}: {error}",
                            search_directory.display()
                        ))
                    })?;
                    if crate::logical_path_key(Path::new(&entry.file_name()))
                        == crate::logical_path_key(Path::new(&requested))
                    {
                        matches.push(entry);
                    }
                }
                matches.sort_by(|lhs, rhs| {
                    let lhs_name = lhs.file_name();
                    let rhs_name = rhs.file_name();
                    let lhs_exact = lhs_name == requested;
                    let rhs_exact = rhs_name == requested;
                    rhs_exact
                        .cmp(&lhs_exact)
                        .then_with(|| lhs_name.cmp(&rhs_name))
                });
                if matches.len() > 1 {
                    let paths = matches
                        .iter()
                        .map(|entry| entry.path().display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ");
                    return Err(SourceError::resolver(format!(
                        "ambiguous case-insensitive script path component {requested:?}: {paths}"
                    )));
                }
                let Some(entry) = matches.into_iter().next() else {
                    return Ok(None);
                };
                current = entry.path();
            }
        }
    }

    match fs::metadata(&current) {
        Ok(metadata) if metadata.is_file() => Ok(Some(current)),
        Ok(_) => Ok(None),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(SourceError::resolver(format!(
            "failed to inspect resolved script path {}: {error}",
            current.display()
        ))),
    }
}
