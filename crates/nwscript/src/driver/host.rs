use super::*;

#[derive(Clone)]
pub(super) enum PendingArtifact {
    Output(PendingOutput),
    Removal(PathBuf),
    Graphviz {
        dot:           String,
        dot_path:      Option<PathBuf>,
        rendered_path: PathBuf,
        format:        GraphvizOutputFormat,
    },
}

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

    pub(super) const fn requires_renderer(self) -> bool {
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

    /// Resolves source bytes with cooperative cancellation and a hard size
    /// bound.
    fn resolve_script_bytes_with_cancellation(
        &self,
        script_name: &str,
        res_type: ResType,
        max_bytes: usize,
        cancellation: &crate::CancellationToken,
    ) -> Result<Option<Vec<u8>>, SourceError>;

    /// Begins one atomic artifact transaction.
    ///
    /// Hosts without persistent multi-artifact storage may keep the default
    /// no-op implementation.
    fn begin_outputs(&mut self) -> Result<(), CompilerHostError> {
        Ok(())
    }

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

    /// Commits every artifact accepted since [`Self::begin_outputs`].
    fn commit_outputs(
        &mut self,
        _cancellation: Option<&crate::CancellationToken>,
    ) -> Result<(), CompilerHostError> {
        Ok(())
    }

    /// Discards every artifact accepted since [`Self::begin_outputs`].
    ///
    /// # Errors
    ///
    /// Returns [`CompilerHostError`] if the host cannot restore its state.
    fn rollback_outputs(&mut self) -> Result<(), CompilerHostError> {
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

    fn resolve_script_bytes_with_cancellation(
        &self,
        script_name: &str,
        res_type: ResType,
        max_bytes: usize,
        cancellation: &crate::CancellationToken,
    ) -> Result<Option<Vec<u8>>, SourceError> {
        self.host.resolve_script_bytes_with_cancellation(
            script_name,
            res_type,
            max_bytes,
            cancellation,
        )
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
    let (artifacts, graphviz) = {
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
            Some(if let Some(cancellation) = cancellation {
                crate::render_script_graphviz_with_cancellation(
                    &prepared.script,
                    Some(&prepared.bundle.source_map),
                    cancellation,
                )
                .map_err(CompilerSessionError::from)?
            } else {
                crate::render_script_graphviz(&prepared.script, Some(&prepared.bundle.source_map))
            })
        } else {
            None
        };
        if let Some(cancellation) = cancellation {
            cancellation.check().map_err(CompilerSessionError::from)?;
        }
        let artifacts = if let Some(cancellation) = cancellation {
            session.compile_prepared_with_cancellation(&prepared, cancellation)
        } else {
            session
                .compile_prepared(&prepared)
                .map_err(CompilerSessionError::from)
        }
        .map_err(CompilerDriverError::from)?;
        (artifacts, graphviz)
    };

    if let Some(cancellation) = cancellation {
        cancellation.check().map_err(CompilerSessionError::from)?;
    }

    host.begin_outputs()?;
    let writes = (|| {
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
        if let Some(cancellation) = cancellation {
            cancellation
                .check()
                .map_err(|error| CompilerHostError::new(error.to_string()))?;
        }
        host.commit_outputs(cancellation)
    })();
    if let Err(error) = writes {
        return match host.rollback_outputs() {
            Ok(()) => Err(error.into()),
            Err(rollback) => Err(CompilerHostError::new(format!(
                "{error}; compiler host rollback also failed: {rollback}"
            ))
            .into()),
        };
    }
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
