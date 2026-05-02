use std::{
    collections::BTreeSet,
    error::Error,
    fmt, fs, io,
    path::{Path, PathBuf},
};

use nwnrs_types::resman::prelude::{ResType, get_res_ext};

use crate::{
    CompileArtifacts, CompilerSession, CompilerSessionError, CompilerSessionOptions,
    NW_SCRIPT_SOURCE_RES_TYPE, ScriptResolver, SourceError, session::PreparedScript,
};

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
    /// Resource type requested for source resolution.
    pub source_res_type:         ResType,
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
            source_res_type:         NW_SCRIPT_SOURCE_RES_TYPE,
            binary_res_type:         ResType(2010),
            debug_res_type:          ResType(2064),
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
    let (prepared, artifacts, graphviz) = {
        let resolver = HostResolver {
            host: &*host
        };
        let mut session = CompilerSession::with_options(&resolver, options.session.clone());
        let prepared = session.prepare_script_name(script_name)?;
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
        let artifacts = session
            .compile_prepared(&prepared)
            .map_err(CompilerSessionError::from)
            .map_err(CompilerDriverError::from)?;
        (prepared, artifacts, graphviz)
    };

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
    roots: Vec<PathBuf>,
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
                candidates.push(name.clone());
                for root in &self.roots {
                    candidates.push(root.join(&name));
                }
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
            if candidate.is_file() {
                return fs::read(&candidate)
                    .map(Some)
                    .map_err(|error| SourceError::resolver(error.to_string()));
            }
        }
        Ok(None)
    }
}

/// One directory-backed compiler host that reads source files from filesystem
/// roots and writes outputs back to disk.
#[derive(Debug, Clone)]
pub struct DirectoryCompilerHost {
    resolver:           FileSystemScriptResolver,
    output_directory:   PathBuf,
    graphviz_directory: Option<PathBuf>,
    simulate:           bool,
    written_paths:      Vec<PathBuf>,
}

impl DirectoryCompilerHost {
    /// Creates one directory host rooted at `output_directory`.
    #[must_use]
    pub fn new(resolver: FileSystemScriptResolver, output_directory: impl Into<PathBuf>) -> Self {
        Self {
            resolver,
            output_directory: output_directory.into(),
            graphviz_directory: None,
            simulate: false,
            written_paths: Vec::new(),
        }
    }

    /// Sets an alternate directory for Graphviz DOT output.
    pub fn set_graphviz_directory(&mut self, directory: impl Into<PathBuf>) {
        self.graphviz_directory = Some(directory.into());
    }

    /// Enables or disables simulate mode, which records target paths without
    /// writing files.
    pub fn set_simulate(&mut self, simulate: bool) {
        self.simulate = simulate;
    }

    /// Returns the paths written or scheduled during the most recent compile.
    #[must_use]
    pub fn written_paths(&self) -> &[PathBuf] {
        &self.written_paths
    }
}

impl CompilerHost for DirectoryCompilerHost {
    fn resolve_script_bytes(
        &self,
        script_name: &str,
        res_type: ResType,
    ) -> Result<Option<Vec<u8>>, SourceError> {
        self.resolver.resolve_script_bytes(script_name, res_type)
    }

    fn write_file(
        &mut self,
        file_name: &str,
        res_type: ResType,
        data: &[u8],
        _binary: bool,
    ) -> Result<(), CompilerHostError> {
        let path = self
            .output_directory
            .join(format!("{file_name}.{}", get_res_ext(res_type)));
        self.written_paths.push(path.clone());
        if self.simulate {
            return Ok(());
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
        let path = base.join(format!("{file_name}.dot"));
        self.written_paths.push(path.clone());
        if self.simulate {
            return Ok(());
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, dot.as_bytes())?;
        Ok(())
    }
}

/// Options controlling multi-file directory and file compilation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchCompileOptions {
    /// Callback/session behavior for each compilation.
    pub driver:             CompilerDriverOptions,
    /// Extra filesystem roots used for langspec and include resolution.
    pub search_roots:       Vec<PathBuf>,
    /// Whether directory traversal should recurse.
    pub recurse:            bool,
    /// Whether directory traversal should follow symlinks.
    pub follow_symlinks:    bool,
    /// Whether compilation should continue after one file fails.
    pub continue_on_error:  bool,
    /// Whether outputs should be simulated without writing files.
    pub simulate:           bool,
    /// Optional output directory overriding each source file's parent.
    pub output_directory:   Option<PathBuf>,
    /// Optional directory for Graphviz DOT output.
    pub graphviz_directory: Option<PathBuf>,
}

impl Default for BatchCompileOptions {
    fn default() -> Self {
        Self {
            driver:             CompilerDriverOptions {
                skip_missing_entrypoint: true,
                ..CompilerDriverOptions::default()
            },
            search_roots:       Vec::new(),
            recurse:            false,
            follow_symlinks:    false,
            continue_on_error:  false,
            simulate:           false,
            output_directory:   None,
            graphviz_directory: None,
        }
    }
}

/// One per-input result from a batch compile run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchCompileEntry {
    /// Input file path.
    pub input:   PathBuf,
    /// Final status for this input.
    pub status:  BatchCompileStatus,
    /// Output paths written or scheduled by the host.
    pub outputs: Vec<PathBuf>,
    /// Human-readable error text when compilation failed.
    pub error:   Option<String>,
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
    /// Directory traversal or output setup failed.
    Io(io::Error),
    /// One compile failed and `continue_on_error` was disabled.
    Driver(CompilerDriverError),
}

impl fmt::Display for BatchCompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::Driver(error) => error.fmt(f),
        }
    }
}

impl Error for BatchCompileError {}

impl From<io::Error> for BatchCompileError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<CompilerDriverError> for BatchCompileError {
    fn from(value: CompilerDriverError) -> Self {
        Self::Driver(value)
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
    let queue = collect_compile_inputs(paths, options)?;
    let mut report = BatchCompileReport::default();

    for input in queue {
        let parent = input
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let output_directory = options
            .output_directory
            .clone()
            .unwrap_or_else(|| parent.clone());
        let mut resolver = FileSystemScriptResolver::with_root(&parent);
        for root in &options.search_roots {
            resolver.add_root(root);
        }
        let mut host = DirectoryCompilerHost::new(resolver, output_directory);
        if let Some(graphviz_directory) = &options.graphviz_directory {
            host.set_graphviz_directory(graphviz_directory.clone());
        }
        host.set_simulate(options.simulate);

        let mut driver = options.driver.clone();
        driver.output_alias = input
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("scriptout")
            .to_string();
        if driver.graphviz_alias.is_none() {
            driver.graphviz_alias = Some(driver.output_alias.clone());
        }

        match compile_file_with_host(&mut host, &input.to_string_lossy(), &driver) {
            Ok(CompileFileOutcome::Compiled(_)) => {
                report.successes += 1;
                report.entries.push(BatchCompileEntry {
                    input,
                    status: BatchCompileStatus::Success,
                    outputs: host.written_paths().to_vec(),
                    error: None,
                });
            }
            Ok(CompileFileOutcome::SkippedNoEntrypoint) => {
                report.skips += 1;
                report.entries.push(BatchCompileEntry {
                    input,
                    status: BatchCompileStatus::Skipped,
                    outputs: host.written_paths().to_vec(),
                    error: None,
                });
            }
            Err(error) => {
                let message = error.to_string();
                report.errors += 1;
                report.entries.push(BatchCompileEntry {
                    input,
                    status: BatchCompileStatus::Error,
                    outputs: host.written_paths().to_vec(),
                    error: Some(message),
                });
                if !options.continue_on_error {
                    return Err(BatchCompileError::Driver(error));
                }
            }
        }
    }

    Ok(report)
}

fn collect_compile_inputs(
    paths: &[PathBuf],
    options: &BatchCompileOptions,
) -> Result<Vec<PathBuf>, io::Error> {
    let mut queue = BTreeSet::new();
    for path in paths {
        collect_one(path, options, &mut queue)?;
    }
    Ok(queue.into_iter().collect())
}

fn collect_one(
    path: &Path,
    options: &BatchCompileOptions,
    queue: &mut BTreeSet<PathBuf>,
) -> Result<(), io::Error> {
    if path.is_file() {
        if can_compile_file(path) {
            queue.insert(path.to_path_buf());
        }
        return Ok(());
    }
    if path.is_dir() {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            let entry_path = entry.path();
            if file_type.is_symlink() && !options.follow_symlinks {
                continue;
            }
            if file_type.is_dir() {
                if options.recurse {
                    collect_one(&entry_path, options, queue)?;
                }
            } else if file_type.is_file() && can_compile_file(&entry_path) {
                queue.insert(entry_path);
            }
        }
    }
    Ok(())
}

fn can_compile_file(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("nss")
        && path.file_name().and_then(|name| name.to_str()) != Some("nwscript.nss")
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
        CompilerHost, CompilerHostError, FileSystemScriptResolver, compile_file_with_host,
        compile_paths,
    };
    use crate::{NW_SCRIPT_SOURCE_RES_TYPE, ScriptResolver};

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
}
