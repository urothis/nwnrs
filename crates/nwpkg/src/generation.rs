use std::{
    collections::BTreeMap,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};

use nwnrs_nwscript::{
    CompilerDiagnostic, MacroExpansionOptions, MacroRegistry, NwDelimiter, NwTokenGroup,
    NwTokenStream, NwTokenTree, SourceFile, SourceId, Span, Token, TokenKind,
    collect_nwscript_macros, expand_registered_macro, lex_source, lex_text, render_nwscript_tokens,
};

use crate::{PROJECT_MANIFEST_FILENAME, read_project_manifest};

const EVENT_DISPATCHER_MACRO_SOURCE: &str = include_str!("../macros/nwnrs_macros.nss");
const EVENT_DISPATCHER_MACRO_SOURCE_ID: SourceId = SourceId::new(u32::MAX);

/// The deterministic native module-load dispatcher generated for a module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedEventDispatcher {
    /// Logical NWScript name, without an extension.
    pub name:         String,
    /// Complete generated NWScript source.
    pub source:       String,
    /// Root used to resolve module-local includes in the generated script.
    pub include_root: PathBuf,
}

/// Runs the nwnrs NSS project macro over the nearest module project and
/// produces the always-present `_nwnrs_onload` dispatcher.
///
/// # Errors
///
/// Returns an error when the project cannot be resolved, a source cannot be
/// read or lexed, or the NSS macro rejects an event registration.
pub fn generate_event_dispatcher(input: &Path) -> Result<Option<GeneratedEventDispatcher>, String> {
    generate_event_dispatcher_with_diagnostics(input).map_err(|diagnostic| diagnostic.message)
}

/// Runs the canonical event project macro while retaining structured source
/// diagnostics for editor and automation clients.
///
/// # Errors
///
/// Returns a source-aware diagnostic when project discovery, source loading,
/// macro compilation, validation, or expansion fails.
pub fn generate_event_dispatcher_with_diagnostics(
    input: &Path,
) -> Result<Option<GeneratedEventDispatcher>, CompilerDiagnostic> {
    generate_event_dispatcher_with_overlays(input, &BTreeMap::new())
}

/// Runs the canonical event project macro using unsaved source overlays.
///
/// # Errors
///
/// Returns the same source-aware diagnostics as
/// [`generate_event_dispatcher_with_diagnostics`].
pub fn generate_event_dispatcher_with_overlays(
    input: &Path,
    overlays: &BTreeMap<PathBuf, Vec<u8>>,
) -> Result<Option<GeneratedEventDispatcher>, CompilerDiagnostic> {
    let Some(project_root) = find_project_root(input).map_err(plain_diagnostic)? else {
        return Ok(None);
    };
    let manifest = read_project_manifest(&project_root)
        .map_err(plain_diagnostic)?
        .ok_or_else(|| {
            plain_diagnostic(format!(
                "missing {}",
                project_root.join(PROJECT_MANIFEST_FILENAME).display()
            ))
        })?;
    if manifest.project.kind != crate::ProjectKind::Mod {
        return Ok(None);
    }
    let source_root = project_root.join(manifest.source.path);
    let source_root = fs::canonicalize(&source_root).map_err(|error| {
        plain_diagnostic(format!(
            "failed to resolve module source {}: {error}",
            source_root.display()
        ))
    })?;
    if !source_root.starts_with(&project_root) {
        return Err(plain_diagnostic(format!(
            "module source {} escapes project root {}",
            source_root.display(),
            project_root.display()
        )));
    }

    let mut paths = Vec::new();
    collect_nss_paths(&source_root, &mut paths).map_err(plain_diagnostic)?;
    paths.sort();
    let mut macro_input = NwTokenStream::new();
    let mut source_files = BTreeMap::new();
    let mut invocation_span = None;
    for (source_index, path) in paths.iter().enumerate() {
        if path
            .file_stem()
            .and_then(OsStr::to_str)
            .is_some_and(|stem| {
                stem.eq_ignore_ascii_case("_nwnrs_onload") || stem.eq_ignore_ascii_case("nwscript")
            })
        {
            continue;
        }
        let bytes = overlay_contents(overlays, path).map_or_else(
            || {
                fs::read(path).map_err(|error| {
                    plain_diagnostic(format!("failed to read {}: {error}", path.display()))
                })
            },
            |contents| Ok(contents.to_vec()),
        )?;
        let source_id = SourceId::new(u32::try_from(source_index).map_err(|_error| {
            plain_diagnostic("module contains too many NWScript source files for event collection")
        })?);
        let source = SourceFile::new(source_id, path.display().to_string(), bytes);
        let tokens = lex_source(&source).map_err(|error| {
            source_diagnostic(
                format!("failed to lex {}: {error}", path.display()),
                Some(error.code.code()),
                &source,
                error.span,
            )
        })?;
        let stream = NwTokenStream::from_tokens(&tokens).map_err(|error| {
            error.span.map_or_else(
                || plain_diagnostic(format!("failed to balance {}: {error}", path.display())),
                |span| {
                    source_diagnostic(
                        format!("failed to balance {}: {error}", path.display()),
                        None,
                        &source,
                        span,
                    )
                },
            )
        })?;
        let relative = path.strip_prefix(&source_root).map_err(|error| {
            plain_diagnostic(format!("failed to relativize {}: {error}", path.display()))
        })?;
        let include = relative
            .with_extension("")
            .to_string_lossy()
            .replace('\\', "/");
        let start = Span::new(source_id, 0, 0);
        let end = Span::new(source_id, source.len(), source.len());
        invocation_span.get_or_insert(Span::new(source_id, 0, source.len()));
        macro_input.push(NwTokenTree::Token(Token::new(
            TokenKind::Identifier,
            start,
            "__nwnrs_source",
        )));
        macro_input.push(NwTokenTree::Token(Token::new(
            TokenKind::String,
            start,
            include,
        )));
        macro_input.push(NwTokenTree::Group(NwTokenGroup {
            delimiter: NwDelimiter::Brace,
            open_span: start,
            close_span: end,
            stream,
        }));
        source_files.insert(source_id.get(), source);
    }
    let macro_source = resolve_event_dispatcher_macro(input)
        .and_then(|source| materialize_event_catalog(&source))
        .map_err(plain_diagnostic)?;
    let generated = run_event_dispatcher_macro(
        macro_input,
        invocation_span.unwrap_or_else(|| Span::new(SourceId::new(0), 0, 0)),
        &macro_source,
    )
    .map_err(|error| {
        error
            .span
            .and_then(|span| {
                source_files
                    .get(&span.source_id.get())
                    .map(|file| (file, span))
            })
            .map_or_else(
                || {
                    plain_diagnostic(format!(
                        "failed to execute nwnrs event project macro: {error}"
                    ))
                },
                |(file, span)| {
                    source_diagnostic(
                        format!("failed to execute nwnrs event project macro: {error}"),
                        None,
                        file,
                        span,
                    )
                },
            )
    })?;
    Ok(Some(GeneratedEventDispatcher {
        name:         "_nwnrs_onload".to_string(),
        source:       generated,
        include_root: source_root,
    }))
}

fn overlay_contents<'a>(overlays: &'a BTreeMap<PathBuf, Vec<u8>>, path: &Path) -> Option<&'a [u8]> {
    overlays.iter().find_map(|(candidate, contents)| {
        paths_match(candidate, path).then_some(contents.as_slice())
    })
}

fn paths_match(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => left
            .to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy()),
    }
}

fn resolve_event_dispatcher_macro(input: &Path) -> Result<String, String> {
    for dependency in crate::resolve_include_dependencies(input)? {
        let candidate = dependency.source_root.join("nwnrs_macros.nss");
        if candidate.is_file() {
            return fs::read_to_string(&candidate).map_err(|error| {
                format!(
                    "failed to read nwnrs project macro {}: {error}",
                    candidate.display()
                )
            });
        }
    }
    Ok(EVENT_DISPATCHER_MACRO_SOURCE.to_string())
}

fn materialize_event_catalog(template: &str) -> Result<String, String> {
    const BEGIN: &str = "// NWNRS_EVENT_CATALOG_BEGIN";
    const END: &str = "// NWNRS_EVENT_CATALOG_END";
    let Some(begin) = template.find(BEGIN) else {
        if template.contains(END) {
            return Err("nwnrs event macro is missing its catalog begin marker".to_string());
        }
        return Ok(template.to_string());
    };
    let end = template
        .find(END)
        .ok_or_else(|| "nwnrs event macro is missing its catalog end marker".to_string())?;
    if end <= begin {
        return Err("nwnrs event macro catalog markers are out of order".to_string());
    }

    let mut generated = String::new();
    for (index, event) in nwnrs_runtime::EVENT_CATALOG.iter().enumerate() {
        if index == 0 {
            generated.push_str("if");
        } else {
            generated.push_str("else if");
        }
        generated.push_str(" (event_name == \"");
        generated.push_str(event.identity);
        generated.push_str("\")\n{\n    dispatcher = quote! { if (sEventName == \"");
        generated.push_str(event.name);
        generated.push_str("\" && sEventPhase == \"");
        generated.push_str(event.phase);
        generated.push_str("\") { $handler(jEvent); } };\n");
        generated.push_str("    subscription = quote! { NWNXPushString(\"");
        generated.push_str(event.identity);
        generated.push_str("\"); NWNXCall(\"NWNRS\", \"SubscribeEvent\"); };\n}\n");
    }

    let mut materialized = String::with_capacity(template.len() + generated.len());
    materialized.push_str(&template[..begin]);
    materialized.push_str(&generated);
    materialized.push_str(&template[end + END.len()..]);
    Ok(materialized)
}

fn run_event_dispatcher_macro(
    input: NwTokenStream,
    invocation_span: Span,
    macro_source: &str,
) -> Result<String, nwnrs_nwscript::MacroExpansionError> {
    let tokens = lex_text(EVENT_DISPATCHER_MACRO_SOURCE_ID, macro_source).map_err(|error| {
        nwnrs_nwscript::MacroExpansionError::without_span(format!(
            "failed to lex nwnrs event project macro: {error}"
        ))
    })?;
    let mut definitions = NwTokenStream::from_tokens(&tokens)?;
    let mut registry = MacroRegistry::new();
    collect_nwscript_macros(&mut definitions, &mut registry)?;
    let quoted = expand_registered_macro(
        &registry,
        "nwnrs::__build_event_dispatcher",
        input,
        invocation_span,
        MacroExpansionOptions::default(),
    )?;
    let mut generated = String::from("/// Generated by nwpkg. Do not edit.\n");
    generated.push_str(&render_nwscript_tokens(&quoted));
    generated.push('\n');
    Ok(generated)
}

fn plain_diagnostic(message: impl Into<String>) -> CompilerDiagnostic {
    CompilerDiagnostic {
        code:         None,
        message:      message.into(),
        file:         None,
        start_line:   None,
        start_column: None,
        end_line:     None,
        end_column:   None,
    }
}

fn source_diagnostic(
    message: impl Into<String>,
    code: Option<i32>,
    file: &SourceFile,
    span: Span,
) -> CompilerDiagnostic {
    let start = file.location(span.start);
    let end = file.location(span.end.min(file.len()));
    CompilerDiagnostic {
        code,
        message: message.into(),
        file: Some(file.name.clone()),
        start_line: start.map(|location| location.line),
        start_column: start.map(|location| location.column),
        end_line: end.map(|location| location.line),
        end_column: end.map(|location| location.column),
    }
}

fn collect_nss_paths(root: &Path, paths: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(root)
        .map_err(|error| format!("failed to read module source {}: {error}", root.display()))?
    {
        let path = entry
            .map_err(|error| format!("failed to read entry in {}: {error}", root.display()))?
            .path();
        if path.is_dir() {
            collect_nss_paths(&path, paths)?;
        } else if path.is_file()
            && path
                .extension()
                .and_then(OsStr::to_str)
                .is_some_and(|extension| extension.eq_ignore_ascii_case("nss"))
        {
            paths.push(path);
        }
    }
    Ok(())
}

/// Finds the nearest project root owning a file or directory.
///
/// # Errors
///
/// Returns an error when the discovered project directory cannot be resolved.
pub fn find_project_root(input: &Path) -> Result<Option<PathBuf>, String> {
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
            return fs::canonicalize(&current).map(Some).map_err(|error| {
                format!("failed to resolve project {}: {error}", current.display())
            });
        }
        if !current.pop() {
            return Ok(None);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{
        EVENT_DISPATCHER_MACRO_SOURCE, generate_event_dispatcher,
        generate_event_dispatcher_with_diagnostics, generate_event_dispatcher_with_overlays,
        materialize_event_catalog,
    };

    fn assert_dispatcher_parses(source: &str) -> Result<(), String> {
        let tokens = nwnrs_nwscript::lex_text(nwnrs_nwscript::SourceId::new(0), source)
            .map_err(|error| error.to_string())?;
        let langspec = nwnrs_nwscript::parse_langspec(
            "event-test",
            "#define ENGINE_NUM_STRUCTURES 1\n#define ENGINE_STRUCTURE_0 json\n",
        )
        .map_err(|error| error.to_string())?;
        nwnrs_nwscript::parse_tokens(tokens, Some(&langspec))
            .map(|_script| ())
            .map_err(|error| format!("{error}\ngenerated source:\n{source}"))
    }

    fn test_root(name: &str) -> Result<PathBuf, String> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos();
        let root = std::env::temp_dir().join(format!("nwnrs-generation-{name}-{nonce}"));
        fs::create_dir_all(&root).map_err(|error| error.to_string())?;
        fs::write(
            root.join("nwpkg.toml"),
            "[project]\nname = \"fixture\"\nkind = \"mod\"\n\n[source]\npath = \".\"\n",
        )
        .map_err(|error| error.to_string())?;
        Ok(root)
    }

    fn documentation_before<'a>(lines: &[&'a str], declaration: usize) -> Vec<&'a str> {
        let mut documentation = Vec::new();
        for line in lines.iter().take(declaration).rev() {
            let Some(line) = line.trim().strip_prefix("///") else {
                break;
            };
            documentation.push(line.trim());
        }
        documentation.reverse();
        documentation
    }

    fn parameter_names(declaration: &str) -> Vec<&str> {
        let Some(open) = declaration.find('(') else {
            return Vec::new();
        };
        let Some(close) = declaration.rfind(')') else {
            return Vec::new();
        };
        declaration[open + 1..close]
            .split(',')
            .filter_map(|parameter| {
                let parameter = parameter.split('=').next()?.trim();
                parameter.split_whitespace().last()
            })
            .collect()
    }

    fn assert_documented_parameters(documentation: &[&str], declaration: &str) {
        let documentation = documentation.join("\n");
        for parameter in parameter_names(declaration) {
            assert!(
                documentation.contains(&format!("@param {parameter}")),
                "{declaration} is missing documentation for parameter {parameter}"
            );
        }
    }

    fn is_nwnrs_function_signature(line: &str) -> bool {
        let Some((before_parameters, _parameters)) = line.split_once('(') else {
            return false;
        };
        let mut words = before_parameters.split_whitespace();
        let Some(return_type) = words.next() else {
            return false;
        };
        let Some(name) = words.next() else {
            return false;
        };
        !return_type.is_empty() && name.starts_with("NWNRS_") && words.next().is_none()
    }

    fn assert_public_include_is_documented(source: &str) {
        let lines = source.lines().collect::<Vec<_>>();
        let mut constants = 0;
        let mut enums = 0;
        let mut enum_variants = 0;
        let mut functions = 0;
        let mut line = 0;

        while line < lines.len() {
            let Some(source_line) = lines.get(line) else {
                break;
            };
            let trimmed = source_line.trim();
            if trimmed.starts_with("const ") {
                constants += 1;
                assert!(
                    !documentation_before(&lines, line).is_empty(),
                    "{} is missing a /// documentation block",
                    trimmed.split('=').next().unwrap_or(trimmed).trim()
                );
            } else if trimmed.starts_with("enum ") {
                enums += 1;
                assert!(
                    !documentation_before(&lines, line).is_empty(),
                    "{trimmed} is missing a /// documentation block"
                );
                line += 1;
                while line < lines.len() {
                    let Some(variant_line) = lines.get(line).map(|line| line.trim()) else {
                        break;
                    };
                    if variant_line.starts_with('}') {
                        break;
                    }
                    if variant_line.is_empty() || variant_line.starts_with("///") {
                        line += 1;
                        continue;
                    }
                    let declaration_line = line;
                    let mut variant = variant_line;
                    loop {
                        while let Some(attribute) = variant.strip_prefix("#[") {
                            let Some(end) = attribute.find(']') else {
                                break;
                            };
                            variant = attribute[end + 1..].trim();
                        }
                        if !variant.is_empty() {
                            break;
                        }
                        line += 1;
                        variant = lines.get(line).map_or("", |line| line.trim());
                    }
                    enum_variants += 1;
                    assert!(
                        !documentation_before(&lines, declaration_line).is_empty(),
                        "{variant} is missing a /// documentation block"
                    );
                    line += 1;
                }
            } else if is_nwnrs_function_signature(trimmed) {
                let declaration_line = line;
                let mut declaration = trimmed.to_string();
                while !declaration.contains(')') && line + 1 < lines.len() {
                    line += 1;
                    declaration.push(' ');
                    if let Some(source_line) = lines.get(line) {
                        declaration.push_str(source_line.trim());
                    }
                }

                functions += 1;
                let documentation = documentation_before(&lines, declaration_line);
                assert!(
                    !documentation.is_empty(),
                    "{declaration} is missing a /// documentation block"
                );
                assert_documented_parameters(&documentation, &declaration);
            }
            line += 1;
        }

        assert!(constants > 0, "documentation audit found no API constants");
        assert!(enums > 0, "documentation audit found no API enums");
        assert!(
            enum_variants > 0,
            "documentation audit found no API enum variants"
        );
        assert!(functions > 0, "documentation audit found no API functions");
    }

    fn assert_compiler_macros_are_documented(source: &str) {
        let lines = source.lines().collect::<Vec<_>>();
        let mut macros = 0;
        for (line, source_line) in lines.iter().enumerate() {
            let trimmed = source_line.trim();
            if !trimmed.starts_with("proc_macro!") && !trimmed.starts_with("macro_rules!") {
                continue;
            }

            macros += 1;
            let documentation = documentation_before(&lines, line);
            assert!(
                !documentation.is_empty(),
                "{trimmed} is missing a /// documentation block"
            );

            if trimmed.starts_with("proc_macro!") {
                let signature_line =
                    lines
                        .iter()
                        .enumerate()
                        .skip(line + 1)
                        .find(|(_line, source_line)| {
                            let source_line = source_line.trim();
                            !source_line.starts_with("///") && source_line.contains('(')
                        });
                let (signature_line, signature) = signature_line
                    .map(|(line, signature)| (line, signature.trim()))
                    .unwrap_or((line, ""));
                assert_documented_parameters(&documentation, signature);
                assert!(
                    documentation
                        .iter()
                        .any(|line| line.starts_with("@return ")),
                    "{trimmed} is missing @return documentation"
                );

                let implementation_documentation = documentation_before(&lines, signature_line);
                assert!(
                    !implementation_documentation.is_empty(),
                    "{signature} is missing a /// documentation block"
                );
                assert_documented_parameters(&implementation_documentation, signature);
                assert!(
                    implementation_documentation
                        .iter()
                        .any(|line| line.starts_with("@return ")),
                    "{signature} is missing @return documentation"
                );
            }
        }
        assert!(macros > 0, "documentation audit found no compiler macros");
    }

    #[test]
    fn always_generates_empty_module_dispatcher() -> Result<(), String> {
        let root = test_root("empty")?;
        fs::write(root.join("ordinary.nss"), "void main() {}\n")
            .map_err(|error| error.to_string())?;
        let dispatcher = generate_event_dispatcher(&root)?
            .ok_or_else(|| "module did not generate a dispatcher".to_string())?;
        assert_eq!(dispatcher.name, "_nwnrs_onload");
        assert!(dispatcher.source.contains("void main"));
        assert!(!dispatcher.source.contains("GetCurrentEvent"));
        assert_dispatcher_parses(&dispatcher.source)?;
        fs::remove_dir_all(root).map_err(|error| error.to_string())
    }

    #[test]
    fn aggregates_annotated_functions_deterministically() -> Result<(), String> {
        let root = test_root("handlers")?;
        fs::write(root.join("ordinary.nss"), "void Ordinary() {}\n")
            .map_err(|error| error.to_string())?;
        fs::write(
            root.join("startup.nss"),
            "#[nwnrs::events(module_load)]\nvoid ProjectStart(json jEvent) \
             {}\n#[nwnrs::events(module_load)]\nvoid ProjectBeforeStart(json jEvent) {}\n",
        )
        .map_err(|error| error.to_string())?;
        let dispatcher = generate_event_dispatcher(&root)?
            .ok_or_else(|| "module did not generate a dispatcher".to_string())?;
        assert!(dispatcher.source.contains("#include \"startup\""));
        assert_eq!(dispatcher.source.matches("GetCurrentEvent").count(), 1);
        assert_eq!(dispatcher.source.matches("JsonParse").count(), 1);
        assert_eq!(dispatcher.source.matches("ProjectStart").count(), 1);
        assert_eq!(dispatcher.source.matches("ProjectBeforeStart").count(), 1);
        let before = dispatcher
            .source
            .find("ProjectBeforeStart")
            .ok_or_else(|| "missing first sorted handler".to_string())?;
        let start = dispatcher
            .source
            .find("ProjectStart")
            .ok_or_else(|| "missing second sorted handler".to_string())?;
        assert!(before < start);
        assert!(!dispatcher.source.contains("#include \"ordinary\""));
        assert!(!dispatcher.source.contains("#["));
        assert_dispatcher_parses(&dispatcher.source)?;
        fs::remove_dir_all(root).map_err(|error| error.to_string())
    }

    #[test]
    fn event_generation_uses_unsaved_source_overlays() -> Result<(), String> {
        let root = test_root("overlay-handlers")?;
        let source = root.join("startup.nss");
        fs::write(&source, "void Ordinary() {}\n").map_err(|error| error.to_string())?;
        let overlays = BTreeMap::from([(
            source,
            b"#[nwnrs::events(module_load)]\nvoid UnsavedHandler(json jEvent) {}\n".to_vec(),
        )]);

        let dispatcher = generate_event_dispatcher_with_overlays(&root, &overlays)
            .map_err(|diagnostic| diagnostic.message)?
            .ok_or_else(|| "module did not generate a dispatcher".to_string())?;

        assert!(dispatcher.source.contains("UnsavedHandler"));
        assert!(dispatcher.source.contains("#include \"startup\""));
        assert_dispatcher_parses(&dispatcher.source)?;
        fs::remove_dir_all(root).map_err(|error| error.to_string())
    }

    #[test]
    fn routes_native_event_phases_to_only_their_registered_handlers() -> Result<(), String> {
        let root = test_root("native-event-handlers")?;
        fs::write(
            root.join("associates.nss"),
            "#[nwnrs::events(associate_add_before)]\nvoid BeforeAdd(json jEvent) \
             {}\n#[nwnrs::events(associate_add_after)]\nvoid AfterAdd(json jEvent) \
             {}\n#[nwnrs::events(associate_remove_before)]\nvoid BeforeRemove(json jEvent) \
             {}\n#[nwnrs::events(associate_remove_after)]\nvoid AfterRemove(json jEvent) \
             {}\n#[nwnrs::events(associate_possess_familiar_before)]\nvoid BeforePossess(json \
             jEvent) {}\n#[nwnrs::events(associate_unpossess_familiar_after)]\nvoid \
             AfterUnpossess(json jEvent) {}\n",
        )
        .map_err(|error| error.to_string())?;
        let dispatcher = generate_event_dispatcher(&root)?
            .ok_or_else(|| "module did not generate a dispatcher".to_string())?;

        assert!(dispatcher.source.contains("associate.add"));
        assert!(dispatcher.source.contains("associate.remove"));
        assert!(dispatcher.source.contains("associate.possess_familiar"));
        assert!(dispatcher.source.contains("associate.unpossess_familiar"));
        assert!(dispatcher.source.contains("sEventPhase == \"before\""));
        assert!(dispatcher.source.contains("sEventPhase == \"after\""));
        for handler in [
            "BeforeAdd",
            "AfterAdd",
            "BeforeRemove",
            "AfterRemove",
            "BeforePossess",
            "AfterUnpossess",
        ] {
            assert_eq!(dispatcher.source.matches(handler).count(), 1);
        }
        assert_dispatcher_parses(&dispatcher.source)?;
        fs::remove_dir_all(root).map_err(|error| error.to_string())
    }

    #[test]
    fn routes_each_native_family_to_its_runtime_identity() -> Result<(), String> {
        let root = test_root("expanded-native-event-handlers")?;
        fs::write(
            root.join("events.nss"),
            "#[nwnrs::events(object_lock_before)]\nvoid Lock(json event) \
             {}\n#[nwnrs::events(object_unlock_after)]\nvoid Unlock(json event) \
             {}\n#[nwnrs::events(object_use_before)]\nvoid Use(json event) \
             {}\n#[nwnrs::events(placeable_open_after)]\nvoid Open(json event) \
             {}\n#[nwnrs::events(placeable_close_before)]\nvoid Close(json event) \
             {}\n#[nwnrs::events(inventory_add_gold_after)]\nvoid AddGold(json event) \
             {}\n#[nwnrs::events(inventory_remove_gold_before)]\nvoid RemoveGold(json event) \
             {}\n#[nwnrs::events(feat_use_after)]\nvoid UseFeat(json event) \
             {}\n#[nwnrs::events(journal_open_before)]\nvoid OpenJournal(json event) \
             {}\n#[nwnrs::events(journal_close_after)]\nvoid CloseJournal(json event) \
             {}\n#[nwnrs::events(timing_bar_start_before)]\nvoid StartTiming(json event) \
             {}\n#[nwnrs::events(timing_bar_stop_after)]\nvoid StopTiming(json event) \
             {}\n#[nwnrs::events(timing_bar_cancel_before)]\nvoid CancelTiming(json event) {}\n",
        )
        .map_err(|error| error.to_string())?;
        let dispatcher = generate_event_dispatcher(&root)?
            .ok_or_else(|| "module did not generate a dispatcher".to_string())?;

        for identity in [
            "object.lock",
            "object.unlock",
            "object.use",
            "placeable.open",
            "placeable.close",
            "inventory.add_gold",
            "inventory.remove_gold",
            "feat.use",
            "journal.open",
            "journal.close",
            "timing_bar.start",
            "timing_bar.stop",
            "timing_bar.cancel",
        ] {
            assert!(dispatcher.source.contains(identity), "missing {identity}");
        }
        assert_dispatcher_parses(&dispatcher.source)?;
        fs::remove_dir_all(root).map_err(|error| error.to_string())
    }

    #[test]
    fn nss_event_macro_rejects_invalid_handlers() -> Result<(), String> {
        let root = test_root("invalid-handler")?;
        fs::write(
            root.join("startup.nss"),
            "#[nwnrs::events(module_load)]\nvoid InvalidHandler() {}\n",
        )
        .map_err(|error| error.to_string())?;
        let error = generate_event_dispatcher(&root)
            .expect_err("NSS project macro should reject the handler");
        assert!(error.contains("event handler must accept exactly one json parameter"));
        fs::remove_dir_all(root).map_err(|error| error.to_string())
    }

    #[test]
    fn nss_event_macro_diagnostics_retain_the_handler_source_span() -> Result<(), String> {
        let root = test_root("invalid-handler-diagnostic")?;
        let source_path = root.join("startup.nss");
        fs::write(
            &source_path,
            "#[nwnrs::events(module_load)]\nvoid InvalidHandler() {}\n",
        )
        .map_err(|error| error.to_string())?;
        let error = generate_event_dispatcher_with_diagnostics(&root)
            .expect_err("NSS project macro should reject the handler");
        assert!(
            error
                .message
                .contains("event handler must accept exactly one json parameter")
        );
        let source_path = fs::canonicalize(source_path).map_err(|error| error.to_string())?;
        assert_eq!(
            error.file.as_deref(),
            Some(source_path.to_string_lossy().as_ref())
        );
        assert_eq!(error.start_line, Some(2));
        fs::remove_dir_all(root).map_err(|error| error.to_string())
    }

    #[test]
    fn nss_event_macro_diagnostics_point_to_an_unsupported_identity() -> Result<(), String> {
        let root = test_root("invalid-identity-diagnostic")?;
        let source_path = root.join("events.nss");
        fs::write(
            &source_path,
            "#[nwnrs::events(not_a_real_event)]\nvoid InvalidEvent(json event) {}\n",
        )
        .map_err(|error| error.to_string())?;
        let error = generate_event_dispatcher_with_diagnostics(&root)
            .expect_err("NSS project macro should reject the event identity");
        assert!(error.message.contains("unsupported nwnrs event identity"));
        let source_path = fs::canonicalize(source_path).map_err(|error| error.to_string())?;
        assert_eq!(
            error.file.as_deref(),
            Some(source_path.to_string_lossy().as_ref())
        );
        assert_eq!(error.start_line, Some(1));
        assert_eq!(error.start_column, Some(17));
        assert_eq!(error.end_column, Some(33));
        fs::remove_dir_all(root).map_err(|error| error.to_string())
    }

    #[test]
    fn include_and_embedded_templates_use_the_runtime_catalog() -> Result<(), String> {
        let workspace_copy =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../include/nwnrs/nwnrs_macros.nss");
        if workspace_copy.is_file() {
            let source = fs::read_to_string(&workspace_copy).map_err(|error| error.to_string())?;
            assert_eq!(source, EVENT_DISPATCHER_MACRO_SOURCE);
            for template in [&source, EVENT_DISPATCHER_MACRO_SOURCE] {
                let materialized = materialize_event_catalog(template)?;
                for event in nwnrs_runtime::EVENT_CATALOG {
                    assert!(materialized.contains(event.identity));
                }
            }
        }
        Ok(())
    }

    #[test]
    fn nwnrs_includes_have_complete_public_documentation() -> Result<(), String> {
        let include_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../include/nwnrs");
        let api = fs::read_to_string(include_root.join("nwnrs.nss"))
            .map_err(|error| error.to_string())?;
        let macros = fs::read_to_string(include_root.join("nwnrs_macros.nss"))
            .map_err(|error| error.to_string())?;

        assert_public_include_is_documented(&api);
        assert_compiler_macros_are_documented(&macros);
        assert_compiler_macros_are_documented(EVENT_DISPATCHER_MACRO_SOURCE);
        Ok(())
    }

    #[test]
    fn project_uses_macro_from_resolved_nwnrs_include_package() -> Result<(), String> {
        let root = test_root("dependency-macro")?;
        let include = root
            .parent()
            .ok_or_else(|| "test root has no parent".to_string())?
            .join(format!(
                "{}-include",
                root.file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("nwnrs")
            ));
        fs::create_dir_all(&include).map_err(|error| error.to_string())?;
        fs::write(
            root.join("nwpkg.toml"),
            format!(
                "[project]\nname = \"fixture\"\nkind = \"mod\"\n\n[source]\npath = \
                 \".\"\n\n[dependencies]\nnwnrs = {{ path = {:?} }}\n",
                include
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map_or_else(|| "../include".to_string(), |name| format!("../{name}"))
            ),
        )
        .map_err(|error| error.to_string())?;
        fs::write(
            include.join("nwpkg.toml"),
            "[project]\nname = \"nwnrs\"\nkind = \"include\"\n\n[source]\npath = \".\"\n",
        )
        .map_err(|error| error.to_string())?;
        fs::write(
            include.join("nwnrs_macros.nss"),
            r#"
                proc_macro! nwnrs::__build_event_dispatcher {
                    tokenstream __build_event_dispatcher(tokenstream input) {
                        return quote! { void main() { int dependency_macro = 1; } };
                    }
                }
            "#,
        )
        .map_err(|error| error.to_string())?;
        fs::write(root.join("ordinary.nss"), "void Ordinary() {}\n")
            .map_err(|error| error.to_string())?;

        let dispatcher = generate_event_dispatcher(&root)?
            .ok_or_else(|| "module did not generate a dispatcher".to_string())?;
        assert!(dispatcher.source.contains("dependency_macro"));

        fs::remove_dir_all(&root).map_err(|error| error.to_string())?;
        fs::remove_dir_all(include).map_err(|error| error.to_string())
    }
}
