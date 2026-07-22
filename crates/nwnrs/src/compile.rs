use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};

use nwnrs_nwscript as nwscript;
use nwnrs_types::{
    install,
    resman::prelude::{CachePolicy, ResMan, ResRef, ResType},
};
use serde::Serialize;
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

/// Non-mutating NWScript compiler settings for editor and automation clients.
#[derive(Clone, Debug)]
pub struct NwScriptCheckOptions {
    /// Source files or directories to check.
    pub paths: Vec<PathBuf>,
    /// Permit include-style scripts without an executable entrypoint.
    pub no_entrypoint_check: bool,
    /// Optional explicit `nwscript.nss` path.
    pub langspec: Option<PathBuf>,
    /// Additional include search directories.
    pub include_dirs: Vec<PathBuf>,
    /// Unsaved source contents keyed by their filesystem paths.
    pub source_overlays: BTreeMap<PathBuf, Vec<u8>>,
    /// Optimization preset used while validating code generation.
    pub optimization: String,
    /// Exact optimization flags, replacing the preset when non-empty.
    pub optimization_flags: Vec<String>,
    /// Maximum recursive include depth.
    pub max_include_depth: usize,
    /// Maximum diagnostics recovered for each failed input.
    pub max_diagnostics_per_input: usize,
    /// Recurse through directory inputs.
    pub recurse: bool,
    /// Follow symlinks while collecting directory inputs.
    pub follow_symlinks: bool,
    /// Optional parallel worker count.
    pub jobs: Option<usize>,
    /// Optional Neverwinter Nights installation root override.
    pub root: Option<PathBuf>,
    /// Optional Neverwinter Nights user-directory override.
    pub user: Option<PathBuf>,
    /// Installation language used for resource lookup.
    pub language: String,
    /// Include the installation override directory in resource lookup.
    pub load_ovr: bool,
}

impl Default for NwScriptCheckOptions {
    fn default() -> Self {
        Self {
            paths: Vec::new(),
            no_entrypoint_check: true,
            langspec: None,
            include_dirs: Vec::new(),
            source_overlays: BTreeMap::new(),
            optimization: "O1".to_string(),
            optimization_flags: Vec::new(),
            max_include_depth: 16,
            max_diagnostics_per_input: 50,
            recurse: false,
            follow_symlinks: false,
            jobs: None,
            root: None,
            user: None,
            language: "english".to_string(),
            load_ovr: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DiagnosticFormat {
    Text,
    Json,
}

impl DiagnosticFormat {
    fn parse(value: &str) -> Result<Self, String> {
        match value.to_ascii_lowercase().as_str() {
            "text" => Ok(Self::Text),
            "json" => Ok(Self::Json),
            _ => Err(format!(
                "unsupported diagnostic format {value:?}; expected text or json"
            )),
        }
    }
}

#[derive(Serialize)]
struct JsonCompileDiagnostic<'entry> {
    kind:         &'static str,
    severity:     &'static str,
    input:        &'entry Path,
    code:         Option<i32>,
    message:      &'entry str,
    file:         Option<&'entry str>,
    start_line:   Option<usize>,
    start_column: Option<usize>,
    end_line:     Option<usize>,
    end_column:   Option<usize>,
}

#[derive(Serialize)]
struct JsonCompileSummary {
    kind:      &'static str,
    compiled:  usize,
    skipped:   usize,
    failed:    usize,
    simulated: bool,
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
    let install_resman = if root.is_none()
        && user.is_none()
        && language.eq_ignore_ascii_case("english")
        && !load_ovr
    {
        autodetected_install_resman()
    } else {
        build_install_resman(root, user, language, load_ovr)?
    };
    Ok(install_resman.map(|install_resman| {
        Arc::new(CliCompilerHost {
            resolver:       nwscript::FileSystemScriptResolver::new(),
            install_resman: Some(install_resman),
            overlay:        None,
        }) as nwscript::SharedScriptResolver
    }))
}

pub(crate) fn autodetected_install_resman() -> Option<SharedInstallResMan> {
    static INSTALL_RESMAN: OnceLock<Option<SharedInstallResMan>> = OnceLock::new();
    INSTALL_RESMAN
        .get_or_init(|| build_install_resman(None, None, "english", false).unwrap_or(None))
        .clone()
}

fn compile_environment(
    paths: &[PathBuf],
    include_dirs: &[PathBuf],
    root: Option<&Path>,
    user: Option<&Path>,
    language: &str,
    load_ovr: bool,
) -> Result<(Vec<PathBuf>, Option<nwscript::SharedScriptResolver>), String> {
    let fallback_resolver = build_install_script_resolver(root, user, language, load_ovr)?;
    let mut search_roots = include_dirs.to_vec();
    for input in paths {
        for dependency in nwnrs_nwpkg::resolve_include_dependencies(input)? {
            if !search_roots.contains(&dependency.source_root) {
                search_roots.push(dependency.source_root);
            }
        }
    }
    Ok((search_roots, fallback_resolver))
}

/// Checks NWScript files through the same compiler and resource lookup used by
/// the CLI without writing NCS, NDB, or Graphviz artifacts.
///
/// # Errors
///
/// Returns an error when configuration, project dependency discovery,
/// installation discovery, input collection, or compiler setup fails. Normal
/// per-script compiler failures are returned in the batch report.
pub fn check_nwscript(
    options: &NwScriptCheckOptions,
) -> Result<nwscript::BatchCompileReport, String> {
    check_nwscript_impl(options, None)
}

/// Checks NWScript inputs with cooperative cancellation across collection,
/// compilation, recovery, and generated event validation.
///
/// # Errors
///
/// Returns the same failures as [`check_nwscript`] plus `operation cancelled`.
pub fn check_nwscript_with_cancellation(
    options: &NwScriptCheckOptions,
    cancellation: &nwscript::CancellationToken,
) -> Result<nwscript::BatchCompileReport, String> {
    check_nwscript_impl(options, Some(cancellation))
}

fn check_nwscript_impl(
    options: &NwScriptCheckOptions,
    cancellation: Option<&nwscript::CancellationToken>,
) -> Result<nwscript::BatchCompileReport, String> {
    check_nwscript_cancellation(cancellation)?;
    if !(1..=200).contains(&options.max_include_depth) {
        return Err("maximum include depth must be between 1 and 200".to_string());
    }
    if !(1..=200).contains(&options.max_diagnostics_per_input) {
        return Err("maximum diagnostics per input must be between 1 and 200".to_string());
    }
    let optimizations = parse_optimizations(&options.optimization, &options.optimization_flags)?;
    let (search_roots, fallback_resolver) = compile_environment(
        &options.paths,
        &options.include_dirs,
        options.root.as_deref(),
        options.user.as_deref(),
        &options.language,
        options.load_ovr,
    )?;
    let batch_options = nwscript::BatchCompileOptions {
        driver: nwscript::CompilerDriverOptions {
            session: nwscript::CompilerSessionOptions {
                langspec_script_name: options.langspec.as_ref().map_or_else(
                    || nwscript::DEFAULT_LANGSPEC_SCRIPT_NAME.to_string(),
                    |path| path.to_string_lossy().into_owned(),
                ),
                source_load:          nwscript::SourceLoadOptions {
                    max_include_depth: options.max_include_depth,
                    ..nwscript::SourceLoadOptions::default()
                },
                compile:              nwscript::CompileOptions {
                    semantic: nwscript::SemanticOptions {
                        require_entrypoint:       !options.no_entrypoint_check,
                        allow_conditional_script: true,
                    },
                    optimizations,
                },
                emit_debug:           false,
            },
            skip_missing_entrypoint: !options.no_entrypoint_check,
            ..nwscript::CompilerDriverOptions::default()
        },
        search_roots,
        fallback_resolver,
        source_overlays: options.source_overlays.clone(),
        recurse: options.recurse,
        follow_symlinks: options.follow_symlinks,
        continue_on_error: true,
        simulate: true,
        overwrite_existing: true,
        remove_stale_debug: false,
        jobs: options.jobs,
        cancellation: cancellation.cloned(),
        ..nwscript::BatchCompileOptions::default()
    };
    let mut report = nwscript::compile_paths(&options.paths, &batch_options)
        .map_err(|error| error.to_string())?;
    check_nwscript_cancellation(cancellation)?;
    recover_additional_diagnostics(
        &mut report,
        &batch_options,
        options.max_diagnostics_per_input,
    )?;
    let mut projects = BTreeMap::new();
    for input in &options.paths {
        if let Some(project_root) = nwnrs_nwpkg::find_project_root(input)? {
            projects
                .entry(project_root)
                .or_insert_with(|| input.clone());
        }
    }
    for (project_root, input) in projects {
        check_nwscript_cancellation(cancellation)?;
        match nwnrs_nwpkg::generate_event_dispatcher_with_overlays(&input, &options.source_overlays)
        {
            Ok(Some(dispatcher)) => {
                if let Some(diagnostic) = check_generated_event_dispatcher(
                    &dispatcher,
                    &batch_options.driver,
                    &batch_options.search_roots,
                    batch_options.fallback_resolver.as_ref(),
                    &options.source_overlays,
                ) {
                    append_project_diagnostic(&mut report, &project_root, diagnostic);
                }
            }
            Ok(None) => {}
            Err(diagnostic) => {
                append_project_diagnostic(&mut report, &project_root, diagnostic);
            }
        }
    }
    Ok(report)
}

fn check_nwscript_cancellation(
    cancellation: Option<&nwscript::CancellationToken>,
) -> Result<(), String> {
    cancellation.map_or(Ok(()), |cancellation| {
        cancellation.check().map_err(|error| error.to_string())
    })
}

/// Removes project roots that are already transitive include dependencies of
/// another requested root.
///
/// # Errors
///
/// Returns an error when a root's local package dependency graph is invalid.
pub fn deduplicate_nwscript_project_roots(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let mut normalized = Vec::new();
    for root in roots {
        let path = root.canonicalize().unwrap_or_else(|_| root.clone());
        if !normalized.contains(&path) {
            normalized.push(path);
        }
    }
    let mut dependencies = BTreeSet::new();
    for root in &normalized {
        for dependency in nwnrs_nwpkg::resolve_include_dependencies(root)? {
            dependencies.insert(dependency.package_root);
        }
    }
    Ok(normalized
        .into_iter()
        .filter(|root| !dependencies.contains(root))
        .collect())
}

/// Returns project and transitive local dependency directories whose NSS or
/// manifest changes can affect the requested inputs.
///
/// # Errors
///
/// Returns an error when project discovery or a local dependency graph is
/// invalid.
pub fn nwscript_watch_roots(inputs: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let mut roots = Vec::new();
    for input in inputs {
        if let Some(project_root) = nwnrs_nwpkg::find_project_root(input)? {
            let project_root = project_root.canonicalize().unwrap_or(project_root);
            if !roots.contains(&project_root) {
                roots.push(project_root);
            }
        }
        for dependency in nwnrs_nwpkg::resolve_include_dependencies(input)? {
            if !roots.contains(&dependency.package_root) {
                roots.push(dependency.package_root);
            }
        }
    }
    Ok(roots)
}

fn recover_additional_diagnostics(
    report: &mut nwscript::BatchCompileReport,
    batch_options: &nwscript::BatchCompileOptions,
    maximum: usize,
) -> Result<(), String> {
    for entry in &mut report.entries {
        if entry.status != nwscript::BatchCompileStatus::Error || !entry.input.is_file() {
            continue;
        }
        let Some(mut diagnostic) = entry.diagnostic.clone() else {
            continue;
        };
        let mut overlays = batch_options.source_overlays.clone();
        let mut seen = BTreeSet::from([diagnostic_identity(&diagnostic)]);
        let mut masked_ranges = BTreeMap::<PathBuf, Vec<(usize, usize)>>::new();

        while seen.len() < maximum {
            let Some((source_path, masked_range)) =
                mask_failed_source(&entry.input, &diagnostic, batch_options, &mut overlays)?
            else {
                break;
            };
            masked_ranges
                .entry(source_path)
                .or_default()
                .push(masked_range);

            let mut recovery_options = batch_options.clone();
            recovery_options.source_overlays.clone_from(&overlays);
            recovery_options.recurse = false;
            recovery_options.follow_symlinks = false;
            recovery_options.jobs = Some(1);
            recovery_options.continue_on_error = true;
            let recovery =
                nwscript::compile_paths(std::slice::from_ref(&entry.input), &recovery_options)
                    .map_err(|error| error.to_string())?;
            let Some(next) = recovery
                .entries
                .into_iter()
                .find_map(|candidate| candidate.diagnostic)
            else {
                break;
            };
            let identity = diagnostic_identity(&next);
            if !seen.insert(identity)
                || diagnostic_overlaps_masked_source(
                    &entry.input,
                    &next,
                    batch_options,
                    &masked_ranges,
                )?
            {
                break;
            }
            entry.additional_diagnostics.push(next.clone());
            diagnostic = next;
        }
    }
    Ok(())
}

fn diagnostic_identity(
    diagnostic: &nwscript::CompilerDiagnostic,
) -> (
    Option<String>,
    Option<usize>,
    Option<usize>,
    Option<i32>,
    String,
) {
    (
        diagnostic.file.clone(),
        diagnostic.start_line,
        diagnostic.start_column,
        diagnostic.code,
        diagnostic.message.clone(),
    )
}

type SourceByteRange = (usize, usize);
type MaskedSource = (PathBuf, SourceByteRange);

fn diagnostic_overlaps_masked_source(
    input: &Path,
    diagnostic: &nwscript::CompilerDiagnostic,
    options: &nwscript::BatchCompileOptions,
    masked: &BTreeMap<PathBuf, Vec<(usize, usize)>>,
) -> Result<bool, String> {
    let Some(path) = diagnostic_source_path(input, diagnostic, options) else {
        return Ok(false);
    };
    let Some(source) = source_contents(&path, &options.source_overlays)? else {
        return Ok(false);
    };
    let Some((start, end)) = diagnostic_byte_range(&source, diagnostic) else {
        return Ok(false);
    };
    Ok(masked.get(&path).is_some_and(|ranges| {
        ranges
            .iter()
            .any(|(masked_start, masked_end)| start < *masked_end && end > *masked_start)
    }))
}

fn mask_failed_source(
    input: &Path,
    diagnostic: &nwscript::CompilerDiagnostic,
    options: &nwscript::BatchCompileOptions,
    overlays: &mut BTreeMap<PathBuf, Vec<u8>>,
) -> Result<Option<MaskedSource>, String> {
    let Some(path) = diagnostic_source_path(input, diagnostic, options) else {
        return Ok(None);
    };
    let Some(mut source) = source_contents(&path, overlays)? else {
        return Ok(None);
    };
    let Some((start, end)) = diagnostic_byte_range(&source, diagnostic) else {
        return Ok(None);
    };
    if start >= end || source.get(start..end).is_none() {
        return Ok(None);
    }

    let replacement = if diagnostic.code == Some(-573) {
        b';'
    } else if diagnostic
        .message
        .to_ascii_lowercase()
        .contains("static assertion")
    {
        b'1'
    } else {
        b'0'
    };
    let mut replacement_written = false;
    for byte in source.get_mut(start..end).unwrap_or_default() {
        if matches!(*byte, b'\n' | b'\r') {
            continue;
        }
        if !replacement_written && !byte.is_ascii_whitespace() {
            *byte = replacement;
            replacement_written = true;
        } else {
            *byte = b' ';
        }
    }
    if !replacement_written {
        return Ok(None);
    }
    overlays.insert(path.clone(), source);
    Ok(Some((path, (start, end))))
}

fn diagnostic_source_path(
    input: &Path,
    diagnostic: &nwscript::CompilerDiagnostic,
    options: &nwscript::BatchCompileOptions,
) -> Option<PathBuf> {
    let raw = diagnostic.file.as_deref().map(PathBuf::from)?;
    if raw.is_absolute() {
        return Some(raw);
    }
    let mut candidates = Vec::new();
    if let Some(parent) = input.parent() {
        candidates.push(parent.join(&raw));
    }
    for root in &options.search_roots {
        candidates.push(root.join(&raw));
    }
    if let Some(path) = options.source_overlays.keys().find(|path| {
        path.file_name()
            .zip(raw.file_name())
            .is_some_and(|(left, right)| left.eq_ignore_ascii_case(right))
    }) {
        candidates.push(path.clone());
    }
    candidates.into_iter().find(|candidate| {
        candidate.is_file()
            || options
                .source_overlays
                .keys()
                .any(|overlay| paths_refer_to_same_source(overlay, candidate))
    })
}

fn source_contents(
    path: &Path,
    overlays: &BTreeMap<PathBuf, Vec<u8>>,
) -> Result<Option<Vec<u8>>, String> {
    if let Some(contents) = overlays.iter().find_map(|(overlay, contents)| {
        paths_refer_to_same_source(overlay, path).then_some(contents.clone())
    }) {
        return Ok(Some(contents));
    }
    match std::fs::read(path) {
        Ok(contents) => Ok(Some(contents)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!(
            "failed to read diagnostic source {}: {error}",
            path.display()
        )),
    }
}

fn paths_refer_to_same_source(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left
            .to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy()),
    }
}

fn diagnostic_byte_range(
    source: &[u8],
    diagnostic: &nwscript::CompilerDiagnostic,
) -> Option<(usize, usize)> {
    let start = source_offset(source, diagnostic.start_line?, diagnostic.start_column?)?;
    let end = source_offset(
        source,
        diagnostic.end_line.unwrap_or(diagnostic.start_line?),
        diagnostic
            .end_column
            .unwrap_or(diagnostic.start_column?.saturating_add(1)),
    )?
    .max(start.saturating_add(1))
    .min(source.len());
    Some((start, end))
}

fn source_offset(source: &[u8], line: usize, column: usize) -> Option<usize> {
    if line == 0 || column == 0 {
        return None;
    }
    let mut current_line = 1_usize;
    let mut line_start = 0_usize;
    while current_line < line {
        let relative = source
            .get(line_start..)?
            .iter()
            .position(|byte| *byte == b'\n')?;
        line_start = line_start.saturating_add(relative).saturating_add(1);
        current_line += 1;
    }
    Some(
        line_start
            .saturating_add(column.saturating_sub(1))
            .min(source.len()),
    )
}

fn append_project_diagnostic(
    report: &mut nwscript::BatchCompileReport,
    project_root: &Path,
    diagnostic: nwscript::CompilerDiagnostic,
) {
    let duplicate = report.entries.iter().any(|entry| {
        entry.diagnostic.as_ref().is_some_and(|existing| {
            existing.file == diagnostic.file
                && existing.start_line == diagnostic.start_line
                && existing.start_column == diagnostic.start_column
                && existing.message == diagnostic.message
        })
    });
    if duplicate {
        return;
    }
    let input = project_root.join("_nwnrs_onload.nss");
    report.entries.push(nwscript::BatchCompileEntry {
        input:                  input.clone(),
        status:                 nwscript::BatchCompileStatus::Error,
        outputs:                Vec::new(),
        error:                  Some(format!(
            "failed to validate {}: {}",
            input.display(),
            diagnostic.message
        )),
        diagnostic:             Some(diagnostic),
        additional_diagnostics: Vec::new(),
    });
    report.errors += 1;
    report
        .entries
        .sort_by(|left, right| left.input.cmp(&right.input));
}

fn check_generated_event_dispatcher(
    dispatcher: &nwnrs_nwpkg::GeneratedEventDispatcher,
    driver_options: &nwscript::CompilerDriverOptions,
    search_roots: &[PathBuf],
    fallback: Option<&nwscript::SharedScriptResolver>,
    source_overlays: &BTreeMap<PathBuf, Vec<u8>>,
) -> Option<nwscript::CompilerDiagnostic> {
    let logical_name = format!("{}.nss", dispatcher.name);
    let mut resolver = nwscript::FileSystemScriptResolver::with_root(&dispatcher.include_root);
    for root in search_roots {
        resolver.add_root(root);
    }
    for (path, contents) in source_overlays {
        resolver.add_overlay(path.clone(), contents.clone());
    }
    let mut host = GeneratedCheckHost {
        logical_name: logical_name.clone(),
        source: dispatcher.source.as_bytes().to_vec(),
        resolver,
        fallback: fallback.cloned(),
    };
    let mut options = driver_options.clone();
    options.session.compile.semantic.require_entrypoint = true;
    options.skip_missing_entrypoint = false;
    options.output_alias.clone_from(&dispatcher.name);
    match nwscript::compile_file_with_host(&mut host, &logical_name, &options) {
        Ok(nwscript::CompileFileOutcome::Compiled(_)) => None,
        Ok(nwscript::CompileFileOutcome::SkippedNoEntrypoint) => {
            Some(nwscript::CompilerDiagnostic {
                code:         None,
                message:      "generated event dispatcher did not define main()".to_string(),
                file:         Some(logical_name),
                start_line:   None,
                start_column: None,
                end_line:     None,
                end_column:   None,
            })
        }
        Err(error) => Some(nwscript::source_aware_driver_diagnostic(
            &error,
            &host,
            Path::new(&logical_name),
            options.session.source_load,
        )),
    }
}

struct GeneratedCheckHost {
    logical_name: String,
    source:       Vec<u8>,
    resolver:     nwscript::FileSystemScriptResolver,
    fallback:     Option<nwscript::SharedScriptResolver>,
}

impl nwscript::ScriptResolver for GeneratedCheckHost {
    fn resolve_script_bytes(
        &self,
        script_name: &str,
        res_type: ResType,
    ) -> Result<Option<Vec<u8>>, nwscript::SourceError> {
        if res_type == nwscript::NW_SCRIPT_SOURCE_RES_TYPE
            && Path::new(script_name)
                .file_name()
                .is_some_and(|name| name.eq_ignore_ascii_case(&self.logical_name))
        {
            return Ok(Some(self.source.clone()));
        }
        if let Some(source) = self.resolver.resolve_script_bytes(script_name, res_type)? {
            return Ok(Some(source));
        }
        self.fallback.as_ref().map_or(Ok(None), |fallback| {
            fallback.resolve_script_bytes(script_name, res_type)
        })
    }
}

impl nwscript::CompilerHost for GeneratedCheckHost {
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
}

#[instrument(level = "info", skip_all, err, fields(path_count = cmd.paths.len()))]
pub(crate) fn run_compile(cmd: CompileCmd) -> Result<(), String> {
    if !(1..=200).contains(&cmd.max_include_depth) {
        return Err("maximum include depth must be between 1 and 200".to_string());
    }
    if cmd.keep_graphviz_dot && cmd.graphviz.is_none() {
        return Err("--keep-graphviz-dot requires --graphviz".to_string());
    }
    let diagnostic_format = DiagnosticFormat::parse(&cmd.diagnostic_format)?;
    let optimizations = parse_optimizations(&cmd.optimization, &cmd.optimization_flag)?;
    let graphviz_format = parse_graphviz_format(&cmd.graphviz_format)?;
    let (search_roots, fallback_resolver) = compile_environment(
        &cmd.paths,
        &cmd.include_dir,
        cmd.root.as_deref(),
        cmd.user.as_deref(),
        &cmd.language,
        cmd.load_ovr,
    )?;
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
        source_overlays: BTreeMap::new(),
        recurse: cmd.recurse,
        follow_symlinks: cmd.follow_symlinks,
        continue_on_error: cmd.continue_on_error,
        simulate: cmd.simulate,
        overwrite_existing: cmd.force,
        remove_stale_debug: true,
        jobs: cmd.jobs,
        cancellation: None,
        output_file: cmd.output.clone(),
        output_directory: cmd.directory.clone(),
        graphviz_directory: cmd.graphviz.clone(),
        graphviz_format,
        keep_graphviz_dot: cmd.keep_graphviz_dot,
    };
    let report =
        nwscript::compile_paths(&cmd.paths, &options).map_err(|error| error.to_string())?;
    if diagnostic_format == DiagnosticFormat::Json {
        for entry in &report.entries {
            if entry.status != nwscript::BatchCompileStatus::Error {
                continue;
            }
            let fallback_message = entry
                .error
                .as_deref()
                .unwrap_or("unknown compilation error");
            let record = entry.diagnostic.as_ref().map_or(
                JsonCompileDiagnostic {
                    kind:         "diagnostic",
                    severity:     "error",
                    input:        &entry.input,
                    code:         None,
                    message:      fallback_message,
                    file:         None,
                    start_line:   None,
                    start_column: None,
                    end_line:     None,
                    end_column:   None,
                },
                |diagnostic| JsonCompileDiagnostic {
                    kind:         "diagnostic",
                    severity:     "error",
                    input:        &entry.input,
                    code:         diagnostic.code,
                    message:      &diagnostic.message,
                    file:         diagnostic.file.as_deref(),
                    start_line:   diagnostic.start_line,
                    start_column: diagnostic.start_column,
                    end_line:     diagnostic.end_line,
                    end_column:   diagnostic.end_column,
                },
            );
            write_stdout_line(
                &serde_json::to_string(&record)
                    .map_err(|error| format!("failed to serialize compiler diagnostic: {error}"))?,
            )?;
        }
        write_stdout_line(
            &serde_json::to_string(&JsonCompileSummary {
                kind:      "summary",
                compiled:  report.successes,
                skipped:   report.skips,
                failed:    report.errors,
                simulated: cmd.simulate,
            })
            .map_err(|error| format!("failed to serialize compiler summary: {error}"))?,
        )?;
        return if report.errors == 0 {
            Ok(())
        } else {
            Err(format!("{} NWScript compilations failed", report.errors))
        };
    }
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
        collections::BTreeMap,
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{
        CompileScriptOptions, NwScriptCheckOptions, check_nwscript, compile_script_file,
        deduplicate_nwscript_project_roots, nwscript_watch_roots, parse_graphviz_format,
        parse_optimizations, run_compile,
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
            diagnostic_format:   "text".to_string(),
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
    fn editor_check_recovers_multiple_independent_diagnostics_in_one_file() {
        let temp_dir = unique_test_dir("nwscript-multiple-diagnostics");
        fs::create_dir_all(&temp_dir).expect("create temp dir");
        let input = temp_dir.join("test.nss");
        let langspec = temp_dir.join("nwscript.nss");
        fs::write(&langspec, minimal_langspec()).expect("write langspec");
        fs::write(
            &input,
            "void main()\n{\n    MissingOne;\n    MissingTwo;\n}\n",
        )
        .expect("write invalid source");

        let report = check_nwscript(&NwScriptCheckOptions {
            paths: vec![input],
            langspec: Some(langspec),
            ..NwScriptCheckOptions::default()
        })
        .expect("check invalid source");

        assert_eq!(report.errors, 1);
        let entry = report.entries.first().expect("failed entry");
        assert_eq!(
            entry.diagnostic.as_ref().and_then(|item| item.start_line),
            Some(3)
        );
        assert_eq!(entry.additional_diagnostics.len(), 1);
        assert_eq!(
            entry
                .additional_diagnostics
                .first()
                .and_then(|item| item.start_line),
            Some(4)
        );
        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn editor_check_compiles_unsaved_overlay_contents() {
        let temp_dir = unique_test_dir("nwscript-overlay-check");
        fs::create_dir_all(&temp_dir).expect("create temp dir");
        let input = temp_dir.join("test.nss");
        let langspec = temp_dir.join("nwscript.nss");
        fs::write(&langspec, minimal_langspec()).expect("write langspec");
        fs::write(&input, "void main() {}\n").expect("write valid disk source");

        let report = check_nwscript(&NwScriptCheckOptions {
            paths: vec![input.clone()],
            langspec: Some(langspec),
            source_overlays: BTreeMap::from([(
                input,
                b"void main() { MissingFromOverlay; }\n".to_vec(),
            )]),
            ..NwScriptCheckOptions::default()
        })
        .expect("check overlay source");

        assert_eq!(report.errors, 1);
        assert!(
            report
                .entries
                .first()
                .expect("failed entry")
                .diagnostic
                .as_ref()
                .is_some_and(|item| item.message.contains("MissingFromOverlay"))
        );
        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn workspace_roots_deduplicate_local_include_projects_and_watch_dependencies() {
        let temp_dir = unique_test_dir("nwscript-workspace-roots");
        let app = temp_dir.join("app");
        let include = temp_dir.join("include");
        fs::create_dir_all(&app).expect("create app");
        fs::create_dir_all(&include).expect("create include");
        fs::write(
            app.join("nwpkg.toml"),
            "[project]\nname = \"app\"\nkind = \"mod\"\n\n[source]\npath = \
             \".\"\n\n[dependencies]\nshared = { path = \"../include\" }\n",
        )
        .expect("write app manifest");
        fs::write(
            include.join("nwpkg.toml"),
            "[project]\nname = \"shared\"\nkind = \"include\"\n\n[source]\npath = \".\"\n",
        )
        .expect("write include manifest");

        let deduplicated = deduplicate_nwscript_project_roots(&[app.clone(), include.clone()])
            .expect("deduplicate roots");
        assert_eq!(
            deduplicated,
            vec![app.canonicalize().expect("canonical app")]
        );
        let watched =
            nwscript_watch_roots(std::slice::from_ref(&app)).expect("resolve watch roots");
        assert!(watched.contains(&app.canonicalize().expect("canonical app")));
        assert!(watched.contains(&include.canonicalize().expect("canonical include")));
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
            project.join("nwpkg.toml"),
            "[project]\nname = \"fixture\"\nkind = \"mod\"\n\n[source]\npath = \
             \".\"\n\n[dependencies]\nfixture = { path = \"../include\" }\n",
        )
        .expect("write project manifest");
        fs::write(
            include.join("nwpkg.toml"),
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
