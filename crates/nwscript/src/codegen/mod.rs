use std::{
    collections::{BTreeMap, VecDeque},
    error::Error,
    fmt,
    ops::{BitOr, BitOrAssign},
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::{
    AssignmentOp, BinaryOp, BuiltinFunction, BuiltinType, BuiltinValue, CancellationToken,
    HirBlock, HirCallTarget, HirExpr, HirExprKind, HirFunction, HirLocalId, HirModule, HirStmt,
    LangSpec, Literal, NCS_OPERATION_BASE_SIZE, NcsAuxCode, NcsInstruction, NcsOpcode, Script,
    ScriptString, SemanticOptions, SemanticType, SourceBundle, SourceId, SourceMap, UnaryOp,
    analyze_script_with_options, encode_ncs_instructions,
    encode_ncs_instructions_with_cancellation, lower_to_hir,
    opt::{
        ConstValue, build_constant_env, evaluate_const_expr, melded_instruction,
        optimization_needs_hir_passes, optimization_needs_post_codegen_passes, optimize_hir,
        optimize_hir_with_cancellation,
    },
    parse_source_bundle, parse_text, write_ndb, write_ndb_with_cancellation,
};

mod assembler;
mod compiler;
mod emission;

use emission::*;
mod debug;
mod layout;

use assembler::*;
use compiler::*;
use debug::build_ndb;
use layout::*;

/// Maximum identifier-table entries accepted by the native compiler.
pub const MAX_COMPILER_IDENTIFIERS: usize = 65_536;
/// Maximum 32-bit runtime stack cells tracked by the native compiler.
pub const MAX_COMPILER_RUNTIME_CELLS: usize = 8_192;

/// Optimization levels accepted by the pure-Rust compiler pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum OptimizationLevel {
    /// Unoptimized code generation.
    #[default]
    O0,
    /// Eliminates unreachable user functions.
    O1,
    /// Applies O1 plus constant dead-branch elimination.
    O2,
    /// Applies O2 plus upstream-compatible instruction melding.
    O3,
}

impl OptimizationLevel {
    /// Returns the independent optimization flags represented by this level.
    #[must_use]
    pub const fn flags(self) -> OptimizationFlags {
        match self {
            Self::O0 => OptimizationFlags::O0,
            Self::O1 => OptimizationFlags::O1,
            Self::O2 => OptimizationFlags::O2,
            Self::O3 => OptimizationFlags::O3,
        }
    }
}

/// One independently selectable native compiler optimization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum OptimizationFlag {
    /// Removes user functions that cannot be reached from the script loader.
    RemoveDeadCode = 0x01,
    /// Merges native-compatible instruction sequences after code generation.
    MeldInstructions = 0x02,
    /// Removes branches proven unreachable from constant conditions.
    RemoveDeadBranches = 0x04,
}

/// A set of independently selectable compiler optimizations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(transparent)]
pub struct OptimizationFlags(u8);

impl OptimizationFlags {
    const KNOWN_BITS: u8 = Self::O3.0;
    /// No optimization flags, corresponding to O0.
    pub const O0: Self = Self(0);
    /// Dead-code removal, corresponding to O1.
    pub const O1: Self = Self(OptimizationFlag::RemoveDeadCode as u8);
    /// Dead-code and dead-branch removal, corresponding to O2.
    pub const O2: Self =
        Self(OptimizationFlag::RemoveDeadCode as u8 | OptimizationFlag::RemoveDeadBranches as u8);
    /// Every native optimization, corresponding to O3.
    pub const O3: Self = Self(
        OptimizationFlag::RemoveDeadCode as u8
            | OptimizationFlag::MeldInstructions as u8
            | OptimizationFlag::RemoveDeadBranches as u8,
    );

    /// Creates a flag set containing one optimization.
    #[must_use]
    pub const fn from_flag(flag: OptimizationFlag) -> Self {
        Self(flag as u8)
    }

    /// Creates a flag set from its native bit representation.
    #[must_use]
    pub const fn from_bits(bits: u8) -> Option<Self> {
        if bits & !Self::KNOWN_BITS == 0 {
            Some(Self(bits))
        } else {
            None
        }
    }

    /// Returns the native bit representation of this flag set.
    #[must_use]
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// Returns whether this set contains `flag`.
    #[must_use]
    pub const fn contains(self, flag: OptimizationFlag) -> bool {
        self.0 & flag as u8 != 0
    }

    /// Returns the standard O-level matching this exact set, when one exists.
    #[must_use]
    pub const fn level(self) -> Option<OptimizationLevel> {
        match self.0 {
            0 => Some(OptimizationLevel::O0),
            value if value == Self::O1.0 => Some(OptimizationLevel::O1),
            value if value == Self::O2.0 => Some(OptimizationLevel::O2),
            value if value == Self::O3.0 => Some(OptimizationLevel::O3),
            _ => None,
        }
    }
}

impl From<OptimizationFlag> for OptimizationFlags {
    fn from(value: OptimizationFlag) -> Self {
        Self::from_flag(value)
    }
}

impl From<OptimizationLevel> for OptimizationFlags {
    fn from(value: OptimizationLevel) -> Self {
        value.flags()
    }
}

impl BitOr for OptimizationFlags {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl BitOr for OptimizationFlag {
    type Output = OptimizationFlags;

    fn bitor(self, rhs: Self) -> Self::Output {
        OptimizationFlags::from(self) | OptimizationFlags::from(rhs)
    }
}

impl BitOr<OptimizationFlag> for OptimizationFlags {
    type Output = Self;

    fn bitor(self, rhs: OptimizationFlag) -> Self::Output {
        self | Self::from(rhs)
    }
}

impl BitOrAssign for OptimizationFlags {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl BitOrAssign<OptimizationFlag> for OptimizationFlags {
    fn bitor_assign(&mut self, rhs: OptimizationFlag) {
        self.0 |= rhs as u8;
    }
}

/// One compilation request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CompileOptions {
    /// Entry-point validation policy forwarded to semantic analysis.
    pub semantic:      SemanticOptions,
    /// Independently selectable optimization passes for code generation.
    pub optimizations: OptimizationFlags,
}

impl Default for CompileOptions {
    fn default() -> Self {
        Self {
            semantic:      SemanticOptions::default(),
            optimizations: OptimizationFlags::O0,
        }
    }
}

/// Compiler outputs produced in memory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompileArtifacts {
    /// Encoded `NCS` bytecode.
    pub ncs: Vec<u8>,
    /// Encoded `NDB` debug output when available.
    pub ndb: Option<Vec<u8>>,
}

/// One pure-Rust code generation failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodegenError {
    /// Upstream-aligned compiler error code when this failure has one.
    pub code:    Option<crate::CompilerErrorCode>,
    /// Optional source span associated with the failure.
    pub span:    Option<crate::Span>,
    /// Human-readable error text.
    pub message: String,
}

impl CodegenError {
    fn new(span: Option<crate::Span>, message: impl Into<String>) -> Self {
        Self {
            code: None,
            span,
            message: message.into(),
        }
    }

    fn native(
        code: crate::CompilerErrorCode,
        span: Option<crate::Span>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code: Some(code),
            span,
            message: message.into(),
        }
    }
}

impl fmt::Display for CodegenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.code {
            Some(code) => write!(f, "{} ({})", self.message, code.code()),
            None => f.write_str(&self.message),
        }
    }
}

impl Error for CodegenError {}

impl From<crate::NdbError> for CodegenError {
    fn from(value: crate::NdbError) -> Self {
        Self::new(None, value.to_string())
    }
}

impl From<crate::NcsEncodeError> for CodegenError {
    fn from(value: crate::NcsEncodeError) -> Self {
        Self::new(None, value.to_string())
    }
}

fn usize_to_i32(value: usize, what: &str) -> Result<i32, CodegenError> {
    i32::try_from(value)
        .map_err(|_error| CodegenError::new(None, format!("{what} exceeds i32 range")))
}

fn usize_to_u32(value: usize, what: &str) -> Result<u32, CodegenError> {
    u32::try_from(value)
        .map_err(|_error| CodegenError::new(None, format!("{what} exceeds u32 range")))
}

fn usize_to_u16(value: usize, what: &str) -> Result<u16, CodegenError> {
    u16::try_from(value)
        .map_err(|_error| CodegenError::new(None, format!("{what} exceeds u16 range")))
}

fn usize_to_u8(value: usize, what: &str) -> Result<u8, CodegenError> {
    u8::try_from(value)
        .map_err(|_error| CodegenError::new(None, format!("{what} exceeds u8 range")))
}

/// One compilation failure across analysis, lowering, or code generation.
#[derive(Debug)]
pub enum CompileError {
    /// Source resolution, preprocessing, or parsing failed.
    Parse(crate::ResolvedParseError),
    /// Semantic analysis failed.
    Semantic(crate::SemanticError),
    /// HIR lowering failed.
    Hir(crate::HirLowerError),
    /// Code generation failed.
    Codegen(CodegenError),
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(error) => error.fmt(f),
            Self::Semantic(error) => error.fmt(f),
            Self::Hir(error) => error.fmt(f),
            Self::Codegen(error) => error.fmt(f),
        }
    }
}

impl Error for CompileError {}

impl From<crate::ResolvedParseError> for CompileError {
    fn from(value: crate::ResolvedParseError) -> Self {
        Self::Parse(value)
    }
}

impl From<crate::SemanticError> for CompileError {
    fn from(value: crate::SemanticError) -> Self {
        Self::Semantic(value)
    }
}

impl From<crate::HirLowerError> for CompileError {
    fn from(value: crate::HirLowerError) -> Self {
        Self::Hir(value)
    }
}

impl From<CodegenError> for CompileError {
    fn from(value: CodegenError) -> Self {
        Self::Codegen(value)
    }
}

/// Compiles one parsed script through semantic analysis, HIR lowering, and `O0`
/// NCS emission.
///
/// # Errors
///
/// Returns [`CompileError`] if semantic analysis, HIR lowering, or NCS emission
/// fails.
///
/// # Examples
///
/// ```
/// let script = nwnrs_nwscript::parse_text(
///     nwnrs_nwscript::SourceId::new(0),
///     "void main() {}",
///     None,
/// )?;
/// let artifacts = nwnrs_nwscript::compile_script(
///     &script,
///     None,
///     nwnrs_nwscript::CompileOptions::default(),
/// )?;
/// assert!(!artifacts.ncs.is_empty());
/// assert!(artifacts.ndb.is_none());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn compile_script(
    script: &Script,
    langspec: Option<&LangSpec>,
    options: CompileOptions,
) -> Result<CompileArtifacts, CompileError> {
    compile_script_with_debug(script, None, None, langspec, options)
}

/// Compiles one parsed script and emits `NDB` when a source map is available.
///
/// # Errors
///
/// Returns [`CompileError`] if compilation fails.
pub fn compile_script_with_source_map(
    script: &Script,
    source_map: &SourceMap,
    root_id: SourceId,
    langspec: Option<&LangSpec>,
    options: CompileOptions,
) -> Result<CompileArtifacts, CompileError> {
    compile_script_with_debug(script, Some(source_map), Some(root_id), langspec, options)
}

/// Parses and compiles one already-loaded source bundle with `NDB` output.
///
/// # Errors
///
/// Returns [`CompileError`] if parsing or compilation fails.
///
/// # Examples
///
/// ```
/// let mut resolver = nwnrs_nwscript::InMemoryScriptResolver::new();
/// resolver.insert_source("main", "void main() {}");
/// let bundle = nwnrs_nwscript::load_source_bundle(
///     &resolver,
///     "main",
///     nwnrs_nwscript::SourceLoadOptions::default(),
/// )?;
/// let artifacts = nwnrs_nwscript::compile_source_bundle(
///     &bundle,
///     None,
///     nwnrs_nwscript::CompileOptions::default(),
/// )?;
/// assert!(!artifacts.ncs.is_empty());
/// assert!(artifacts.ndb.is_some());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn compile_source_bundle(
    bundle: &SourceBundle,
    langspec: Option<&LangSpec>,
    options: CompileOptions,
) -> Result<CompileArtifacts, CompileError> {
    let script = parse_source_bundle(bundle, langspec)?;
    compile_script_with_source_map(
        &script,
        &bundle.source_map,
        bundle.root_id,
        langspec,
        options,
    )
}

/// Parses and compiles one source bundle using caller-provided built-in macros
/// and expansion limits.
///
/// Source-defined macros are retained in `registry`, allowing a build host to
/// establish a shared macro environment explicitly across compilation units.
///
/// # Errors
///
/// Returns [`CompileError`] if preprocessing, macro execution, parsing,
/// semantic analysis, or code generation fails.
pub fn compile_source_bundle_with_macros(
    bundle: &SourceBundle,
    langspec: Option<&LangSpec>,
    options: CompileOptions,
    registry: &mut crate::MacroRegistry,
    macro_options: crate::MacroExpansionOptions,
) -> Result<CompileArtifacts, CompileError> {
    let script = crate::parse_source_bundle_with_macros(bundle, langspec, registry, macro_options)?;
    compile_script_with_source_map(
        &script,
        &bundle.source_map,
        bundle.root_id,
        langspec,
        options,
    )
}

fn compile_script_with_debug(
    script: &Script,
    source_map: Option<&SourceMap>,
    root_id: Option<SourceId>,
    langspec: Option<&LangSpec>,
    options: CompileOptions,
) -> Result<CompileArtifacts, CompileError> {
    let semantic = analyze_script_with_options(script, langspec, options.semantic)?;
    let hir = lower_to_hir(script, &semantic, langspec)?;
    let optimized_hir = if optimization_needs_hir_passes(options.optimizations) {
        optimize_hir(&hir, langspec, options.optimizations)
    } else {
        hir
    };
    validate_hir_limits(&optimized_hir, langspec)?;
    let output = O0Compiler::new(&optimized_hir, langspec, source_map)?.compile(
        optimization_needs_post_codegen_passes(options.optimizations),
    )?;
    let ncs = encode_ncs_instructions(&output.instructions).map_err(CodegenError::from)?;
    let ndb = match (source_map, root_id) {
        (Some(source_map), Some(root_id)) => {
            let ndb = build_ndb(&optimized_hir, langspec, source_map, root_id, &output, None)?;
            let mut bytes = Vec::new();
            write_ndb(&mut bytes, &ndb).map_err(CodegenError::from)?;
            Some(bytes)
        }
        _ => None,
    };
    Ok(CompileArtifacts {
        ncs,
        ndb,
    })
}

pub(crate) fn compile_script_with_cancellation(
    script: &Script,
    source_map: Option<&SourceMap>,
    root_id: Option<SourceId>,
    langspec: Option<&LangSpec>,
    options: CompileOptions,
    cancellation: &CancellationToken,
) -> Result<CompileArtifacts, CompileError> {
    cancellation
        .check()
        .map_err(|error| CompileError::Codegen(CodegenError::new(None, error.to_string())))?;
    let semantic = crate::sema::analyze_script_with_cancellation(
        script,
        langspec,
        options.semantic,
        cancellation,
    )?;
    let hir =
        crate::hir::lower_to_hir_with_cancellation(script, &semantic, langspec, cancellation)?;
    cancellation
        .check()
        .map_err(|error| CompileError::Codegen(CodegenError::new(None, error.to_string())))?;
    let optimized_hir = if optimization_needs_hir_passes(options.optimizations) {
        optimize_hir_with_cancellation(&hir, langspec, options.optimizations, cancellation)
            .map_err(|error| CompileError::Codegen(CodegenError::new(None, error.to_string())))?
    } else {
        hir
    };
    cancellation
        .check()
        .map_err(|error| CompileError::Codegen(CodegenError::new(None, error.to_string())))?;
    validate_hir_limits(&optimized_hir, langspec)?;
    let output =
        O0Compiler::new_with_cancellation(&optimized_hir, langspec, source_map, cancellation)?
            .compile(optimization_needs_post_codegen_passes(
                options.optimizations,
            ))?;
    cancellation
        .check()
        .map_err(|error| CompileError::Codegen(CodegenError::new(None, error.to_string())))?;
    let ncs = encode_ncs_instructions_with_cancellation(&output.instructions, cancellation)
        .map_err(CodegenError::from)?;
    let ndb = match (source_map, root_id) {
        (Some(source_map), Some(root_id)) => {
            let ndb = build_ndb(
                &optimized_hir,
                langspec,
                source_map,
                root_id,
                &output,
                Some(cancellation),
            )?;
            cancellation.check().map_err(|error| {
                CompileError::Codegen(CodegenError::new(None, error.to_string()))
            })?;
            let mut bytes = Vec::new();
            write_ndb_with_cancellation(&mut bytes, &ndb, cancellation)
                .map_err(CodegenError::from)?;
            Some(bytes)
        }
        _ => None,
    };
    Ok(CompileArtifacts {
        ncs,
        ndb,
    })
}

/// Compiles one lowered HIR module to `NCS`.
///
/// # Errors
///
/// Returns [`CodegenError`] if code generation fails.
///
/// # Examples
///
/// ```
/// let script = nwnrs_nwscript::parse_text(
///     nwnrs_nwscript::SourceId::new(0),
///     "void main() {}",
///     None,
/// )?;
/// let semantic = nwnrs_nwscript::analyze_script_with_options(
///     &script,
///     None,
///     nwnrs_nwscript::SemanticOptions::default(),
/// )?;
/// let hir = nwnrs_nwscript::lower_to_hir(&script, &semantic, None)?;
/// let ncs = nwnrs_nwscript::compile_hir_to_ncs(
///     &hir,
///     None,
///     nwnrs_nwscript::OptimizationFlags::O0,
/// )?;
/// assert!(!ncs.is_empty());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn compile_hir_to_ncs(
    hir: &HirModule,
    langspec: Option<&LangSpec>,
    optimizations: OptimizationFlags,
) -> Result<Vec<u8>, CodegenError> {
    let optimized_hir = if optimization_needs_hir_passes(optimizations) {
        optimize_hir(hir, langspec, optimizations)
    } else {
        hir.clone()
    };

    validate_hir_limits(&optimized_hir, langspec)?;

    let output = O0Compiler::new(&optimized_hir, langspec, None)?
        .compile(optimization_needs_post_codegen_passes(optimizations))?;
    encode_ncs_instructions(&output.instructions).map_err(CodegenError::from)
}

#[cfg(test)]
mod tests;
