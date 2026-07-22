use std::{error::Error, fmt};

use crate::{
    CancellationToken, Cancelled, CompileArtifacts, CompileError, CompileOptions,
    DEFAULT_LANGSPEC_SCRIPT_NAME, HirModule, LangSpec, LangSpecError, OptimizationFlags,
    OptimizationLevel, PreprocessError, Script, ScriptResolver, SemanticIndex, SemanticModel,
    SourceBundle, SourceError, SourceLoadOptions, analyze_script_with_options,
    build_semantic_index, compile_script, compile_script_with_source_map,
    graphviz::render_script_graphviz, load_langspec, load_source_bundle,
    load_source_bundle_with_cancellation, lower_to_hir, parse_source_bundle,
    parse_source_bundle_with_cancellation,
};

/// Configuration for one reusable NWScript compiler session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompilerSessionOptions {
    /// Logical script name used to load the builtin language specification.
    pub langspec_script_name: String,
    /// Source loading configuration used for the langspec and all compilations.
    pub source_load:          SourceLoadOptions,
    /// Code generation options applied to each compile request.
    pub compile:              CompileOptions,
    /// Whether compilations should emit `NDB` debugger output when available.
    pub emit_debug:           bool,
}

impl Default for CompilerSessionOptions {
    fn default() -> Self {
        Self {
            langspec_script_name: DEFAULT_LANGSPEC_SCRIPT_NAME.to_string(),
            source_load:          SourceLoadOptions::default(),
            compile:              CompileOptions::default(),
            emit_debug:           true,
        }
    }
}

/// Errors returned while using one reusable compiler session.
#[derive(Debug)]
pub enum CompilerSessionError {
    /// The caller cancelled the compiler request.
    Cancelled(Cancelled),
    /// Loading or parsing the builtin language specification failed.
    LangSpec(LangSpecError),
    /// Loading and preprocessing the requested source bundle failed.
    Preprocess(PreprocessError),
    /// Loading the requested source bundle failed.
    Source(SourceError),
    /// Parsing or code generation failed.
    Compile(CompileError),
}

impl fmt::Display for CompilerSessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled(error) => error.fmt(f),
            Self::LangSpec(error) => error.fmt(f),
            Self::Preprocess(error) => error.fmt(f),
            Self::Source(error) => error.fmt(f),
            Self::Compile(error) => error.fmt(f),
        }
    }
}

impl Error for CompilerSessionError {}

impl From<LangSpecError> for CompilerSessionError {
    fn from(value: LangSpecError) -> Self {
        Self::LangSpec(value)
    }
}

impl From<SourceError> for CompilerSessionError {
    fn from(value: SourceError) -> Self {
        Self::Source(value)
    }
}

impl From<PreprocessError> for CompilerSessionError {
    fn from(value: PreprocessError) -> Self {
        Self::Preprocess(value)
    }
}

impl From<CompileError> for CompilerSessionError {
    fn from(value: CompileError) -> Self {
        Self::Compile(value)
    }
}

impl From<Cancelled> for CompilerSessionError {
    fn from(value: Cancelled) -> Self {
        Self::Cancelled(value)
    }
}

/// One reusable pure-Rust compiler session backed by a script resolver.
pub struct CompilerSession<'a> {
    resolver:        &'a dyn ScriptResolver,
    options:         CompilerSessionOptions,
    cached_langspec: Option<LangSpec>,
}

/// Immutable compiler-front-end artifacts shared by compilation and language
/// tooling.
#[derive(Debug, Clone, PartialEq)]
pub struct CompilerAnalysis {
    /// Active builtin language specification.
    pub langspec: LangSpec,
    /// Root source and its complete include graph.
    pub bundle:   SourceBundle,
    /// Parsed, macro-expanded syntax tree.
    pub script:   Script,
    /// Resolved declarations and types.
    pub semantic: SemanticModel,
    /// Typed representation consumed by code generation.
    pub hir:      HirModule,
    /// Source-addressable declarations and references derived from typed HIR.
    pub index:    SemanticIndex,
}

#[derive(Debug, Clone)]
pub(crate) struct PreparedScript {
    pub(crate) langspec: LangSpec,
    pub(crate) bundle:   SourceBundle,
    pub(crate) script:   Script,
}

impl fmt::Debug for CompilerSession<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CompilerSession")
            .field("options", &self.options)
            .field("has_cached_langspec", &self.cached_langspec.is_some())
            .finish()
    }
}

impl<'a> CompilerSession<'a> {
    /// Creates one compiler session with default options.
    #[must_use]
    pub fn new(resolver: &'a dyn ScriptResolver) -> Self {
        Self::with_options(resolver, CompilerSessionOptions::default())
    }

    /// Creates one compiler session with explicit options.
    #[must_use]
    pub fn with_options(resolver: &'a dyn ScriptResolver, options: CompilerSessionOptions) -> Self {
        Self {
            resolver,
            options,
            cached_langspec: None,
        }
    }

    /// Returns the current immutable session options.
    #[must_use]
    pub fn options(&self) -> &CompilerSessionOptions {
        &self.options
    }

    /// Returns whether this session emits `NDB` debugger output.
    #[must_use]
    pub fn generate_debugger_output(&self) -> bool {
        self.options.emit_debug
    }

    /// Toggles `NDB` debugger output without recreating the session.
    pub fn set_generate_debugger_output(&mut self, state: bool) {
        self.options.emit_debug = state;
    }

    /// Returns the current independent optimization flags.
    #[must_use]
    pub fn optimization_flags(&self) -> OptimizationFlags {
        self.options.compile.optimizations
    }

    /// Updates independent optimization flags without recreating the session.
    pub fn set_optimization_flags(&mut self, optimizations: OptimizationFlags) {
        self.options.compile.optimizations = optimizations;
    }

    /// Returns the standard O-level matching the current flags, when one
    /// exists.
    #[must_use]
    pub fn optimization_level(&self) -> Option<OptimizationLevel> {
        self.optimization_flags().level()
    }

    /// Updates the optimization flags from one standard O-level.
    pub fn set_optimization_level(&mut self, optimization: OptimizationLevel) {
        self.set_optimization_flags(optimization.into());
    }

    /// Returns the current source-load options.
    #[must_use]
    pub fn source_load_options(&self) -> SourceLoadOptions {
        self.options.source_load
    }

    /// Updates source-loading options and invalidates any cached langspec.
    pub fn set_source_load_options(&mut self, options: SourceLoadOptions) {
        self.options.source_load = options;
        self.cached_langspec = None;
    }

    /// Returns the logical langspec script name.
    #[must_use]
    pub fn langspec_script_name(&self) -> &str {
        &self.options.langspec_script_name
    }

    /// Updates the langspec script name and invalidates any cached langspec.
    pub fn set_langspec_script_name(&mut self, script_name: impl Into<String>) {
        self.options.langspec_script_name = script_name.into();
        self.cached_langspec = None;
    }

    /// Compiles one logical script name through the configured resolver.
    ///
    /// # Errors
    ///
    /// Returns [`CompilerSessionError`] if source loading, langspec loading,
    /// parsing, or code generation fails.
    pub fn compile_script_name(
        &mut self,
        script_name: &str,
    ) -> Result<CompileArtifacts, CompilerSessionError> {
        let prepared = self.prepare_script_name(script_name)?;
        self.compile_prepared(&prepared)
            .map_err(CompilerSessionError::from)
    }

    /// Runs the complete compiler front end and returns reusable,
    /// source-addressable analysis.
    ///
    /// # Errors
    ///
    /// Returns [`CompilerSessionError`] when source resolution, parsing,
    /// semantic analysis, or HIR lowering fails.
    pub fn analyze_script_name(
        &mut self,
        script_name: &str,
    ) -> Result<CompilerAnalysis, CompilerSessionError> {
        let prepared = self.prepare_script_name(script_name)?;
        let semantic = analyze_script_with_options(
            &prepared.script,
            Some(&prepared.langspec),
            self.options.compile.semantic,
        )
        .map_err(CompileError::from)?;
        let hir = lower_to_hir(&prepared.script, &semantic, Some(&prepared.langspec))
            .map_err(CompileError::from)?;
        let index = build_semantic_index(
            &prepared.script,
            &semantic,
            &hir,
            Some(&prepared.langspec),
            &prepared.bundle.source_map,
        );
        Ok(CompilerAnalysis {
            langspec: prepared.langspec,
            bundle: prepared.bundle,
            script: prepared.script,
            semantic,
            hir,
            index,
        })
    }

    /// Runs the compiler front end with cooperative cancellation.
    ///
    /// # Errors
    ///
    /// Returns [`CompilerSessionError`] for ordinary compiler failures or
    /// cancellation.
    pub fn analyze_script_name_with_cancellation(
        &mut self,
        script_name: &str,
        cancellation: &CancellationToken,
    ) -> Result<CompilerAnalysis, CompilerSessionError> {
        cancellation.check()?;
        let langspec = self.ensure_langspec_loaded()?.clone();
        cancellation.check()?;
        let bundle = load_source_bundle_with_cancellation(
            self.resolver,
            script_name,
            self.options.source_load,
            cancellation,
        )?;
        cancellation.check()?;
        let script = parse_source_bundle_with_cancellation(&bundle, Some(&langspec), cancellation)
            .map_err(CompileError::from)
            .map_err(CompilerSessionError::Compile)?;
        cancellation.check()?;
        let semantic =
            analyze_script_with_options(&script, Some(&langspec), self.options.compile.semantic)
                .map_err(CompileError::from)?;
        cancellation.check()?;
        let hir = lower_to_hir(&script, &semantic, Some(&langspec)).map_err(CompileError::from)?;
        cancellation.check()?;
        let index = build_semantic_index(
            &script,
            &semantic,
            &hir,
            Some(&langspec),
            &bundle.source_map,
        );
        cancellation.check()?;
        Ok(CompilerAnalysis {
            langspec,
            bundle,
            script,
            semantic,
            hir,
            index,
        })
    }

    /// Renders one logical script name to Graphviz DOT using the cached
    /// langspec and loaded source bundle.
    ///
    /// # Errors
    ///
    /// Returns [`CompilerSessionError`] if source loading or parsing fails.
    pub fn render_graphviz_for_script_name(
        &mut self,
        script_name: &str,
    ) -> Result<String, CompilerSessionError> {
        let prepared = self.prepare_script_name(script_name)?;
        Ok(render_script_graphviz(
            &prepared.script,
            Some(&prepared.bundle.source_map),
        ))
    }

    fn ensure_langspec_loaded(&mut self) -> Result<&LangSpec, CompilerSessionError> {
        if self.cached_langspec.is_none() {
            let langspec = load_langspec(
                self.resolver,
                &self.options.langspec_script_name,
                self.options.source_load,
            )?;
            self.cached_langspec = Some(langspec);
        }
        self.cached_langspec.as_ref().ok_or_else(|| {
            CompilerSessionError::Source(SourceError::resolver(
                "failed to cache langspec after successful load",
            ))
        })
    }

    pub(crate) fn prepare_script_name(
        &mut self,
        script_name: &str,
    ) -> Result<PreparedScript, CompilerSessionError> {
        let langspec = self.ensure_langspec_loaded()?.clone();
        let bundle = load_source_bundle(self.resolver, script_name, self.options.source_load)?;
        let script = parse_source_bundle(&bundle, Some(&langspec))
            .map_err(CompileError::from)
            .map_err(CompilerSessionError::Compile)?;
        Ok(PreparedScript {
            langspec,
            bundle,
            script,
        })
    }

    pub(crate) fn prepare_script_name_with_cancellation(
        &mut self,
        script_name: &str,
        cancellation: &CancellationToken,
    ) -> Result<PreparedScript, CompilerSessionError> {
        cancellation.check()?;
        let langspec = self.ensure_langspec_loaded()?.clone();
        cancellation.check()?;
        let bundle = load_source_bundle_with_cancellation(
            self.resolver,
            script_name,
            self.options.source_load,
            cancellation,
        )?;
        cancellation.check()?;
        let script = parse_source_bundle_with_cancellation(&bundle, Some(&langspec), cancellation)
            .map_err(CompileError::from)
            .map_err(CompilerSessionError::Compile)?;
        cancellation.check()?;
        Ok(PreparedScript {
            langspec,
            bundle,
            script,
        })
    }

    pub(crate) fn compile_prepared(
        &self,
        prepared: &PreparedScript,
    ) -> Result<CompileArtifacts, CompileError> {
        if self.options.emit_debug {
            compile_script_with_source_map(
                &prepared.script,
                &prepared.bundle.source_map,
                prepared.bundle.root_id,
                Some(&prepared.langspec),
                self.options.compile,
            )
        } else {
            compile_script(
                &prepared.script,
                Some(&prepared.langspec),
                self.options.compile,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CompilerSession;
    use crate::{InMemoryScriptResolver, OptimizationLevel};

    #[test]
    fn compiler_session_reuses_langspec_and_toggles_debug_output()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut resolver = InMemoryScriptResolver::new();
        resolver.insert_source("nwscript", "void PrintInteger(int n);");
        resolver.insert_source("main", "void main() { PrintInteger(42); }");

        let mut session = CompilerSession::new(&resolver);
        let first = session.compile_script_name("main")?;
        assert!(!first.ncs.is_empty());
        assert!(first.ndb.is_some());

        session.set_generate_debugger_output(false);
        let second = session.compile_script_name("main")?;
        assert!(!second.ncs.is_empty());
        assert!(second.ndb.is_none());
        Ok(())
    }

    #[test]
    fn compiler_session_updates_optimization_without_recreation()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut resolver = InMemoryScriptResolver::new();
        resolver.insert_source("nwscript", "void PrintInteger(int n);");
        resolver.insert_source("main", "void main() { PrintInteger(42); }");

        let mut session = CompilerSession::new(&resolver);
        session.set_optimization_level(OptimizationLevel::O1);
        let artifacts = session.compile_script_name("main")?;
        assert!(!artifacts.ncs.is_empty());
        assert!(artifacts.ndb.is_some());
        Ok(())
    }
}
