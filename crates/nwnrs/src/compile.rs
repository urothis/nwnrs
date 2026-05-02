use std::{
    ffi::OsStr,
    fs,
    path::{Component, Path, PathBuf},
};

use nwnrs_nwscript as nwscript;
use nwnrs_types::prelude::*;
#[cfg(test)]
pub(crate) const DEFAULT_OPTIMIZATION: &str = "O0";

#[derive(Clone)]
pub(crate) struct CompileScriptOptions {
    pub(crate) debug:               bool,
    pub(crate) no_entrypoint_check: bool,
    pub(crate) langspec:            Option<PathBuf>,
    pub(crate) include_dirs:        Vec<PathBuf>,
    pub(crate) optimization:        nwscript::OptimizationLevel,
}

pub(crate) enum CompileScriptOutcome {
    Compiled(nwscript::CompileArtifacts),
    SkippedNoEntrypoint,
}

pub(crate) fn parse_optimization_level(value: &str) -> Result<nwscript::OptimizationLevel, String> {
    match value.to_ascii_uppercase().as_str() {
        "O0" => Ok(nwscript::OptimizationLevel::O0),
        "O1" => Ok(nwscript::OptimizationLevel::O1),
        "O2" => Ok(nwscript::OptimizationLevel::O2),
        "O3" => Ok(nwscript::OptimizationLevel::O3),
        _ => Err(format!("unsupported optimization level: {value}")),
    }
}

pub(crate) fn compile_script_file(
    input: &Path,
    options: &CompileScriptOptions,
) -> Result<nwscript::CompileArtifacts, String> {
    match compile_script_file_with_skip(input, options, false)? {
        CompileScriptOutcome::Compiled(artifacts) => Ok(artifacts),
        CompileScriptOutcome::SkippedNoEntrypoint => Err(format!(
            "failed to compile {}: script did not define an entrypoint",
            input.display()
        )),
    }
}

pub(crate) fn compile_script_file_with_skip(
    input: &Path,
    options: &CompileScriptOptions,
    skip_missing_entrypoint: bool,
) -> Result<CompileScriptOutcome, String> {
    if !input.is_file() {
        return Err(format!("input source does not exist: {}", input.display()));
    }

    let langspec_path =
        resolve_langspec_path(input, options.langspec.as_deref(), &options.include_dirs)?;
    let langspec_bytes = fs::read(&langspec_path)
        .map_err(|error| format!("failed to read {}: {error}", langspec_path.display()))?;
    let langspec =
        nwscript::parse_langspec_bytes(&langspec_path.display().to_string(), &langspec_bytes)
            .map_err(|error| format!("failed to parse {}: {error}", langspec_path.display()))?;

    let search_roots = script_search_roots(input, &options.include_dirs);
    let resolver = FilesystemScriptResolver::new(search_roots);
    let root_name = input
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| format!("input file name is not valid UTF-8: {}", input.display()))?;
    let bundle =
        nwscript::load_source_bundle(&resolver, root_name, nwscript::SourceLoadOptions::default())
            .map_err(|error| {
                format!(
                    "failed to load source bundle for {}: {error}",
                    input.display()
                )
            })?;
    let script = nwscript::parse_source_bundle(&bundle, Some(&langspec))
        .map_err(|error| format!("failed to parse {}: {error}", input.display()))?;
    if skip_missing_entrypoint && !script_has_entrypoint(&script) {
        return Ok(CompileScriptOutcome::SkippedNoEntrypoint);
    }

    let compile_options = nwscript::CompileOptions {
        semantic:     nwscript::SemanticOptions {
            require_entrypoint:       !options.no_entrypoint_check,
            allow_conditional_script: true,
        },
        optimization: options.optimization,
    };

    if options.debug {
        nwscript::compile_script_with_source_map(
            &script,
            &bundle.source_map,
            bundle.root_id,
            Some(&langspec),
            compile_options,
        )
    } else {
        nwscript::compile_script(&script, Some(&langspec), compile_options)
    }
    .map(CompileScriptOutcome::Compiled)
    .map_err(|error| format!("failed to compile {}: {error}", input.display()))
}

fn resolve_langspec_path(
    input: &Path,
    explicit: Option<&Path>,
    include_dirs: &[PathBuf],
) -> Result<PathBuf, String> {
    if let Some(path) = explicit {
        if !path.is_file() {
            return Err(format!("langspec file does not exist: {}", path.display()));
        }
        return Ok(path.to_path_buf());
    }

    for root in script_search_roots(input, include_dirs) {
        for candidate in [
            root.join("nwscript.nss"),
            root.join(nwscript::DEFAULT_LANGSPEC_SCRIPT_NAME),
        ] {
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }

    Err("failed to find nwscript.nss; pass --langspec explicitly".to_string())
}

fn script_search_roots(input: &Path, include_dirs: &[PathBuf]) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(parent) = input.parent() {
        roots.push(parent.to_path_buf());
    }
    for dir in include_dirs {
        if !dir.as_os_str().is_empty() && !roots.contains(dir) {
            roots.push(dir.clone());
        }
    }
    roots
}

struct FilesystemScriptResolver {
    search_roots: Vec<PathBuf>,
}

impl FilesystemScriptResolver {
    fn new(mut search_roots: Vec<PathBuf>) -> Self {
        search_roots.retain(|path| !path.as_os_str().is_empty());
        Self {
            search_roots,
        }
    }

    fn read_candidate(path: &Path) -> Result<Option<Vec<u8>>, nwscript::SourceError> {
        let Some(resolved) = resolve_case_insensitive(path) else {
            return Ok(None);
        };
        fs::read(&resolved).map(Some).map_err(|error| {
            nwscript::SourceError::resolver(format!(
                "failed to read {}: {error}",
                resolved.display()
            ))
        })
    }
}

impl nwscript::ScriptResolver for FilesystemScriptResolver {
    fn resolve_script_bytes(
        &self,
        script_name: &str,
        _res_type: resman::ResType,
    ) -> Result<Option<Vec<u8>>, nwscript::SourceError> {
        let path = Path::new(script_name);
        let mut candidates = Vec::new();

        if path.is_absolute() {
            candidates.push(path.to_path_buf());
            if path.extension().is_none() {
                candidates.push(path.with_extension("nss"));
            }
        } else {
            for root in &self.search_roots {
                let joined = root.join(path);
                candidates.push(joined.clone());
                if joined.extension().is_none() {
                    candidates.push(joined.with_extension("nss"));
                }
            }
        }

        for candidate in candidates {
            if let Some(bytes) = Self::read_candidate(&candidate)? {
                return Ok(Some(bytes));
            }
        }
        Ok(None)
    }
}

fn resolve_case_insensitive(path: &Path) -> Option<PathBuf> {
    if path.is_file() {
        return Some(path.to_path_buf());
    }

    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => current.push(prefix.as_os_str()),
            Component::RootDir => current.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => current.push(".."),
            Component::Normal(name) => {
                let search_dir = if current.as_os_str().is_empty() {
                    Path::new(".")
                } else {
                    current.as_path()
                };
                let entries = fs::read_dir(search_dir).ok()?;
                let mut matched = None;
                for entry in entries.flatten() {
                    if entry
                        .file_name()
                        .to_string_lossy()
                        .eq_ignore_ascii_case(&name.to_string_lossy())
                    {
                        matched = Some(entry.path());
                        break;
                    }
                }
                current = matched?;
            }
        }
    }

    current.is_file().then_some(current)
}

fn script_has_entrypoint(script: &nwscript::Script) -> bool {
    script.items.iter().any(|item| match item {
        nwscript::TopLevelItem::Function(function) => {
            function.body.is_some()
                && matches!(function.name.as_str(), "main" | "StartingConditional")
        }
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use nwnrs_nwscript::OptimizationLevel;

    use super::{CompileScriptOptions, compile_script_file};

    fn unique_test_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!("nwnrs-{prefix}-{nanos}"))
    }

    fn minimal_langspec() -> &'static str {
        r#"
#define ENGINE_NUM_STRUCTURES 0

int TRUE = 1;
int FALSE = 0;
"#
    }

    #[test]
    fn compile_helper_produces_ncs_and_ndb_outputs() {
        let temp_dir = unique_test_dir("nwscript-compile");
        fs::create_dir_all(&temp_dir).expect("create temp dir");
        let input = temp_dir.join("test.nss");
        fs::write(temp_dir.join("nwscript.nss"), minimal_langspec()).expect("write langspec");
        fs::write(&input, "int StartingConditional() { return TRUE; }").expect("write input");

        let artifacts = compile_script_file(
            &input,
            &CompileScriptOptions {
                debug:               true,
                no_entrypoint_check: false,
                langspec:            None,
                include_dirs:        Vec::new(),
                optimization:        OptimizationLevel::O0,
            },
        )
        .expect("compile should succeed");

        assert!(!artifacts.ncs.is_empty(), "NCS output should exist");
        assert!(artifacts.ndb.is_some(), "NDB output should exist");

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn compile_helper_resolves_include_directories() {
        let temp_dir = unique_test_dir("nwscript-compile-include");
        let include_dir = temp_dir.join("inc");
        fs::create_dir_all(&include_dir).expect("create include dir");
        let input = temp_dir.join("test.nss");
        fs::write(temp_dir.join("nwscript.nss"), minimal_langspec()).expect("write langspec");
        fs::write(
            include_dir.join("helper.nss"),
            "int helper() { return TRUE; }",
        )
        .expect("write include");
        fs::write(
            &input,
            "#include \"helper\"\nint StartingConditional() { return helper(); }",
        )
        .expect("write input");

        let artifacts = compile_script_file(
            &input,
            &CompileScriptOptions {
                debug:               false,
                no_entrypoint_check: false,
                langspec:            None,
                include_dirs:        vec![include_dir],
                optimization:        OptimizationLevel::O1,
            },
        )
        .expect("compile should succeed");

        assert!(!artifacts.ncs.is_empty(), "NCS output should exist");

        let _ = fs::remove_dir_all(temp_dir);
    }
}
