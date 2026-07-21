use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};

use nwnrs_nwscript as nwscript;
use nwnrs_types::{
    install,
    resman::prelude::{CachePolicy, ResMan, ResRef, ResType},
};
use tracing::instrument;

use crate::{args::CompileCmd, util::write_stdout_line};

#[cfg(test)]
pub(crate) const DEFAULT_OPTIMIZATION: &str = "O1";

pub(crate) type SharedInstallResMan = Arc<Mutex<ResMan>>;

#[derive(Clone)]
pub(crate) struct CompileScriptOptions {
    pub(crate) debug:               bool,
    pub(crate) no_entrypoint_check: bool,
    pub(crate) langspec:            Option<PathBuf>,
    pub(crate) include_dirs:        Vec<PathBuf>,
    pub(crate) optimizations:       nwscript::OptimizationFlags,
    pub(crate) max_include_depth:   usize,
    pub(crate) install_resman:      Option<SharedInstallResMan>,
}

pub(crate) enum CompileScriptOutcome {
    Compiled(nwscript::CompileArtifacts),
    SkippedNoEntrypoint,
}

fn parse_optimization_level(value: &str) -> Result<nwscript::OptimizationLevel, String> {
    match value.to_ascii_uppercase().as_str() {
        "O0" => Ok(nwscript::OptimizationLevel::O0),
        "O1" => Ok(nwscript::OptimizationLevel::O1),
        "O2" => Ok(nwscript::OptimizationLevel::O2),
        "O3" => Ok(nwscript::OptimizationLevel::O3),
        _ => Err(format!("unsupported optimization level: {value}")),
    }
}

pub(crate) fn parse_optimizations(
    preset: &str,
    individual: &[String],
) -> Result<nwscript::OptimizationFlags, String> {
    if individual.is_empty() {
        return Ok(parse_optimization_level(preset)?.into());
    }

    let mut flags = nwscript::OptimizationFlags::O0;
    for value in individual {
        let flag = match value.to_ascii_lowercase().replace('_', "-").as_str() {
            "remove-dead-code" | "dead-code" => nwscript::OptimizationFlag::RemoveDeadCode,
            "meld-instructions" | "meld" => nwscript::OptimizationFlag::MeldInstructions,
            "remove-dead-branches" | "dead-branches" => {
                nwscript::OptimizationFlag::RemoveDeadBranches
            }
            _ => {
                return Err(format!(
                    "unsupported optimization flag {value:?}; expected remove-dead-code, \
                     meld-instructions, or remove-dead-branches"
                ));
            }
        };
        flags |= flag;
    }
    Ok(flags)
}

fn parse_graphviz_format(value: &str) -> Result<nwscript::GraphvizOutputFormat, String> {
    match value.to_ascii_lowercase().as_str() {
        "dot" => Ok(nwscript::GraphvizOutputFormat::Dot),
        "svg" => Ok(nwscript::GraphvizOutputFormat::Svg),
        "png" => Ok(nwscript::GraphvizOutputFormat::Png),
        "pdf" => Ok(nwscript::GraphvizOutputFormat::Pdf),
        _ => Err(format!(
            "unsupported Graphviz format {value:?}; expected svg, png, pdf, or dot"
        )),
    }
}

pub(crate) fn compile_script_file(
    input: &Path,
    options: &CompileScriptOptions,
) -> Result<nwscript::CompileArtifacts, String> {
    match compile_script_file_outcome(input, options, false)? {
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
    compile_script_file_outcome(input, options, skip_missing_entrypoint)
}

fn compile_script_file_outcome(
    input: &Path,
    options: &CompileScriptOptions,
    skip_missing_entrypoint: bool,
) -> Result<CompileScriptOutcome, String> {
    if !input.is_file() {
        return Err(format!("input source does not exist: {}", input.display()));
    }

    if let Some(langspec) = options.langspec.as_deref()
        && !langspec.is_file()
    {
        return Err(format!(
            "langspec file does not exist: {}",
            langspec.display()
        ));
    }

    let mut resolver = nwscript::FileSystemScriptResolver::new();
    for root in script_search_roots(input, &options.include_dirs) {
        resolver.add_root(root);
    }
    let mut host = CliCompilerHost {
        resolver,
        install_resman: options.install_resman.clone(),
        overlay: None,
    };
    let driver_options = nwscript::CompilerDriverOptions {
        session: nwscript::CompilerSessionOptions {
            langspec_script_name: options.langspec.as_ref().map_or_else(
                || nwscript::DEFAULT_LANGSPEC_SCRIPT_NAME.to_string(),
                |path| path.to_string_lossy().into_owned(),
            ),
            compile:              nwscript::CompileOptions {
                semantic:      nwscript::SemanticOptions {
                    require_entrypoint:       !options.no_entrypoint_check,
                    allow_conditional_script: true,
                },
                optimizations: options.optimizations,
            },
            source_load:          nwscript::SourceLoadOptions {
                max_include_depth: options.max_include_depth,
                ..nwscript::SourceLoadOptions::default()
            },
            emit_debug:           options.debug,
        },
        output_alias: input
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("scriptout")
            .to_string(),
        skip_missing_entrypoint,
        ..nwscript::CompilerDriverOptions::default()
    };

    let outcome =
        nwscript::compile_file_with_host(&mut host, &input.to_string_lossy(), &driver_options)
            .map_err(|compile_error| {
                nwscript::format_source_aware_driver_error(
                    &compile_error,
                    &host,
                    input,
                    driver_options.session.source_load,
                )
            })?;
    let outcome = match outcome {
        nwscript::CompileFileOutcome::Compiled(artifacts) => {
            CompileScriptOutcome::Compiled(artifacts)
        }
        nwscript::CompileFileOutcome::SkippedNoEntrypoint => {
            CompileScriptOutcome::SkippedNoEntrypoint
        }
    };
    Ok(outcome)
}

pub(crate) fn compile_generated_script(
    source_name: &str,
    source: &[u8],
    include_dirs: &[PathBuf],
    options: &CompileScriptOptions,
) -> Result<nwscript::CompileArtifacts, String> {
    let mut resolver = nwscript::FileSystemScriptResolver::new();
    for root in include_dirs.iter().chain(&options.include_dirs) {
        resolver.add_root(root);
    }
    let logical_name = format!("{source_name}.nss");
    let mut host = CliCompilerHost {
        resolver,
        install_resman: options.install_resman.clone(),
        overlay: Some((logical_name.clone(), source.to_vec())),
    };
    let driver_options = nwscript::CompilerDriverOptions {
        session: nwscript::CompilerSessionOptions {
            langspec_script_name: options.langspec.as_ref().map_or_else(
                || nwscript::DEFAULT_LANGSPEC_SCRIPT_NAME.to_string(),
                |path| path.to_string_lossy().into_owned(),
            ),
            compile:              nwscript::CompileOptions {
                semantic:      nwscript::SemanticOptions {
                    require_entrypoint:       true,
                    allow_conditional_script: true,
                },
                optimizations: options.optimizations,
            },
            source_load:          nwscript::SourceLoadOptions {
                max_include_depth: options.max_include_depth,
                ..nwscript::SourceLoadOptions::default()
            },
            emit_debug:           options.debug,
        },
        output_alias: source_name.to_string(),
        ..nwscript::CompilerDriverOptions::default()
    };
    match nwscript::compile_file_with_host(&mut host, &logical_name, &driver_options)
        .map_err(|error| format!("failed to compile generated {logical_name}: {error}"))?
    {
        nwscript::CompileFileOutcome::Compiled(artifacts) => Ok(artifacts),
        nwscript::CompileFileOutcome::SkippedNoEntrypoint => Err(format!(
            "generated {logical_name} did not define an entrypoint"
        )),
    }
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

pub(crate) struct CliCompilerHost {
    resolver:       nwscript::FileSystemScriptResolver,
    install_resman: Option<SharedInstallResMan>,
    overlay:        Option<(String, Vec<u8>)>,
}

impl CliCompilerHost {
    pub(crate) fn for_source(
        input: &Path,
        include_dirs: &[PathBuf],
        install_resman: Option<SharedInstallResMan>,
    ) -> Self {
        let mut resolver = nwscript::FileSystemScriptResolver::new();
        for root in script_search_roots(input, include_dirs) {
            resolver.add_root(root);
        }
        Self {
            resolver,
            install_resman,
            overlay: None,
        }
    }
}

impl nwscript::ScriptResolver for CliCompilerHost {
    fn resolve_script_bytes(
        &self,
        script_name: &str,
        res_type: ResType,
    ) -> Result<Option<Vec<u8>>, nwscript::SourceError> {
        if res_type == nwscript::NW_SCRIPT_SOURCE_RES_TYPE
            && let Some((name, bytes)) = &self.overlay
            && Path::new(script_name)
                .file_name()
                .is_some_and(|requested| requested.eq_ignore_ascii_case(name))
        {
            return Ok(Some(bytes.clone()));
        }
        if let Some(bytes) =
            nwscript::ScriptResolver::resolve_script_bytes(&self.resolver, script_name, res_type)?
        {
            return Ok(Some(bytes));
        }
        let Some(install_resman) = &self.install_resman else {
            return Ok(None);
        };
        let logical_name = Path::new(script_name)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(script_name);
        let logical_path = Path::new(logical_name);
        let expected_extension = nwnrs_types::resman::get_res_ext(res_type);
        let resref = if logical_path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case(&expected_extension))
        {
            logical_path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or(logical_name)
        } else {
            logical_name
        };
        let rr = match ResRef::new(resref, res_type) {
            Ok(rr) => rr,
            Err(_) => return Ok(None),
        };
        let mut resman = install_resman
            .lock()
            .map_err(|lock_error| nwscript::SourceError::resolver(lock_error.to_string()))?;
        let Some(resource) = resman.get(&rr) else {
            return Ok(None);
        };
        resource
            .read_all(CachePolicy::Bypass)
            .map(Some)
            .map_err(|read_error| nwscript::SourceError::resolver(read_error.to_string()))
    }
}

impl nwscript::CompilerHost for CliCompilerHost {
    fn resolve_script_bytes(
        &self,
        script_name: &str,
        res_type: ResType,
    ) -> Result<Option<Vec<u8>>, nwscript::SourceError> {
        nwscript::ScriptResolver::resolve_script_bytes(self, script_name, res_type)
    }

    fn write_file(
        &mut self,
        _file_name: &str,
        _res_type: ResType,
        _data: &[u8],
        _binary: bool,
    ) -> Result<(), nwscript::CompilerHostError> {
        Ok(())
    }

    fn write_graphviz(
        &mut self,
        _file_name: &str,
        _dot: &str,
    ) -> Result<(), nwscript::CompilerHostError> {
        Ok(())
    }
}

pub(crate) fn build_install_resman(
    root: Option<&Path>,
    user: Option<&Path>,
    language: &str,
    load_ovr: bool,
) -> Result<Option<SharedInstallResMan>, String> {
    let explicit = root.is_some() || user.is_some() || load_ovr;
    let root_override = root
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default();
    let user_override = user
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default();
    let root = match install::find_nwnrs_root(&root_override) {
        Ok(root) => root,
        Err(_error) if !explicit => return Ok(None),
        Err(error) => return Err(format!("failed to locate NWN installation: {error}")),
    };
    let user = match install::find_user_root(&user_override) {
        Ok(user) => user,
        Err(_error) if !explicit => return Ok(None),
        Err(error) => return Err(format!("failed to locate NWN user directory: {error}")),
    };
    let resman = match install::new_default_resman(
        &root,
        &user,
        language,
        64,
        true,
        load_ovr,
        &[],
        &[],
        &[],
        &[],
    ) {
        Ok(resman) => resman,
        Err(_error) if !explicit => return Ok(None),
        Err(error) => {
            return Err(format!(
                "failed to build install resource manager (root={}, user={}): {error}",
                root.display(),
                user.display()
            ));
        }
    };
    Ok(Some(Arc::new(Mutex::new(resman))))
}

pub(crate) fn build_install_script_resolver(
    root: Option<&Path>,
    user: Option<&Path>,
    language: &str,
    load_ovr: bool,
) -> Result<Option<nwscript::SharedScriptResolver>, String> {
    Ok(
        build_install_resman(root, user, language, load_ovr)?.map(|install_resman| {
            Arc::new(CliCompilerHost {
                resolver:       nwscript::FileSystemScriptResolver::new(),
                install_resman: Some(install_resman),
                overlay:        None,
            }) as nwscript::SharedScriptResolver
        }),
    )
}

pub(crate) fn autodetected_install_resman() -> Option<SharedInstallResMan> {
    static INSTALL_RESMAN: OnceLock<Option<SharedInstallResMan>> = OnceLock::new();
    INSTALL_RESMAN
        .get_or_init(|| build_install_resman(None, None, "english", false).unwrap_or(None))
        .clone()
}

#[instrument(level = "info", skip_all, err, fields(path_count = cmd.paths.len()))]
pub(crate) fn run_compile(cmd: CompileCmd) -> Result<(), String> {
    if !(1..=200).contains(&cmd.max_include_depth) {
        return Err("maximum include depth must be between 1 and 200".to_string());
    }
    if cmd.keep_graphviz_dot && cmd.graphviz.is_none() {
        return Err("--keep-graphviz-dot requires --graphviz".to_string());
    }
    let optimizations = parse_optimizations(&cmd.optimization, &cmd.optimization_flag)?;
    let graphviz_format = parse_graphviz_format(&cmd.graphviz_format)?;
    let fallback_resolver = build_install_script_resolver(
        cmd.root.as_deref(),
        cmd.user.as_deref(),
        &cmd.language,
        cmd.load_ovr,
    )?;
    let mut search_roots = cmd.include_dir.clone();
    for input in &cmd.paths {
        for dependency in nwnrs_nwpkg::resolve_include_dependencies(input)? {
            if !search_roots.contains(&dependency.source_root) {
                search_roots.push(dependency.source_root);
            }
        }
    }
    let options = nwscript::BatchCompileOptions {
        driver: nwscript::CompilerDriverOptions {
            session: nwscript::CompilerSessionOptions {
                langspec_script_name: cmd.langspec.as_ref().map_or_else(
                    || nwscript::DEFAULT_LANGSPEC_SCRIPT_NAME.to_string(),
                    |path| path.to_string_lossy().into_owned(),
                ),
                source_load:          nwscript::SourceLoadOptions {
                    max_include_depth: cmd.max_include_depth,
                    ..nwscript::SourceLoadOptions::default()
                },
                compile:              nwscript::CompileOptions {
                    semantic: nwscript::SemanticOptions {
                        require_entrypoint:       !cmd.no_entrypoint_check,
                        allow_conditional_script: true,
                    },
                    optimizations,
                },
                emit_debug:           cmd.debug,
            },
            emit_graphviz: cmd.graphviz.is_some(),
            skip_missing_entrypoint: !cmd.no_entrypoint_check,
            ..nwscript::CompilerDriverOptions::default()
        },
        search_roots,
        fallback_resolver,
        recurse: cmd.recurse,
        follow_symlinks: cmd.follow_symlinks,
        continue_on_error: cmd.continue_on_error,
        simulate: cmd.simulate,
        overwrite_existing: cmd.force,
        remove_stale_debug: true,
        jobs: cmd.jobs,
        output_file: cmd.output.clone(),
        output_directory: cmd.directory.clone(),
        graphviz_directory: cmd.graphviz.clone(),
        graphviz_format,
        keep_graphviz_dot: cmd.keep_graphviz_dot,
    };
    let report =
        nwscript::compile_paths(&cmd.paths, &options).map_err(|error| error.to_string())?;
    for entry in &report.entries {
        match entry.status {
            nwscript::BatchCompileStatus::Success => {
                let verb = if cmd.simulate {
                    "would compile"
                } else {
                    "compiled"
                };
                let outputs = entry
                    .outputs
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                write_stdout_line(&format!(
                    "[ok] {verb} {} -> {outputs}",
                    entry.input.display()
                ))?;
            }
            nwscript::BatchCompileStatus::Skipped => {
                write_stdout_line(&format!(
                    "[skip] {} has no entrypoint",
                    entry.input.display()
                ))?;
            }
            nwscript::BatchCompileStatus::Error => {
                write_stdout_line(&format!(
                    "[error] {}: {}",
                    entry.input.display(),
                    entry
                        .error
                        .as_deref()
                        .unwrap_or("unknown compilation error")
                ))?;
            }
        }
    }
    let mode = if cmd.simulate {
        "simulated"
    } else {
        "complete"
    };
    write_stdout_line(&format!(
        "NWScript compile {mode}: {} compiled, {} skipped, {} failed",
        report.successes, report.skips, report.errors
    ))?;
    if report.errors == 0 {
        Ok(())
    } else {
        Err(format!("{} NWScript compilations failed", report.errors))
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{
        CompileScriptOptions, compile_script_file, parse_graphviz_format, parse_optimizations,
        run_compile,
    };
    use crate::args::CompileCmd;

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
int LOCAL_ONLY = 7;
"#
    }

    fn compile_cmd(input: PathBuf) -> CompileCmd {
        CompileCmd {
            force:               false,
            debug:               false,
            no_entrypoint_check: false,
            langspec:            None,
            include_dir:         Vec::new(),
            optimization:        "O1".to_string(),
            optimization_flag:   Vec::new(),
            max_include_depth:   16,
            graphviz:            None,
            graphviz_format:     "svg".to_string(),
            keep_graphviz_dot:   false,
            simulate:            false,
            continue_on_error:   false,
            recurse:             false,
            follow_symlinks:     false,
            jobs:                Some(1),
            output:              None,
            directory:           None,
            root:                None,
            user:                None,
            language:            "english".to_string(),
            load_ovr:            false,
            paths:               vec![input],
        }
    }

    #[test]
    fn individual_optimization_flags_override_the_preset() {
        let flags = parse_optimizations(
            "O3",
            &[
                "remove-dead-code".to_string(),
                "remove-dead-branches".to_string(),
            ],
        )
        .expect("flags should parse");

        assert_eq!(flags, nwnrs_nwscript::OptimizationFlags::O2);
        assert!(parse_optimizations("O1", &["unknown".to_string()]).is_err());
    }

    #[test]
    fn graphviz_formats_are_explicit_and_validated() {
        assert_eq!(
            parse_graphviz_format("SVG"),
            Ok(nwnrs_nwscript::GraphvizOutputFormat::Svg)
        );
        assert_eq!(
            parse_graphviz_format("png"),
            Ok(nwnrs_nwscript::GraphvizOutputFormat::Png)
        );
        assert!(parse_graphviz_format("jpeg").is_err());
    }

    #[test]
    fn compile_helper_produces_ncs_and_ndb_outputs() {
        let temp_dir = unique_test_dir("nwscript-compile");
        fs::create_dir_all(&temp_dir).expect("create temp dir");
        let input = temp_dir.join("test.nss");
        fs::write(temp_dir.join("nwscript.nss"), minimal_langspec()).expect("write langspec");
        fs::write(&input, "int StartingConditional() { return LOCAL_ONLY; }").expect("write input");

        let artifacts = compile_script_file(
            &input,
            &CompileScriptOptions {
                debug:               true,
                no_entrypoint_check: false,
                langspec:            None,
                include_dirs:        Vec::new(),
                optimizations:       nwnrs_nwscript::OptimizationFlags::O0,
                max_include_depth:   nwnrs_nwscript::DEFAULT_MAX_INCLUDE_DEPTH,
                install_resman:      None,
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
                optimizations:       nwnrs_nwscript::OptimizationFlags::O1,
                max_include_depth:   nwnrs_nwscript::DEFAULT_MAX_INCLUDE_DEPTH,
                install_resman:      None,
            },
        )
        .expect("compile should succeed");

        assert!(!artifacts.ncs.is_empty(), "NCS output should exist");

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn compile_command_resolves_local_nwpkg_include_dependency() {
        let temp_dir = unique_test_dir("nwscript-nwpkg-include");
        let project = temp_dir.join("project");
        let include = temp_dir.join("include");
        fs::create_dir_all(&project).expect("create project dir");
        fs::create_dir_all(&include).expect("create include dir");
        fs::write(
            project.join("nwproject.toml"),
            "[project]\nname = \"fixture\"\nkind = \"mod\"\n\n[source]\npath = \
             \".\"\n\n[dependencies]\nfixture = { path = \"../include\" }\n",
        )
        .expect("write project manifest");
        fs::write(
            include.join("nwproject.toml"),
            "[project]\nname = \"fixture\"\nkind = \"include\"\n\n[source]\npath = \".\"\n",
        )
        .expect("write include manifest");
        fs::write(project.join("nwscript.nss"), minimal_langspec()).expect("write langspec");
        fs::write(
            include.join("fixture.nss"),
            "int FixtureValue() { return TRUE; }\n",
        )
        .expect("write dependency include");
        let input = project.join("main.nss");
        fs::write(
            &input,
            "#include \"fixture\"\nvoid main() { int nValue = FixtureValue(); }\n",
        )
        .expect("write project script");

        run_compile(compile_cmd(input)).expect("compile with local include dependency");
        assert!(project.join("main.ncs").is_file());

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn compile_command_writes_graphviz_and_removes_stale_ndb() {
        let temp_dir = unique_test_dir("nwscript-command");
        let graphviz_dir = temp_dir.join("graphs");
        fs::create_dir_all(&temp_dir).expect("create temp dir");
        let input = temp_dir.join("test.nss");
        fs::write(temp_dir.join("nwscript.nss"), minimal_langspec()).expect("write langspec");
        fs::write(&input, "int StartingConditional() { return LOCAL_ONLY; }").expect("write input");

        let mut first = compile_cmd(input.clone());
        first.debug = true;
        first.graphviz = Some(graphviz_dir.clone());
        first.graphviz_format = "dot".to_string();
        run_compile(first).expect("debug compile should succeed");

        assert!(temp_dir.join("test.ncs").is_file());
        assert!(temp_dir.join("test.ndb").is_file());
        assert!(graphviz_dir.join("test.dot").is_file());

        let mut second = compile_cmd(input);
        second.force = true;
        run_compile(second).expect("non-debug recompile should succeed");
        assert!(!temp_dir.join("test.ndb").exists());

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn compile_command_preserves_relative_and_absolute_output_paths() {
        let temp_dir = unique_test_dir("nwscript-exact-output");
        fs::create_dir_all(&temp_dir).expect("create temp dir");
        let input = temp_dir.join("test.nss");
        fs::write(temp_dir.join("nwscript.nss"), minimal_langspec()).expect("write langspec");
        fs::write(&input, "void main() {}").expect("write input");

        let absolute_output = temp_dir.join("absolute/artifact.custom");
        let mut absolute = compile_cmd(input.clone());
        absolute.debug = true;
        absolute.output = Some(absolute_output.clone());
        run_compile(absolute).expect("absolute output compile should succeed");
        assert!(absolute_output.is_file());
        assert!(absolute_output.with_extension("ndb").is_file());
        assert!(!absolute_output.with_extension("ncs").exists());

        let relative_root = PathBuf::from("target").join(
            temp_dir
                .file_name()
                .expect("temporary directory should have a name"),
        );
        let relative_output = relative_root.join("relative/artifact");
        let mut relative = compile_cmd(input.clone());
        relative.output = Some(relative_output.clone());
        run_compile(relative).expect("relative output compile should succeed");
        assert!(relative_output.is_file());
        assert!(!relative_output.with_extension("ncs").exists());

        let mut destructive = compile_cmd(input.clone());
        destructive.force = true;
        destructive.output = Some(input);
        let error = run_compile(destructive).expect_err("source overwrite should be rejected");
        assert!(error.contains("would overwrite its source file"));

        let _ = fs::remove_dir_all(relative_root);
        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn compile_command_reports_source_locations() {
        let temp_dir = unique_test_dir("nwscript-diagnostic");
        fs::create_dir_all(&temp_dir).expect("create temp dir");
        let input = temp_dir.join("broken.nss");
        fs::write(temp_dir.join("nwscript.nss"), minimal_langspec()).expect("write langspec");
        fs::write(&input, "void main() { break; }").expect("write input");

        let error = run_compile(compile_cmd(input)).expect_err("compile should fail");
        assert!(error.contains("broken.nss:1:"), "unexpected error: {error}");
        assert!(error.contains("-4834"), "unexpected error: {error}");

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn compile_command_falls_back_to_installed_langspec() {
        if nwnrs_types::test_support::read_resource_bytes(
            "nwscript",
            nwnrs_nwscript::NW_SCRIPT_SOURCE_RES_TYPE,
        )
        .is_err()
        {
            return;
        }

        let temp_dir = unique_test_dir("nwscript-install-langspec");
        fs::create_dir_all(&temp_dir).expect("create temp dir");
        let input = temp_dir.join("installed.nss");
        fs::write(&input, "int StartingConditional() { return TRUE; }").expect("write input");

        run_compile(compile_cmd(input)).expect("installed langspec should resolve");
        assert!(temp_dir.join("installed.ncs").is_file());

        let _ = fs::remove_dir_all(temp_dir);
    }
}
