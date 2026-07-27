use super::*;

/// Complete command-line batch compilation settings.
///
/// This transport-neutral structure lets the CLI remain a thin presenter over
/// the compiler environment used by language clients.
#[derive(Clone, Debug)]
pub struct CompileCommandOptions {
    /// Permit replacing existing outputs.
    pub force:               bool,
    /// Emit NDB debug information.
    pub debug:               bool,
    /// Permit include-only sources without an entrypoint.
    pub no_entrypoint_check: bool,
    /// Optional explicit language specification source.
    pub langspec:            Option<PathBuf>,
    /// Additional include roots.
    pub include_dir:         Vec<PathBuf>,
    /// Named optimization preset.
    pub optimization:        String,
    /// Explicit optimization flags that replace the preset.
    pub optimization_flag:   Vec<String>,
    /// Maximum recursive include depth.
    pub max_include_depth:   usize,
    /// Optional Graphviz output directory.
    pub graphviz:            Option<PathBuf>,
    /// Graphviz output format.
    pub graphviz_format:     String,
    /// Retain intermediate Graphviz DOT files.
    pub keep_graphviz_dot:   bool,
    /// Validate without writing compiler outputs.
    pub simulate:            bool,
    /// Diagnostic presentation format requested by the CLI.
    pub diagnostic_format:   String,
    /// Continue compiling after an input fails.
    pub continue_on_error:   bool,
    /// Recurse through directory inputs.
    pub recurse:             bool,
    /// Follow symbolic links while collecting inputs.
    pub follow_symlinks:     bool,
    /// Optional parallel worker count.
    pub jobs:                Option<usize>,
    /// Optional exact output path.
    pub output:              Option<PathBuf>,
    /// Optional output directory.
    pub directory:           Option<PathBuf>,
    /// Optional installation root.
    pub root:                Option<PathBuf>,
    /// Optional user-directory root.
    pub user:                Option<PathBuf>,
    /// Installation language.
    pub language:            String,
    /// Include the installation override directory.
    pub load_ovr:            bool,
    /// Source files or directories to compile.
    pub paths:               Vec<PathBuf>,
}

/// Default optimization preset used by command-line and editor compilation.
pub const DEFAULT_OPTIMIZATION: &str = "O1";

/// Shared installation resource manager used by compiler-backed clients.
pub type SharedInstallResMan = Arc<Mutex<ResMan>>;

#[derive(Clone)]
/// Settings for compiling one NWScript source in memory.
pub struct CompileScriptOptions {
    /// Emit NDB debug information.
    pub debug:               bool,
    /// Permit a source without an entrypoint.
    pub no_entrypoint_check: bool,
    /// Optional explicit language specification.
    pub langspec:            Option<PathBuf>,
    /// Additional include roots.
    pub include_dirs:        Vec<PathBuf>,
    /// Compiler optimization flags.
    pub optimizations:       nwscript::OptimizationFlags,
    /// Maximum recursive include depth.
    pub max_include_depth:   usize,
    /// Optional installation-backed resource manager.
    pub install_resman:      Option<SharedInstallResMan>,
}

/// Result of compiling a source when missing entrypoints may be skipped.
pub enum CompileScriptOutcome {
    /// The source compiled into artifacts.
    Compiled(nwscript::CompileArtifacts),
    /// The source had no executable entrypoint.
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
    /// Optional system Git executable for locked package dependencies.
    pub git_executable: Option<PathBuf>,
    /// Optional immutable Git dependency cache root.
    pub package_cache: Option<PathBuf>,
}

impl Default for NwScriptCheckOptions {
    fn default() -> Self {
        Self {
            paths: Vec::new(),
            no_entrypoint_check: true,
            langspec: None,
            include_dirs: Vec::new(),
            source_overlays: BTreeMap::new(),
            optimization: DEFAULT_OPTIMIZATION.to_string(),
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
            git_executable: None,
            package_cache: None,
        }
    }
}
