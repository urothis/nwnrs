use super::*;

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
    /// Exact physical or immutable virtual source origin.
    pub source:       Option<crate::ScriptSourceOrigin>,
    /// One-based start line.
    pub start_line:   Option<usize>,
    /// One-based start column.
    pub start_column: Option<usize>,
    /// One-based end line.
    pub end_line:     Option<usize>,
    /// One-based end column.
    pub end_column:   Option<usize>,
    /// Exact UTF-8 byte offset of the first covered byte.
    pub start_offset: Option<usize>,
    /// Exact UTF-8 byte offset immediately after the covered bytes.
    pub end_offset:   Option<usize>,
    /// Exact source bytes used by the compiler for UTF-16 editor mapping.
    pub source_bytes: Option<Vec<u8>>,
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
        graphviz_output::ensure_graphviz_renderer(
            options.graphviz_format,
            options.cancellation.as_ref(),
        )?;
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
        check_collection_cancellation(options)?;
        match fs::metadata(path) {
            Ok(metadata) if metadata.is_dir() => {
                collect_one(path, path, options, &mut visited_directories, &mut queue)?;
            }
            Ok(metadata) if metadata.is_file() && can_compile_file(path) => {
                let alias = path
                    .file_stem()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("scriptout"));
                let source_root = path
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| PathBuf::from("."));
                queue.insert(path.to_path_buf(), (alias, source_root));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("compile input does not exist: {}", path.display()),
                ));
            }
            Err(error) => return Err(error),
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
    const MAX_DISCOVERY_ENTRIES: usize = 5_000_000;
    const MAX_DISCOVERY_DEPTH: usize = 4_096;

    let mut pending = vec![(path.to_path_buf(), 0_usize)];
    let mut inspected = 0_usize;
    while let Some((directory, depth)) = pending.pop() {
        check_collection_cancellation(options)?;
        if depth > MAX_DISCOVERY_DEPTH {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "compile input traversal exceeded maximum depth {MAX_DISCOVERY_DEPTH} at {}",
                    directory.display()
                ),
            ));
        }
        let canonical = fs::canonicalize(&directory)?;
        if !visited_directories.insert(canonical) {
            continue;
        }
        let iterator = fs::read_dir(&directory)?;
        let mut entries = Vec::new();
        for entry in iterator {
            check_collection_cancellation(options)?;
            inspected = inspected.checked_add(1).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "compile input traversal entry count overflowed",
                )
            })?;
            if inspected > MAX_DISCOVERY_ENTRIES {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "compile input traversal exceeded maximum of {MAX_DISCOVERY_ENTRIES} \
                         entries"
                    ),
                ));
            }
            entries.push(entry?);
        }
        entries.sort_by_key(std::fs::DirEntry::file_name);
        let mut children = Vec::new();
        for entry in entries {
            check_collection_cancellation(options)?;
            let file_type = entry.file_type()?;
            let entry_path = entry.path();
            if file_type.is_symlink() && !options.follow_symlinks {
                continue;
            }
            let metadata = entry.metadata()?;
            if metadata.is_dir() {
                let nested_manifest = entry_path.join("nwpkg.toml");
                let nested_package = match fs::metadata(&nested_manifest) {
                    Ok(metadata) => metadata.is_file(),
                    Err(error) if error.kind() == io::ErrorKind::NotFound => false,
                    Err(error) => return Err(error),
                };
                if options.recurse && !nested_package {
                    children.push(entry_path);
                }
            } else if metadata.is_file() && can_compile_file(&entry_path) {
                let relative = entry_path.strip_prefix(root).map_err(|error| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!(
                            "compile input {} escaped traversal root {}: {error}",
                            entry_path.display(),
                            root.display()
                        ),
                    )
                })?;
                let mut alias = relative.to_path_buf();
                alias.set_extension("");
                queue.insert(entry_path, (alias, root.to_path_buf()));
            }
        }
        for child in children.into_iter().rev() {
            pending.push((child, depth + 1));
        }
    }
    Ok(())
}

fn check_collection_cancellation(options: &BatchCompileOptions) -> Result<(), io::Error> {
    options
        .cancellation
        .as_ref()
        .map_or(Ok(()), crate::CancellationToken::check)
        .map_err(|error| io::Error::new(io::ErrorKind::Interrupted, error))
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
    let mut targets = BTreeMap::<Vec<u8>, PathBuf>::new();
    for input in queue {
        for target in target_paths(input, options) {
            if paths_refer_to_same_file(&target, &input.input)? {
                return Err(BatchCompileError::Configuration(format!(
                    "compiled output would overwrite its source file: {}",
                    target.display()
                )));
            }
            let logical = crate::logical_path_key(&target);
            if let Some(existing) = targets.insert(logical.clone(), target.clone()) {
                return Err(BatchCompileError::Configuration(format!(
                    "multiple inputs have ambiguous case-insensitive output paths: {} and {}",
                    existing.display(),
                    target.display(),
                )));
            }
            match fs::metadata(&target) {
                Ok(_) if !options.overwrite_existing && !options.simulate => {
                    return Err(BatchCompileError::Configuration(format!(
                        "output already exists; use overwrite to replace {}",
                        target.display()
                    )));
                }
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(BatchCompileError::Io(error)),
            }
        }
    }
    Ok(())
}

fn paths_refer_to_same_file(left: &Path, right: &Path) -> Result<bool, BatchCompileError> {
    crate::try_physical_sources_equal(left, right).map_err(BatchCompileError::Io)
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
                Err(payload) => entries.push(BatchCompileEntry {
                    input:                  PathBuf::from("<compile worker>"),
                    status:                 BatchCompileStatus::Error,
                    outputs:                Vec::new(),
                    error:                  Some(format!(
                        "parallel NWScript compile worker panicked: {}",
                        panic_payload_message(payload.as_ref())
                    )),
                    diagnostic:             None,
                    additional_diagnostics: Vec::new(),
                }),
            }
        }
    });
    entries
}

fn panic_payload_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_string()
    }
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
    let resolver = (|| {
        let mut resolver = FileSystemScriptResolver::with_root(&parent)?;
        resolver.add_root(&input.source_root)?;
        for root in &options.search_roots {
            resolver.add_root(root)?;
        }
        for (path, contents) in &options.source_overlays {
            resolver.add_overlay_if_in_scope(path.clone(), contents.clone())?;
        }
        Ok::<_, SourceError>(resolver)
    })();
    let resolver = match resolver {
        Ok(resolver) => resolver,
        Err(error) => {
            return BatchCompileEntry {
                input:                  input.input.clone(),
                status:                 BatchCompileStatus::Error,
                outputs:                Vec::new(),
                error:                  Some(error.to_string()),
                diagnostic:             None,
                additional_diagnostics: Vec::new(),
            };
        }
    };
    let (output_directory, output_alias, graphviz_alias) = output_context(input, options);
    let Some(output_alias_text) = output_alias.to_str() else {
        return batch_input_error(
            input,
            format!(
                "compiler output alias is not valid UTF-8: {}",
                crate::physical_path_display(&output_alias)
            ),
        );
    };
    let Some(graphviz_alias_text) = graphviz_alias.to_str() else {
        return batch_input_error(
            input,
            format!(
                "Graphviz output alias is not valid UTF-8: {}",
                crate::physical_path_display(&graphviz_alias)
            ),
        );
    };
    let Some(input_text) = input.input.to_str() else {
        return batch_input_error(
            input,
            format!(
                "NWScript input path is not valid UTF-8: {}",
                crate::physical_path_display(&input.input)
            ),
        );
    };
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
    driver.output_alias = output_alias_text.to_string();
    driver.graphviz_alias = Some(graphviz_alias_text.to_string());
    if options.remove_stale_debug && !driver.session.emit_debug && !options.simulate {
        let stale = options.output_file.as_ref().map_or_else(
            || output_directory.join(output_alias.with_extension("ndb")),
            |output_file| output_file.with_extension("ndb"),
        );
        if let Err(error) = host.schedule_removal(stale) {
            return BatchCompileEntry {
                input:                  input.input.clone(),
                status:                 BatchCompileStatus::Error,
                outputs:                Vec::new(),
                error:                  Some(error.to_string()),
                diagnostic:             None,
                additional_diagnostics: Vec::new(),
            };
        }
    }

    let compiled = match options.cancellation.as_ref() {
        Some(cancellation) => {
            compile_file_with_host_with_cancellation(&mut host, input_text, &driver, cancellation)
        }
        None => compile_file_with_host(&mut host, input_text, &driver),
    };
    match compiled {
        Ok(CompileFileOutcome::Compiled(_)) => BatchCompileEntry {
            input:                  input.input.clone(),
            status:                 BatchCompileStatus::Success,
            outputs:                host.written_paths().to_vec(),
            error:                  None,
            diagnostic:             None,
            additional_diagnostics: Vec::new(),
        },
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

fn batch_input_error(input: &CollectedCompileInput, message: String) -> BatchCompileEntry {
    BatchCompileEntry {
        input:                  input.input.clone(),
        status:                 BatchCompileStatus::Error,
        outputs:                Vec::new(),
        error:                  Some(message),
        diagnostic:             None,
        additional_diagnostics: Vec::new(),
    }
}
