use std::{
    fs,
    path::{Path, PathBuf},
};

use nwnrs_nwscript::{ScriptResolver, SourceError};
use nwnrs_types::resman::ResType;

use super::core::*;
use crate::LanguageResult;

pub(super) struct SymbolResolver {
    pub(super) filesystem: nwnrs_nwscript::FileSystemScriptResolver,
    pub(super) fallback:   Option<nwnrs_nwscript::SharedScriptResolver>,
}

pub(super) fn build_symbol_resolver(
    query: &NwScriptDefinitionQuery,
) -> LanguageResult<SymbolResolver> {
    build_symbol_resolver_with_cancellation(query, None)
}

pub(super) fn build_symbol_resolver_with_cancellation(
    query: &NwScriptDefinitionQuery,
    cancellation: Option<&nwnrs_nwscript::CancellationToken>,
) -> LanguageResult<SymbolResolver> {
    check_cancellation(cancellation)?;
    let owned_cancellation = nwnrs_nwscript::CancellationToken::new();
    let cancellation = cancellation.unwrap_or(&owned_cancellation);
    let dependency_context = query.project_root.as_ref().unwrap_or(&query.source_path);
    let session = nwnrs_nwpkg::ResolvedPackageSession::open_with_workspace_overlays(
        dependency_context,
        &query.source_overlays,
        cancellation,
    )?;
    let mut roots = Vec::new();
    if let Some(root) = &query.project_root {
        push_unique_path(&mut roots, root.clone());
    }
    if let Some(parent) = query.source_path.parent() {
        push_unique_path(&mut roots, parent.to_path_buf());
    }
    for root in &query.include_directories {
        push_unique_path(&mut roots, root.clone());
    }
    if let Some(session) = &session {
        push_unique_path(&mut roots, session.source_root().to_path_buf());
        for dependency in session.dependencies() {
            push_unique_path(&mut roots, dependency.source_root.clone());
        }
    }
    if let Some(langspec) = &query.langspec
        && let Some(parent) = langspec.parent()
    {
        push_unique_path(&mut roots, parent.to_path_buf());
    }
    let mut filesystem = session
        .map_or_else(nwnrs_nwscript::FileSystemScriptResolver::new, |session| {
            session.resolver().clone()
        });
    for root in &roots {
        filesystem
            .add_root(root.clone())
            .map_err(|error| error.to_string())?;
    }
    for (path, contents) in &query.source_overlays {
        if roots.iter().try_fold(false, |within, root| {
            Ok::<_, String>(within || path_is_within(path, root)?)
        })? {
            filesystem
                .add_overlay(path.clone(), contents.clone())
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(SymbolResolver {
        filesystem,
        fallback: crate::checking::build_install_script_resolver(
            query.root.as_deref(),
            query.user.as_deref(),
            &query.language,
            query.load_ovr,
        )?,
    })
}

fn path_is_within(path: &Path, root: &Path) -> Result<bool, String> {
    Ok(canonical_source_path(path)?.starts_with(canonical_source_path(root)?))
}

impl ScriptResolver for SymbolResolver {
    fn resolve_script_bytes(
        &self,
        script_name: &str,
        res_type: ResType,
    ) -> Result<Option<Vec<u8>>, SourceError> {
        if let Some(source) = self
            .filesystem
            .resolve_script_bytes(script_name, res_type)?
        {
            return Ok(Some(source));
        }
        self.fallback.as_ref().map_or(Ok(None), |fallback| {
            fallback.resolve_script_bytes(script_name, res_type)
        })
    }

    fn resolve_script_bytes_with_cancellation(
        &self,
        script_name: &str,
        res_type: ResType,
        max_bytes: usize,
        cancellation: &nwnrs_nwscript::CancellationToken,
    ) -> Result<Option<Vec<u8>>, SourceError> {
        if let Some(source) = self.filesystem.resolve_script_bytes_with_cancellation(
            script_name,
            res_type,
            max_bytes,
            cancellation,
        )? {
            return Ok(Some(source));
        }
        self.fallback.as_ref().map_or(Ok(None), |fallback| {
            fallback.resolve_script_bytes_with_cancellation(
                script_name,
                res_type,
                max_bytes,
                cancellation,
            )
        })
    }
}

/// Immutable packed NSS source exposed to an editor from an installation
/// resource.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NwScriptVirtualSource {
    /// Stable read-only editor URI derived from the exact source contents.
    pub uri:      String,
    /// Source text presented by the editor document provider.
    pub contents: Vec<u8>,
}

/// One include or script resource resolved with normal compiler precedence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NwScriptResolvedSource {
    /// Editable filesystem path when a physical override exists.
    pub path:          PathBuf,
    /// Read-only URI when the source came from a packed game resource.
    pub virtual_uri:   Option<String>,
    /// Logical resource name needed to reopen packed source.
    pub resource_name: Option<String>,
}

/// Resolves an NSS include or script name exactly as the compiler would.
///
/// # Errors
///
/// Returns an error when project dependencies or game resource roots cannot
/// be resolved.
pub fn resolve_nwscript_source(
    query: &NwScriptDefinitionQuery,
    resource_name: &str,
) -> LanguageResult<Option<NwScriptResolvedSource>> {
    let resolver = build_symbol_resolver(query)?;
    if let Some(path) = resolver
        .filesystem
        .resolve_script_path(resource_name)
        .map_err(|error| error.to_string())?
    {
        return Ok(Some(NwScriptResolvedSource {
            path,
            virtual_uri: None,
            resource_name: None,
        }));
    }
    let Some(bytes) = resolver
        .resolve_script_bytes(resource_name, nwnrs_nwscript::NW_SCRIPT_SOURCE_RES_TYPE)
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    Ok(Some(NwScriptResolvedSource {
        path:          virtual_source_path(resource_name),
        virtual_uri:   Some(virtual_source_uri(resource_name, &bytes)),
        resource_name: Some(resource_name.to_string()),
    }))
}

/// One source file that uniquely provides an unresolved symbol when included.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NwScriptIncludeCandidate {
    /// Include name without the `.nss` extension.
    pub include_name: String,
    /// Declaration provided by the candidate source.
    pub definition:   NwScriptSymbolDefinition,
}

/// Searches project and configured include roots for source files that define
/// a currently unresolved symbol but are not in the current include graph.
///
/// # Errors
///
/// Returns an error when project dependencies or candidate sources cannot be
/// resolved safely.
pub fn find_nwscript_include_candidates(
    query: &NwScriptDefinitionQuery,
) -> LanguageResult<Vec<NwScriptIncludeCandidate>> {
    NwScriptProjectIndex::new().include_candidates(query, None)
}

/// Enumerates every editable NSS source owned by a package, its configured
/// include roots, and its transitive local include dependencies.
///
/// # Errors
///
/// Returns an error when a project manifest or source root cannot be read, or
/// when the bounded editor index would exceed 20,000 source files.
pub fn list_nwscript_project_sources(
    query: &NwScriptDefinitionQuery,
) -> LanguageResult<Vec<PathBuf>> {
    list_nwscript_project_sources_with_cancellation(query, None)
}

pub(super) fn list_nwscript_project_sources_with_cancellation(
    query: &NwScriptDefinitionQuery,
    cancellation: Option<&nwnrs_nwscript::CancellationToken>,
) -> LanguageResult<Vec<PathBuf>> {
    check_cancellation(cancellation)?;
    let owned_cancellation = nwnrs_nwscript::CancellationToken::new();
    let effective_cancellation = cancellation.unwrap_or(&owned_cancellation);
    let dependency_context = query.project_root.as_ref().unwrap_or(&query.source_path);
    let session = nwnrs_nwpkg::ResolvedPackageSession::open_with_workspace_overlays(
        dependency_context,
        &query.source_overlays,
        effective_cancellation,
    )?;
    let mut roots = Vec::new();
    let mut files = Vec::new();
    if let Some(session) = &session {
        files.extend(session.all_nss_source_paths()?);
    } else if let Some(project_root) = &query.project_root {
        push_unique_path(&mut roots, project_root.clone());
    }
    for root in &query.include_directories {
        push_unique_path(&mut roots, root.clone());
    }
    for root in roots {
        collect_nss_files(&root, &mut files, cancellation)?;
    }
    files = files
        .into_iter()
        .map(|file| {
            file.canonicalize().map_err(|error| {
                format!(
                    "failed to canonicalize indexed source {}: {error}",
                    file.display()
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    files.sort();
    files.dedup();
    const MAX_EDITOR_INDEX_FILES: usize = 20_000;
    if files.len() > MAX_EDITOR_INDEX_FILES {
        return Err(format!(
            "NWScript project index found {} NSS files; maximum is {MAX_EDITOR_INDEX_FILES}",
            files.len()
        )
        .into());
    }
    Ok(files)
}

fn collect_nss_files(
    root: &Path,
    files: &mut Vec<PathBuf>,
    cancellation: Option<&nwnrs_nwscript::CancellationToken>,
) -> LanguageResult<()> {
    check_cancellation(cancellation)?;
    let root_metadata = match fs::metadata(root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(format!(
                "failed to inspect include search root {}: {error}",
                root.display()
            )
            .into());
        }
    };
    if !root_metadata.is_dir() {
        return Err(format!("include search root is not a directory: {}", root.display()).into());
    }
    let mut entries = fs::read_dir(root)
        .map_err(|error| {
            format!(
                "failed to read include search root {}: {error}",
                root.display()
            )
        })?
        .map(|entry| {
            entry.map_err(|error| {
                format!(
                    "failed to read entry in include search root {}: {error}",
                    root.display()
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        check_cancellation(cancellation)?;
        let path = entry.path();
        if entry.file_name().to_str().is_none() {
            return Err(format!(
                "include source entry names must be valid UTF-8: {}",
                path.display()
            )
            .into());
        }
        let metadata = fs::symlink_metadata(&path).map_err(|error| {
            format!(
                "failed to inspect include source {}: {error}",
                path.display()
            )
        })?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "include source traversal does not permit symbolic links: {}",
                path.display()
            )
            .into());
        }
        if metadata.is_dir() {
            if !matches!(
                path.file_name().and_then(|name| name.to_str()),
                Some(".git" | "node_modules" | "target")
            ) {
                collect_nss_files(&path, files, cancellation)?;
            }
        } else if metadata.is_file()
            && path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("nss"))
        {
            files.push(path);
        }
    }
    Ok(())
}

pub(super) struct LoadedBuiltinSource {
    pub(super) bytes:         Vec<u8>,
    pub(super) physical_path: Option<PathBuf>,
    pub(super) virtual_uri:   String,
    pub(super) resource_name: String,
}

/// Resolves the effective implicit `nwscript.nss` using normal compiler
/// precedence.
///
/// Workspace files and explicit compiler langspec paths return `None` because
/// VS Code can open them normally. Packed installation resources return a
/// read-only virtual document.
///
/// # Errors
///
/// Returns an error when source resolution fails.
pub fn load_nwscript_builtin_source(
    query: &NwScriptDefinitionQuery,
) -> LanguageResult<Option<NwScriptVirtualSource>> {
    let resolver = build_symbol_resolver(query)?;
    let Some(source) = load_builtin_source(&resolver, query)? else {
        return Ok(None);
    };
    if source.physical_path.is_some() {
        return Ok(None);
    }
    Ok(Some(NwScriptVirtualSource {
        uri:      source.virtual_uri,
        contents: source.bytes,
    }))
}

/// Resolves one effective NSS resource and returns its immutable virtual
/// contents only when resolution fell through to a packed installation asset.
/// Workspace files and unsaved overlays take precedence and return `None`.
///
/// # Errors
///
/// Returns an error when project or installation resource resolution fails.
pub fn load_nwscript_virtual_source(
    query: &NwScriptDefinitionQuery,
    resource_name: &str,
) -> LanguageResult<Option<NwScriptVirtualSource>> {
    let resolver = build_symbol_resolver(query)?;
    if resolver
        .filesystem
        .resolve_script_path(resource_name)
        .map_err(|error| error.to_string())?
        .is_some()
    {
        return Ok(None);
    }
    let Some(bytes) = resolver
        .resolve_script_bytes(resource_name, nwnrs_nwscript::NW_SCRIPT_SOURCE_RES_TYPE)
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    Ok(Some(NwScriptVirtualSource {
        uri:      virtual_source_uri(resource_name, &bytes),
        contents: bytes,
    }))
}

pub(super) fn load_builtin_source(
    resolver: &SymbolResolver,
    query: &NwScriptDefinitionQuery,
) -> Result<Option<LoadedBuiltinSource>, String> {
    let script_name = query.langspec.as_ref().map_or_else(
        || nwnrs_nwscript::DEFAULT_LANGSPEC_SCRIPT_NAME.to_string(),
        |path| nwnrs_nwscript::physical_path_display(path),
    );
    let physical_path = resolver
        .filesystem
        .resolve_script_path(&script_name)
        .map_err(|error| error.to_string())?;
    let Some(bytes) = resolver
        .resolve_script_bytes(&script_name, nwnrs_nwscript::NW_SCRIPT_SOURCE_RES_TYPE)
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    Ok(Some(LoadedBuiltinSource {
        virtual_uri: virtual_source_uri(&script_name, &bytes),
        bytes,
        physical_path,
        resource_name: script_name,
    }))
}

pub(super) fn virtual_source_uri(resource_name: &str, source: &[u8]) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in resource_name.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash ^= 0xff;
    hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    for byte in source {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    let stem = virtual_source_path(resource_name)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("script")
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '_' | '-') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    format!("nwnrs-game:/{hash:016x}/{stem}.nss")
}

pub(super) fn virtual_source_path(resource_name: &str) -> PathBuf {
    let stem = Path::new(resource_name)
        .file_stem()
        .unwrap_or_else(|| std::ffi::OsStr::new("script"));
    PathBuf::from(stem).with_extension("nss")
}
