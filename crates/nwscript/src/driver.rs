use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt, fs,
    io::{self, Write},
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    sync::Arc,
    thread,
};

use nwnrs_types::resman::prelude::{ResType, get_res_ext};
use serde::{Deserialize, Serialize};

use crate::{
    CompileArtifacts, CompilerSession, CompilerSessionError, CompilerSessionOptions,
    NW_SCRIPT_BINARY_RES_TYPE, NW_SCRIPT_DEBUG_RES_TYPE, NW_SCRIPT_SOURCE_RES_TYPE, ScriptResolver,
    SourceError, session::PreparedScript,
};

/// A thread-safe resolver shared by batch compiler workers.
pub type SharedScriptResolver = Arc<dyn ScriptResolver + Send + Sync>;

/// Output format for a Graphviz syntax-tree artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GraphvizOutputFormat {
    /// Write Graphviz DOT source without invoking an external renderer.
    #[default]
    Dot,
    /// Render a scalable SVG image.
    Svg,
    /// Render a PNG image.
    Png,
    /// Render a PDF document.
    Pdf,
}

impl GraphvizOutputFormat {
    /// Returns the conventional file extension for this format.
    #[must_use]
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Dot => "dot",
            Self::Svg => "svg",
            Self::Png => "png",
            Self::Pdf => "pdf",
        }
    }

    const fn requires_renderer(self) -> bool {
        !matches!(self, Self::Dot)
    }
}

/// Errors returned while resolving or writing through a callback-driven
/// compiler host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompilerHostError {
    /// Human-readable error message.
    pub message: String,
}

impl CompilerHostError {
    /// Creates one host error from arbitrary text.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for CompilerHostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for CompilerHostError {}

impl From<io::Error> for CompilerHostError {
    fn from(value: io::Error) -> Self {
        Self::new(value.to_string())
    }
}

/// One callback-driven host for loading NWScript source and receiving compiler
/// outputs.
pub trait CompilerHost {
    /// Resolves one logical script name for the requested resource type.
    ///
    /// # Errors
    ///
    /// Returns [`SourceError`] if the underlying lookup fails.
    fn resolve_script_bytes(
        &self,
        script_name: &str,
        res_type: ResType,
    ) -> Result<Option<Vec<u8>>, SourceError>;

    /// Receives one emitted compiler artifact.
    ///
    /// # Errors
    ///
    /// Returns [`CompilerHostError`] if the host cannot persist or accept the
    /// output.
    fn write_file(
        &mut self,
        file_name: &str,
        res_type: ResType,
        data: &[u8],
        binary: bool,
    ) -> Result<(), CompilerHostError>;

    /// Receives one Graphviz DOT file when graphviz output is requested.
    ///
    /// # Errors
    ///
    /// Returns [`CompilerHostError`] if the host cannot persist or accept the
    /// output.
    fn write_graphviz(&mut self, _file_name: &str, _dot: &str) -> Result<(), CompilerHostError> {
        Ok(())
    }
}

/// Options controlling one callback-driven compiler invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompilerDriverOptions {
    /// Reusable session settings for parsing and code generation.
    pub session:                 CompilerSessionOptions,
    /// Resource type used when emitting compiled bytecode.
    pub binary_res_type:         ResType,
    /// Resource type used when emitting debug output.
    pub debug_res_type:          ResType,
    /// Base output name used for emitted artifacts.
    pub output_alias:            String,
    /// Whether to emit Graphviz DOT for the parsed AST.
    pub emit_graphviz:           bool,
    /// Optional output name for the emitted Graphviz DOT file.
    pub graphviz_alias:          Option<String>,
    /// Whether scripts without `main()` or `StartingConditional()` should be
    /// skipped.
    pub skip_missing_entrypoint: bool,
}

impl Default for CompilerDriverOptions {
    fn default() -> Self {
        Self {
            session:                 CompilerSessionOptions::default(),
            binary_res_type:         NW_SCRIPT_BINARY_RES_TYPE,
            debug_res_type:          NW_SCRIPT_DEBUG_RES_TYPE,
            output_alias:            "scriptout".to_string(),
            emit_graphviz:           false,
            graphviz_alias:          None,
            skip_missing_entrypoint: false,
        }
    }
}

/// Result of one callback-driven compile request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompileFileOutcome {
    /// The script compiled successfully and artifacts were written through the
    /// host.
    Compiled(CompileArtifacts),
    /// The script was skipped because it has no executable entrypoint.
    SkippedNoEntrypoint,
}

/// Errors returned while executing one callback-driven compile request.
#[derive(Debug)]
pub enum CompilerDriverError {
    /// Session loading, parsing, or code generation failed.
    Session(CompilerSessionError),
    /// The host failed while persisting output.
    Host(CompilerHostError),
}

impl fmt::Display for CompilerDriverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Session(error) => error.fmt(f),
            Self::Host(error) => error.fmt(f),
        }
    }
}

impl Error for CompilerDriverError {}

impl From<CompilerSessionError> for CompilerDriverError {
    fn from(value: CompilerSessionError) -> Self {
        Self::Session(value)
    }
}

impl From<CompilerHostError> for CompilerDriverError {
    fn from(value: CompilerHostError) -> Self {
        Self::Host(value)
    }
}

struct HostResolver<'a, H> {
    host: &'a H,
}

impl<H: CompilerHost> ScriptResolver for HostResolver<'_, H> {
    fn resolve_script_bytes(
        &self,
        script_name: &str,
        res_type: ResType,
    ) -> Result<Option<Vec<u8>>, SourceError> {
        self.host.resolve_script_bytes(script_name, res_type)
    }
}

/// Compiles one logical script through a callback-driven host.
///
/// # Errors
///
/// Returns [`CompilerDriverError`] if source loading, parsing, code generation,
/// or host output persistence fails.
pub fn compile_file_with_host<H: CompilerHost>(
    host: &mut H,
    script_name: &str,
    options: &CompilerDriverOptions,
) -> Result<CompileFileOutcome, CompilerDriverError> {
    compile_file_with_host_impl(host, script_name, options, None)
}

/// Compiles one logical script with cooperative cancellation.
///
/// # Errors
///
/// Returns [`CompilerDriverError`] for compiler, host, or cancellation
/// failures.
pub fn compile_file_with_host_with_cancellation<H: CompilerHost>(
    host: &mut H,
    script_name: &str,
    options: &CompilerDriverOptions,
    cancellation: &crate::CancellationToken,
) -> Result<CompileFileOutcome, CompilerDriverError> {
    compile_file_with_host_impl(host, script_name, options, Some(cancellation))
}

fn compile_file_with_host_impl<H: CompilerHost>(
    host: &mut H,
    script_name: &str,
    options: &CompilerDriverOptions,
    cancellation: Option<&crate::CancellationToken>,
) -> Result<CompileFileOutcome, CompilerDriverError> {
    if let Some(cancellation) = cancellation {
        cancellation.check().map_err(CompilerSessionError::from)?;
    }
    let (prepared, artifacts, graphviz) = {
        let resolver = HostResolver {
            host: &*host
        };
        let mut session = CompilerSession::with_options(&resolver, options.session.clone());
        let prepared = if let Some(cancellation) = cancellation {
            session.prepare_script_name_with_cancellation(script_name, cancellation)?
        } else {
            session.prepare_script_name(script_name)?
        };
        if options.skip_missing_entrypoint && !prepared_has_entrypoint(&prepared) {
            return Ok(CompileFileOutcome::SkippedNoEntrypoint);
        }
        let graphviz = if options.emit_graphviz {
            Some(crate::render_script_graphviz(
                &prepared.script,
                Some(&prepared.bundle.source_map),
            ))
        } else {
            None
        };
        if let Some(cancellation) = cancellation {
            cancellation.check().map_err(CompilerSessionError::from)?;
        }
        let artifacts = session
            .compile_prepared(&prepared)
            .map_err(CompilerSessionError::from)
            .map_err(CompilerDriverError::from)?;
        (prepared, artifacts, graphviz)
    };

    if let Some(cancellation) = cancellation {
        cancellation.check().map_err(CompilerSessionError::from)?;
    }

    host.write_file(
        &options.output_alias,
        options.binary_res_type,
        &artifacts.ncs,
        true,
    )?;
    if let Some(ndb) = artifacts.ndb.as_ref() {
        host.write_file(&options.output_alias, options.debug_res_type, ndb, true)?;
    }
    if let Some(dot) = graphviz.as_deref() {
        let graphviz_alias = options
            .graphviz_alias
            .as_deref()
            .unwrap_or(&options.output_alias);
        host.write_graphviz(graphviz_alias, dot)?;
    }
    let _ = prepared;
    Ok(CompileFileOutcome::Compiled(artifacts))
}

fn prepared_has_entrypoint(prepared: &PreparedScript) -> bool {
    prepared.script.items.iter().any(|item| match item {
        crate::TopLevelItem::Function(function) => {
            function.body.is_some()
                && (function.name == "main" || function.name == "StartingConditional")
        }
        _ => false,
    })
}

/// One filesystem-backed source resolver that searches one or more root
/// directories.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileSystemScriptResolver {
    roots:    Vec<PathBuf>,
    overlays: BTreeMap<PathBuf, Vec<u8>>,
}

impl FileSystemScriptResolver {
    /// Creates one empty filesystem resolver.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates one filesystem resolver with an initial root.
    #[must_use]
    pub fn with_root(root: impl Into<PathBuf>) -> Self {
        let mut resolver = Self::new();
        resolver.add_root(root);
        resolver
    }

    /// Adds one search root used for relative script names.
    pub fn add_root(&mut self, root: impl Into<PathBuf>) {
        self.roots.push(root.into());
    }

    /// Adds or replaces an in-memory source file overlay.
    ///
    /// Overlay paths participate in the same ordered, case-insensitive
    /// candidate resolution as files on disk.
    pub fn add_overlay(&mut self, path: impl Into<PathBuf>, contents: impl Into<Vec<u8>>) {
        self.overlays.insert(path.into(), contents.into());
    }

    fn overlay(&self, candidate: &Path) -> Option<&[u8]> {
        self.overlays.iter().find_map(|(path, contents)| {
            paths_match_case_insensitively(path, candidate).then_some(contents.as_slice())
        })
    }

    fn overlay_path(&self, candidate: &Path) -> Option<&Path> {
        self.overlays
            .keys()
            .find(|path| paths_match_case_insensitively(path, candidate))
            .map(PathBuf::as_path)
    }

    /// Resolves a logical source name to the filesystem or overlay path that
    /// would supply it.
    #[must_use]
    pub fn resolve_script_path(&self, script_name: &str) -> Option<PathBuf> {
        for candidate in self.candidate_paths(script_name) {
            if let Some(path) = self.overlay_path(&candidate) {
                return Some(path.to_path_buf());
            }
            if let Some(path) = resolve_case_insensitive_file(&candidate) {
                return Some(path);
            }
        }
        None
    }

    fn candidate_paths(&self, script_name: &str) -> Vec<PathBuf> {
        let path = Path::new(script_name);
        let mut names = vec![PathBuf::from(script_name)];
        if path.extension().is_none() {
            names.push(PathBuf::from(format!(
                "{script_name}.{}",
                get_res_ext(NW_SCRIPT_SOURCE_RES_TYPE)
            )));
        }

        let mut candidates = Vec::new();
        for name in names {
            if path.is_absolute() || name.is_absolute() {
                candidates.push(name.clone());
            } else {
                for root in &self.roots {
                    candidates.push(root.join(&name));
                }
                candidates.push(name.clone());
            }
        }
        candidates
    }
}

impl ScriptResolver for FileSystemScriptResolver {
    fn resolve_script_bytes(
        &self,
        script_name: &str,
        res_type: ResType,
    ) -> Result<Option<Vec<u8>>, SourceError> {
        if res_type != NW_SCRIPT_SOURCE_RES_TYPE {
            return Ok(None);
        }
        for candidate in self.candidate_paths(script_name) {
            if let Some(contents) = self.overlay(&candidate) {
                return Ok(Some(contents.to_vec()));
            }
            if let Some(resolved) = resolve_case_insensitive_file(&candidate) {
                if let Some(contents) = self.overlay(&resolved) {
                    return Ok(Some(contents.to_vec()));
                }
                return fs::read(&resolved)
                    .map(Some)
                    .map_err(|error| SourceError::resolver(error.to_string()));
            }
        }
        Ok(None)
    }
}

fn paths_match_case_insensitively(left: &Path, right: &Path) -> bool {
    let normalize = |path: &Path| {
        path.components()
            .filter_map(|component| match component {
                Component::CurDir => None,
                other => Some(other.as_os_str().to_string_lossy().to_ascii_lowercase()),
            })
            .collect::<Vec<_>>()
    };
    normalize(left) == normalize(right)
}

fn resolve_case_insensitive_file(path: &Path) -> Option<PathBuf> {
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
                let search_directory = if current.as_os_str().is_empty() {
                    Path::new(".")
                } else {
                    current.as_path()
                };
                current = fs::read_dir(search_directory)
                    .ok()?
                    .filter_map(Result::ok)
                    .find(|entry| {
                        entry
                            .file_name()
                            .to_string_lossy()
                            .eq_ignore_ascii_case(&name.to_string_lossy())
                    })?
                    .path();
            }
        }
    }

    current.is_file().then_some(current)
}

/// One directory-backed compiler host that reads source files from filesystem
/// roots and writes outputs back to disk.
#[derive(Clone)]
pub struct DirectoryCompilerHost {
    resolver:           FileSystemScriptResolver,
    fallback_resolver:  Option<SharedScriptResolver>,
    output_directory:   PathBuf,
    binary_output_file: Option<PathBuf>,
    graphviz_directory: Option<PathBuf>,
    graphviz_format:    GraphvizOutputFormat,
    keep_graphviz_dot:  bool,
    simulate:           bool,
    overwrite_existing: bool,
    written_paths:      Vec<PathBuf>,
}

impl fmt::Debug for DirectoryCompilerHost {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DirectoryCompilerHost")
            .field("resolver", &self.resolver)
            .field("has_fallback_resolver", &self.fallback_resolver.is_some())
            .field("output_directory", &self.output_directory)
            .field("binary_output_file", &self.binary_output_file)
            .field("graphviz_directory", &self.graphviz_directory)
            .field("graphviz_format", &self.graphviz_format)
            .field("keep_graphviz_dot", &self.keep_graphviz_dot)
            .field("simulate", &self.simulate)
            .field("overwrite_existing", &self.overwrite_existing)
            .field("written_paths", &self.written_paths)
            .finish()
    }
}

impl DirectoryCompilerHost {
    /// Creates one directory host rooted at `output_directory`.
    #[must_use]
    pub fn new(resolver: FileSystemScriptResolver, output_directory: impl Into<PathBuf>) -> Self {
        Self {
            resolver,
            fallback_resolver: None,
            output_directory: output_directory.into(),
            binary_output_file: None,
            graphviz_directory: None,
            graphviz_format: GraphvizOutputFormat::Dot,
            keep_graphviz_dot: false,
            simulate: false,
            overwrite_existing: true,
            written_paths: Vec::new(),
        }
    }

    /// Sets a resolver consulted after all filesystem roots miss.
    pub fn set_fallback_resolver(&mut self, resolver: SharedScriptResolver) {
        self.fallback_resolver = Some(resolver);
    }

    /// Sets the exact path used for the compiled NCS artifact.
    pub fn set_binary_output_file(&mut self, path: impl Into<PathBuf>) {
        self.binary_output_file = Some(path.into());
    }

    /// Sets an alternate directory for Graphviz source or image output.
    pub fn set_graphviz_directory(&mut self, directory: impl Into<PathBuf>) {
        self.graphviz_directory = Some(directory.into());
    }

    /// Selects the Graphviz output format and whether rendered images retain
    /// their DOT source alongside them.
    pub fn set_graphviz_output(&mut self, format: GraphvizOutputFormat, keep_dot: bool) {
        self.graphviz_format = format;
        self.keep_graphviz_dot = keep_dot;
    }

    /// Enables or disables simulate mode, which records target paths without
    /// writing files.
    pub fn set_simulate(&mut self, simulate: bool) {
        self.simulate = simulate;
    }

    /// Controls whether existing output artifacts may be replaced.
    pub fn set_overwrite_existing(&mut self, overwrite: bool) {
        self.overwrite_existing = overwrite;
    }

    /// Returns the paths written or scheduled during the most recent compile.
    #[must_use]
    pub fn written_paths(&self) -> &[PathBuf] {
        &self.written_paths
    }
}

impl ScriptResolver for DirectoryCompilerHost {
    fn resolve_script_bytes(
        &self,
        script_name: &str,
        res_type: ResType,
    ) -> Result<Option<Vec<u8>>, SourceError> {
        if let Some(bytes) = self.resolver.resolve_script_bytes(script_name, res_type)? {
            return Ok(Some(bytes));
        }
        match &self.fallback_resolver {
            Some(resolver) => resolver.resolve_script_bytes(script_name, res_type),
            None => Ok(None),
        }
    }
}

impl CompilerHost for DirectoryCompilerHost {
    fn resolve_script_bytes(
        &self,
        script_name: &str,
        res_type: ResType,
    ) -> Result<Option<Vec<u8>>, SourceError> {
        ScriptResolver::resolve_script_bytes(self, script_name, res_type)
    }

    fn write_file(
        &mut self,
        file_name: &str,
        res_type: ResType,
        data: &[u8],
        _binary: bool,
    ) -> Result<(), CompilerHostError> {
        let path = if res_type == NW_SCRIPT_BINARY_RES_TYPE {
            self.binary_output_file.clone().unwrap_or_else(|| {
                self.output_directory
                    .join(format!("{file_name}.{}", get_res_ext(res_type)))
            })
        } else {
            self.output_directory
                .join(format!("{file_name}.{}", get_res_ext(res_type)))
        };
        self.written_paths.push(path.clone());
        if self.simulate {
            return Ok(());
        }
        if path.exists() && !self.overwrite_existing {
            return Err(CompilerHostError::new(format!(
                "output already exists; use overwrite to replace {}",
                path.display()
            )));
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, data)?;
        Ok(())
    }

    fn write_graphviz(&mut self, file_name: &str, dot: &str) -> Result<(), CompilerHostError> {
        let base = self
            .graphviz_directory
            .as_ref()
            .unwrap_or(&self.output_directory);
        let rendered_path = base.join(format!("{file_name}.{}", self.graphviz_format.extension()));
        let dot_path = base.join(format!("{file_name}.dot"));
        let write_dot = self.graphviz_format == GraphvizOutputFormat::Dot || self.keep_graphviz_dot;
        let mut paths = Vec::with_capacity(2);
        if write_dot {
            paths.push(dot_path.clone());
        }
        if self.graphviz_format != GraphvizOutputFormat::Dot {
            paths.push(rendered_path.clone());
        }
        self.written_paths.extend(paths.iter().cloned());
        if self.simulate {
            return Ok(());
        }
        for path in &paths {
            if path.exists() && !self.overwrite_existing {
                return Err(CompilerHostError::new(format!(
                    "output already exists; use overwrite to replace {}",
                    path.display()
                )));
            }
        }
        let rendered = if self.graphviz_format.requires_renderer() {
            Some(render_graphviz(dot, self.graphviz_format)?)
        } else {
            None
        };
        if write_dot {
            if let Some(parent) = dot_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&dot_path, dot.as_bytes())?;
        }
        if let Some(rendered) = rendered {
            if let Some(parent) = rendered_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&rendered_path, rendered)?;
        }
        Ok(())
    }
}

fn render_graphviz(dot: &str, format: GraphvizOutputFormat) -> Result<Vec<u8>, CompilerHostError> {
    let mut child = Command::new("dot")
        .arg(format!("-T{}", format.extension()))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            CompilerHostError::new(format!(
                "failed to start Graphviz 'dot'; install Graphviz or request DOT output: {error}"
            ))
        })?;
    child
        .stdin
        .take()
        .ok_or_else(|| CompilerHostError::new("failed to open Graphviz stdin"))?
        .write_all(dot.as_bytes())?;
    let output = child.wait_with_output()?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        return Err(CompilerHostError::new(format!(
            "Graphviz rendering failed for {} output: {}",
            format.extension(),
            detail.trim()
        )));
    }
    Ok(output.stdout)
}

fn ensure_graphviz_renderer(format: GraphvizOutputFormat) -> Result<(), BatchCompileError> {
    if !format.requires_renderer() {
        return Ok(());
    }
    let output = Command::new("dot").arg("-V").output().map_err(|error| {
        BatchCompileError::Configuration(format!(
            "Graphviz image output requires the 'dot' executable; install Graphviz or select DOT \
             output: {error}"
        ))
    })?;
    if output.status.success() {
        Ok(())
    } else {
        Err(BatchCompileError::Configuration(
            "Graphviz 'dot -V' returned an unsuccessful status".to_string(),
        ))
    }
}

/// Options controlling multi-file directory and file compilation.
#[derive(Clone)]
pub struct BatchCompileOptions {
    /// Callback/session behavior for each compilation.
    pub driver:             CompilerDriverOptions,
    /// Extra filesystem roots used for langspec and include resolution.
    pub search_roots:       Vec<PathBuf>,
    /// Resolver consulted after the input directory and search roots miss.
    pub fallback_resolver:  Option<SharedScriptResolver>,
    /// Unsaved source contents keyed by their filesystem paths.
    pub source_overlays:    BTreeMap<PathBuf, Vec<u8>>,
    /// Whether directory traversal should recurse.
    pub recurse:            bool,
    /// Whether directory traversal should follow symlinks.
    pub follow_symlinks:    bool,
    /// Whether compilation should continue after one file fails.
    pub continue_on_error:  bool,
    /// Whether outputs should be simulated without writing files.
    pub simulate:           bool,
    /// Whether existing output files may be replaced.
    pub overwrite_existing: bool,
    /// Whether stale debugger output should be removed after a non-debug build.
    pub remove_stale_debug: bool,
    /// Optional worker count used when continuing after individual failures.
    pub jobs:               Option<usize>,
    /// Cooperative cancellation shared by input discovery and compiler workers.
    pub cancellation:       Option<crate::CancellationToken>,
    /// Optional exact NCS output path, valid only for one input file.
    pub output_file:        Option<PathBuf>,
    /// Optional output directory overriding each source file's parent.
    pub output_directory:   Option<PathBuf>,
    /// Optional directory for Graphviz source or rendered-image output.
    pub graphviz_directory: Option<PathBuf>,
    /// Graphviz source or rendered-image format.
    pub graphviz_format:    GraphvizOutputFormat,
    /// Whether rendered Graphviz images retain their DOT source.
    pub keep_graphviz_dot:  bool,
}

impl Default for BatchCompileOptions {
    fn default() -> Self {
        Self {
            driver:             CompilerDriverOptions {
                skip_missing_entrypoint: true,
                ..CompilerDriverOptions::default()
            },
            search_roots:       Vec::new(),
            fallback_resolver:  None,
            source_overlays:    BTreeMap::new(),
            recurse:            false,
            follow_symlinks:    false,
            continue_on_error:  false,
            simulate:           false,
            overwrite_existing: true,
            remove_stale_debug: false,
            jobs:               None,
            cancellation:       None,
            output_file:        None,
            output_directory:   None,
            graphviz_directory: None,
            graphviz_format:    GraphvizOutputFormat::Dot,
            keep_graphviz_dot:  false,
        }
    }
}

impl fmt::Debug for BatchCompileOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BatchCompileOptions")
            .field("driver", &self.driver)
            .field("search_roots", &self.search_roots)
            .field("has_fallback_resolver", &self.fallback_resolver.is_some())
            .field("source_overlay_count", &self.source_overlays.len())
            .field("recurse", &self.recurse)
            .field("follow_symlinks", &self.follow_symlinks)
            .field("continue_on_error", &self.continue_on_error)
            .field("simulate", &self.simulate)
            .field("overwrite_existing", &self.overwrite_existing)
            .field("remove_stale_debug", &self.remove_stale_debug)
            .field("jobs", &self.jobs)
            .field("cancellable", &self.cancellation.is_some())
            .field("output_file", &self.output_file)
            .field("output_directory", &self.output_directory)
            .field("graphviz_directory", &self.graphviz_directory)
            .field("graphviz_format", &self.graphviz_format)
            .field("keep_graphviz_dot", &self.keep_graphviz_dot)
            .finish()
    }
}

/// One per-input result from a batch compile run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchCompileEntry {
    /// Input file path.
    pub input:                  PathBuf,
    /// Final status for this input.
    pub status:                 BatchCompileStatus,
    /// Output paths written or scheduled by the host.
    pub outputs:                Vec<PathBuf>,
    /// Human-readable error text when compilation failed.
    pub error:                  Option<String>,
    /// Structured compiler diagnostic when the failure originated in the
    /// NWScript frontend or code generator.
    pub diagnostic:             Option<CompilerDiagnostic>,
    /// Additional independent diagnostics recovered for this input by an
    /// editor-facing caller. The batch compiler itself leaves this empty.
    pub additional_diagnostics: Vec<CompilerDiagnostic>,
}

/// One source-aware compiler diagnostic suitable for editor integrations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompilerDiagnostic {
    /// Stable upstream-aligned compiler error code, when available.
    pub code:         Option<i32>,
    /// Human-readable diagnostic message.
    pub message:      String,
    /// Logical or filesystem source name, when the error has a source span.
    pub file:         Option<String>,
    /// One-based start line.
    pub start_line:   Option<usize>,
    /// One-based start column.
    pub start_column: Option<usize>,
    /// One-based end line.
    pub end_line:     Option<usize>,
    /// One-based end column.
    pub end_column:   Option<usize>,
}

/// One status emitted for a batch compile input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchCompileStatus {
    /// Compilation succeeded.
    Success,
    /// The input was skipped because it has no entrypoint.
    Skipped,
    /// Compilation failed.
    Error,
}

/// Summary of one batch compile run.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BatchCompileReport {
    /// Per-input results in compile order.
    pub entries:   Vec<BatchCompileEntry>,
    /// Number of successful inputs.
    pub successes: usize,
    /// Number of skipped inputs.
    pub skips:     usize,
    /// Number of failed inputs.
    pub errors:    usize,
}

/// Errors returned before or outside individual compile attempts in batch mode.
#[derive(Debug)]
pub enum BatchCompileError {
    /// The caller cancelled the batch.
    Cancelled(crate::Cancelled),
    /// The requested batch configuration is internally inconsistent.
    Configuration(String),
    /// One input failed compilation while continue-on-error was disabled.
    Compilation(String),
    /// Directory traversal or output setup failed.
    Io(io::Error),
}

impl fmt::Display for BatchCompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled(error) => error.fmt(f),
            Self::Configuration(message) => f.write_str(message),
            Self::Compilation(message) => f.write_str(message),
            Self::Io(error) => error.fmt(f),
        }
    }
}

impl Error for BatchCompileError {}

impl From<io::Error> for BatchCompileError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<crate::Cancelled> for BatchCompileError {
    fn from(value: crate::Cancelled) -> Self {
        Self::Cancelled(value)
    }
}

/// Collects and compiles a set of script files and directories.
///
/// # Errors
///
/// Returns [`BatchCompileError`] if directory traversal fails or if one compile
/// fails while `continue_on_error` is disabled.
pub fn compile_paths(
    paths: &[PathBuf],
    options: &BatchCompileOptions,
) -> Result<BatchCompileReport, BatchCompileError> {
    check_batch_cancellation(options)?;
    if paths.is_empty() {
        return Err(BatchCompileError::Configuration(
            "compile requires at least one source file or directory".to_string(),
        ));
    }
    if options.jobs == Some(0) {
        return Err(BatchCompileError::Configuration(
            "compile worker count must be greater than zero".to_string(),
        ));
    }
    if options.output_file.is_some() && options.output_directory.is_some() {
        return Err(BatchCompileError::Configuration(
            "compile accepts either an output file or an output directory, not both".to_string(),
        ));
    }
    let queue = collect_compile_inputs(paths, options)?;
    check_batch_cancellation(options)?;
    if queue.is_empty() {
        return Err(BatchCompileError::Configuration(
            "compile inputs did not contain any .nss source files".to_string(),
        ));
    }
    if options.output_file.is_some() && queue.len() != 1 {
        return Err(BatchCompileError::Configuration(
            "an exact output file requires exactly one input source".to_string(),
        ));
    }
    validate_batch_targets(&queue, options)?;
    if options.driver.emit_graphviz && !options.simulate {
        ensure_graphviz_renderer(options.graphviz_format)?;
    }

    let workers = options.jobs.unwrap_or_else(|| {
        thread::available_parallelism()
            .map(std::num::NonZeroUsize::get)
            .unwrap_or(1)
    });
    let entries = if options.continue_on_error && workers > 1 && queue.len() > 1 {
        compile_queue_parallel(&queue, options, workers)
    } else {
        let mut entries = Vec::with_capacity(queue.len());
        for input in &queue {
            check_batch_cancellation(options)?;
            let entry = compile_batch_input(input, options);
            if entry.status == BatchCompileStatus::Error && !options.continue_on_error {
                return Err(BatchCompileError::Compilation(entry.error.unwrap_or_else(
                    || format!("failed to compile {}", entry.input.display()),
                )));
            }
            entries.push(entry);
        }
        entries
    };
    check_batch_cancellation(options)?;

    let mut report = BatchCompileReport::default();
    for entry in entries {
        match entry.status {
            BatchCompileStatus::Success => report.successes += 1,
            BatchCompileStatus::Skipped => report.skips += 1,
            BatchCompileStatus::Error => report.errors += 1,
        }
        report.entries.push(entry);
    }
    report
        .entries
        .sort_by(|left, right| left.input.cmp(&right.input));
    Ok(report)
}

fn check_batch_cancellation(options: &BatchCompileOptions) -> Result<(), BatchCompileError> {
    options
        .cancellation
        .as_ref()
        .map_or(Ok(()), crate::CancellationToken::check)
        .map_err(BatchCompileError::from)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CollectedCompileInput {
    input:          PathBuf,
    relative_alias: PathBuf,
    source_root:    PathBuf,
}

fn collect_compile_inputs(
    paths: &[PathBuf],
    options: &BatchCompileOptions,
) -> Result<Vec<CollectedCompileInput>, io::Error> {
    let mut queue = BTreeMap::new();
    let mut visited_directories = BTreeSet::new();
    for path in paths {
        if path.is_dir() {
            collect_one(path, path, options, &mut visited_directories, &mut queue)?;
        } else if can_compile_file(path) {
            let alias = path
                .file_stem()
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("scriptout"));
            let source_root = path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from("."));
            queue.insert(path.to_path_buf(), (alias, source_root));
        } else if !path.exists() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("compile input does not exist: {}", path.display()),
            ));
        }
    }
    Ok(queue
        .into_iter()
        .map(
            |(input, (relative_alias, source_root))| CollectedCompileInput {
                input,
                relative_alias,
                source_root,
            },
        )
        .collect())
}

fn collect_one(
    path: &Path,
    root: &Path,
    options: &BatchCompileOptions,
    visited_directories: &mut BTreeSet<PathBuf>,
    queue: &mut BTreeMap<PathBuf, (PathBuf, PathBuf)>,
) -> Result<(), io::Error> {
    let canonical = fs::canonicalize(path)?;
    if !visited_directories.insert(canonical) {
        return Ok(());
    }
    let mut entries = fs::read_dir(path)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let file_type = entry.file_type()?;
        let entry_path = entry.path();
        if file_type.is_symlink() && !options.follow_symlinks {
            continue;
        }
        if entry_path.is_dir() {
            if options.recurse {
                collect_one(&entry_path, root, options, visited_directories, queue)?;
            }
        } else if entry_path.is_file() && can_compile_file(&entry_path) {
            let relative = entry_path.strip_prefix(root).unwrap_or(&entry_path);
            let mut alias = relative.to_path_buf();
            alias.set_extension("");
            queue.insert(entry_path, (alias, root.to_path_buf()));
        }
    }
    Ok(())
}

fn can_compile_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("nss"))
        && !path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("nwscript.nss"))
}

fn output_context(
    input: &CollectedCompileInput,
    options: &BatchCompileOptions,
) -> (PathBuf, PathBuf, PathBuf) {
    if let Some(output_file) = &options.output_file {
        let directory = output_file
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let alias = output_file
            .file_stem()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("scriptout"));
        return (directory, alias, input.relative_alias.clone());
    }
    if let Some(directory) = &options.output_directory {
        return (
            directory.clone(),
            input.relative_alias.clone(),
            input.relative_alias.clone(),
        );
    }
    let directory = input
        .input
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let alias = input
        .input
        .file_stem()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("scriptout"));
    (directory, alias, input.relative_alias.clone())
}

fn target_paths(input: &CollectedCompileInput, options: &BatchCompileOptions) -> Vec<PathBuf> {
    let (output_directory, output_alias, graphviz_alias) = output_context(input, options);
    let binary_output = options
        .output_file
        .clone()
        .unwrap_or_else(|| output_directory.join(output_alias.with_extension("ncs")));
    let mut paths = vec![binary_output.clone()];
    if options.driver.session.emit_debug {
        paths.push(binary_output.with_extension("ndb"));
    }
    if options.driver.emit_graphviz {
        let directory = options
            .graphviz_directory
            .as_ref()
            .unwrap_or(&output_directory);
        if options.graphviz_format == GraphvizOutputFormat::Dot || options.keep_graphviz_dot {
            paths.push(directory.join(graphviz_alias.with_extension("dot")));
        }
        if options.graphviz_format != GraphvizOutputFormat::Dot {
            paths.push(
                directory.join(graphviz_alias.with_extension(options.graphviz_format.extension())),
            );
        }
    }
    paths
}

fn validate_batch_targets(
    queue: &[CollectedCompileInput],
    options: &BatchCompileOptions,
) -> Result<(), BatchCompileError> {
    let mut targets = BTreeSet::new();
    for input in queue {
        for target in target_paths(input, options) {
            if paths_refer_to_same_file(&target, &input.input) {
                return Err(BatchCompileError::Configuration(format!(
                    "compiled output would overwrite its source file: {}",
                    target.display()
                )));
            }
            if !targets.insert(target.clone()) {
                return Err(BatchCompileError::Configuration(format!(
                    "multiple inputs would write the same output path: {}",
                    target.display()
                )));
            }
            if target.exists() && !options.overwrite_existing && !options.simulate {
                return Err(BatchCompileError::Configuration(format!(
                    "output already exists; use overwrite to replace {}",
                    target.display()
                )));
            }
        }
    }
    Ok(())
}

fn paths_refer_to_same_file(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

fn compile_queue_parallel(
    queue: &[CollectedCompileInput],
    options: &BatchCompileOptions,
    workers: usize,
) -> Vec<BatchCompileEntry> {
    let worker_count = queue.len().min(workers).max(1);
    let chunk_size = queue.len().div_ceil(worker_count);
    let mut entries = Vec::with_capacity(queue.len());
    thread::scope(|scope| {
        let mut handles = Vec::new();
        for chunk in queue.chunks(chunk_size) {
            handles.push(scope.spawn(move || {
                chunk
                    .iter()
                    .map(|input| compile_batch_input(input, options))
                    .collect::<Vec<_>>()
            }));
        }
        for handle in handles {
            match handle.join() {
                Ok(worker_entries) => entries.extend(worker_entries),
                Err(_) => entries.push(BatchCompileEntry {
                    input:                  PathBuf::from("<compile worker>"),
                    status:                 BatchCompileStatus::Error,
                    outputs:                Vec::new(),
                    error:                  Some(
                        "parallel NWScript compile worker panicked".to_string(),
                    ),
                    diagnostic:             None,
                    additional_diagnostics: Vec::new(),
                }),
            }
        }
    });
    entries
}

fn compile_batch_input(
    input: &CollectedCompileInput,
    options: &BatchCompileOptions,
) -> BatchCompileEntry {
    if options
        .cancellation
        .as_ref()
        .is_some_and(crate::CancellationToken::is_cancelled)
    {
        return BatchCompileEntry {
            input:                  input.input.clone(),
            status:                 BatchCompileStatus::Error,
            outputs:                Vec::new(),
            error:                  Some("operation cancelled".to_string()),
            diagnostic:             None,
            additional_diagnostics: Vec::new(),
        };
    }
    let parent = input
        .input
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let mut resolver = FileSystemScriptResolver::with_root(&parent);
    resolver.add_root(&input.source_root);
    for root in &options.search_roots {
        resolver.add_root(root);
    }
    for (path, contents) in &options.source_overlays {
        resolver.add_overlay(path.clone(), contents.clone());
    }
    let (output_directory, output_alias, graphviz_alias) = output_context(input, options);
    let mut host = DirectoryCompilerHost::new(resolver, output_directory.clone());
    if let Some(output_file) = &options.output_file {
        host.set_binary_output_file(output_file.clone());
    }
    if let Some(fallback) = &options.fallback_resolver {
        host.set_fallback_resolver(Arc::clone(fallback));
    }
    if let Some(graphviz_directory) = &options.graphviz_directory {
        host.set_graphviz_directory(graphviz_directory.clone());
    }
    host.set_graphviz_output(options.graphviz_format, options.keep_graphviz_dot);
    host.set_simulate(options.simulate);
    host.set_overwrite_existing(options.overwrite_existing);

    let mut driver = options.driver.clone();
    driver.output_alias = output_alias.to_string_lossy().into_owned();
    driver.graphviz_alias = Some(graphviz_alias.to_string_lossy().into_owned());

    let compiled = match options.cancellation.as_ref() {
        Some(cancellation) => compile_file_with_host_with_cancellation(
            &mut host,
            &input.input.to_string_lossy(),
            &driver,
            cancellation,
        ),
        None => compile_file_with_host(&mut host, &input.input.to_string_lossy(), &driver),
    };
    match compiled {
        Ok(CompileFileOutcome::Compiled(_)) => {
            if options.remove_stale_debug && !driver.session.emit_debug && !options.simulate {
                let stale = options.output_file.as_ref().map_or_else(
                    || output_directory.join(output_alias.with_extension("ndb")),
                    |output_file| output_file.with_extension("ndb"),
                );
                if stale.is_file()
                    && let Err(error) = fs::remove_file(&stale)
                {
                    return BatchCompileEntry {
                        input:                  input.input.clone(),
                        status:                 BatchCompileStatus::Error,
                        outputs:                host.written_paths().to_vec(),
                        error:                  Some(format!(
                            "failed to remove stale debugger output {}: {error}",
                            stale.display()
                        )),
                        diagnostic:             None,
                        additional_diagnostics: Vec::new(),
                    };
                }
            }
            BatchCompileEntry {
                input:                  input.input.clone(),
                status:                 BatchCompileStatus::Success,
                outputs:                host.written_paths().to_vec(),
                error:                  None,
                diagnostic:             None,
                additional_diagnostics: Vec::new(),
            }
        }
        Ok(CompileFileOutcome::SkippedNoEntrypoint) => BatchCompileEntry {
            input:                  input.input.clone(),
            status:                 BatchCompileStatus::Skipped,
            outputs:                host.written_paths().to_vec(),
            error:                  None,
            diagnostic:             None,
            additional_diagnostics: Vec::new(),
        },
        Err(error) => {
            let diagnostic = source_aware_driver_diagnostic(
                &error,
                &host,
                &input.input,
                driver.session.source_load,
            );
            BatchCompileEntry {
                input:                  input.input.clone(),
                status:                 BatchCompileStatus::Error,
                outputs:                host.written_paths().to_vec(),
                error:                  Some(format_compiler_diagnostic(&diagnostic, &input.input)),
                diagnostic:             Some(diagnostic),
                additional_diagnostics: Vec::new(),
            }
        }
    }
}

fn compiler_driver_error_span(error: &CompilerDriverError) -> Option<crate::Span> {
    let CompilerDriverError::Session(session_error) = error else {
        return None;
    };
    match session_error {
        CompilerSessionError::Preprocess(crate::PreprocessError::Lex(error)) => Some(error.span),
        CompilerSessionError::Preprocess(crate::PreprocessError::Macro(error)) => error.span,
        CompilerSessionError::Compile(compile_error) => match compile_error {
            crate::CompileError::Parse(crate::ResolvedParseError::Parse(error)) => Some(error.span),
            crate::CompileError::Parse(crate::ResolvedParseError::Preprocess(
                crate::PreprocessError::Lex(error),
            )) => Some(error.span),
            crate::CompileError::Parse(crate::ResolvedParseError::Preprocess(
                crate::PreprocessError::Macro(error),
            )) => error.span,
            crate::CompileError::Semantic(error) => Some(error.span),
            crate::CompileError::Hir(error) => Some(error.span),
            crate::CompileError::Codegen(error) => error.span,
            crate::CompileError::Parse(crate::ResolvedParseError::Preprocess(
                crate::PreprocessError::Cancelled(_) | crate::PreprocessError::Source(_),
            )) => None,
        },
        CompilerSessionError::Cancelled(_)
        | CompilerSessionError::LangSpec(_)
        | CompilerSessionError::Preprocess(crate::PreprocessError::Cancelled(_))
        | CompilerSessionError::Preprocess(crate::PreprocessError::Source(_))
        | CompilerSessionError::Source(_) => None,
    }
}

fn compiler_driver_error_code(error: &CompilerDriverError) -> Option<i32> {
    let CompilerDriverError::Session(session_error) = error else {
        return None;
    };
    let code = match session_error {
        CompilerSessionError::Preprocess(crate::PreprocessError::Lex(error)) => Some(error.code),
        CompilerSessionError::Compile(compile_error) => match compile_error {
            crate::CompileError::Parse(crate::ResolvedParseError::Parse(error)) => Some(error.code),
            crate::CompileError::Parse(crate::ResolvedParseError::Preprocess(
                crate::PreprocessError::Lex(error),
            )) => Some(error.code),
            crate::CompileError::Semantic(error) => Some(error.code),
            crate::CompileError::Codegen(error) => error.code,
            crate::CompileError::Parse(crate::ResolvedParseError::Preprocess(
                crate::PreprocessError::Cancelled(_)
                | crate::PreprocessError::Macro(_)
                | crate::PreprocessError::Source(_),
            ))
            | crate::CompileError::Hir(_) => None,
        },
        CompilerSessionError::Cancelled(_)
        | CompilerSessionError::LangSpec(_)
        | CompilerSessionError::Preprocess(
            crate::PreprocessError::Cancelled(_)
            | crate::PreprocessError::Macro(_)
            | crate::PreprocessError::Source(_),
        )
        | CompilerSessionError::Source(_) => None,
    }?;
    Some(code.code())
}

/// Converts a compiler-driver failure into structured, source-aware editor
/// diagnostic data.
pub fn source_aware_driver_diagnostic<R: ScriptResolver + ?Sized>(
    error: &CompilerDriverError,
    resolver: &R,
    input: &Path,
    source_load: crate::SourceLoadOptions,
) -> CompilerDiagnostic {
    let mut diagnostic = CompilerDiagnostic {
        code:         compiler_driver_error_code(error),
        message:      error.to_string(),
        file:         None,
        start_line:   None,
        start_column: None,
        end_line:     None,
        end_column:   None,
    };
    let Some(span) = compiler_driver_error_span(error) else {
        return diagnostic;
    };
    let Ok(bundle) = crate::load_source_bundle(resolver, &input.to_string_lossy(), source_load)
    else {
        return diagnostic;
    };
    let Some(file) = bundle.source_map.get(span.source_id) else {
        return diagnostic;
    };
    let span = visible_diagnostic_span(file, span);
    let Some(start) = file.location(span.start) else {
        return diagnostic;
    };
    let end_offset = if span.is_empty() {
        span.end.saturating_add(1).min(file.len())
    } else {
        span.end.min(file.len())
    };
    let end = file.location(end_offset).unwrap_or(start);
    diagnostic.file = Some(file.name.clone());
    diagnostic.start_line = Some(start.line);
    diagnostic.start_column = Some(start.column);
    diagnostic.end_line = Some(end.line);
    diagnostic.end_column = Some(end.column);
    diagnostic
}

/// Ensures editor diagnostics cover source text instead of a zero-width EOF
/// token or whitespace on the following line. Frontend phases should still
/// report the most precise span they have; this is the shared safety net for
/// every diagnostic producer.
fn visible_diagnostic_span(file: &crate::SourceFile, span: crate::Span) -> crate::Span {
    let bytes = file.bytes();
    if span.source_id != file.id || span.start > span.end || span.end > bytes.len() {
        return span;
    }
    if bytes
        .get(span.start..span.end)
        .unwrap_or_default()
        .iter()
        .any(|byte| !byte.is_ascii_whitespace())
    {
        return span;
    }

    let mut end = span.start.min(bytes.len());
    while end > 0 && bytes.get(end - 1).is_some_and(u8::is_ascii_whitespace) {
        end -= 1;
    }
    if end > 0 {
        let start = diagnostic_unit_start(bytes, end);
        return crate::Span::new(span.source_id, start, end);
    }

    let mut start = span.end.min(bytes.len());
    while bytes.get(start).is_some_and(u8::is_ascii_whitespace) {
        start += 1;
    }
    if start < bytes.len() {
        let end = diagnostic_unit_end(bytes, start);
        return crate::Span::new(span.source_id, start, end);
    }

    span
}

fn diagnostic_unit_start(bytes: &[u8], end: usize) -> usize {
    let last = bytes
        .get(end.saturating_sub(1))
        .copied()
        .unwrap_or_default();
    if !is_diagnostic_word_byte(last) {
        return end - 1;
    }
    let mut start = end - 1;
    while start > 0
        && bytes
            .get(start - 1)
            .copied()
            .is_some_and(is_diagnostic_word_byte)
    {
        start -= 1;
    }
    start
}

fn diagnostic_unit_end(bytes: &[u8], start: usize) -> usize {
    if !bytes
        .get(start)
        .copied()
        .is_some_and(is_diagnostic_word_byte)
    {
        return start + 1;
    }
    let mut end = start + 1;
    while bytes.get(end).copied().is_some_and(is_diagnostic_word_byte) {
        end += 1;
    }
    end
}

const fn is_diagnostic_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn format_compiler_diagnostic(diagnostic: &CompilerDiagnostic, input: &Path) -> String {
    match (
        diagnostic.file.as_deref(),
        diagnostic.start_line,
        diagnostic.start_column,
    ) {
        (Some(file), Some(line), Some(column)) => {
            format!("{file}:{line}:{column}: {}", diagnostic.message)
        }
        _ => format!(
            "failed to compile {}: {}",
            input.display(),
            diagnostic.message
        ),
    }
}

/// Formats a compiler-driver failure with its resolved source location when
/// the error carries a source span.
pub fn format_source_aware_driver_error<R: ScriptResolver + ?Sized>(
    error: &CompilerDriverError,
    resolver: &R,
    input: &Path,
    source_load: crate::SourceLoadOptions,
) -> String {
    let diagnostic = source_aware_driver_diagnostic(error, resolver, input, source_load);
    format_compiler_diagnostic(&diagnostic, input)
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use nwnrs_types::resman::prelude::ResType;

    use super::{
        BatchCompileOptions, BatchCompileStatus, CompileFileOutcome, CompilerDriverOptions,
        CompilerHost, CompilerHostError, FileSystemScriptResolver, GraphvizOutputFormat,
        compile_file_with_host, compile_paths, visible_diagnostic_span,
    };
    use crate::{
        CancellationToken, NW_SCRIPT_SOURCE_RES_TYPE, ScriptResolver, SourceFile, SourceId, Span,
    };

    #[derive(Default)]
    struct MemoryHost {
        sources:  HashMap<(ResType, String), Vec<u8>>,
        files:    Vec<(String, ResType, Vec<u8>)>,
        graphviz: Vec<(String, String)>,
    }

    impl MemoryHost {
        fn insert_source(&mut self, name: &str, contents: &str) {
            self.sources.insert(
                (NW_SCRIPT_SOURCE_RES_TYPE, name.to_ascii_lowercase()),
                contents.as_bytes().to_vec(),
            );
        }
    }

    impl CompilerHost for MemoryHost {
        fn resolve_script_bytes(
            &self,
            script_name: &str,
            res_type: ResType,
        ) -> Result<Option<Vec<u8>>, crate::SourceError> {
            Ok(self
                .sources
                .get(&(res_type, script_name.to_ascii_lowercase()))
                .cloned())
        }

        fn write_file(
            &mut self,
            file_name: &str,
            res_type: ResType,
            data: &[u8],
            _binary: bool,
        ) -> Result<(), CompilerHostError> {
            self.files
                .push((file_name.to_string(), res_type, data.to_vec()));
            Ok(())
        }

        fn write_graphviz(&mut self, file_name: &str, dot: &str) -> Result<(), CompilerHostError> {
            self.graphviz.push((file_name.to_string(), dot.to_string()));
            Ok(())
        }
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!("nwnrs-types-{prefix}-{nanos}"))
    }

    #[test]
    fn source_diagnostics_never_select_eof_or_whitespace() {
        let source_id = SourceId::new(0);
        let source = "  void main() {\n    Broken();\n}\n\n";
        let file = SourceFile::new(source_id, "broken.nss", source);
        let cases = [
            (Span::new(source_id, source.len(), source.len()), "}"),
            (Span::new(source_id, source.len() - 2, source.len()), "}"),
            (Span::new(source_id, 0, 0), "void"),
            (Span::new(source_id, 0, 2), "void"),
        ];

        for (span, expected) in cases {
            let corrected = visible_diagnostic_span(&file, span);
            assert_eq!(file.span_text(corrected), Some(expected));
        }
    }

    #[test]
    fn source_diagnostics_preserve_precise_non_whitespace_spans() {
        let source_id = SourceId::new(0);
        let source = "void main() { Broken(); }";
        let file = SourceFile::new(source_id, "broken.nss", source);
        let start = source.find("Broken").expect("fixture contains symbol");
        let span = Span::new(source_id, start, start + "Broken".len());

        assert_eq!(visible_diagnostic_span(&file, span), span);
    }

    #[test]
    fn compiles_through_callback_host_and_emits_graphviz() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut host = MemoryHost::default();
        host.insert_source("nwscript", "void PrintInteger(int n);");
        host.insert_source("main", "void main() { PrintInteger(42); }");

        let options = CompilerDriverOptions {
            emit_graphviz: true,
            output_alias: "main".to_string(),
            ..CompilerDriverOptions::default()
        };
        let outcome = compile_file_with_host(&mut host, "main", &options)?;
        assert!(matches!(outcome, CompileFileOutcome::Compiled(_)));
        assert_eq!(host.files.len(), 2);
        assert_eq!(host.graphviz.len(), 1);
        assert_eq!(
            host.graphviz
                .first()
                .map(|(_name, dot)| dot.contains("Function main")),
            Some(true)
        );
        Ok(())
    }

    #[test]
    fn batch_compiler_reports_success_skip_and_error() -> Result<(), Box<dyn std::error::Error>> {
        let root = unique_temp_dir("batch");
        fs::create_dir_all(&root)?;
        fs::write(root.join("nwscript.nss"), "void PrintInteger(int n);")?;
        fs::write(root.join("main.nss"), "void main() { PrintInteger(42); }")?;
        fs::write(
            root.join("helper.nss"),
            "int AddOne(int n) { return n + 1; }",
        )?;
        fs::write(root.join("broken.nss"), "void main( {")?;

        let mut options = BatchCompileOptions {
            recurse: true,
            continue_on_error: true,
            simulate: true,
            graphviz_format: GraphvizOutputFormat::Svg,
            keep_graphviz_dot: true,
            driver: CompilerDriverOptions {
                emit_graphviz: true,
                skip_missing_entrypoint: true,
                ..CompilerDriverOptions::default()
            },
            ..BatchCompileOptions::default()
        };
        options.search_roots.push(root.clone());

        let report = compile_paths(std::slice::from_ref(&root), &options)?;
        assert_eq!(report.successes, 1);
        assert_eq!(report.skips, 1);
        assert_eq!(report.errors, 1);
        assert!(
            report
                .entries
                .iter()
                .any(|entry| entry.status == BatchCompileStatus::Success)
        );
        assert!(
            report
                .entries
                .iter()
                .any(|entry| entry.status == BatchCompileStatus::Skipped)
        );
        assert!(
            report
                .entries
                .iter()
                .any(|entry| entry.status == BatchCompileStatus::Error)
        );
        let diagnostic = report
            .entries
            .iter()
            .find_map(|entry| entry.diagnostic.as_ref())
            .ok_or("missing structured compiler diagnostic")?;
        assert!(
            diagnostic
                .file
                .as_deref()
                .is_some_and(|file| file.ends_with("broken.nss"))
        );
        assert_eq!(diagnostic.start_line, Some(1));
        assert!(diagnostic.start_column.is_some());
        assert!(diagnostic.end_line.is_some());
        assert!(diagnostic.end_column.is_some());
        let success_outputs = report
            .entries
            .iter()
            .find(|entry| entry.status == BatchCompileStatus::Success)
            .map(|entry| entry.outputs.as_slice())
            .unwrap_or_default();
        assert!(
            success_outputs
                .iter()
                .any(|path| path.extension() == Some("dot".as_ref()))
        );
        assert!(
            success_outputs
                .iter()
                .any(|path| path.extension() == Some("svg".as_ref()))
        );
        fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[test]
    fn recursive_batch_preserves_output_and_graphviz_hierarchy()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = unique_temp_dir("batch-hierarchy");
        let source_root = root.join("source");
        let nested = source_root.join("chapter/encounters");
        let output_root = root.join("compiled");
        let graphviz_root = root.join("graphs");
        fs::create_dir_all(&nested)?;
        fs::write(
            source_root.join("nwscript.nss"),
            "void PrintInteger(int n);",
        )?;
        fs::write(
            nested.join("ambush.nss"),
            "void main() { PrintInteger(42); }",
        )?;

        let options = BatchCompileOptions {
            recurse: true,
            output_directory: Some(output_root.clone()),
            graphviz_directory: Some(graphviz_root.clone()),
            driver: CompilerDriverOptions {
                emit_graphviz: true,
                ..CompilerDriverOptions::default()
            },
            ..BatchCompileOptions::default()
        };
        let report = compile_paths(std::slice::from_ref(&source_root), &options)?;

        assert_eq!(report.successes, 1);
        assert!(output_root.join("chapter/encounters/ambush.ncs").is_file());
        assert!(
            graphviz_root
                .join("chapter/encounters/ambush.dot")
                .is_file()
        );
        fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[test]
    fn filesystem_resolver_checks_roots_and_default_extension()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = unique_temp_dir("resolver");
        fs::create_dir_all(&root)?;
        fs::write(root.join("test.nss"), "void main() {}")?;
        let resolver = FileSystemScriptResolver::with_root(&root);
        let resolved = resolver.resolve_script_bytes("test", NW_SCRIPT_SOURCE_RES_TYPE)?;
        assert!(resolved.is_some());
        fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[test]
    fn filesystem_resolver_matches_script_names_case_insensitively()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = unique_temp_dir("resolver-case");
        fs::create_dir_all(&root)?;
        fs::write(root.join("MixedCase.NSS"), "void main() {}")?;
        let resolver = FileSystemScriptResolver::with_root(&root);
        let resolved = resolver.resolve_script_bytes("mixedcase.nss", NW_SCRIPT_SOURCE_RES_TYPE)?;
        assert!(resolved.is_some());
        fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[test]
    fn batch_compilation_stops_before_work_when_cancelled() -> Result<(), Box<dyn std::error::Error>>
    {
        let root = unique_temp_dir("cancelled-batch");
        fs::create_dir_all(&root)?;
        let source = root.join("main.nss");
        fs::write(&source, "void main() {}")?;
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        let options = BatchCompileOptions {
            cancellation: Some(cancellation),
            simulate: true,
            ..BatchCompileOptions::default()
        };
        let error = compile_paths(&[source], &options).expect_err("cancelled batch must fail");
        assert_eq!(error.to_string(), "operation cancelled");
        fs::remove_dir_all(root)?;
        Ok(())
    }
}
