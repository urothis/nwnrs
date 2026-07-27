use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
};

use nwnrs_nwscript::{
    CancellationToken, CompilerResourceLimits, FileSystemScriptResolver, MacroRegistry,
    NW_SCRIPT_SOURCE_RES_TYPE, PreprocessError, ScriptResolver, SourceBundle, SourceLoadOptions,
    canonicalize_with_missing_suffix, collect_source_bundle_macros_with_cancellation,
    load_source_bundle_with_cancellation, register_compiler_macros,
};

use crate::{
    NwpkgError, NwpkgErrorKind, fs::collect_nss_paths_with_cancellation,
    resolve_include_dependencies_with_cancellation,
};

/// Loads compiler macros exported by every resolved include dependency.
///
/// Macro source files are discovered by syntax rather than filename. Each
/// candidate is loaded through the normal NWScript include and object-define
/// preprocessor before its `proc_macro!` and `macro_rules!` definitions are
/// registered. Transitive dependencies participate in the same deterministic
/// registry, and duplicate macro paths are rejected.
///
/// # Errors
///
/// Returns an error when dependencies cannot be resolved, a candidate source
/// cannot be read or preprocessed, a macro definition is invalid, or two
/// packages export the same macro path.
pub fn load_dependency_macro_registry(input: &Path) -> Result<MacroRegistry, NwpkgError> {
    let cancellation = CancellationToken::new();
    let dependencies = resolve_include_dependencies_with_cancellation(input, &cancellation)?;
    load_dependency_macro_registry_resolved(&dependencies, &BTreeMap::new(), &cancellation)
}

pub(crate) fn load_dependency_macro_registry_resolved(
    dependencies: &[crate::ResolvedIncludeDependency],
    overlays: &BTreeMap<PathBuf, Arc<[u8]>>,
    cancellation: &CancellationToken,
) -> Result<MacroRegistry, NwpkgError> {
    let mut resolver = FileSystemScriptResolver::new();
    for dependency in dependencies {
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
    for (path, contents) in overlays {
        resolver
            .add_shared_overlay_if_in_scope(path, Arc::clone(contents))
            .map_err(|error| NwpkgError::at(NwpkgErrorKind::Validation, path, error.to_string()))?;
    }

    let mut candidates = Vec::new();
    let source_limits = CompilerResourceLimits::default();
    for dependency in dependencies {
        for path in collect_nss_paths_with_cancellation(&dependency.source_root, cancellation)? {
            cancellation
                .check()
                .map_err(|error| NwpkgError::new(NwpkgErrorKind::Cancelled, error.to_string()))?;
            let source_name = path.to_str().ok_or_else(|| {
                NwpkgError::at(
                    NwpkgErrorKind::Validation,
                    &path,
                    "compiler macro source path is not UTF-8",
                )
            })?;
            let bytes = resolver
                .resolve_script_bytes_with_cancellation(
                    source_name,
                    NW_SCRIPT_SOURCE_RES_TYPE,
                    source_limits.max_source_file_bytes,
                    cancellation,
                )
                .map_err(|error| {
                    NwpkgError::at(NwpkgErrorKind::Compiler, &path, error.to_string())
                })?
                .ok_or_else(|| {
                    NwpkgError::at(
                        NwpkgErrorKind::NotFound,
                        &path,
                        "compiler macro source disappeared during discovery",
                    )
                })?;
            if contains_macro_definition_marker(&bytes) {
                candidates.push(path);
            }
        }
        for path in overlays.keys() {
            if !path.starts_with(&dependency.source_root)
                || !path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("nss"))
            {
                continue;
            }
            let exists = path.try_exists().map_err(|error| {
                NwpkgError::at(
                    NwpkgErrorKind::Io,
                    path,
                    format!("failed to inspect compiler macro overlay path: {error}"),
                )
            })?;
            if exists {
                continue;
            }
            let Some(bytes) = overlays.get(path).map(|contents| contents.as_ref()) else {
                return Err(NwpkgError::at(
                    NwpkgErrorKind::NotFound,
                    path,
                    "compiler macro overlay disappeared during discovery",
                ));
            };
            if contains_macro_definition_marker(bytes) {
                candidates.push(path.clone());
            }
        }
    }
    crate::fs::sort_paths_case_insensitive(&mut candidates, "compiler macro source")?;
    candidates.dedup();
    let mut registry = MacroRegistry::new();
    register_compiler_macros(&mut registry).map_err(|error| {
        NwpkgError::new(
            NwpkgErrorKind::Compiler,
            format!("failed to register built-in compiler macros: {error}"),
        )
    })?;
    let mut processed = HashSet::new();
    for path in candidates {
        cancellation
            .check()
            .map_err(|error| NwpkgError::new(NwpkgErrorKind::Cancelled, error.to_string()))?;
        let canonical = canonicalize_with_missing_suffix(&path).map_err(|error| {
            NwpkgError::at(
                NwpkgErrorKind::Io,
                &path,
                format!("failed to resolve compiler macro source path: {error}"),
            )
        })?;
        if processed.contains(&canonical) {
            continue;
        }
        let source_name = canonical.to_str().ok_or_else(|| {
            NwpkgError::at(
                NwpkgErrorKind::Validation,
                &canonical,
                "compiler macro source path is not UTF-8",
            )
        })?;
        let bundle = load_source_bundle_with_cancellation(
            &resolver,
            source_name,
            SourceLoadOptions::default(),
            cancellation,
        )
        .map_err(|error| {
            let kind = if cancellation.is_cancelled() {
                NwpkgErrorKind::Cancelled
            } else {
                NwpkgErrorKind::Compiler
            };
            NwpkgError::at(
                kind,
                &canonical,
                format!("failed to load compiler macro source: {error}"),
            )
        })?;
        mark_bundle_sources_processed(&resolver, &bundle, &mut processed)?;
        collect_source_bundle_macros_with_cancellation(&bundle, &mut registry, cancellation)
            .map_err(|error| {
                NwpkgError::at(
                    if cancellation.is_cancelled() {
                        NwpkgErrorKind::Cancelled
                    } else {
                        NwpkgErrorKind::Compiler
                    },
                    &canonical,
                    format_macro_error(&canonical, &bundle, &error),
                )
            })?;
    }
    Ok(registry)
}

fn contains_macro_definition_marker(bytes: &[u8]) -> bool {
    [b"proc_macro".as_slice(), b"macro_rules".as_slice()]
        .iter()
        .any(|marker| bytes.windows(marker.len()).any(|window| window == *marker))
}

fn mark_bundle_sources_processed(
    resolver: &FileSystemScriptResolver,
    bundle: &SourceBundle,
    processed: &mut HashSet<PathBuf>,
) -> Result<(), NwpkgError> {
    for source in bundle.source_map.iter() {
        let path = Path::new(&source.name);
        let resolved = if path.is_absolute() {
            Some(path.to_path_buf())
        } else {
            resolver
                .resolve_script_path(&source.name)
                .map_err(|error| {
                    NwpkgError::at(NwpkgErrorKind::Validation, path, error.to_string())
                })?
        };
        if let Some(path) = resolved {
            let canonical = canonicalize_with_missing_suffix(&path).map_err(|error| {
                NwpkgError::at(
                    NwpkgErrorKind::Io,
                    &path,
                    format!("failed to resolve compiler macro source: {error}"),
                )
            })?;
            processed.insert(canonical);
        }
    }
    Ok(())
}

fn format_macro_error(path: &Path, bundle: &SourceBundle, error: &PreprocessError) -> String {
    let location = match error {
        PreprocessError::Lex(error) => Some(error.span),
        PreprocessError::Macro(error) => error.span,
        PreprocessError::Cancelled(_) | PreprocessError::Source(_) => None,
    }
    .and_then(|span| {
        let source = bundle.source_map.get(span.source_id)?;
        let location = source.location(span.start)?;
        Some(format!(
            "{}:{}:{}",
            source.name, location.line, location.column
        ))
    });
    match location {
        Some(location) => format!("failed to load compiler macros at {location}: {error}"),
        None => format!(
            "failed to load compiler macros from {}: {error}",
            path.display()
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        fs,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    use nwnrs_nwscript::{
        CancellationToken, MacroExpansionOptions, NwTokenStream, SourceId, Span,
        expand_registered_macro, lex_text, render_nwscript_tokens,
    };

    use super::load_dependency_macro_registry;
    use crate::ResolvedPackageSession;

    fn test_root(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock before unix epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("nwnrs-package-macros-{name}-{nonce}"));
        fs::create_dir_all(&root).expect("create macro test root");
        root
    }

    fn write_manifest(root: &Path, name: &str, kind: &str, dependencies: &str) {
        fs::write(
            root.join("nwpkg.toml"),
            format!(
                "[project]\nname = {name:?}\nkind = {kind:?}\n\n[source]\npath = \
                 \".\"\n{dependencies}"
            ),
        )
        .expect("write macro fixture manifest");
    }

    #[test]
    fn discovers_arbitrarily_named_package_macros_after_include_preprocessing() {
        let root = test_root("discovery");
        let module = root.join("module");
        let facade = root.join("facade");
        let include = root.join("include");
        fs::create_dir_all(&module).expect("create module");
        fs::create_dir_all(&facade).expect("create facade include");
        fs::create_dir_all(&include).expect("create include");
        write_manifest(
            &module,
            "module",
            "mod",
            "\n[dependencies]\nfacade = { path = \"../facade\" }\n",
        );
        write_manifest(
            &facade,
            "facade",
            "include",
            "\n[dependencies]\nmacros = { path = \"../include\" }\n",
        );
        write_manifest(&include, "macros", "include", "");
        fs::write(include.join("macro_config.nss"), "#define EXPORTED emit\n")
            .expect("write macro config");
        fs::write(
            include.join("compiler_extensions.nss"),
            r#"
                #include "macro_config"

                proc_macro! fixture::EXPORTED {
                    tokenstream emit(tokenstream input) {
                        return quote! { int GeneratedValue() { return 7; } };
                    }
                }

                macro_rules! package_identity {
                    {$value:ident} => { $value };
                }
            "#,
        )
        .expect("write compiler extensions");

        let registry = load_dependency_macro_registry(&module).expect("load package macros");
        let generated = expand_registered_macro(
            &registry,
            "fixture::emit",
            NwTokenStream::new(),
            Span::new(SourceId::new(0), 0, 0),
            MacroExpansionOptions::default(),
        )
        .expect("expand procedural package macro");
        assert!(render_nwscript_tokens(&generated).contains("GeneratedValue"));

        let tokens =
            lex_text(SourceId::new(0), "package_value").expect("lex declarative macro input");
        let input = NwTokenStream::from_tokens(&tokens).expect("balance declarative macro input");
        let generated = expand_registered_macro(
            &registry,
            "package_identity",
            input,
            Span::new(SourceId::new(0), 0, 0),
            MacroExpansionOptions::default(),
        )
        .expect("expand declarative package macro");
        assert_eq!(render_nwscript_tokens(&generated), "package_value");

        fs::remove_dir_all(root).expect("remove macro test root");
    }

    #[test]
    fn package_session_indexes_unsaved_dependency_macro_sources() {
        let root = test_root("overlay-dependency");
        let module = root.join("module");
        let include = root.join("include");
        fs::create_dir_all(&module).expect("create module");
        fs::create_dir_all(&include).expect("create include");
        write_manifest(
            &module,
            "module",
            "mod",
            "\n[dependencies]\nmacros = { path = \"../include\" }\n",
        );
        write_manifest(&include, "macros", "include", "");

        let overlay_path = include.join("unsaved_macros.nss");
        let overlays = BTreeMap::from([(
            overlay_path,
            br#"macro_rules! overlay_value { {} => { const int OVERLAY_VALUE = 7; } }"#.to_vec(),
        )]);
        let cancellation = CancellationToken::new();
        let session = ResolvedPackageSession::open(&module, &overlays, &cancellation)
            .expect("open package session")
            .expect("package exists");
        let macro_registry = session.macro_registry().expect("load session macros");
        let expanded = expand_registered_macro(
            macro_registry.as_ref(),
            "overlay_value",
            NwTokenStream::new(),
            Span::new(SourceId::new(0), 0, 0),
            MacroExpansionOptions::default(),
        )
        .expect("expand unsaved dependency macro");

        assert!(
            render_nwscript_tokens(&expanded).contains("OVERLAY_VALUE"),
            "unsaved dependency macro output should be available"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_duplicate_macro_paths_across_packages() {
        let root = test_root("duplicate");
        let module = root.join("module");
        let left = root.join("left");
        let right = root.join("right");
        for path in [&module, &left, &right] {
            fs::create_dir_all(path).expect("create package");
        }
        write_manifest(
            &module,
            "module",
            "mod",
            "\n[dependencies]\nleft = { path = \"../left\" }\nright = { path = \"../right\" }\n",
        );
        write_manifest(&left, "left", "include", "");
        write_manifest(&right, "right", "include", "");
        let definition = |entry: &str| {
            format!(
                "proc_macro! shared::duplicate {{ tokenstream duplicate(tokenstream input) {{ \
                 return quote! {{ int {entry}; }}; }} }}"
            )
        };
        fs::write(left.join("left_extensions.nss"), definition("Left")).expect("write left macro");
        fs::write(right.join("right_extensions.nss"), definition("Right"))
            .expect("write right macro");

        let error = match load_dependency_macro_registry(&module) {
            Ok(_registry) => panic!("duplicate macro must be rejected"),
            Err(error) => error,
        };
        assert!(
            error
                .to_string()
                .contains("macro `shared::duplicate` is already defined")
        );

        fs::remove_dir_all(root).expect("remove macro test root");
    }
}
