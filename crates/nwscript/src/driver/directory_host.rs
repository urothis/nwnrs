use super::*;

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
    scheduled_removals: Vec<PathBuf>,
    pending_outputs:    Option<Vec<PendingArtifact>>,
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
            .field("scheduled_removals", &self.scheduled_removals)
            .field(
                "pending_output_count",
                &self.pending_outputs.as_ref().map(Vec::len),
            )
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
            scheduled_removals: Vec::new(),
            pending_outputs: None,
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

    /// Schedules an existing output for deletion in the next artifact
    /// transaction.
    ///
    /// The deletion is rollback-protected together with every file written by
    /// that transaction. A missing target is accepted.
    ///
    /// # Errors
    ///
    /// Returns an error if the target escapes the output directory.
    pub fn schedule_removal(&mut self, path: impl Into<PathBuf>) -> Result<(), CompilerHostError> {
        let path = path.into();
        validate_output_target(&self.output_directory, &path)?;
        self.scheduled_removals.push(path);
        Ok(())
    }
}

impl ScriptResolver for DirectoryCompilerHost {
    fn resolve_script_origin(
        &self,
        script_name: &str,
        res_type: ResType,
    ) -> Result<Option<crate::ScriptSourceOrigin>, SourceError> {
        if let Some(origin) = self.resolver.resolve_script_origin(script_name, res_type)? {
            return Ok(Some(origin));
        }
        self.fallback_resolver
            .as_ref()
            .map_or(Ok(None), |resolver| {
                resolver.resolve_script_origin(script_name, res_type)
            })
    }

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

    fn resolve_script_bytes_with_cancellation(
        &self,
        script_name: &str,
        res_type: ResType,
        max_bytes: usize,
        cancellation: &crate::CancellationToken,
    ) -> Result<Option<Vec<u8>>, SourceError> {
        if let Some(bytes) = self.resolver.resolve_script_bytes_with_cancellation(
            script_name,
            res_type,
            max_bytes,
            cancellation,
        )? {
            return Ok(Some(bytes));
        }
        match &self.fallback_resolver {
            Some(resolver) => resolver.resolve_script_bytes_with_cancellation(
                script_name,
                res_type,
                max_bytes,
                cancellation,
            ),
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

    fn resolve_script_bytes_with_cancellation(
        &self,
        script_name: &str,
        res_type: ResType,
        max_bytes: usize,
        cancellation: &crate::CancellationToken,
    ) -> Result<Option<Vec<u8>>, SourceError> {
        ScriptResolver::resolve_script_bytes_with_cancellation(
            self,
            script_name,
            res_type,
            max_bytes,
            cancellation,
        )
    }

    fn begin_outputs(&mut self) -> Result<(), CompilerHostError> {
        if self.pending_outputs.is_some() {
            return Err(CompilerHostError::new(
                "compiler output transaction is already active",
            ));
        }
        self.pending_outputs = Some(
            self.scheduled_removals
                .drain(..)
                .map(PendingArtifact::Removal)
                .collect(),
        );
        Ok(())
    }

    fn write_file(
        &mut self,
        file_name: &str,
        res_type: ResType,
        data: &[u8],
        _binary: bool,
    ) -> Result<(), CompilerHostError> {
        validate_logical_output_alias(file_name)?;
        let path = if res_type == NW_SCRIPT_BINARY_RES_TYPE {
            self.binary_output_file.clone().unwrap_or_else(|| {
                self.output_directory
                    .join(format!("{file_name}.{}", get_res_ext(res_type)))
            })
        } else {
            self.output_directory
                .join(format!("{file_name}.{}", get_res_ext(res_type)))
        };
        if res_type != NW_SCRIPT_BINARY_RES_TYPE || self.binary_output_file.is_none() {
            validate_output_target(&self.output_directory, &path)?;
        }
        self.written_paths.push(path.clone());
        if self.simulate {
            return Ok(());
        }
        let output = PendingArtifact::Output(PendingOutput {
            path,
            data: Some(data.to_vec()),
        });
        if let Some(pending) = &mut self.pending_outputs {
            pending.push(output);
            return Ok(());
        }
        graphviz_output::publish_artifacts_transactionally(
            vec![output],
            self.overwrite_existing,
            None,
        )
    }

    fn write_graphviz(&mut self, file_name: &str, dot: &str) -> Result<(), CompilerHostError> {
        validate_logical_output_alias(file_name)?;
        let base = self
            .graphviz_directory
            .as_ref()
            .unwrap_or(&self.output_directory);
        let rendered_path = base.join(format!("{file_name}.{}", self.graphviz_format.extension()));
        let dot_path = base.join(format!("{file_name}.dot"));
        validate_output_target(base, &rendered_path)?;
        validate_output_target(base, &dot_path)?;
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
        let artifact = if self.graphviz_format.requires_renderer() {
            PendingArtifact::Graphviz {
                dot: dot.to_owned(),
                dot_path: write_dot.then_some(dot_path),
                rendered_path,
                format: self.graphviz_format,
            }
        } else {
            PendingArtifact::Output(PendingOutput {
                path: dot_path,
                data: Some(dot.as_bytes().to_vec()),
            })
        };
        if let Some(pending) = &mut self.pending_outputs {
            pending.push(artifact);
            return Ok(());
        }
        graphviz_output::publish_artifacts_transactionally(
            vec![artifact],
            self.overwrite_existing,
            None,
        )
    }

    fn commit_outputs(
        &mut self,
        cancellation: Option<&crate::CancellationToken>,
    ) -> Result<(), CompilerHostError> {
        let pending = self
            .pending_outputs
            .take()
            .ok_or_else(|| CompilerHostError::new("compiler output transaction is not active"))?;
        if self.simulate {
            return Ok(());
        }
        graphviz_output::publish_artifacts_transactionally(
            pending,
            self.overwrite_existing,
            cancellation,
        )
    }

    fn rollback_outputs(&mut self) -> Result<(), CompilerHostError> {
        self.pending_outputs = None;
        Ok(())
    }
}
