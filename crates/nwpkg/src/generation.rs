use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};

use nwnrs_nwscript::{
    MacroExpansionOptions, MacroRegistry, NwTokenStream, SourceFile, SourceId,
    expand_source_macros, lex_source, lex_text, render_nwscript_tokens,
};

use crate::{PROJECT_MANIFEST_FILENAME, read_project_manifest};

const EVENT_DISPATCHER_MACRO_SOURCE: &str = include_str!("../macros/nwnrs_macros.nss");

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
    let Some(project_root) = find_project_root(input)? else {
        return Ok(None);
    };
    let manifest = read_project_manifest(&project_root)?.ok_or_else(|| {
        format!(
            "missing {}",
            project_root.join(PROJECT_MANIFEST_FILENAME).display()
        )
    })?;
    if manifest.project.kind != crate::ProjectKind::Mod {
        return Ok(None);
    }
    let source_root = project_root.join(manifest.source.path);
    let source_root = fs::canonicalize(&source_root).map_err(|error| {
        format!(
            "failed to resolve module source {}: {error}",
            source_root.display()
        )
    })?;
    if !source_root.starts_with(&project_root) {
        return Err(format!(
            "module source {} escapes project root {}",
            source_root.display(),
            project_root.display()
        ));
    }

    let mut paths = Vec::new();
    collect_nss_paths(&source_root, &mut paths)?;
    paths.sort();
    let mut macro_input = String::new();
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
        let bytes = fs::read(path)
            .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
        let source_id = SourceId::new(u32::try_from(source_index).map_err(|_error| {
            "module contains too many NWScript source files for event collection".to_string()
        })?);
        let source = SourceFile::new(source_id, path.display().to_string(), bytes);
        let tokens = lex_source(&source)
            .map_err(|error| format!("failed to lex {}: {error}", path.display()))?;
        let stream = NwTokenStream::from_tokens(&tokens)
            .map_err(|error| format!("failed to balance {}: {error}", path.display()))?;
        let relative = path
            .strip_prefix(&source_root)
            .map_err(|error| format!("failed to relativize {}: {error}", path.display()))?;
        let include = relative
            .with_extension("")
            .to_string_lossy()
            .replace('\\', "/");
        macro_input.push_str("__nwnrs_source ");
        push_nwscript_string(&mut macro_input, &include);
        macro_input.push_str(" { ");
        macro_input.push_str(&render_nwscript_tokens(&stream));
        macro_input.push_str(" }\n");
    }
    let macro_source = resolve_event_dispatcher_macro(input)?;
    let generated = run_event_dispatcher_macro(&macro_input, &macro_source)?;
    Ok(Some(GeneratedEventDispatcher {
        name:         "_nwnrs_onload".to_string(),
        source:       generated,
        include_root: source_root,
    }))
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

fn run_event_dispatcher_macro(input: &str, macro_source: &str) -> Result<String, String> {
    let source_id = SourceId::new(0);
    let source = format!("{macro_source}\nnwnrs::__build_event_dispatcher! {{ {input} }}\n");
    let tokens = lex_text(source_id, &source)
        .map_err(|error| format!("failed to lex nwnrs event project macro: {error}"))?;
    let expanded = expand_source_macros(
        tokens,
        &mut MacroRegistry::new(),
        MacroExpansionOptions::default(),
    )
    .map_err(|error| format!("failed to execute nwnrs event project macro: {error}"))?;
    let quoted = NwTokenStream::from_tokens(&expanded)
        .map_err(|error| format!("event project macro returned invalid tokens: {error}"))?;
    let mut generated = String::from("/// Generated by nwpkg. Do not edit.\n");
    generated.push_str(&render_nwscript_tokens(&quoted));
    generated.push('\n');
    Ok(generated)
}

fn push_nwscript_string(output: &mut String, value: &str) {
    output.push('"');
    for character in value.chars() {
        match character {
            '\n' => output.push_str("\\n"),
            '\\' => output.push_str("\\\\"),
            '"' => output.push_str("\\\""),
            character => output.push(character),
        }
    }
    output.push('"');
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
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{EVENT_DISPATCHER_MACRO_SOURCE, generate_event_dispatcher};

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
            root.join("nwproject.toml"),
            "[project]\nname = \"fixture\"\nkind = \"mod\"\n\n[source]\npath = \".\"\n",
        )
        .map_err(|error| error.to_string())?;
        Ok(root)
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
    fn include_package_macro_matches_embedded_project_macro() -> Result<(), String> {
        let workspace_copy =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../include/nwnrs/nwnrs_macros.nss");
        if workspace_copy.is_file() {
            let source = fs::read_to_string(&workspace_copy).map_err(|error| error.to_string())?;
            assert_eq!(source, EVENT_DISPATCHER_MACRO_SOURCE);
        }
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
            root.join("nwproject.toml"),
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
            include.join("nwproject.toml"),
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
