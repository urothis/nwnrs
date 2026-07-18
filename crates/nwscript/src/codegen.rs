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
    AssignmentOp, BinaryOp, BuiltinFunction, BuiltinType, BuiltinValue, HirBlock, HirCallTarget,
    HirExpr, HirExprKind, HirFunction, HirLocalId, HirModule, HirStmt, LangSpec, Literal,
    NCS_BINARY_HEADER_SIZE, NCS_OPERATION_BASE_SIZE, NcsAuxCode, NcsInstruction, NcsOpcode, Ndb,
    NdbFile, NdbFunction, NdbLine, NdbStruct, NdbStructField, NdbType, NdbVariable, Script,
    ScriptString, SemanticOptions, SemanticType, SourceBundle, SourceId, SourceMap, UnaryOp,
    analyze_script_with_options, encode_ncs_instructions, lower_to_hir, nwscript_string_hash_bytes,
    opt::{
        ConstValue, build_constant_env, evaluate_const_expr, melded_instruction,
        optimization_needs_hir_passes, optimization_needs_post_codegen_passes, optimize_hir,
    },
    parse_source_bundle, parse_text, write_ndb,
};

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
    let ncs = encode_ncs_instructions(&output.instructions);
    let ndb = match (source_map, root_id) {
        (Some(source_map), Some(root_id)) => {
            let ndb = build_ndb(&optimized_hir, langspec, source_map, root_id, &output)?;
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
    Ok(encode_ncs_instructions(&output.instructions))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct LabelId(u32);

struct ResolvedAssembly {
    instructions: Vec<NcsInstruction>,
    offsets:      BTreeMap<LabelId, u32>,
}

struct FunctionDebugInfo {
    start: LabelId,
    end:   LabelId,
}

struct VariableDebugInfo {
    name:      String,
    ty:        SemanticType,
    start:     LabelId,
    end:       Option<LabelId>,
    stack_loc: u32,
}

struct LineDebugInfo {
    source_id: SourceId,
    line_num:  usize,
    start:     LabelId,
    end:       LabelId,
}

struct OpenLineDebug {
    source_id: SourceId,
    line_num:  usize,
    refs:      usize,
    start:     LabelId,
}

#[derive(Default)]
struct LineDebugTracker {
    current: Option<OpenLineDebug>,
    entries: Vec<LineDebugInfo>,
}

struct CodegenOutput {
    instructions:  Vec<NcsInstruction>,
    label_offsets: BTreeMap<LabelId, u32>,
    functions:     BTreeMap<String, FunctionDebugInfo>,
    variables:     Vec<VariableDebugInfo>,
    lines:         Vec<LineDebugInfo>,
}

enum AssemblyItem {
    Label(LabelId),
    Instruction(NcsInstruction),
    RelativeJump { opcode: NcsOpcode, target: LabelId },
}

#[derive(Default)]
struct Assembler {
    items:      Vec<AssemblyItem>,
    next_label: u32,
}

impl Assembler {
    fn new_label(&mut self) -> LabelId {
        let label = LabelId(self.next_label);
        self.next_label += 1;
        label
    }

    fn place_label(&mut self, label: LabelId) {
        self.items.push(AssemblyItem::Label(label));
    }

    fn push(&mut self, instruction: NcsInstruction) {
        self.items.push(AssemblyItem::Instruction(instruction));
    }

    fn push_jump(&mut self, opcode: NcsOpcode, target: LabelId) {
        self.items.push(AssemblyItem::RelativeJump {
            opcode,
            target,
        });
    }

    fn meld_instructions(&mut self) {
        let mut pending = VecDeque::from(std::mem::take(&mut self.items));
        let mut optimized = Vec::with_capacity(self.items.len());
        while !pending.is_empty() {
            let candidate = {
                let mut instructions = Vec::with_capacity(4);
                let mut positions = Vec::with_capacity(4);
                for (position, item) in pending.iter().enumerate() {
                    match item {
                        AssemblyItem::Label(_) => {}
                        AssemblyItem::Instruction(instruction) => {
                            instructions.push(instruction);
                            positions.push(position);
                            if instructions.len() == 4 {
                                break;
                            }
                        }
                        AssemblyItem::RelativeJump {
                            ..
                        } => break,
                    }
                }
                match (instructions.as_slice(), positions.as_slice()) {
                    ([first, second, third, fourth], [_, second_pos, _, fourth_pos]) => {
                        melded_instruction([first, second, third, fourth])
                            .map(|replacement| (replacement, *second_pos, *fourth_pos))
                    }
                    _ => None,
                }
            };

            if let Some((replacement, constant_position, end_position)) = candidate {
                let mut leading_labels = Vec::new();
                let mut trailing_labels = Vec::new();
                for position in 0..=end_position {
                    if let Some(AssemblyItem::Label(label)) = pending.pop_front() {
                        if position < constant_position {
                            leading_labels.push(AssemblyItem::Label(label));
                        } else {
                            trailing_labels.push(AssemblyItem::Label(label));
                        }
                    }
                }
                optimized.extend(leading_labels);
                optimized.push(AssemblyItem::Instruction(replacement));
                optimized.extend(trailing_labels);
            } else if let Some(item) = pending.pop_front() {
                optimized.push(item);
            }
        }
        self.items = optimized;
    }

    fn finalize(self) -> Result<ResolvedAssembly, CodegenError> {
        let mut offsets = BTreeMap::new();
        let mut offset = 0usize;
        for item in &self.items {
            match item {
                AssemblyItem::Label(label) => {
                    offsets.insert(*label, offset);
                }
                AssemblyItem::Instruction(instruction) => {
                    offset += instruction.encoded_len();
                }
                AssemblyItem::RelativeJump {
                    ..
                } => {
                    offset += NCS_OPERATION_BASE_SIZE + 4;
                }
            }
        }

        let mut instructions = Vec::new();
        let mut offset = 0usize;
        for item in self.items {
            match item {
                AssemblyItem::Label(_) => {}
                AssemblyItem::Instruction(instruction) => {
                    offset += instruction.encoded_len();
                    instructions.push(instruction);
                }
                AssemblyItem::RelativeJump {
                    opcode,
                    target,
                } => {
                    let target_offset = offsets.get(&target).copied().ok_or_else(|| {
                        CodegenError::new(None, format!("unresolved code label {target:?}"))
                    })?;
                    let delta = usize_to_i32(target_offset, "jump target offset")?
                        - usize_to_i32(offset, "jump offset")?;
                    let instruction = NcsInstruction {
                        opcode,
                        auxcode: NcsAuxCode::None,
                        extra: delta.to_be_bytes().to_vec(),
                    };
                    offset += instruction.encoded_len();
                    instructions.push(instruction);
                }
            }
        }

        Ok(ResolvedAssembly {
            instructions,
            offsets: offsets
                .into_iter()
                .map(|(label, offset)| {
                    Ok::<_, CodegenError>((label, usize_to_u32(offset, "label offset")?))
                })
                .collect::<Result<_, _>>()?,
        })
    }
}

struct O0Compiler<'a> {
    hir:                   &'a HirModule,
    langspec:              Option<&'a LangSpec>,
    builtin_functions:     BTreeMap<String, (u16, &'a BuiltinFunction)>,
    builtin_constants:     BTreeMap<String, BuiltinValue>,
    constant_env:          BTreeMap<String, ConstValue>,
    structs:               BTreeMap<String, &'a crate::HirStruct>,
    functions:             BTreeMap<String, &'a HirFunction>,
    entry_function:        Option<&'a HirFunction>,
    global_layout:         BTreeMap<String, ValueLayout>,
    global_size:           usize,
    global_init_bytes:     Option<usize>,
    function_labels:       BTreeMap<String, LabelId>,
    function_exit_labels:  BTreeMap<String, LabelId>,
    function_end_labels:   BTreeMap<String, LabelId>,
    globals_label:         Option<LabelId>,
    globals_end_label:     Option<LabelId>,
    variable_debug:        Vec<VariableDebugInfo>,
    line_debug:            LineDebugTracker,
    source_map:            Option<&'a SourceMap>,
    current_function_name: Option<&'a str>,
    compile_time:          SystemTime,
    assembler:             Assembler,
}

#[derive(Clone)]
struct ValueLayout {
    offset: usize,
    size:   usize,
}

#[derive(Clone)]
struct FieldLayout {
    ty:     SemanticType,
    offset: usize,
    size:   usize,
}

#[derive(Clone)]
struct FunctionLayout {
    return_layout:      Option<ValueLayout>,
    locals:             BTreeMap<HirLocalId, ValueLayout>,
    parameter_size:     usize,
    active_locals_size: usize,
}

struct FunctionScope {
    variable_debug: Vec<usize>,
    locals:         Vec<HirLocalId>,
    local_bytes:    usize,
    temp_bytes:     usize,
}

#[derive(Clone, Copy)]
struct ControlTarget {
    label:       LabelId,
    scope_depth: usize,
    temp_bytes:  usize,
}

struct FunctionEmitter<'a, 'b> {
    compiler:         &'b mut O0Compiler<'a>,
    function:         &'a HirFunction,
    layout:           FunctionLayout,
    temp_bytes:       usize,
    break_targets:    Vec<ControlTarget>,
    continue_targets: Vec<ControlTarget>,
    scope_stack:      Vec<FunctionScope>,
}

impl<'a> O0Compiler<'a> {
    fn new(
        hir: &'a HirModule,
        langspec: Option<&'a LangSpec>,
        source_map: Option<&'a SourceMap>,
    ) -> Result<Self, CodegenError> {
        let mut builtin_functions = BTreeMap::new();
        let mut builtin_constants = BTreeMap::new();
        if let Some(langspec) = langspec {
            for (index, function) in langspec.functions.iter().enumerate() {
                builtin_functions.insert(
                    function.name.clone(),
                    (usize_to_u16(index, "builtin function index")?, function),
                );
            }
            for constant in &langspec.constants {
                builtin_constants.insert(constant.name.clone(), constant.value.clone());
            }
        }
        let constant_env = build_constant_env(hir, langspec);

        let structs = hir
            .structs
            .iter()
            .map(|structure| (structure.name.clone(), structure))
            .collect::<BTreeMap<_, _>>();
        let functions = hir
            .functions
            .iter()
            .map(|function| (function.name.clone(), function))
            .collect::<BTreeMap<_, _>>();
        let entry_function = functions
            .get("main")
            .copied()
            .or_else(|| functions.get("StartingConditional").copied());

        let mut global_layout = BTreeMap::new();
        let mut global_size = 0usize;
        for global in hir.globals.iter().filter(|global| !global.is_const) {
            let size = size_of_type(&global.ty, &structs)?;
            global_layout.insert(
                global.name.clone(),
                ValueLayout {
                    offset: global_size,
                    size,
                },
            );
            global_size += size;
        }

        let mut assembler = Assembler::default();
        // Native parsing places top-level structure definitions in the global
        // parse tree. They therefore retain the saved-base-pointer wrapper
        // even when no mutable global storage is allocated. Const-only units
        // do not create that wrapper.
        let globals_label =
            (global_size > 0 || !hir.structs.is_empty()).then(|| assembler.new_label());
        let globals_end_label = globals_label.map(|_| assembler.new_label());
        let function_labels = hir
            .functions
            .iter()
            .map(|function| (function.name.clone(), assembler.new_label()))
            .collect::<BTreeMap<_, _>>();
        let function_end_labels = hir
            .functions
            .iter()
            .map(|function| (function.name.clone(), assembler.new_label()))
            .collect::<BTreeMap<_, _>>();
        let function_exit_labels = hir
            .functions
            .iter()
            .map(|function| (function.name.clone(), assembler.new_label()))
            .collect::<BTreeMap<_, _>>();

        Ok(Self {
            hir,
            langspec,
            builtin_functions,
            builtin_constants,
            constant_env,
            structs,
            functions,
            entry_function,
            global_layout,
            global_size,
            global_init_bytes: None,
            function_labels,
            function_exit_labels,
            function_end_labels,
            globals_label,
            globals_end_label,
            variable_debug: Vec::new(),
            line_debug: LineDebugTracker::default(),
            source_map,
            current_function_name: None,
            compile_time: SystemTime::now(),
            assembler,
        })
    }

    fn compile(mut self, meld: bool) -> Result<CodegenOutput, CodegenError> {
        self.emit_loader()?;

        if let Some(globals_label) = self.globals_label {
            self.assembler.place_label(globals_label);
            self.emit_globals()?;
            if let Some(end) = self.globals_end_label {
                self.assembler.place_label(end);
            }
        }

        for function in &self.hir.functions {
            if function.is_builtin {
                continue;
            }
            let label = self
                .function_labels
                .get(&function.name)
                .copied()
                .ok_or_else(|| {
                    CodegenError::new(
                        Some(function.span),
                        format!("missing function label for {:?}", function.name),
                    )
                })?;
            self.assembler.place_label(label);
            self.emit_function(function)?;
            let end_label = self
                .function_end_labels
                .get(&function.name)
                .copied()
                .ok_or_else(|| {
                    CodegenError::new(
                        Some(function.span),
                        format!("missing function end label for {:?}", function.name),
                    )
                })?;
            self.assembler.place_label(end_label);
        }

        if meld {
            self.assembler.meld_instructions();
        }
        let assembly = self.assembler.finalize()?;
        Ok(CodegenOutput {
            instructions:  assembly.instructions,
            label_offsets: assembly.offsets,
            functions:     self
                .function_labels
                .iter()
                .map(|(name, start)| {
                    let end = self.function_end_labels.get(name).copied().ok_or_else(|| {
                        CodegenError::new(None, format!("missing function end label for {name:?}"))
                    })?;
                    Ok::<_, CodegenError>((
                        name.clone(),
                        FunctionDebugInfo {
                            start: *start,
                            end,
                        },
                    ))
                })
                .collect::<Result<_, _>>()?,
            variables:     self.variable_debug,
            lines:         self.line_debug.entries,
        })
    }

    fn emit_loader(&mut self) -> Result<(), CodegenError> {
        if let Some(entry) = self.entry_function
            && entry.return_type != SemanticType::Void
        {
            self.emit_stack_alloc(&entry.return_type)?;
            let start = self.assembler.new_label();
            self.assembler.place_label(start);
            self.variable_debug.push(VariableDebugInfo {
                name: "#retval".to_string(),
                ty: entry.return_type.clone(),
                start,
                end: None,
                stack_loc: 0,
            });
        }

        if let Some(globals_label) = self.globals_label {
            self.assembler.push_jump(NcsOpcode::Jsr, globals_label);
        } else if let Some(entry) = self.entry_function {
            let label = self
                .function_labels
                .get(&entry.name)
                .copied()
                .ok_or_else(|| {
                    CodegenError::new(
                        Some(entry.span),
                        format!("missing function label for {:?}", entry.name),
                    )
                })?;
            self.assembler.push_jump(NcsOpcode::Jsr, label);
        }

        self.assembler.push(simple_instruction(NcsOpcode::Ret));
        Ok(())
    }

    fn emit_globals(&mut self) -> Result<(), CodegenError> {
        let globals = self
            .hir
            .globals
            .iter()
            .filter(|global| !global.is_const)
            .cloned()
            .collect::<Vec<_>>();
        let mut emitter = GlobalEmitter {
            compiler:        self,
            allocated_bytes: 0,
            temp_bytes:      0,
        };
        for global in &globals {
            emitter.compiler.emit_stack_alloc(&global.ty)?;
            let size = size_of_type(&global.ty, &emitter.compiler.structs)?;
            emitter.allocated_bytes += size;
            let start = emitter.compiler.assembler.new_label();
            emitter.compiler.assembler.place_label(start);
            let layout = emitter
                .compiler
                .global_layout
                .get(&global.name)
                .ok_or_else(|| {
                    CodegenError::new(
                        Some(global.span),
                        format!("unknown global {:?}", global.name),
                    )
                })?;
            emitter.compiler.variable_debug.push(VariableDebugInfo {
                name: global.name.clone(),
                ty: global.ty.clone(),
                start,
                end: None,
                stack_loc: usize_to_u32(layout.offset, "global stack location")?,
            });
            if let Some(initializer) = &global.initializer {
                let start = emitter.compiler.assembler.new_label();
                emitter.compiler.assembler.place_label(start);
                emitter.compiler.start_line_at(global.span, start);
                emitter.emit_expr(initializer)?;
                emitter.emit_store_global(&global.name, initializer.span)?;
                emitter.emit_pop_type(&initializer.ty)?;
                let end = emitter.compiler.assembler.new_label();
                emitter.compiler.assembler.place_label(end);
                emitter.compiler.end_line_at(global.span, end);
            }
        }
        emitter
            .compiler
            .assembler
            .push(simple_instruction(NcsOpcode::SaveBasePointer));

        if let Some(entry) = emitter.compiler.entry_function {
            if entry.return_type != SemanticType::Void {
                emitter.compiler.emit_stack_alloc(&entry.return_type)?;
            }
            let label = emitter
                .compiler
                .function_labels
                .get(&entry.name)
                .copied()
                .ok_or_else(|| {
                    CodegenError::new(
                        Some(entry.span),
                        format!("missing function label for {:?}", entry.name),
                    )
                })?;
            emitter.compiler.assembler.push_jump(NcsOpcode::Jsr, label);

            if entry.return_type != SemanticType::Void {
                let return_size = size_of_type(&entry.return_type, &emitter.compiler.structs)?;
                // The loader owns the externally visible conditional return
                // slot. Copy the nested entrypoint result down across the
                // globals and saved base-pointer cell before unwinding them.
                let frame_distance = emitter
                    .compiler
                    .global_size
                    .checked_add(4)
                    .and_then(|size| size.checked_add(return_size.saturating_mul(2)))
                    .ok_or_else(|| CodegenError::new(Some(entry.span), "global frame overflow"))?;
                emitter.compiler.assembler.push(NcsInstruction {
                    opcode:  NcsOpcode::Assignment,
                    auxcode: NcsAuxCode::TypeVoid,
                    extra:   assignment_extra(
                        -usize_to_i32(frame_distance, "conditional global frame size")?,
                        return_size,
                    ),
                });
                emitter.compiler.assembler.push(NcsInstruction {
                    opcode:  NcsOpcode::ModifyStackPointer,
                    auxcode: NcsAuxCode::None,
                    extra:   (-usize_to_i32(return_size, "conditional return size")?)
                        .to_be_bytes()
                        .to_vec(),
                });
            }
        }

        emitter
            .compiler
            .assembler
            .push(simple_instruction(NcsOpcode::RestoreBasePointer));
        if emitter.compiler.global_size > 0 {
            emitter.compiler.assembler.push(NcsInstruction {
                opcode:  NcsOpcode::ModifyStackPointer,
                auxcode: NcsAuxCode::None,
                extra:   (-usize_to_i32(emitter.compiler.global_size, "global cleanup size")?)
                    .to_be_bytes()
                    .to_vec(),
            });
        }
        emitter
            .compiler
            .assembler
            .push(simple_instruction(NcsOpcode::Ret));
        Ok(())
    }

    fn emit_function(&mut self, function: &'a HirFunction) -> Result<(), CodegenError> {
        let previous_function_name = self.current_function_name.replace(function.name.as_str());
        let result = (|| {
            let layout = self.function_layout(function)?;
            let start = self
                .function_labels
                .get(&function.name)
                .copied()
                .ok_or_else(|| {
                    CodegenError::new(
                        Some(function.span),
                        format!("missing function label for {:?}", function.name),
                    )
                })?;
            let exit = self
                .function_exit_labels
                .get(&function.name)
                .copied()
                .ok_or_else(|| {
                    CodegenError::new(
                        Some(function.span),
                        format!("missing function exit label for {:?}", function.name),
                    )
                })?;
            let end = self
                .function_end_labels
                .get(&function.name)
                .copied()
                .ok_or_else(|| {
                    CodegenError::new(
                        Some(function.span),
                        format!("missing function end label for {:?}", function.name),
                    )
                })?;
            if let Some(retval) = &layout.return_layout {
                self.variable_debug.push(VariableDebugInfo {
                    name: "#retval".to_string(),
                    ty: function.return_type.clone(),
                    start,
                    end: Some(end),
                    stack_loc: usize_to_u32(retval.offset, "return stack location")?,
                });
            }
            for parameter in &function.parameters {
                let slot = layout.locals.get(&parameter.local).ok_or_else(|| {
                    CodegenError::new(
                        Some(function.span),
                        format!("unknown local slot {:?}", parameter.local),
                    )
                })?;
                self.variable_debug.push(VariableDebugInfo {
                    name: parameter.name.clone(),
                    ty: parameter.ty.clone(),
                    start,
                    end: Some(end),
                    stack_loc: usize_to_u32(slot.offset, "parameter stack location")?,
                });
            }
            let mut emitter = FunctionEmitter {
                compiler: self,
                function,
                layout,
                temp_bytes: 0,
                break_targets: Vec::new(),
                continue_targets: Vec::new(),
                scope_stack: Vec::new(),
            };
            emitter.emit_prologue()?;
            if let Some(body) = &function.body {
                emitter.emit_block(body)?;
                emitter.compiler.assembler.place_label(exit);
                let final_line_start = emitter.compiler.assembler.new_label();
                emitter.compiler.assembler.place_label(final_line_start);
                emitter
                    .compiler
                    .start_line_end_at(body.span, final_line_start);
                emitter.emit_parameter_cleanup();
                emitter
                    .compiler
                    .assembler
                    .push(simple_instruction(NcsOpcode::Ret));
                let final_line_end = emitter.compiler.assembler.new_label();
                emitter.compiler.assembler.place_label(final_line_end);
                emitter.compiler.end_line_end_at(body.span, final_line_end);
            } else {
                emitter.compiler.assembler.place_label(exit);
                emitter.emit_parameter_cleanup();
                emitter
                    .compiler
                    .assembler
                    .push(simple_instruction(NcsOpcode::Ret));
            }
            Ok(())
        })();
        self.current_function_name = previous_function_name;
        result
    }

    fn function_layout(&self, function: &HirFunction) -> Result<FunctionLayout, CodegenError> {
        let mut offset = 0usize;
        let return_layout = if function.return_type == SemanticType::Void {
            None
        } else {
            let size = size_of_type(&function.return_type, &self.structs)?;
            let layout = ValueLayout {
                offset,
                size,
            };
            offset += size;
            Some(layout)
        };

        let mut locals = BTreeMap::new();
        for parameter in function.parameters.iter().rev() {
            let size = size_of_type(&parameter.ty, &self.structs)?;
            locals.insert(
                parameter.local,
                ValueLayout {
                    offset,
                    size,
                },
            );
            offset += size;
        }

        let return_size = return_layout.as_ref().map_or(0, |layout| layout.size);
        let parameter_size = offset - return_size;
        Ok(FunctionLayout {
            return_layout,
            locals,
            parameter_size,
            active_locals_size: 0,
        })
    }

    fn emit_stack_alloc(&mut self, ty: &SemanticType) -> Result<(), CodegenError> {
        match ty {
            SemanticType::Int => self.assembler.push(simple_aux_instruction(
                NcsOpcode::RunstackAdd,
                NcsAuxCode::TypeInteger,
            )),
            SemanticType::Float => self.assembler.push(simple_aux_instruction(
                NcsOpcode::RunstackAdd,
                NcsAuxCode::TypeFloat,
            )),
            SemanticType::String => self.assembler.push(simple_aux_instruction(
                NcsOpcode::RunstackAdd,
                NcsAuxCode::TypeString,
            )),
            SemanticType::Object => self.assembler.push(simple_aux_instruction(
                NcsOpcode::RunstackAdd,
                NcsAuxCode::TypeObject,
            )),
            SemanticType::EngineStructure(name) => self.assembler.push(simple_aux_instruction(
                NcsOpcode::RunstackAdd,
                aux_for_engine_structure(name, self.hir, &self.structs)?,
            )),
            SemanticType::Vector => {
                self.emit_stack_alloc(&SemanticType::Float)?;
                self.emit_stack_alloc(&SemanticType::Float)?;
                self.emit_stack_alloc(&SemanticType::Float)?;
            }
            SemanticType::Struct(name) => {
                let structure = self.structs.get(name).ok_or_else(|| {
                    CodegenError::new(None, format!("unknown structure {name:?}"))
                })?;
                for field in &structure.fields {
                    self.emit_stack_alloc(&field.ty)?;
                }
            }
            SemanticType::Void | SemanticType::Action => {}
        }
        Ok(())
    }

    fn start_line_at(&mut self, span: crate::Span, label: LabelId) {
        let Some((source_id, line_num)) = self.line_location(span) else {
            return;
        };
        self.start_line(source_id, line_num, label);
    }

    fn start_line_end_at(&mut self, span: crate::Span, label: LabelId) {
        let Some((source_id, line_num)) = self.line_end_location(span) else {
            return;
        };
        self.start_line(source_id, line_num, label);
    }

    fn start_line(&mut self, source_id: SourceId, line_num: usize, label: LabelId) {
        match &mut self.line_debug.current {
            Some(current) if current.source_id == source_id && current.line_num == line_num => {
                current.refs += 1;
            }
            _ => {
                self.line_debug.current = Some(OpenLineDebug {
                    source_id,
                    line_num,
                    refs: 1,
                    start: label,
                });
            }
        }
    }

    fn end_line_at(&mut self, span: crate::Span, label: LabelId) {
        let Some((source_id, line_num)) = self.line_location(span) else {
            self.line_debug.current = None;
            return;
        };
        self.end_line(source_id, line_num, label);
    }

    fn end_line_end_at(&mut self, span: crate::Span, label: LabelId) {
        let Some((source_id, line_num)) = self.line_end_location(span) else {
            self.line_debug.current = None;
            return;
        };
        self.end_line(source_id, line_num, label);
    }

    fn end_line(&mut self, source_id: SourceId, line_num: usize, label: LabelId) {
        let Some(current) = &mut self.line_debug.current else {
            return;
        };
        if current.source_id != source_id || current.line_num != line_num {
            self.line_debug.current = None;
            return;
        }
        if current.refs > 1 {
            current.refs -= 1;
            return;
        }
        let Some(current) = self.line_debug.current.take() else {
            return;
        };
        self.line_debug.entries.push(LineDebugInfo {
            source_id: current.source_id,
            line_num:  current.line_num,
            start:     current.start,
            end:       label,
        });
    }

    fn line_location(&self, span: crate::Span) -> Option<(SourceId, usize)> {
        let source_map = self.source_map?;
        let file = source_map.get(span.source_id)?;
        let location = file.location(span.start)?;
        Some((span.source_id, location.line))
    }

    fn line_end_location(&self, span: crate::Span) -> Option<(SourceId, usize)> {
        let source_map = self.source_map?;
        let file = source_map.get(span.source_id)?;
        let position = if span.end > span.start {
            span.end - 1
        } else {
            span.start
        };
        let location = file.location(position)?;
        Some((span.source_id, location.line))
    }

    fn magic_literal_value(
        &self,
        literal: crate::MagicLiteral,
        span: Option<crate::Span>,
    ) -> Literal {
        match literal {
            crate::MagicLiteral::Function => {
                Literal::String(self.current_function_name.unwrap_or_default().into())
            }
            crate::MagicLiteral::File => {
                let mut value = span
                    .and_then(|span| self.source_map?.get(span.source_id))
                    .map(|file| file.name.clone())
                    .unwrap_or_default();
                if !value.is_empty() && Path::new(&value).extension().is_none() {
                    value.push_str(".nss");
                }
                Literal::String(value.into())
            }
            crate::MagicLiteral::Line => {
                let value = span
                    .and_then(|span| self.line_location(span))
                    .map_or(0, |(_source_id, line)| {
                        i32::try_from(line).ok().unwrap_or(i32::MAX)
                    });
                Literal::Integer(value)
            }
            crate::MagicLiteral::Date => {
                Literal::String(format_magic_date(self.compile_time).into())
            }
            crate::MagicLiteral::Time => {
                Literal::String(format_magic_time(self.compile_time).into())
            }
        }
    }
}

struct GlobalEmitter<'a, 'b> {
    compiler:        &'b mut O0Compiler<'a>,
    allocated_bytes: usize,
    temp_bytes:      usize,
}

impl GlobalEmitter<'_, '_> {
    fn emit_expr(&mut self, expr: &HirExpr) -> Result<(), CodegenError> {
        self.compiler.global_init_bytes = Some(self.allocated_bytes);
        let result = emit_expr_common(self.compiler, &mut self.temp_bytes, None, expr);
        self.compiler.global_init_bytes = None;
        result
    }

    fn emit_store_global(&mut self, name: &str, span: crate::Span) -> Result<(), CodegenError> {
        let layout = self
            .compiler
            .global_layout
            .get(name)
            .ok_or_else(|| CodegenError::new(Some(span), format!("unknown global {name:?}")))?;
        let offset = usize_to_i32(layout.offset, "global offset")?
            - usize_to_i32(
                self.allocated_bytes + self.temp_bytes,
                "global initialization stack size",
            )?;
        self.compiler.assembler.push(NcsInstruction {
            opcode:  NcsOpcode::Assignment,
            auxcode: NcsAuxCode::TypeVoid,
            extra:   assignment_extra(offset, layout.size),
        });
        Ok(())
    }

    fn emit_pop_type(&mut self, ty: &SemanticType) -> Result<(), CodegenError> {
        let size = size_of_type(ty, &self.compiler.structs)?;
        if size > 0 {
            self.temp_bytes = self.temp_bytes.saturating_sub(size);
            self.compiler.assembler.push(NcsInstruction {
                opcode:  NcsOpcode::ModifyStackPointer,
                auxcode: NcsAuxCode::None,
                extra:   (-usize_to_i32(size, "stack pop size")?)
                    .to_be_bytes()
                    .to_vec(),
            });
        }
        Ok(())
    }
}

impl FunctionEmitter<'_, '_> {
    fn emit_prologue(&mut self) -> Result<(), CodegenError> {
        Ok(())
    }

    fn emit_block(&mut self, block: &HirBlock) -> Result<(), CodegenError> {
        self.scope_stack.push(FunctionScope {
            variable_debug: Vec::new(),
            locals:         Vec::new(),
            local_bytes:    0,
            temp_bytes:     self.temp_bytes,
        });
        for statement in &block.statements {
            self.emit_stmt(statement)?;
        }
        self.close_scope()?;
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    fn emit_stmt(&mut self, statement: &HirStmt) -> Result<(), CodegenError> {
        match statement {
            HirStmt::Block(block) => self.emit_block(block),
            HirStmt::Declare(statement) => {
                let start = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(start);
                self.compiler.start_line_at(statement.span, start);
                let start = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(start);
                for declarator in &statement.declarators {
                    let local = self.local_info(declarator.local, statement.span)?.clone();
                    let size = size_of_type(&local.ty, &self.compiler.structs)?;
                    // A switch keeps its selector on the runtime stack while
                    // emitting the case body. Locals declared inside that
                    // body are therefore above both the normal frame and the
                    // live selector value.
                    let offset = self.current_stack_bytes();
                    self.compiler.emit_stack_alloc(&local.ty)?;
                    self.layout.locals.insert(
                        declarator.local,
                        ValueLayout {
                            offset,
                            size,
                        },
                    );
                    self.layout.active_locals_size += size;
                    let scope = self.current_scope();
                    scope.locals.push(declarator.local);
                    scope.local_bytes += size;
                    let end_index = self.compiler.variable_debug.len();
                    self.compiler.variable_debug.push(VariableDebugInfo {
                        name: local.name.clone(),
                        ty: local.ty.clone(),
                        start,
                        end: None,
                        stack_loc: self.local_stack_loc(declarator.local, statement.span)?,
                    });
                    self.current_scope().variable_debug.push(end_index);
                }
                for declarator in &statement.declarators {
                    if let Some(initializer) = &declarator.initializer {
                        self.emit_expr(initializer)?;
                        self.emit_store_local(declarator.local, initializer.span)?;
                        self.emit_pop_type(&initializer.ty)?;
                    }
                }
                let end = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(end);
                self.compiler.end_line_at(statement.span, end);
                Ok(())
            }
            HirStmt::Expr(expr) => {
                let start = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(start);
                self.compiler.start_line_at(expr.span, start);
                self.emit_expr(expr)?;
                self.emit_pop_type(&expr.ty)?;
                let end = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(end);
                self.compiler.end_line_at(expr.span, end);
                Ok(())
            }
            HirStmt::If(statement) => {
                let stmt_start = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(stmt_start);
                self.compiler.start_line_at(statement.span, stmt_start);
                let cond_start = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(cond_start);
                self.compiler
                    .start_line_at(statement.condition.span, cond_start);
                let else_label = self.compiler.assembler.new_label();
                let end_label = self.compiler.assembler.new_label();
                self.emit_expr(&statement.condition)?;
                self.emit_branch_zero(else_label)?;
                let cond_end = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(cond_end);
                self.compiler
                    .end_line_at(statement.condition.span, cond_end);
                self.emit_stmt(&statement.then_branch)?;
                self.compiler.assembler.push_jump(NcsOpcode::Jmp, end_label);
                self.compiler.assembler.place_label(else_label);
                if let Some(else_branch) = &statement.else_branch {
                    let choice_start = self.compiler.assembler.new_label();
                    self.compiler.assembler.place_label(choice_start);
                    self.compiler.start_line_at(statement.span, choice_start);
                    self.compiler
                        .assembler
                        .push(simple_instruction(NcsOpcode::NoOperation));
                    let choice_end = self.compiler.assembler.new_label();
                    self.compiler.assembler.place_label(choice_end);
                    self.compiler.end_line_at(statement.span, choice_end);
                    self.emit_stmt(else_branch)?;
                }
                self.compiler.assembler.place_label(end_label);
                let stmt_end = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(stmt_end);
                self.compiler.end_line_at(statement.span, stmt_end);
                Ok(())
            }
            HirStmt::Switch(statement) => {
                let stmt_start = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(stmt_start);
                self.compiler.start_line_at(statement.span, stmt_start);
                let result = self.emit_switch(statement);
                let stmt_end = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(stmt_end);
                self.compiler.end_line_at(statement.span, stmt_end);
                result
            }
            HirStmt::Return(statement) => {
                let start = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(start);
                self.compiler.start_line_at(statement.span, start);
                if let Some(value) = &statement.value {
                    self.emit_expr(value)?;
                    let Some(retval) = &self.layout.return_layout else {
                        return Err(CodegenError::new(
                            Some(statement.span),
                            "return value in void function during code generation",
                        ));
                    };
                    let offset = usize_to_i32(retval.offset, "return slot offset")?
                        - usize_to_i32(self.current_stack_bytes(), "current stack bytes")?;
                    self.compiler.assembler.push(NcsInstruction {
                        opcode:  NcsOpcode::Assignment,
                        auxcode: NcsAuxCode::TypeVoid,
                        extra:   assignment_extra(offset, retval.size),
                    });
                }
                self.emit_return_cleanup();
                let exit = self
                    .compiler
                    .function_exit_labels
                    .get(&self.function.name)
                    .copied()
                    .ok_or_else(|| {
                        CodegenError::new(
                            Some(statement.span),
                            format!("missing function exit label for {:?}", self.function.name),
                        )
                    })?;
                self.compiler.assembler.push_jump(NcsOpcode::Jmp, exit);
                // Native codegen still emits the expression-temporary pop
                // after the return jump. It is unreachable at runtime, but it
                // restores compile-time stack accounting for sibling control
                // flow and is required for byte-identical output.
                if let Some(value) = &statement.value {
                    self.emit_pop_type(&value.ty)?;
                }
                let end = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(end);
                self.compiler.end_line_at(statement.span, end);
                Ok(())
            }
            HirStmt::While(statement) => {
                let stmt_start = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(stmt_start);
                self.compiler.start_line_at(statement.span, stmt_start);
                let cond_label = self.compiler.assembler.new_label();
                let end_label = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(cond_label);
                let loop_start = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(loop_start);
                self.compiler.start_line_at(statement.span, loop_start);
                self.emit_expr(&statement.condition)?;
                self.emit_branch_zero(end_label)?;
                let loop_end = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(loop_end);
                self.compiler.end_line_at(statement.span, loop_end);
                self.break_targets.push(self.control_target(end_label));
                self.continue_targets.push(self.control_target(cond_label));
                self.emit_stmt(&statement.body)?;
                self.continue_targets.pop();
                self.break_targets.pop();
                self.compiler
                    .assembler
                    .push_jump(NcsOpcode::Jmp, cond_label);
                self.compiler.assembler.place_label(end_label);
                let stmt_end = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(stmt_end);
                self.compiler.end_line_at(statement.span, stmt_end);
                Ok(())
            }
            HirStmt::DoWhile(statement) => {
                let stmt_start = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(stmt_start);
                self.compiler.start_line_at(statement.span, stmt_start);
                let body_label = self.compiler.assembler.new_label();
                let cond_label = self.compiler.assembler.new_label();
                let end_label = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(body_label);
                self.break_targets.push(self.control_target(end_label));
                self.continue_targets.push(self.control_target(cond_label));
                self.emit_stmt(&statement.body)?;
                self.continue_targets.pop();
                self.break_targets.pop();
                self.compiler.assembler.place_label(cond_label);
                let continue_start = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(continue_start);
                self.compiler.start_line_at(statement.span, continue_start);
                self.emit_expr(&statement.condition)?;
                self.emit_branch_zero(end_label)?;
                self.compiler
                    .assembler
                    .push_jump(NcsOpcode::Jmp, body_label);
                self.compiler.assembler.place_label(end_label);
                let continue_end = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(continue_end);
                self.compiler.end_line_at(statement.span, continue_end);
                let stmt_end = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(stmt_end);
                self.compiler.end_line_at(statement.span, stmt_end);
                Ok(())
            }
            HirStmt::For(statement) => {
                let start = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(start);
                self.compiler.start_line_at(statement.span, start);
                if let Some(initializer) = &statement.initializer {
                    self.emit_expr(initializer)?;
                    self.emit_pop_type(&initializer.ty)?;
                }
                let cond_label = self.compiler.assembler.new_label();
                let update_label = self.compiler.assembler.new_label();
                let end_label = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(cond_label);
                if let Some(condition) = &statement.condition {
                    self.emit_expr(condition)?;
                    self.emit_branch_zero(end_label)?;
                }
                self.break_targets.push(self.control_target(end_label));
                self.continue_targets
                    .push(self.control_target(update_label));
                self.emit_stmt(&statement.body)?;
                self.continue_targets.pop();
                self.break_targets.pop();
                self.compiler.assembler.place_label(update_label);
                if let Some(update) = &statement.update {
                    self.emit_expr(update)?;
                    self.emit_pop_type(&update.ty)?;
                }
                self.compiler
                    .assembler
                    .push_jump(NcsOpcode::Jmp, cond_label);
                self.compiler.assembler.place_label(end_label);
                let end = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(end);
                self.compiler.end_line_at(statement.span, end);
                Ok(())
            }
            HirStmt::Case(_) | HirStmt::Default(_) => Err(CodegenError::new(
                None,
                "case/default labels must be lowered through emit_switch",
            )),
            HirStmt::Break(span) => {
                let start = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(start);
                self.compiler.start_line_at(*span, start);
                let Some(target) = self.break_targets.last().copied() else {
                    return Err(CodegenError::new(
                        Some(*span),
                        "break used outside loop or switch",
                    ));
                };
                self.emit_control_cleanup(target)?;
                self.compiler
                    .assembler
                    .push_jump(NcsOpcode::Jmp, target.label);
                let end = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(end);
                self.compiler.end_line_at(*span, end);
                Ok(())
            }
            HirStmt::Continue(span) => {
                let start = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(start);
                self.compiler.start_line_at(*span, start);
                let Some(target) = self.continue_targets.last().copied() else {
                    return Err(CodegenError::new(Some(*span), "continue used outside loop"));
                };
                self.emit_control_cleanup(target)?;
                self.compiler
                    .assembler
                    .push_jump(NcsOpcode::Jmp, target.label);
                let end = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(end);
                self.compiler.end_line_at(*span, end);
                Ok(())
            }
            HirStmt::Empty(span) => {
                let start = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(start);
                self.compiler.start_line_at(*span, start);
                let end = self.compiler.assembler.new_label();
                self.compiler.assembler.place_label(end);
                self.compiler.end_line_at(*span, end);
                Ok(())
            }
        }
    }

    fn close_scope(&mut self) -> Result<(), CodegenError> {
        let Some(scope) = self.scope_stack.pop() else {
            return Ok(());
        };
        if self.temp_bytes > scope.temp_bytes {
            self.emit_pop_bytes(self.temp_bytes - scope.temp_bytes);
        }
        let end = self.compiler.assembler.new_label();
        self.compiler.assembler.place_label(end);
        for index in scope.variable_debug {
            if let Some(variable) = self.compiler.variable_debug.get_mut(index) {
                variable.end = Some(end);
            }
        }
        if scope.local_bytes > 0 {
            self.compiler.assembler.push(NcsInstruction {
                opcode:  NcsOpcode::ModifyStackPointer,
                auxcode: NcsAuxCode::None,
                extra:   (-usize_to_i32(scope.local_bytes, "scope cleanup size")?)
                    .to_be_bytes()
                    .to_vec(),
            });
            self.layout.active_locals_size = self
                .layout
                .active_locals_size
                .saturating_sub(scope.local_bytes);
        }
        for local in scope.locals {
            self.layout.locals.remove(&local);
        }
        Ok(())
    }

    fn current_scope(&mut self) -> &mut FunctionScope {
        self.scope_stack
            .last_mut()
            .unwrap_or_else(|| unreachable!("function blocks should always have an active scope"))
    }

    fn control_target(&self, label: LabelId) -> ControlTarget {
        ControlTarget {
            label,
            scope_depth: self.scope_stack.len(),
            temp_bytes: self.temp_bytes,
        }
    }

    fn emit_control_cleanup(&mut self, target: ControlTarget) -> Result<(), CodegenError> {
        let local_bytes = self
            .scope_stack
            .get(target.scope_depth..)
            .unwrap_or_default()
            .iter()
            .map(|scope| scope.local_bytes)
            .sum::<usize>();
        let cleanup = local_bytes + self.temp_bytes.saturating_sub(target.temp_bytes);
        if cleanup > 0 {
            self.compiler.assembler.push(NcsInstruction {
                opcode:  NcsOpcode::ModifyStackPointer,
                auxcode: NcsAuxCode::None,
                extra:   (-usize_to_i32(cleanup, "control-flow cleanup size")?)
                    .to_be_bytes()
                    .to_vec(),
            });
        }
        Ok(())
    }

    fn local_info(
        &self,
        local_id: HirLocalId,
        span: crate::Span,
    ) -> Result<&crate::HirLocal, CodegenError> {
        self.function
            .locals
            .iter()
            .find(|local| local.id == local_id)
            .ok_or_else(|| CodegenError::new(Some(span), format!("unknown local {local_id:?}")))
    }

    fn local_stack_loc(
        &self,
        local_id: HirLocalId,
        span: crate::Span,
    ) -> Result<u32, CodegenError> {
        let slot = self.layout.locals.get(&local_id).ok_or_else(|| {
            CodegenError::new(Some(span), format!("unknown local slot {local_id:?}"))
        })?;
        usize_to_u32(slot.offset, "local stack location")
    }

    fn emit_switch(&mut self, statement: &crate::HirSwitchStmt) -> Result<(), CodegenError> {
        let HirStmt::Block(block) = statement.body.as_ref() else {
            return Err(CodegenError::new(
                Some(statement.span),
                "switch lowering requires a block body",
            ));
        };

        let switch_start = self.compiler.assembler.new_label();
        self.compiler.assembler.place_label(switch_start);
        self.compiler.start_line_at(statement.span, switch_start);
        self.emit_expr(&statement.condition)?;
        let switch_result_size = size_of_type(&statement.condition.ty, &self.compiler.structs)?;
        let body_end = self.compiler.assembler.new_label();
        let switch_eval_start = self.compiler.assembler.new_label();
        self.compiler.assembler.place_label(switch_eval_start);
        self.compiler.variable_debug.push(VariableDebugInfo {
            name:      "#switcheval".to_string(),
            ty:        SemanticType::Int,
            start:     switch_eval_start,
            end:       Some(body_end),
            stack_loc: usize_to_u32(
                self.current_stack_bytes()
                    .saturating_sub(switch_result_size),
                "switch stack location",
            )?,
        });

        let mut case_labels = Vec::new();
        let mut default_label = None;
        let mut case_index = 0usize;
        for stmt in &block.statements {
            match stmt {
                HirStmt::Case(case) => {
                    case_labels.push((
                        case_index,
                        evaluate_case_value(case, &self.compiler.constant_env)?,
                        self.compiler.assembler.new_label(),
                    ));
                    case_index += 1;
                }
                HirStmt::Default(_) => {
                    let label = self.compiler.assembler.new_label();
                    default_label = Some((case_index, label));
                }
                _ => {}
            }
        }

        let next_test = self.compiler.assembler.new_label();
        self.compiler.assembler.place_label(next_test);
        for (_, value, label) in &case_labels {
            self.emit_copy_top_value(switch_result_size)?;
            emit_push_literal(
                self.compiler,
                &mut self.temp_bytes,
                &Literal::Integer(*value),
                &SemanticType::Int,
                Some(statement.span),
            )?;
            self.emit_binary(
                BinaryOp::EqualEqual,
                &SemanticType::Int,
                &SemanticType::Int,
                Some(statement.span),
            )?;
            self.temp_bytes = self.temp_bytes.saturating_sub(4);
            self.compiler.assembler.push_jump(NcsOpcode::Jnz, *label);
        }
        if let Some((_, label)) = default_label {
            self.compiler.assembler.push_jump(NcsOpcode::Jmp, label);
        } else {
            self.compiler.assembler.push_jump(NcsOpcode::Jmp, body_end);
        }

        let break_target = self.control_target(body_end);
        self.break_targets.push(break_target);
        self.scope_stack.push(FunctionScope {
            variable_debug: Vec::new(),
            locals:         Vec::new(),
            local_bytes:    0,
            temp_bytes:     self.temp_bytes,
        });
        let mut seen_cases = 0usize;
        for stmt in &block.statements {
            match stmt {
                HirStmt::Case(_) => {
                    let Some((_, _, label)) = case_labels.get(seen_cases).copied() else {
                        return Err(CodegenError::new(
                            Some(statement.span),
                            "switch case label index out of bounds",
                        ));
                    };
                    seen_cases += 1;
                    self.compiler.assembler.place_label(label);
                }
                HirStmt::Default(span) => {
                    let Some((_, label)) = default_label else {
                        return Err(CodegenError::new(Some(*span), "missing default label"));
                    };
                    self.compiler.assembler.place_label(label);
                }
                other => self.emit_stmt(other)?,
            }
        }
        self.break_targets.pop();
        self.close_scope()?;
        self.compiler.assembler.place_label(body_end);
        self.emit_pop_bytes(switch_result_size);
        Ok(())
    }

    fn emit_expr(&mut self, expr: &HirExpr) -> Result<(), CodegenError> {
        emit_expr_common(
            self.compiler,
            &mut self.temp_bytes,
            Some(&self.layout),
            expr,
        )
    }

    fn emit_store_local(
        &mut self,
        local: HirLocalId,
        span: crate::Span,
    ) -> Result<(), CodegenError> {
        let layout = self.layout.locals.get(&local).ok_or_else(|| {
            CodegenError::new(Some(span), format!("unknown local slot {local:?}"))
        })?;
        let offset = usize_to_i32(layout.offset, "local slot offset")?
            - usize_to_i32(self.current_stack_bytes(), "current stack bytes")?;
        self.compiler.assembler.push(NcsInstruction {
            opcode:  NcsOpcode::Assignment,
            auxcode: NcsAuxCode::TypeVoid,
            extra:   assignment_extra(offset, layout.size),
        });
        Ok(())
    }

    fn emit_branch_zero(&mut self, target: LabelId) -> Result<(), CodegenError> {
        if self.temp_bytes < 4 {
            return Err(CodegenError::new(
                None,
                "branch expected an integer condition on the stack",
            ));
        }
        self.temp_bytes -= 4;
        self.compiler.assembler.push_jump(NcsOpcode::Jz, target);
        Ok(())
    }

    fn emit_copy_top_value(&mut self, size: usize) -> Result<(), CodegenError> {
        self.compiler.assembler.push(NcsInstruction {
            opcode:  NcsOpcode::RunstackCopy,
            auxcode: NcsAuxCode::TypeVoid,
            extra:   assignment_extra(-usize_to_i32(size, "copy size")?, size),
        });
        self.temp_bytes += size;
        Ok(())
    }

    fn emit_binary(
        &mut self,
        op: BinaryOp,
        left: &SemanticType,
        right: &SemanticType,
        span: Option<crate::Span>,
    ) -> Result<(), CodegenError> {
        let opcode = opcode_for_binary(op);
        let auxcode = aux_for_binary(left, right, self.compiler.hir, &self.compiler.structs)?;
        let left_size = size_of_type(left, &self.compiler.structs)?;
        let right_size = size_of_type(right, &self.compiler.structs)?;
        let result_size = size_of_binary_result(op, left, right, &self.compiler.structs)?;
        if self.temp_bytes < left_size + right_size {
            return Err(CodegenError::new(
                span,
                "binary operation expected both operands on the stack",
            ));
        }
        self.temp_bytes -= left_size + right_size;
        self.temp_bytes += result_size;
        let extra = if auxcode == NcsAuxCode::TypeTypeStructStruct
            && matches!(opcode, NcsOpcode::Equal | NcsOpcode::NotEqual)
        {
            usize_to_u16(left_size, "struct equality size")?
                .to_be_bytes()
                .to_vec()
        } else {
            Vec::new()
        };
        self.compiler.assembler.push(NcsInstruction {
            opcode,
            auxcode,
            extra,
        });
        Ok(())
    }

    fn emit_pop_type(&mut self, ty: &SemanticType) -> Result<(), CodegenError> {
        let size = size_of_type(ty, &self.compiler.structs)?;
        self.emit_pop_bytes(size);
        Ok(())
    }

    fn emit_pop_bytes(&mut self, size: usize) {
        if size > 0 {
            self.temp_bytes = self.temp_bytes.saturating_sub(size);
            self.compiler.assembler.push(NcsInstruction {
                opcode:  NcsOpcode::ModifyStackPointer,
                auxcode: NcsAuxCode::None,
                extra:   (-i32::try_from(size).ok().unwrap_or(i32::MAX))
                    .to_be_bytes()
                    .to_vec(),
            });
        }
    }

    fn emit_return_cleanup(&mut self) {
        // A return statement removes live locals and expression temporaries,
        // then branches to the shared exit that removes parameters.
        let cleanup = self.layout.active_locals_size + self.temp_bytes;
        self.emit_cleanup_bytes(cleanup);
    }

    fn emit_parameter_cleanup(&mut self) {
        self.emit_cleanup_bytes(self.layout.parameter_size);
    }

    fn emit_cleanup_bytes(&mut self, cleanup: usize) {
        if cleanup > 0 {
            self.compiler.assembler.push(NcsInstruction {
                opcode:  NcsOpcode::ModifyStackPointer,
                auxcode: NcsAuxCode::None,
                extra:   (-i32::try_from(cleanup).ok().unwrap_or(i32::MAX))
                    .to_be_bytes()
                    .to_vec(),
            });
        }
    }

    fn current_stack_bytes(&self) -> usize {
        function_frame_bytes(&self.layout) + self.temp_bytes
    }
}

#[allow(clippy::too_many_lines)]
fn emit_expr_common(
    compiler: &mut O0Compiler<'_>,
    temp_bytes: &mut usize,
    layout: Option<&FunctionLayout>,
    expr: &HirExpr,
) -> Result<(), CodegenError> {
    // Constant folding is part of the native compiler's ordinary parse-tree
    // walk, independent of its selectable optimization flags.
    if matches!(
        &expr.kind,
        HirExprKind::Unary { .. } | HirExprKind::Binary { .. }
    ) && let Some(value) = evaluate_const_expr(expr, &compiler.constant_env)
    {
        let literal = match value {
            ConstValue::Int(value) => Literal::Integer(value),
            ConstValue::Float(value) => Literal::Float(value),
            ConstValue::String(value) => Literal::String(value),
        };
        return emit_push_literal(compiler, temp_bytes, &literal, &expr.ty, Some(expr.span));
    }

    match &expr.kind {
        HirExprKind::Literal(literal) => {
            emit_push_literal(compiler, temp_bytes, literal, &expr.ty, Some(expr.span))
        }
        HirExprKind::Value(value) => match value {
            crate::HirValueRef::Local(local) => {
                let layout = layout.ok_or_else(|| {
                    CodegenError::new(Some(expr.span), "local value used outside a function")
                })?;
                let slot = layout.locals.get(local).ok_or_else(|| {
                    CodegenError::new(Some(expr.span), format!("unknown local slot {local:?}"))
                })?;
                let frame_bytes = function_frame_bytes(layout);
                let offset = usize_to_i32(slot.offset, "local load offset")?
                    - usize_to_i32(frame_bytes + *temp_bytes, "local frame bytes")?;
                compiler.assembler.push(NcsInstruction {
                    opcode:  NcsOpcode::RunstackCopy,
                    auxcode: NcsAuxCode::TypeVoid,
                    extra:   assignment_extra(offset, slot.size),
                });
                *temp_bytes += slot.size;
                Ok(())
            }
            crate::HirValueRef::Global(name) => {
                let slot = compiler.global_layout.get(name).ok_or_else(|| {
                    CodegenError::new(Some(expr.span), format!("unknown global {name:?}"))
                })?;
                let (opcode, stack_size) = compiler.global_init_bytes.map_or(
                    (NcsOpcode::RunstackCopyBase, compiler.global_size),
                    |allocated| (NcsOpcode::RunstackCopy, allocated + *temp_bytes),
                );
                let offset = usize_to_i32(slot.offset, "global load offset")?
                    - usize_to_i32(stack_size, "global load stack size")?;
                compiler.assembler.push(NcsInstruction {
                    opcode,
                    auxcode: NcsAuxCode::TypeVoid,
                    extra: assignment_extra(offset, slot.size),
                });
                *temp_bytes += slot.size;
                Ok(())
            }
            crate::HirValueRef::ConstGlobal(name) => {
                let value = compiler.constant_env.get(name).ok_or_else(|| {
                    CodegenError::new(Some(expr.span), format!("unknown const global {name:?}"))
                })?;
                let literal = match value {
                    ConstValue::Int(value) => Literal::Integer(*value),
                    ConstValue::Float(value) => Literal::Float(*value),
                    ConstValue::String(value) => Literal::String(value.clone()),
                };
                emit_push_literal(compiler, temp_bytes, &literal, &expr.ty, Some(expr.span))
            }
            crate::HirValueRef::BuiltinConstant(name) => {
                let value = compiler.builtin_constants.get(name).ok_or_else(|| {
                    CodegenError::new(
                        Some(expr.span),
                        format!("unknown builtin constant {name:?}"),
                    )
                })?;
                let literal = literal_from_builtin_value(value).ok_or_else(|| {
                    CodegenError::new(
                        Some(expr.span),
                        format!("unsupported builtin constant value for {name:?}"),
                    )
                })?;
                emit_push_literal(compiler, temp_bytes, &literal, &expr.ty, Some(expr.span))
            }
        },
        HirExprKind::Call {
            target,
            arguments,
        } => emit_call(compiler, temp_bytes, layout, expr, target, arguments),
        HirExprKind::FieldAccess {
            base,
            field,
        } => {
            emit_expr_common(compiler, temp_bytes, layout, base)?;
            let base_size = size_of_type(&base.ty, &compiler.structs)?;
            let field_layout = field_layout(&base.ty, field, &compiler.structs, Some(expr.span))?;
            debug_assert_eq!(field_layout.ty, expr.ty);
            let mut extra = Vec::with_capacity(6);
            extra.extend_from_slice(&usize_to_u16(base_size, "structure size")?.to_be_bytes());
            extra.extend_from_slice(
                &usize_to_u16(field_layout.offset, "structure field offset")?.to_be_bytes(),
            );
            extra.extend_from_slice(
                &usize_to_u16(field_layout.size, "structure field size")?.to_be_bytes(),
            );
            compiler.assembler.push(NcsInstruction {
                opcode: NcsOpcode::DeStruct,
                auxcode: NcsAuxCode::TypeVoid,
                extra,
            });
            *temp_bytes = temp_bytes.saturating_sub(base_size);
            *temp_bytes += field_layout.size;
            Ok(())
        }
        HirExprKind::Unary {
            op,
            expr: inner,
        } => {
            if matches!(
                op,
                UnaryOp::PreIncrement
                    | UnaryOp::PreDecrement
                    | UnaryOp::PostIncrement
                    | UnaryOp::PostDecrement
            ) {
                if matches!(op, UnaryOp::PreIncrement | UnaryOp::PreDecrement) {
                    emit_increment_target(compiler, *temp_bytes, layout, inner, *op, expr.span)?;
                    emit_expr_common(compiler, temp_bytes, layout, inner)?;
                    return Ok(());
                }
                emit_expr_common(compiler, temp_bytes, layout, inner)?;
                emit_increment_target(compiler, *temp_bytes, layout, inner, *op, expr.span)?;
                return Ok(());
            }
            emit_expr_common(compiler, temp_bytes, layout, inner)?;
            let opcode = match op {
                UnaryOp::Negate => NcsOpcode::Negation,
                UnaryOp::OnesComplement => NcsOpcode::OnesComplement,
                UnaryOp::BooleanNot => NcsOpcode::BooleanNot,
                UnaryOp::PreIncrement
                | UnaryOp::PreDecrement
                | UnaryOp::PostIncrement
                | UnaryOp::PostDecrement => unreachable!(),
            };
            compiler.assembler.push(NcsInstruction {
                opcode,
                auxcode: aux_for_unary(&expr.ty, compiler.hir, &compiler.structs)?,
                extra: Vec::new(),
            });
            Ok(())
        }
        HirExprKind::Binary {
            op,
            left,
            right,
        } => {
            emit_expr_common(compiler, temp_bytes, layout, left)?;
            if matches!(op, BinaryOp::LogicalAnd | BinaryOp::LogicalOr) {
                let base_temp_bytes = temp_bytes.saturating_sub(4);
                emit_copy_top_bytes(compiler, temp_bytes, 4);
                let short_circuit = compiler.assembler.new_label();
                *temp_bytes = temp_bytes.saturating_sub(4);
                compiler.assembler.push_jump(NcsOpcode::Jz, short_circuit);

                if *op == BinaryOp::LogicalOr {
                    emit_copy_top_bytes(compiler, temp_bytes, 4);
                    let merge = compiler.assembler.new_label();
                    compiler.assembler.push_jump(NcsOpcode::Jmp, merge);
                    compiler.assembler.place_label(short_circuit);
                    *temp_bytes = base_temp_bytes + 4;
                    emit_expr_common(compiler, temp_bytes, layout, right)?;
                    compiler.assembler.place_label(merge);
                } else {
                    emit_expr_common(compiler, temp_bytes, layout, right)?;
                }

                let opcode = opcode_for_binary(*op);
                *temp_bytes = temp_bytes.saturating_sub(8) + 4;
                compiler.assembler.push(NcsInstruction {
                    opcode,
                    auxcode: NcsAuxCode::TypeTypeIntegerInteger,
                    extra: Vec::new(),
                });
                if *op == BinaryOp::LogicalAnd {
                    compiler.assembler.place_label(short_circuit);
                }
                return Ok(());
            }
            emit_expr_common(compiler, temp_bytes, layout, right)?;
            let opcode = opcode_for_binary(*op);
            let aux = aux_for_binary(&left.ty, &right.ty, compiler.hir, &compiler.structs)?;
            let left_size = size_of_type(&left.ty, &compiler.structs)?;
            let right_size = size_of_type(&right.ty, &compiler.structs)?;
            let result_size = size_of_binary_result(*op, &left.ty, &right.ty, &compiler.structs)?;
            *temp_bytes = temp_bytes.saturating_sub(left_size + right_size);
            *temp_bytes += result_size;
            let extra = if aux == NcsAuxCode::TypeTypeStructStruct
                && matches!(opcode, NcsOpcode::Equal | NcsOpcode::NotEqual)
            {
                usize_to_u16(left_size, "struct equality size")?
                    .to_be_bytes()
                    .to_vec()
            } else {
                Vec::new()
            };
            compiler.assembler.push(NcsInstruction {
                opcode,
                auxcode: aux,
                extra,
            });
            Ok(())
        }
        HirExprKind::Conditional {
            condition,
            when_true,
            when_false,
        } => {
            let base_temp_bytes = *temp_bytes;
            emit_expr_common(compiler, temp_bytes, layout, condition)?;
            if *temp_bytes < base_temp_bytes + 4 {
                return Err(CodegenError::new(
                    Some(condition.span),
                    "conditional expression requires an integer condition",
                ));
            }

            let false_label = compiler.assembler.new_label();
            let end_label = compiler.assembler.new_label();
            *temp_bytes -= 4;
            compiler.assembler.push_jump(NcsOpcode::Jz, false_label);

            emit_expr_common(compiler, temp_bytes, layout, when_true)?;
            compiler.assembler.push_jump(NcsOpcode::Jmp, end_label);

            compiler.assembler.place_label(false_label);
            *temp_bytes = base_temp_bytes;
            emit_expr_common(compiler, temp_bytes, layout, when_false)?;
            compiler.assembler.place_label(end_label);
            Ok(())
        }
        HirExprKind::Assignment {
            op,
            left,
            right,
        } => {
            if *op == AssignmentOp::Assign {
                emit_expr_common(compiler, temp_bytes, layout, right)?;
                emit_store_target(compiler, temp_bytes, layout, left, right.span)?;
                return Ok(());
            }

            let binary_op = match op {
                AssignmentOp::Assign => unreachable!(),
                AssignmentOp::AssignMinus => BinaryOp::Subtract,
                AssignmentOp::AssignPlus => BinaryOp::Add,
                AssignmentOp::AssignMultiply => BinaryOp::Multiply,
                AssignmentOp::AssignDivide => BinaryOp::Divide,
                AssignmentOp::AssignModulus => BinaryOp::Modulus,
                AssignmentOp::AssignAnd => BinaryOp::BooleanAnd,
                AssignmentOp::AssignXor => BinaryOp::ExclusiveOr,
                AssignmentOp::AssignOr => BinaryOp::InclusiveOr,
                AssignmentOp::AssignShiftLeft => BinaryOp::ShiftLeft,
                AssignmentOp::AssignShiftRight => BinaryOp::ShiftRight,
                AssignmentOp::AssignUnsignedShiftRight => BinaryOp::UnsignedShiftRight,
            };
            emit_expr_common(compiler, temp_bytes, layout, left)?;
            emit_expr_common(compiler, temp_bytes, layout, right)?;
            let aux = aux_for_binary(&left.ty, &right.ty, compiler.hir, &compiler.structs)?;
            let left_size = size_of_type(&left.ty, &compiler.structs)?;
            let right_size = size_of_type(&right.ty, &compiler.structs)?;
            let result_size =
                size_of_binary_result(binary_op, &left.ty, &right.ty, &compiler.structs)?;
            *temp_bytes = temp_bytes.saturating_sub(left_size + right_size);
            *temp_bytes += result_size;
            compiler.assembler.push(NcsInstruction {
                opcode:  opcode_for_binary(binary_op),
                auxcode: aux,
                extra:   Vec::new(),
            });
            emit_store_target(compiler, temp_bytes, layout, left, expr.span)
        }
    }
}

fn emit_call(
    compiler: &mut O0Compiler<'_>,
    temp_bytes: &mut usize,
    layout: Option<&FunctionLayout>,
    expr: &HirExpr,
    target: &HirCallTarget,
    arguments: &[HirExpr],
) -> Result<(), CodegenError> {
    let base_temp = *temp_bytes;
    match target {
        HirCallTarget::Builtin(name) => {
            let (id, function) =
                compiler
                    .builtin_functions
                    .get(name)
                    .copied()
                    .ok_or_else(|| {
                        CodegenError::new(Some(expr.span), format!("unknown builtin {name:?}"))
                    })?;
            // ACTION handlers pop parameters in declaration order, so the
            // compiler must place the last parameter on the stack first.
            // Action parameters do not occupy a normal stack cell, but they
            // still participate in this right-to-left emission order.
            for (index, parameter) in function.parameters.iter().enumerate().rev() {
                if let Some(argument) = arguments.get(index) {
                    if matches!(parameter.ty, BuiltinType::Action) {
                        emit_action_parameter(compiler, temp_bytes, layout, argument)?;
                    } else {
                        emit_expr_common(compiler, temp_bytes, layout, argument)?;
                    }
                    continue;
                }

                let default = parameter.default.as_ref().ok_or_else(|| {
                    CodegenError::new(
                        Some(expr.span),
                        format!("missing required parameter for builtin {name:?}"),
                    )
                })?;
                if matches!(parameter.ty, BuiltinType::Action) {
                    let action = lower_builtin_action_default_expr(compiler, default, expr.span)?;
                    emit_action_parameter(compiler, temp_bytes, layout, &action)?;
                    continue;
                }
                if matches!(parameter.ty, BuiltinType::Object)
                    && let BuiltinValue::ObjectId(value) = default
                {
                    // The legacy langspec spells its invalid-object default as
                    // the integer OBJECT_TYPE_INVALID (32767), but native
                    // codegen widens that sentinel to INVALID_OBJECT_ID.
                    let object_id = if *value == 32_767 {
                        0x7f00_0000
                    } else {
                        *value
                    };
                    compiler.assembler.push(NcsInstruction {
                        opcode:  NcsOpcode::Constant,
                        auxcode: NcsAuxCode::TypeObject,
                        extra:   object_id.to_be_bytes().to_vec(),
                    });
                    *temp_bytes += 4;
                    continue;
                }
                let literal = literal_from_builtin_value(default).ok_or_else(|| {
                    CodegenError::new(
                        Some(expr.span),
                        format!("unsupported builtin default value for {name:?}"),
                    )
                })?;
                let ty = semantic_type_from_builtin_type(&parameter.ty);
                emit_push_literal(compiler, temp_bytes, &literal, &ty, Some(expr.span))?;
            }

            let return_size = size_of_type(&expr.ty, &compiler.structs)?;
            *temp_bytes = base_temp + return_size;
            compiler.assembler.push(NcsInstruction {
                opcode:  NcsOpcode::ExecuteCommand,
                auxcode: NcsAuxCode::None,
                extra:   builtin_call_extra(
                    id,
                    usize_to_u8(function.parameters.len(), "builtin argc")?,
                ),
            });
            Ok(())
        }
        HirCallTarget::Function(name) => {
            let function = compiler.functions.get(name).copied().ok_or_else(|| {
                CodegenError::new(Some(expr.span), format!("unknown function {name:?}"))
            })?;
            if function.return_type != SemanticType::Void {
                compiler.emit_stack_alloc(&function.return_type)?;
                *temp_bytes += size_of_type(&function.return_type, &compiler.structs)?;
            }
            for (index, parameter) in function.parameters.iter().enumerate().rev() {
                if let Some(argument) = arguments.get(index) {
                    emit_expr_common(compiler, temp_bytes, layout, argument)?;
                } else {
                    let default = parameter.default.as_ref().ok_or_else(|| {
                        CodegenError::new(
                            Some(expr.span),
                            format!("missing required parameter for function {name:?}"),
                        )
                    })?;
                    emit_expr_common(compiler, temp_bytes, layout, default)?;
                }
            }
            let label = compiler.function_labels.get(name).copied().ok_or_else(|| {
                CodegenError::new(
                    Some(expr.span),
                    format!("missing function label for {name:?}"),
                )
            })?;
            compiler.assembler.push_jump(NcsOpcode::Jsr, label);
            let return_size = size_of_type(&function.return_type, &compiler.structs)?;
            *temp_bytes = base_temp + return_size;
            Ok(())
        }
    }
}

fn emit_action_parameter(
    compiler: &mut O0Compiler<'_>,
    temp_bytes: &mut usize,
    layout: Option<&FunctionLayout>,
    argument: &HirExpr,
) -> Result<(), CodegenError> {
    let stack_bytes = layout.map_or(0, function_frame_bytes) + *temp_bytes;

    // Upstream emits STORESTATE with aux byte 0x10 before a JMP over the
    // embedded action body. Our NCS model does not have a dedicated STORESTATE
    // aux variant, so we preserve the raw byte value via the matching enum
    // discriminant.
    compiler.assembler.push(NcsInstruction {
        opcode:  NcsOpcode::StoreState,
        auxcode: NcsAuxCode::TypeEngst0,
        extra:   store_state_extra(
            usize_to_u32(compiler.global_size, "global size")?,
            usize_to_u32(stack_bytes, "stack size")?,
        ),
    });

    let action_end = compiler.assembler.new_label();
    compiler.assembler.push_jump(NcsOpcode::Jmp, action_end);
    emit_expr_common(compiler, temp_bytes, layout, argument)?;
    compiler.assembler.push(simple_instruction(NcsOpcode::Ret));
    compiler.assembler.place_label(action_end);
    Ok(())
}

fn lower_builtin_action_default_expr(
    compiler: &O0Compiler<'_>,
    default: &BuiltinValue,
    span: crate::Span,
) -> Result<HirExpr, CodegenError> {
    let BuiltinValue::Raw(raw) = default else {
        return Err(CodegenError::new(
            Some(span),
            format!("unsupported builtin action default value {default:?}"),
        ));
    };
    let langspec = compiler.langspec.ok_or_else(|| {
        CodegenError::new(
            Some(span),
            "builtin action defaults require an active langspec".to_string(),
        )
    })?;
    let synthetic = format!("void __nwnrs_builtin_action_default__() {{ {raw}; }}");
    let script =
        parse_text(SourceId::new(u32::MAX - 1), &synthetic, Some(langspec)).map_err(|error| {
            CodegenError::new(
                Some(span),
                format!("failed to parse builtin action default {raw:?}: {error}"),
            )
        })?;
    let semantic = analyze_script_with_options(&script, Some(langspec), SemanticOptions::default())
        .map_err(|error| {
            CodegenError::new(
                Some(span),
                format!("failed to analyze builtin action default {raw:?}: {error}"),
            )
        })?;
    let hir = lower_to_hir(&script, &semantic, Some(langspec)).map_err(|error| {
        CodegenError::new(
            Some(span),
            format!("failed to lower builtin action default {raw:?}: {error}"),
        )
    })?;
    let function = hir.functions.first().ok_or_else(|| {
        CodegenError::new(
            Some(span),
            format!("builtin action default {raw:?} did not lower to a function body"),
        )
    })?;
    let body = function.body.as_ref().ok_or_else(|| {
        CodegenError::new(
            Some(span),
            format!("builtin action default {raw:?} lowered without a function body"),
        )
    })?;
    let statement = body.statements.first().ok_or_else(|| {
        CodegenError::new(
            Some(span),
            format!("builtin action default {raw:?} lowered to an empty body"),
        )
    })?;
    match statement {
        HirStmt::Expr(expr) => Ok((*expr.clone()).clone()),
        _ => Err(CodegenError::new(
            Some(span),
            format!("builtin action default {raw:?} must lower to an expression statement"),
        )),
    }
}

fn emit_store_target(
    compiler: &mut O0Compiler<'_>,
    temp_bytes: &mut usize,
    layout: Option<&FunctionLayout>,
    target: &HirExpr,
    span: crate::Span,
) -> Result<(), CodegenError> {
    let resolved = resolve_assignment_target(target, &compiler.structs, Some(span))?;
    match resolved.root {
        AssignmentTargetRoot::Local(local) => {
            let layout = layout.ok_or_else(|| {
                CodegenError::new(Some(span), "local assignment used outside a function")
            })?;
            let slot = layout.locals.get(&local).ok_or_else(|| {
                CodegenError::new(Some(span), format!("unknown local slot {local:?}"))
            })?;
            let offset = usize_to_i32(slot.offset + resolved.offset, "local assignment offset")?
                - usize_to_i32(
                    function_frame_bytes(layout) + *temp_bytes,
                    "local assignment frame size",
                )?;
            compiler.assembler.push(NcsInstruction {
                opcode:  NcsOpcode::Assignment,
                auxcode: NcsAuxCode::TypeVoid,
                extra:   assignment_extra(offset, resolved.size),
            });
            Ok(())
        }
        AssignmentTargetRoot::Global(name) => {
            let slot = compiler
                .global_layout
                .get(name)
                .ok_or_else(|| CodegenError::new(Some(span), format!("unknown global {name:?}")))?;
            let offset = usize_to_i32(slot.offset + resolved.offset, "global assignment offset")?
                - usize_to_i32(compiler.global_size, "global size")?;
            compiler.assembler.push(NcsInstruction {
                opcode:  NcsOpcode::AssignmentBase,
                auxcode: NcsAuxCode::TypeVoid,
                extra:   assignment_extra(offset, resolved.size),
            });
            Ok(())
        }
    }
}

fn emit_increment_target(
    compiler: &mut O0Compiler<'_>,
    stack_bytes: usize,
    layout: Option<&FunctionLayout>,
    target: &HirExpr,
    op: UnaryOp,
    span: crate::Span,
) -> Result<(), CodegenError> {
    let resolved = resolve_assignment_target(target, &compiler.structs, Some(span))?;
    let increment = matches!(op, UnaryOp::PreIncrement | UnaryOp::PostIncrement);
    let (opcode, offset) = match resolved.root {
        AssignmentTargetRoot::Local(local) => {
            let layout = layout.ok_or_else(|| {
                CodegenError::new(Some(span), "local increment used outside a function")
            })?;
            let slot = layout.locals.get(&local).ok_or_else(|| {
                CodegenError::new(Some(span), format!("unknown local slot {local:?}"))
            })?;
            let offset = usize_to_i32(slot.offset + resolved.offset, "local increment offset")?
                - usize_to_i32(
                    function_frame_bytes(layout) + stack_bytes,
                    "local increment frame size",
                )?;
            let opcode = if increment {
                NcsOpcode::Increment
            } else {
                NcsOpcode::Decrement
            };
            (opcode, offset)
        }
        AssignmentTargetRoot::Global(name) => {
            let slot = compiler
                .global_layout
                .get(name)
                .ok_or_else(|| CodegenError::new(Some(span), format!("unknown global {name:?}")))?;
            let offset = usize_to_i32(slot.offset + resolved.offset, "global increment offset")?
                - usize_to_i32(compiler.global_size, "global size")?;
            let opcode = if increment {
                NcsOpcode::IncrementBase
            } else {
                NcsOpcode::DecrementBase
            };
            (opcode, offset)
        }
    };
    compiler.assembler.push(NcsInstruction {
        opcode,
        auxcode: NcsAuxCode::TypeInteger,
        extra: offset.to_be_bytes().to_vec(),
    });
    Ok(())
}

fn emit_push_literal(
    compiler: &mut O0Compiler<'_>,
    temp_bytes: &mut usize,
    literal: &Literal,
    ty: &SemanticType,
    span: Option<crate::Span>,
) -> Result<(), CodegenError> {
    match literal {
        Literal::Integer(value) => compiler.assembler.push(NcsInstruction {
            opcode:  NcsOpcode::Constant,
            auxcode: NcsAuxCode::TypeInteger,
            extra:   value.to_be_bytes().to_vec(),
        }),
        Literal::Float(value) => compiler.assembler.push(NcsInstruction {
            opcode:  NcsOpcode::Constant,
            auxcode: NcsAuxCode::TypeFloat,
            extra:   value.to_bits().to_be_bytes().to_vec(),
        }),
        Literal::String(value) => compiler.assembler.push(NcsInstruction {
            opcode:  NcsOpcode::Constant,
            auxcode: NcsAuxCode::TypeString,
            extra:   string_extra(value)?,
        }),
        Literal::ObjectSelf => compiler.assembler.push(NcsInstruction {
            opcode:  NcsOpcode::Constant,
            auxcode: NcsAuxCode::TypeObject,
            extra:   0_i32.to_be_bytes().to_vec(),
        }),
        Literal::ObjectInvalid => compiler.assembler.push(NcsInstruction {
            opcode:  NcsOpcode::Constant,
            auxcode: NcsAuxCode::TypeObject,
            extra:   1_i32.to_be_bytes().to_vec(),
        }),
        Literal::LocationInvalid => compiler.assembler.push(NcsInstruction {
            opcode:  NcsOpcode::Constant,
            auxcode: NcsAuxCode::TypeEngst2,
            extra:   0_u32.to_be_bytes().to_vec(),
        }),
        Literal::Json(value) => compiler.assembler.push(NcsInstruction {
            opcode:  NcsOpcode::Constant,
            auxcode: NcsAuxCode::TypeEngst7,
            extra:   string_extra_bytes(value.as_bytes())?,
        }),
        Literal::Vector(values) => {
            for value in values {
                compiler.assembler.push(NcsInstruction {
                    opcode:  NcsOpcode::Constant,
                    auxcode: NcsAuxCode::TypeFloat,
                    extra:   value.to_bits().to_be_bytes().to_vec(),
                });
            }
        }
        Literal::Magic(magic) => {
            let resolved = compiler.magic_literal_value(*magic, span);
            return emit_push_literal(compiler, temp_bytes, &resolved, ty, span);
        }
    }

    *temp_bytes += size_of_type(ty, &compiler.structs)?;
    Ok(())
}

fn literal_from_builtin_value(value: &BuiltinValue) -> Option<Literal> {
    match value {
        BuiltinValue::Int(value) | BuiltinValue::ObjectId(value) => Some(Literal::Integer(*value)),
        BuiltinValue::Float(value) => Some(Literal::Float(*value)),
        BuiltinValue::String(value) => Some(Literal::String(value.clone())),
        BuiltinValue::ObjectSelf => Some(Literal::ObjectSelf),
        BuiltinValue::ObjectInvalid => Some(Literal::ObjectInvalid),
        BuiltinValue::LocationInvalid => Some(Literal::LocationInvalid),
        BuiltinValue::Json(value) => Some(Literal::Json(value.clone())),
        BuiltinValue::Vector(value) => Some(Literal::Vector(*value)),
        BuiltinValue::Raw(_) => None,
    }
}

fn semantic_type_from_builtin_type(ty: &BuiltinType) -> SemanticType {
    match ty {
        BuiltinType::Int => SemanticType::Int,
        BuiltinType::Float => SemanticType::Float,
        BuiltinType::String => SemanticType::String,
        BuiltinType::Object => SemanticType::Object,
        BuiltinType::Void => SemanticType::Void,
        BuiltinType::Action => SemanticType::Action,
        BuiltinType::Vector => SemanticType::Vector,
        BuiltinType::EngineStructure(name) => SemanticType::EngineStructure(name.clone()),
    }
}

fn evaluate_case_value(
    expr: &HirExpr,
    constant_env: &BTreeMap<String, ConstValue>,
) -> Result<i32, CodegenError> {
    match evaluate_const_expr(expr, constant_env) {
        Some(ConstValue::Int(value)) => Ok(value),
        Some(ConstValue::String(value)) => Ok(nwscript_string_hash_bytes(value.as_bytes())),
        Some(ConstValue::Float(_)) | None => Err(CodegenError::new(
            Some(expr.span),
            "switch case code generation requires a constant int or string",
        )),
    }
}

fn function_frame_bytes(layout: &FunctionLayout) -> usize {
    layout
        .return_layout
        .as_ref()
        .map_or(0, |layout| layout.size)
        + layout.parameter_size
        + layout.active_locals_size
}

fn size_of_binary_result(
    op: BinaryOp,
    left: &SemanticType,
    right: &SemanticType,
    structs: &BTreeMap<String, &crate::HirStruct>,
) -> Result<usize, CodegenError> {
    let ty = match op {
        BinaryOp::EqualEqual
        | BinaryOp::NotEqual
        | BinaryOp::GreaterEqual
        | BinaryOp::GreaterThan
        | BinaryOp::LessThan
        | BinaryOp::LessEqual
        | BinaryOp::LogicalAnd
        | BinaryOp::LogicalOr
        | BinaryOp::InclusiveOr
        | BinaryOp::ExclusiveOr
        | BinaryOp::BooleanAnd
        | BinaryOp::ShiftLeft
        | BinaryOp::ShiftRight
        | BinaryOp::UnsignedShiftRight
        | BinaryOp::Modulus => SemanticType::Int,
        BinaryOp::Add | BinaryOp::Subtract | BinaryOp::Multiply | BinaryOp::Divide => {
            if left == &SemanticType::Float || right == &SemanticType::Float {
                if left == &SemanticType::Vector || right == &SemanticType::Vector {
                    SemanticType::Vector
                } else {
                    SemanticType::Float
                }
            } else if left == &SemanticType::String {
                SemanticType::String
            } else if left == &SemanticType::Vector {
                SemanticType::Vector
            } else {
                left.clone()
            }
        }
    };
    size_of_type(&ty, structs)
}

fn opcode_for_binary(op: BinaryOp) -> NcsOpcode {
    match op {
        BinaryOp::Multiply => NcsOpcode::Mul,
        BinaryOp::Divide => NcsOpcode::Div,
        BinaryOp::Modulus => NcsOpcode::Modulus,
        BinaryOp::Add => NcsOpcode::Add,
        BinaryOp::Subtract => NcsOpcode::Sub,
        BinaryOp::ShiftLeft => NcsOpcode::ShiftLeft,
        BinaryOp::ShiftRight => NcsOpcode::ShiftRight,
        BinaryOp::UnsignedShiftRight => NcsOpcode::UShiftRight,
        BinaryOp::GreaterEqual => NcsOpcode::Geq,
        BinaryOp::GreaterThan => NcsOpcode::Gt,
        BinaryOp::LessThan => NcsOpcode::Lt,
        BinaryOp::LessEqual => NcsOpcode::Leq,
        BinaryOp::NotEqual => NcsOpcode::NotEqual,
        BinaryOp::EqualEqual => NcsOpcode::Equal,
        BinaryOp::BooleanAnd => NcsOpcode::BooleanAnd,
        BinaryOp::ExclusiveOr => NcsOpcode::ExclusiveOr,
        BinaryOp::InclusiveOr => NcsOpcode::InclusiveOr,
        BinaryOp::LogicalAnd => NcsOpcode::LogicalAnd,
        BinaryOp::LogicalOr => NcsOpcode::LogicalOr,
    }
}

fn aux_for_binary(
    left: &SemanticType,
    right: &SemanticType,
    hir: &HirModule,
    structs: &BTreeMap<String, &crate::HirStruct>,
) -> Result<NcsAuxCode, CodegenError> {
    match (left, right) {
        (SemanticType::Int, SemanticType::Int) => Ok(NcsAuxCode::TypeTypeIntegerInteger),
        (SemanticType::Float, SemanticType::Float) => Ok(NcsAuxCode::TypeTypeFloatFloat),
        (SemanticType::Object, SemanticType::Object) => Ok(NcsAuxCode::TypeTypeObjectObject),
        (SemanticType::String, SemanticType::String) => Ok(NcsAuxCode::TypeTypeStringString),
        (SemanticType::Struct(_), SemanticType::Struct(_)) => Ok(NcsAuxCode::TypeTypeStructStruct),
        (SemanticType::Int, SemanticType::Float) => Ok(NcsAuxCode::TypeTypeIntegerFloat),
        (SemanticType::Float, SemanticType::Int) => Ok(NcsAuxCode::TypeTypeFloatInteger),
        (SemanticType::Vector, SemanticType::Vector) => Ok(NcsAuxCode::TypeTypeVectorVector),
        (SemanticType::Vector, SemanticType::Float) => Ok(NcsAuxCode::TypeTypeVectorFloat),
        (SemanticType::Float, SemanticType::Vector) => Ok(NcsAuxCode::TypeTypeFloatVector),
        (SemanticType::EngineStructure(name), SemanticType::EngineStructure(other))
            if name == other =>
        {
            aux_for_engine_structure(name, hir, structs).and_then(|left_aux| match left_aux {
                NcsAuxCode::TypeEngst0 => Ok(NcsAuxCode::TypeTypeEngst0Engst0),
                NcsAuxCode::TypeEngst1 => Ok(NcsAuxCode::TypeTypeEngst1Engst1),
                NcsAuxCode::TypeEngst2 => Ok(NcsAuxCode::TypeTypeEngst2Engst2),
                NcsAuxCode::TypeEngst3 => Ok(NcsAuxCode::TypeTypeEngst3Engst3),
                NcsAuxCode::TypeEngst4 => Ok(NcsAuxCode::TypeTypeEngst4Engst4),
                NcsAuxCode::TypeEngst5 => Ok(NcsAuxCode::TypeTypeEngst5Engst5),
                NcsAuxCode::TypeEngst6 => Ok(NcsAuxCode::TypeTypeEngst6Engst6),
                NcsAuxCode::TypeEngst7 => Ok(NcsAuxCode::TypeTypeEngst7Engst7),
                NcsAuxCode::TypeEngst8 => Ok(NcsAuxCode::TypeTypeEngst8Engst8),
                NcsAuxCode::TypeEngst9 => Ok(NcsAuxCode::TypeTypeEngst9Engst9),
                _ => Err(CodegenError::new(None, "invalid engine-structure auxcode")),
            })
        }
        _ => Err(CodegenError::new(
            None,
            format!("unsupported binary operand pair for code generation: {left:?} and {right:?}"),
        )),
    }
}

fn aux_for_unary(
    ty: &SemanticType,
    hir: &HirModule,
    structs: &BTreeMap<String, &crate::HirStruct>,
) -> Result<NcsAuxCode, CodegenError> {
    match ty {
        SemanticType::Int => Ok(NcsAuxCode::TypeInteger),
        SemanticType::Float => Ok(NcsAuxCode::TypeFloat),
        SemanticType::String => Ok(NcsAuxCode::TypeString),
        SemanticType::Object => Ok(NcsAuxCode::TypeObject),
        SemanticType::Vector => Ok(NcsAuxCode::TypeTypeVectorVector),
        SemanticType::EngineStructure(name) => aux_for_engine_structure(name, hir, structs),
        SemanticType::Struct(_) => Ok(NcsAuxCode::TypeTypeStructStruct),
        SemanticType::Void | SemanticType::Action => Err(CodegenError::new(
            None,
            format!("unsupported unary operand type {ty:?}"),
        )),
    }
}

fn aux_for_engine_structure(
    name: &str,
    hir: &HirModule,
    _structs: &BTreeMap<String, &crate::HirStruct>,
) -> Result<NcsAuxCode, CodegenError> {
    let index = hir
        .structs
        .iter()
        .position(|structure| structure.name == name)
        .or_else(|| {
            [
                "effect",
                "event",
                "location",
                "talent",
                "itemproperty",
                "sqlquery",
                "cassowary",
                "json",
            ]
            .iter()
            .position(|candidate| *candidate == name)
        })
        .ok_or_else(|| CodegenError::new(None, format!("unknown engine structure {name:?}")))?;

    Ok(match index {
        0 => NcsAuxCode::TypeEngst0,
        1 => NcsAuxCode::TypeEngst1,
        2 => NcsAuxCode::TypeEngst2,
        3 => NcsAuxCode::TypeEngst3,
        4 => NcsAuxCode::TypeEngst4,
        5 => NcsAuxCode::TypeEngst5,
        6 => NcsAuxCode::TypeEngst6,
        7 => NcsAuxCode::TypeEngst7,
        8 => NcsAuxCode::TypeEngst8,
        9 => NcsAuxCode::TypeEngst9,
        _ => {
            return Err(CodegenError::new(
                None,
                format!("engine structure index out of range for {name:?}"),
            ));
        }
    })
}

fn size_of_type(
    ty: &SemanticType,
    structs: &BTreeMap<String, &crate::HirStruct>,
) -> Result<usize, CodegenError> {
    match ty {
        SemanticType::Void | SemanticType::Action => Ok(0),
        SemanticType::Int
        | SemanticType::Float
        | SemanticType::String
        | SemanticType::Object
        | SemanticType::EngineStructure(_) => Ok(4),
        SemanticType::Vector => Ok(12),
        SemanticType::Struct(name) => {
            let structure = structs
                .get(name)
                .ok_or_else(|| CodegenError::new(None, format!("unknown structure {name:?}")))?;
            let mut size = 0usize;
            for field in &structure.fields {
                size += size_of_type(&field.ty, structs)?;
            }
            Ok(size)
        }
    }
}

fn validate_hir_limits(hir: &HirModule, langspec: Option<&LangSpec>) -> Result<(), CodegenError> {
    let builtin_identifiers = langspec.map_or(0, |spec| {
        spec.constants.len() + spec.functions.len() + spec.engine_structures.len()
    });
    let user_identifiers = hir.globals.len()
        + hir
            .structs
            .iter()
            .map(|structure| 1 + structure.fields.len())
            .sum::<usize>()
        + hir
            .functions
            .iter()
            .map(|function| 1 + function.locals.len())
            .sum::<usize>();
    let identifier_count = builtin_identifiers.saturating_add(user_identifiers);
    if identifier_count >= MAX_COMPILER_IDENTIFIERS {
        let span = hir
            .functions
            .last()
            .map(|function| function.span)
            .or_else(|| hir.globals.last().map(|global| global.span));
        return Err(CodegenError::native(
            crate::CompilerErrorCode::IdentifierListFull,
            span,
            format!(
                "compiler identifier table contains {identifier_count} entries; the native limit \
                 is {}",
                MAX_COMPILER_IDENTIFIERS - 1
            ),
        ));
    }

    let structs = hir
        .structs
        .iter()
        .map(|structure| (structure.name.clone(), structure))
        .collect::<BTreeMap<_, _>>();
    let global_bytes = hir.globals.iter().try_fold(0usize, |total, global| {
        Ok::<_, CodegenError>(total.saturating_add(size_of_type(&global.ty, &structs)?))
    })?;
    for function in hir
        .functions
        .iter()
        .filter(|function| function.body.is_some())
    {
        let return_bytes = if function.return_type == SemanticType::Void {
            0
        } else {
            size_of_type(&function.return_type, &structs)?
        };
        let frame_bytes = function
            .locals
            .iter()
            .try_fold(return_bytes, |total, local| {
                Ok::<_, CodegenError>(total.saturating_add(size_of_type(&local.ty, &structs)?))
            })?;
        let runtime_cells = global_bytes.saturating_add(frame_bytes).div_ceil(4);
        if runtime_cells > MAX_COMPILER_RUNTIME_CELLS {
            return Err(CodegenError::native(
                crate::CompilerErrorCode::ScriptTooLarge,
                Some(function.span),
                format!(
                    "function {:?} requires {runtime_cells} runtime cells with globals; the \
                     native compiler limit is {MAX_COMPILER_RUNTIME_CELLS}",
                    function.name
                ),
            ));
        }
    }
    Ok(())
}

fn field_layout(
    base: &SemanticType,
    field: &str,
    structs: &BTreeMap<String, &crate::HirStruct>,
    span: Option<crate::Span>,
) -> Result<FieldLayout, CodegenError> {
    match base {
        SemanticType::Vector => {
            let offset = match field {
                "x" => 0,
                "y" => 4,
                "z" => 8,
                _ => {
                    return Err(CodegenError::new(
                        span,
                        format!("field {field:?} does not exist on vector"),
                    ));
                }
            };
            Ok(FieldLayout {
                ty: SemanticType::Float,
                offset,
                size: 4,
            })
        }
        SemanticType::Struct(name) => {
            let structure = structs
                .get(name)
                .ok_or_else(|| CodegenError::new(span, format!("unknown structure {name:?}")))?;
            let mut offset = 0usize;
            for candidate in &structure.fields {
                let size = size_of_type(&candidate.ty, structs)?;
                if candidate.name == field {
                    return Ok(FieldLayout {
                        ty: candidate.ty.clone(),
                        offset,
                        size,
                    });
                }
                offset += size;
            }
            Err(CodegenError::new(
                span,
                format!("field {field:?} does not exist on structure {name:?}"),
            ))
        }
        _ => Err(CodegenError::new(
            span,
            format!("field access requires a vector or struct base, got {base:?}"),
        )),
    }
}

enum AssignmentTargetRoot<'a> {
    Local(HirLocalId),
    Global(&'a str),
}

struct AssignmentTarget<'a> {
    root:   AssignmentTargetRoot<'a>,
    offset: usize,
    size:   usize,
}

fn resolve_assignment_target<'a>(
    target: &'a HirExpr,
    structs: &BTreeMap<String, &'a crate::HirStruct>,
    span: Option<crate::Span>,
) -> Result<AssignmentTarget<'a>, CodegenError> {
    match &target.kind {
        HirExprKind::Value(crate::HirValueRef::Local(local)) => Ok(AssignmentTarget {
            root:   AssignmentTargetRoot::Local(*local),
            offset: 0,
            size:   size_of_type(&target.ty, structs)?,
        }),
        HirExprKind::Value(
            crate::HirValueRef::Global(name) | crate::HirValueRef::ConstGlobal(name),
        ) => Ok(AssignmentTarget {
            root:   AssignmentTargetRoot::Global(name),
            offset: 0,
            size:   size_of_type(&target.ty, structs)?,
        }),
        HirExprKind::FieldAccess {
            base,
            field,
        } => {
            let mut resolved = resolve_assignment_target(base, structs, span)?;
            let field_layout = field_layout(&base.ty, field, structs, span)?;
            resolved.offset += field_layout.offset;
            resolved.size = field_layout.size;
            Ok(resolved)
        }
        _ => Err(CodegenError::new(
            span,
            "assignment target code generation is not implemented yet",
        )),
    }
}

fn string_extra(value: &ScriptString) -> Result<Vec<u8>, CodegenError> {
    string_extra_bytes(value.as_bytes())
}

fn string_extra_bytes(value: &[u8]) -> Result<Vec<u8>, CodegenError> {
    let length = u16::try_from(value.len()).map_err(|_error| {
        CodegenError::new(None, "string constant exceeds NCS 16-bit length limit")
    })?;
    let mut bytes = Vec::with_capacity(2 + value.len());
    bytes.extend_from_slice(&length.to_be_bytes());
    bytes.extend_from_slice(value);
    Ok(bytes)
}

fn format_magic_date(timestamp: SystemTime) -> String {
    let seconds = timestamp
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs();
    let days = i64::try_from(seconds / 86_400).ok().unwrap_or(i64::MAX);
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}")
}

fn format_magic_time(timestamp: SystemTime) -> String {
    let seconds = timestamp
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs();
    let seconds_of_day = seconds % 86_400;
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
    format!("{hour:02}:{minute:02}:{second:02}")
}

fn civil_from_days(days_since_epoch: i64) -> (i32, u32, u32) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let year = y + i64::from(m <= 2);
    (
        i32::try_from(year).ok().unwrap_or(i32::MAX),
        u32::try_from(m).ok().unwrap_or(1),
        u32::try_from(d).ok().unwrap_or(1),
    )
}

fn assignment_extra(offset: i32, size: usize) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(6);
    bytes.extend_from_slice(&offset.to_be_bytes());
    let size = usize_to_u16(size, "assignment size")
        .ok()
        .unwrap_or(u16::MAX);
    bytes.extend_from_slice(&size.to_be_bytes());
    bytes
}

fn builtin_call_extra(id: u16, argc: u8) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(3);
    bytes.extend_from_slice(&id.to_be_bytes());
    bytes.push(argc);
    bytes
}

fn store_state_extra(global_size: u32, stack_size: u32) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(8);
    bytes.extend_from_slice(&global_size.to_be_bytes());
    bytes.extend_from_slice(&stack_size.to_be_bytes());
    bytes
}

fn emit_copy_top_bytes(compiler: &mut O0Compiler<'_>, temp_bytes: &mut usize, size: usize) {
    compiler.assembler.push(NcsInstruction {
        opcode:  NcsOpcode::RunstackCopy,
        auxcode: NcsAuxCode::TypeVoid,
        extra:   assignment_extra(-i32::try_from(size).ok().unwrap_or(i32::MAX), size),
    });
    *temp_bytes += size;
}

fn build_ndb(
    hir: &HirModule,
    langspec: Option<&LangSpec>,
    source_map: &SourceMap,
    root_id: SourceId,
    output: &CodegenOutput,
) -> Result<Ndb, CodegenError> {
    let mut lines = Vec::new();
    let mut file_order = Vec::new();
    let mut file_indices = BTreeMap::new();

    for line in &output.lines {
        let Some(file) = source_map.get(line.source_id) else {
            continue;
        };
        let file_num = if let Some(file_num) = file_indices.get(&file.id).copied() {
            file_num
        } else {
            let file_num = file_order.len();
            file_order.push(file.id);
            file_indices.insert(file.id, file_num);
            file_num
        };
        lines.push(NdbLine {
            file_num,
            line_num: line.line_num,
            binary_start: debug_binary_offset(
                output
                    .label_offsets
                    .get(&line.start)
                    .copied()
                    .unwrap_or_default(),
            ),
            binary_end: debug_binary_offset(
                output
                    .label_offsets
                    .get(&line.end)
                    .copied()
                    .unwrap_or_default(),
            ),
        });
    }

    if !file_indices.contains_key(&root_id)
        && let Some(root) = source_map.get(root_id)
    {
        file_indices.insert(root_id, file_order.len());
        file_order.push(root.id);
    }

    let files = file_order
        .into_iter()
        .filter_map(|file_id| {
            source_map.get(file_id).map(|file| NdbFile {
                name:    file.name.clone(),
                is_root: file.id == root_id,
            })
        })
        .collect::<Vec<_>>();

    let structs = hir
        .structs
        .iter()
        .map(|structure| {
            Ok::<_, CodegenError>(NdbStruct {
                label:  structure.name.clone(),
                fields: structure
                    .fields
                    .iter()
                    .map(|field| {
                        Ok(NdbStructField {
                            label: field.name.clone(),
                            ty:    debug_type_for_semantic(&field.ty, hir, langspec)?,
                        })
                    })
                    .collect::<Result<Vec<_>, CodegenError>>()?,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let functions = hir
        .functions
        .iter()
        .filter(|function| !function.is_builtin)
        .map(|function| {
            let info = output.functions.get(&function.name).ok_or_else(|| {
                CodegenError::new(
                    Some(function.span),
                    format!("missing debug range for {:?}", function.name),
                )
            })?;
            Ok::<_, CodegenError>(NdbFunction {
                label:        function.name.clone(),
                binary_start: debug_binary_offset(
                    output
                        .label_offsets
                        .get(&info.start)
                        .copied()
                        .unwrap_or_default(),
                ),
                binary_end:   debug_binary_offset(
                    output
                        .label_offsets
                        .get(&info.end)
                        .copied()
                        .unwrap_or_default(),
                ),
                return_type:  debug_type_for_semantic(&function.return_type, hir, langspec)?,
                args:         function
                    .parameters
                    .iter()
                    .map(|parameter| debug_type_for_semantic(&parameter.ty, hir, langspec))
                    .collect::<Result<Vec<_>, _>>()?,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let variables = output
        .variables
        .iter()
        .map(|variable| {
            Ok::<_, CodegenError>(NdbVariable {
                label:        variable.name.clone(),
                ty:           debug_type_for_semantic(&variable.ty, hir, langspec)?,
                binary_start: debug_binary_offset(
                    output
                        .label_offsets
                        .get(&variable.start)
                        .copied()
                        .unwrap_or_default(),
                ),
                binary_end:   variable
                    .end
                    .and_then(|end| output.label_offsets.get(&end).copied())
                    .map(debug_binary_offset)
                    .unwrap_or(u32::MAX),
                stack_loc:    variable.stack_loc,
            })
        })
        .collect::<Result<Vec<_>, CodegenError>>()?;

    Ok(Ndb {
        files,
        structs,
        functions,
        variables,
        lines,
    })
}

fn debug_binary_offset(code_offset: u32) -> u32 {
    code_offset.saturating_add(u32::try_from(NCS_BINARY_HEADER_SIZE).ok().unwrap_or(13))
}

fn debug_type_for_semantic(
    ty: &SemanticType,
    hir: &HirModule,
    langspec: Option<&LangSpec>,
) -> Result<NdbType, CodegenError> {
    Ok(match ty {
        SemanticType::Float => NdbType::Float,
        SemanticType::Int => NdbType::Int,
        SemanticType::Void => NdbType::Void,
        SemanticType::Object => NdbType::Object,
        SemanticType::String => NdbType::String,
        SemanticType::EngineStructure(name) => {
            NdbType::EngineStructure(engine_structure_index(name, langspec)?)
        }
        SemanticType::Struct(name) => {
            let index = hir
                .structs
                .iter()
                .position(|structure| structure.name == *name)
                .ok_or_else(|| {
                    CodegenError::new(None, format!("unknown debug structure {name:?}"))
                })?;
            NdbType::Struct(index)
        }
        SemanticType::Vector | SemanticType::Action => NdbType::Unknown,
    })
}

fn engine_structure_index(name: &str, langspec: Option<&LangSpec>) -> Result<u8, CodegenError> {
    if let Some(langspec) = langspec
        && let Some(index) = langspec
            .engine_structures
            .iter()
            .position(|candidate| candidate.eq_ignore_ascii_case(name))
    {
        return u8::try_from(index).map_err(|_error| {
            CodegenError::new(
                None,
                format!("engine structure index out of range for {name:?}"),
            )
        });
    }

    let fallback = [
        "effect",
        "event",
        "location",
        "talent",
        "itemproperty",
        "sqlquery",
        "cassowary",
        "json",
        "vector",
    ];
    fallback
        .iter()
        .position(|candidate| candidate.eq_ignore_ascii_case(name))
        .and_then(|index| u8::try_from(index).ok())
        .ok_or_else(|| CodegenError::new(None, format!("unknown engine structure {name:?}")))
}

fn simple_instruction(opcode: NcsOpcode) -> NcsInstruction {
    NcsInstruction {
        opcode,
        auxcode: NcsAuxCode::None,
        extra: Vec::new(),
    }
}

fn simple_aux_instruction(opcode: NcsOpcode, auxcode: NcsAuxCode) -> NcsInstruction {
    NcsInstruction {
        opcode,
        auxcode,
        extra: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CompileError, CompileOptions, MAX_COMPILER_IDENTIFIERS, MAX_COMPILER_RUNTIME_CELLS,
        OptimizationFlag, OptimizationFlags, validate_hir_limits,
    };
    use crate::{
        BuiltinConstant, BuiltinFunction, BuiltinParameter, BuiltinType, BuiltinValue, HirBlock,
        HirFunction, HirLocal, HirLocalId, HirLocalKind, HirModule, NcsAuxCode, NcsOpcode,
        SemanticType, SourceId, SourceMap, Span, compile_script, compile_script_with_source_map,
        compile_source_bundle, decode_ncs_instructions, load_source_bundle, parse_text, read_ndb,
    };

    fn empty_hir_with_locals(local_count: usize) -> HirModule {
        let span = Span::new(SourceId::new(0), 0, 0);
        HirModule {
            includes:  Vec::new(),
            structs:   Vec::new(),
            globals:   Vec::new(),
            functions: vec![HirFunction {
                span,
                name: "main".to_string(),
                return_type: SemanticType::Void,
                parameters: Vec::new(),
                locals: (0..local_count)
                    .map(|index| HirLocal {
                        id:   HirLocalId(u32::try_from(index).expect("test local index fits u32")),
                        name: format!("local_{index}"),
                        ty:   SemanticType::Int,
                        kind: HirLocalKind::Local,
                    })
                    .collect(),
                body: Some(HirBlock {
                    span,
                    statements: Vec::new(),
                }),
                is_builtin: false,
            }],
        }
    }

    #[test]
    fn reports_native_script_size_limit() {
        let hir = empty_hir_with_locals(MAX_COMPILER_RUNTIME_CELLS + 1);
        let error = validate_hir_limits(&hir, None).expect_err("oversized frame should fail");
        assert_eq!(error.code, Some(crate::CompilerErrorCode::ScriptTooLarge));
    }

    #[test]
    fn reports_native_identifier_table_limit() {
        let mut langspec = test_langspec();
        let prototype = langspec
            .constants
            .first()
            .cloned()
            .expect("test langspec contains a constant");
        langspec.constants = vec![prototype; MAX_COMPILER_IDENTIFIERS];

        let error = validate_hir_limits(&empty_hir_with_locals(0), Some(&langspec))
            .expect_err("full identifier table should fail");
        assert_eq!(
            error.code,
            Some(crate::CompilerErrorCode::IdentifierListFull)
        );
    }

    #[test]
    fn optimization_passes_are_independently_selectable() {
        let flags = OptimizationFlag::RemoveDeadBranches | OptimizationFlag::MeldInstructions;

        assert!(flags.contains(OptimizationFlag::RemoveDeadBranches));
        assert!(flags.contains(OptimizationFlag::MeldInstructions));
        assert!(!flags.contains(OptimizationFlag::RemoveDeadCode));
        assert_eq!(flags.level(), None);
        assert_eq!(OptimizationFlags::from_bits(flags.bits()), Some(flags));
        assert_eq!(OptimizationFlags::from_bits(0x80), None);

        let mut incremental = OptimizationFlags::O0;
        incremental |= OptimizationFlag::RemoveDeadCode;
        assert_eq!(incremental, OptimizationFlags::O1);
    }

    #[test]
    fn source_bundle_parse_failures_preserve_the_parse_error_type() {
        let mut resolver = crate::InMemoryScriptResolver::new();
        resolver.insert_source("broken", "void main( {");
        let bundle = load_source_bundle(&resolver, "broken", crate::SourceLoadOptions::default())
            .expect("source should load before parsing");

        let error = compile_source_bundle(&bundle, None, CompileOptions::default())
            .expect_err("malformed source should fail parsing");
        assert!(matches!(error, CompileError::Parse(_)));
    }

    fn decode_string_constant(extra: &[u8]) -> String {
        let length_bytes: [u8; 2] = extra
            .get(..2)
            .expect("string constant should include a 16-bit length prefix")
            .try_into()
            .expect("string constant length prefix should be two bytes");
        let length = u16::from_be_bytes(length_bytes) as usize;
        let payload = extra
            .get(2..2 + length)
            .expect("string constant payload should match its encoded length");
        String::from_utf8(payload.to_vec()).expect("string constant should be utf-8")
    }

    fn decode_integer_constant(extra: &[u8]) -> i32 {
        let bytes: [u8; 4] = extra
            .get(..4)
            .expect("integer constant should encode four bytes")
            .try_into()
            .expect("integer constant prefix should be four bytes");
        i32::from_be_bytes(bytes)
    }

    fn test_langspec() -> crate::LangSpec {
        crate::LangSpec {
            engine_num_structures: 3,
            engine_structures:     vec![
                "effect".to_string(),
                "location".to_string(),
                "json".to_string(),
            ],
            constants:             vec![
                BuiltinConstant {
                    name:  "TRUE".to_string(),
                    ty:    BuiltinType::Int,
                    value: BuiltinValue::Int(1),
                },
                BuiltinConstant {
                    name:  "FALSE".to_string(),
                    ty:    BuiltinType::Int,
                    value: BuiltinValue::Int(0),
                },
                BuiltinConstant {
                    name:  "OBJECT_SELF".to_string(),
                    ty:    BuiltinType::Object,
                    value: BuiltinValue::ObjectSelf,
                },
                BuiltinConstant {
                    name:  "OBJECT_INVALID".to_string(),
                    ty:    BuiltinType::Object,
                    value: BuiltinValue::ObjectInvalid,
                },
                BuiltinConstant {
                    name:  "OBJECT_TYPE_CREATURE".to_string(),
                    ty:    BuiltinType::Int,
                    value: BuiltinValue::Int(1),
                },
            ],
            functions:             vec![
                BuiltinFunction {
                    name:        "GetCurrentHitPoints".to_string(),
                    return_type: BuiltinType::Int,
                    parameters:  vec![],
                },
                BuiltinFunction {
                    name:        "GetMaxHitPoints".to_string(),
                    return_type: BuiltinType::Int,
                    parameters:  vec![],
                },
                BuiltinFunction {
                    name:        "CreateObject".to_string(),
                    return_type: BuiltinType::Object,
                    parameters:  vec![
                        BuiltinParameter {
                            name:    "nObjectType".to_string(),
                            ty:      BuiltinType::Int,
                            default: None,
                        },
                        BuiltinParameter {
                            name:    "sTemplate".to_string(),
                            ty:      BuiltinType::String,
                            default: None,
                        },
                        BuiltinParameter {
                            name:    "lLocation".to_string(),
                            ty:      BuiltinType::EngineStructure("location".to_string()),
                            default: None,
                        },
                    ],
                },
                BuiltinFunction {
                    name:        "GetLocation".to_string(),
                    return_type: BuiltinType::EngineStructure("location".to_string()),
                    parameters:  vec![BuiltinParameter {
                        name:    "oTarget".to_string(),
                        ty:      BuiltinType::Object,
                        default: None,
                    }],
                },
                BuiltinFunction {
                    name:        "SetListening".to_string(),
                    return_type: BuiltinType::Void,
                    parameters:  vec![
                        BuiltinParameter {
                            name:    "oTarget".to_string(),
                            ty:      BuiltinType::Object,
                            default: None,
                        },
                        BuiltinParameter {
                            name:    "bValue".to_string(),
                            ty:      BuiltinType::Int,
                            default: None,
                        },
                    ],
                },
                BuiltinFunction {
                    name:        "SetListenPattern".to_string(),
                    return_type: BuiltinType::Void,
                    parameters:  vec![
                        BuiltinParameter {
                            name:    "oTarget".to_string(),
                            ty:      BuiltinType::Object,
                            default: None,
                        },
                        BuiltinParameter {
                            name:    "sPattern".to_string(),
                            ty:      BuiltinType::String,
                            default: None,
                        },
                        BuiltinParameter {
                            name:    "nNumber".to_string(),
                            ty:      BuiltinType::Int,
                            default: None,
                        },
                    ],
                },
                BuiltinFunction {
                    name:        "SpeakString".to_string(),
                    return_type: BuiltinType::Void,
                    parameters:  vec![BuiltinParameter {
                        name:    "sMessage".to_string(),
                        ty:      BuiltinType::String,
                        default: None,
                    }],
                },
                BuiltinFunction {
                    name:        "DelayCommand".to_string(),
                    return_type: BuiltinType::Void,
                    parameters:  vec![
                        BuiltinParameter {
                            name:    "fSeconds".to_string(),
                            ty:      BuiltinType::Float,
                            default: None,
                        },
                        BuiltinParameter {
                            name:    "aAction".to_string(),
                            ty:      BuiltinType::Action,
                            default: Some(BuiltinValue::Raw(
                                "SpeakString(\"default action\")".to_string(),
                            )),
                        },
                    ],
                },
            ],
        }
    }

    #[test]
    fn compiles_conditional_script_to_valid_ncs() -> Result<(), Box<dyn std::error::Error>> {
        let script = parse_text(
            SourceId::new(80),
            r#"
                int StartingConditional() {
                    int nCurHP = GetCurrentHitPoints();
                    int nMaxHP = GetMaxHitPoints();
                    if (nCurHP < (nMaxHP / 4)) {
                        return TRUE;
                    }
                    return FALSE;
                }
            "#,
            Some(&test_langspec()),
        )?;

        let artifacts = compile_script(
            &script,
            Some(&test_langspec()),
            CompileOptions {
                optimizations: OptimizationFlags::O0,
                ..CompileOptions::default()
            },
        )?;
        let instructions = decode_ncs_instructions(&artifacts.ncs)?;

        assert!(!instructions.is_empty());
        assert_eq!(
            instructions.first().map(|instruction| instruction.opcode),
            Some(NcsOpcode::RunstackAdd)
        );
        assert!(
            instructions
                .iter()
                .any(|instruction| instruction.opcode == NcsOpcode::Jsr)
        );
        assert!(
            instructions
                .iter()
                .any(|instruction| instruction.opcode == NcsOpcode::Ret)
        );
        Ok(())
    }

    #[test]
    fn compiles_builtin_action_defaults() -> Result<(), Box<dyn std::error::Error>> {
        let script = parse_text(
            SourceId::new(95),
            r#"
                void main() {
                    DelayCommand(1.0);
                }
            "#,
            Some(&test_langspec()),
        )?;

        let artifacts = compile_script(&script, Some(&test_langspec()), CompileOptions::default())?;
        let instructions = decode_ncs_instructions(&artifacts.ncs)?;

        assert!(
            instructions
                .iter()
                .any(|instruction| instruction.opcode == NcsOpcode::StoreState),
            "builtin action defaults should emit an embedded action body"
        );
        assert!(
            instructions.iter().any(|instruction| {
                instruction.opcode == NcsOpcode::Constant
                    && instruction.auxcode == NcsAuxCode::TypeString
                    && decode_string_constant(&instruction.extra) == "default action"
            }),
            "builtin action default body should be compiled from its raw langspec expression"
        );

        Ok(())
    }

    #[test]
    fn compiles_simple_user_and_builtin_calls_to_valid_ncs()
    -> Result<(), Box<dyn std::error::Error>> {
        let script = parse_text(
            SourceId::new(81),
            r#"
                void SetupListening(object oCheater) {
                    SetListening(oCheater, TRUE);
                    SetListenPattern(oCheater, "1", 1001);
                }
                void main() {
                    object oCheater = CreateObject(OBJECT_TYPE_CREATURE, "x0_cheater", GetLocation(OBJECT_SELF));
                    SetupListening(oCheater);
                }
            "#,
            Some(&test_langspec()),
        )?;

        let artifacts = compile_script(&script, Some(&test_langspec()), CompileOptions::default())?;
        let instructions = decode_ncs_instructions(&artifacts.ncs)?;

        assert!(
            instructions
                .iter()
                .any(|instruction| instruction.opcode == NcsOpcode::ExecuteCommand)
        );
        assert!(
            instructions
                .iter()
                .filter(|instruction| instruction.opcode == NcsOpcode::Jsr)
                .count()
                >= 2
        );
        Ok(())
    }

    #[test]
    fn compiles_user_defined_optional_parameter_defaults() -> Result<(), Box<dyn std::error::Error>>
    {
        let script = parse_text(
            SourceId::new(82),
            r#"
                int AddOne(int nBase = TRUE) {
                    return nBase + 1;
                }
                int StartingConditional() {
                    return AddOne();
                }
            "#,
            Some(&test_langspec()),
        )?;

        let artifacts = compile_script(&script, Some(&test_langspec()), CompileOptions::default())?;
        let instructions = decode_ncs_instructions(&artifacts.ncs)?;

        assert!(
            instructions
                .iter()
                .any(|instruction| instruction.opcode == NcsOpcode::Jsr)
        );
        assert!(
            instructions
                .iter()
                .any(|instruction| instruction.opcode == NcsOpcode::Constant
                    && instruction.auxcode == crate::NcsAuxCode::TypeInteger
                    && instruction.extra == 1_i32.to_be_bytes().to_vec()),
            "default integer argument should be materialized before the user call"
        );
        Ok(())
    }

    #[test]
    fn compiles_calls_using_defaults_from_forward_declarations()
    -> Result<(), Box<dyn std::error::Error>> {
        let script = parse_text(
            SourceId::new(84),
            r#"
                void helper(object oTarget = OBJECT_INVALID);
                void helper(object oTarget) {}
                void main() {
                    helper();
                }
            "#,
            Some(&test_langspec()),
        )?;

        let artifacts = compile_script(&script, Some(&test_langspec()), CompileOptions::default())?;
        let instructions = decode_ncs_instructions(&artifacts.ncs)?;

        assert!(
            instructions
                .iter()
                .any(|instruction| instruction.opcode == NcsOpcode::Jsr)
        );
        Ok(())
    }

    #[test]
    fn non_void_returns_jump_to_a_shared_function_exit() -> Result<(), Box<dyn std::error::Error>> {
        let script = parse_text(
            SourceId::new(85),
            r#"
                int StartingConditional() {
                    if (TRUE) {
                        return TRUE;
                    }
                    return FALSE;
                }
            "#,
            Some(&test_langspec()),
        )?;

        let artifacts = compile_script(&script, Some(&test_langspec()), CompileOptions::default())?;
        let instructions = decode_ncs_instructions(&artifacts.ncs)?;

        assert_eq!(
            instructions
                .iter()
                .filter(|instruction| instruction.opcode == NcsOpcode::Ret)
                .count(),
            2,
            "the module should only emit one loader RET and one shared function-exit RET",
        );
        assert!(
            instructions
                .iter()
                .filter(|instruction| instruction.opcode == NcsOpcode::Jmp)
                .count()
                >= 2,
            "non-void return statements should branch to the shared function exit",
        );
        Ok(())
    }

    #[test]
    fn ndb_includes_upstream_debugger_synthetic_retvals() {
        let source = br#"
            int StartingConditional() {
                return TRUE;
            }
        "#;
        let mut source_map = SourceMap::new();
        let root_id = source_map.add_file("synthetic_retval.nss".to_string(), source.to_vec());
        let script = parse_text(
            root_id,
            std::str::from_utf8(source).expect("utf-8"),
            Some(&test_langspec()),
        )
        .expect("script should parse");

        let artifacts = compile_script_with_source_map(
            &script,
            &source_map,
            root_id,
            Some(&test_langspec()),
            CompileOptions::default(),
        )
        .expect("compile should succeed");
        let ndb = read_ndb(&mut std::io::Cursor::new(
            artifacts.ndb.expect("NDB output should be present"),
        ))
        .expect("NDB should parse");

        assert!(
            ndb.variables
                .iter()
                .any(|variable| variable.label == "#retval"),
            "non-void entrypoint should preserve #retval debug records",
        );
        let loader_retval = ndb
            .variables
            .iter()
            .find(|variable| variable.label == "#retval" && variable.binary_end == u32::MAX)
            .expect("loader #retval debug record should be present");
        assert_eq!(
            loader_retval.binary_start, 15,
            "loader #retval should begin after the loader RunstackAdd instruction",
        );
    }

    #[test]
    fn optimized_compiles_preserve_coherent_ndb_ranges() {
        let source = br#"
            int Unused() { return 9; }
            void main() {
                int nValue = 1;
                if (FALSE) { nValue = 2; }
            }
        "#;
        let mut source_map = SourceMap::new();
        let root_id = source_map.add_file("optimized_debug.nss".to_string(), source.to_vec());
        let script = parse_text(
            root_id,
            std::str::from_utf8(source).expect("utf-8"),
            Some(&test_langspec()),
        )
        .expect("script should parse");

        for optimizations in [
            OptimizationFlags::O1,
            OptimizationFlags::O2,
            OptimizationFlags::O3,
        ] {
            let artifacts = compile_script_with_source_map(
                &script,
                &source_map,
                root_id,
                Some(&test_langspec()),
                CompileOptions {
                    optimizations,
                    ..CompileOptions::default()
                },
            )
            .expect("optimized compile should succeed");
            let ndb = read_ndb(&mut std::io::Cursor::new(
                artifacts
                    .ndb
                    .expect("optimized NDB output should be present"),
            ))
            .expect("optimized NDB should parse");
            let code_size = u32::try_from(artifacts.ncs.len()).expect("test NCS should fit in u32");

            assert!(
                ndb.functions
                    .iter()
                    .any(|function| function.label == "main")
            );
            assert!(
                !ndb.functions
                    .iter()
                    .any(|function| function.label == "Unused")
            );
            assert!(ndb.functions.iter().all(|function| {
                function.binary_start <= function.binary_end && function.binary_end <= code_size
            }));
            assert!(ndb.lines.iter().all(|line| {
                line.binary_start <= line.binary_end && line.binary_end <= code_size
            }));
        }
    }

    #[test]
    fn ndb_tracks_switch_eval_and_block_local_lifetimes() {
        let source = br#"
            void main() {
                switch (TRUE) {
                    case TRUE:
                    {
                        int nLocal = 1;
                        break;
                    }
                    default:
                        break;
                }
            }
        "#;
        let mut source_map = SourceMap::new();
        let root_id = source_map.add_file("switch_debug.nss".to_string(), source.to_vec());
        let script = parse_text(
            root_id,
            std::str::from_utf8(source).expect("utf-8"),
            Some(&test_langspec()),
        )
        .expect("script should parse");

        let artifacts = compile_script_with_source_map(
            &script,
            &source_map,
            root_id,
            Some(&test_langspec()),
            CompileOptions::default(),
        )
        .expect("compile should succeed");
        let ndb = read_ndb(&mut std::io::Cursor::new(
            artifacts.ndb.expect("NDB output should be present"),
        ))
        .expect("NDB should parse");

        let switch_eval = ndb
            .variables
            .iter()
            .find(|variable| variable.label == "#switcheval")
            .expect("switch debug variable should be present");
        let local = ndb
            .variables
            .iter()
            .find(|variable| variable.label == "nLocal")
            .expect("block local should be present");

        assert!(
            switch_eval.binary_end >= local.binary_end,
            "switch eval lifetime should cover the switch body",
        );
        assert!(
            local.binary_start > switch_eval.binary_start,
            "block local should start after switch evaluation begins",
        );
    }

    #[test]
    fn ndb_global_debug_starts_after_global_allocations() {
        let source = br#"
            int FIRST = TRUE;
            int SECOND = FALSE;
            void main() {}
        "#;
        let mut source_map = SourceMap::new();
        let root_id = source_map.add_file("globals_debug.nss".to_string(), source.to_vec());
        let script = parse_text(
            root_id,
            std::str::from_utf8(source).expect("utf-8"),
            Some(&test_langspec()),
        )
        .expect("script should parse");

        let artifacts = compile_script_with_source_map(
            &script,
            &source_map,
            root_id,
            Some(&test_langspec()),
            CompileOptions::default(),
        )
        .expect("compile should succeed");
        let ndb = read_ndb(&mut std::io::Cursor::new(
            artifacts.ndb.expect("NDB output should be present"),
        ))
        .expect("NDB should parse");

        let first = ndb
            .variables
            .iter()
            .find(|variable| variable.label == "FIRST")
            .expect("FIRST global should be present");
        let second = ndb
            .variables
            .iter()
            .find(|variable| variable.label == "SECOND")
            .expect("SECOND global should be present");

        assert_eq!(
            first.binary_start, 23,
            "first global should begin after loader + first RunstackAdd"
        );
        assert_eq!(
            second.binary_start, 45,
            "second global should begin after the first initializer + second RunstackAdd"
        );
        assert_eq!(first.stack_loc, 0);
        assert_eq!(second.stack_loc, 4);
    }

    #[test]
    fn compiles_switch_cases_backed_by_const_globals() -> Result<(), Box<dyn std::error::Error>> {
        let script = parse_text(
            SourceId::new(84),
            r#"
                const int CASE_A = 1 + 2;
                int StartingConditional() {
                    int nValue = 3;
                    switch (nValue) {
                        case CASE_A:
                            return TRUE;
                        default:
                            return FALSE;
                    }
                    return FALSE;
                }
            "#,
            Some(&test_langspec()),
        )?;

        let artifacts = compile_script(&script, Some(&test_langspec()), CompileOptions::default())?;
        let instructions = decode_ncs_instructions(&artifacts.ncs)?;

        assert!(
            instructions
                .iter()
                .any(|instruction| instruction.opcode == NcsOpcode::Equal),
            "switch codegen should materialize a case comparison",
        );
        assert!(
            instructions
                .iter()
                .any(|instruction| instruction.opcode == NcsOpcode::Jmp),
            "switch codegen should branch into case bodies",
        );
        Ok(())
    }

    #[test]
    fn compiles_magic_literals_with_source_context() -> Result<(), Box<dyn std::error::Error>> {
        let source = br#"void main() {
    string sFunction = __FUNCTION__;
    string sFile = __FILE__;
    int nLine = __LINE__;
}
"#;
        let mut source_map = SourceMap::new();
        let root_id = source_map.add_file("magic_literals.nss".to_string(), source.to_vec());
        let script = parse_text(
            root_id,
            std::str::from_utf8(source).expect("utf-8"),
            Some(&test_langspec()),
        )?;

        let artifacts = compile_script_with_source_map(
            &script,
            &source_map,
            root_id,
            Some(&test_langspec()),
            CompileOptions::default(),
        )?;
        let instructions = decode_ncs_instructions(&artifacts.ncs)?;

        let string_constants = instructions
            .iter()
            .filter(|instruction| {
                instruction.opcode == NcsOpcode::Constant
                    && instruction.auxcode == crate::NcsAuxCode::TypeString
            })
            .map(|instruction| decode_string_constant(&instruction.extra))
            .collect::<Vec<_>>();
        let integer_constants = instructions
            .iter()
            .filter(|instruction| {
                instruction.opcode == NcsOpcode::Constant
                    && instruction.auxcode == crate::NcsAuxCode::TypeInteger
            })
            .map(|instruction| decode_integer_constant(&instruction.extra))
            .collect::<Vec<_>>();

        assert!(string_constants.iter().any(|value| value == "main"));
        assert!(
            string_constants
                .iter()
                .any(|value| value == "magic_literals.nss")
        );
        assert!(integer_constants.contains(&4));
        Ok(())
    }

    #[test]
    fn file_magic_adds_nss_to_extensionless_include_names() -> Result<(), Box<dyn std::error::Error>>
    {
        let source = br#"void Included() { string file = __FILE__; } void main() {}"#;
        let mut source_map = SourceMap::new();
        let root_id = source_map.add_file("inc_helpers".to_string(), source.to_vec());
        let script = parse_text(
            root_id,
            std::str::from_utf8(source)?,
            Some(&test_langspec()),
        )?;
        let artifacts = compile_script_with_source_map(
            &script,
            &source_map,
            root_id,
            Some(&test_langspec()),
            CompileOptions::default(),
        )?;
        let instructions = decode_ncs_instructions(&artifacts.ncs)?;

        assert!(instructions.iter().any(|instruction| {
            instruction.opcode == NcsOpcode::Constant
                && instruction.auxcode == NcsAuxCode::TypeString
                && decode_string_constant(&instruction.extra) == "inc_helpers.nss"
        }));
        Ok(())
    }

    #[test]
    fn compiles_magic_literals_without_source_map() -> Result<(), Box<dyn std::error::Error>> {
        let script = parse_text(
            SourceId::new(86),
            r#"
                void main() {
                    string sFunction = __FUNCTION__;
                    string sFile = __FILE__;
                    int nLine = __LINE__;
                }
            "#,
            Some(&test_langspec()),
        )?;

        let artifacts = compile_script(&script, Some(&test_langspec()), CompileOptions::default())?;
        let instructions = decode_ncs_instructions(&artifacts.ncs)?;

        let string_constants = instructions
            .iter()
            .filter(|instruction| {
                instruction.opcode == NcsOpcode::Constant
                    && instruction.auxcode == crate::NcsAuxCode::TypeString
            })
            .map(|instruction| decode_string_constant(&instruction.extra))
            .collect::<Vec<_>>();
        let integer_constants = instructions
            .iter()
            .filter(|instruction| {
                instruction.opcode == NcsOpcode::Constant
                    && instruction.auxcode == crate::NcsAuxCode::TypeInteger
            })
            .map(|instruction| decode_integer_constant(&instruction.extra))
            .collect::<Vec<_>>();

        assert!(string_constants.iter().any(|value| value == "main"));
        assert!(string_constants.iter().any(|value| value.is_empty()));
        assert!(integer_constants.contains(&0));
        Ok(())
    }

    #[test]
    fn compiles_date_and_time_magic_literals() -> Result<(), Box<dyn std::error::Error>> {
        let script = parse_text(
            SourceId::new(87),
            r#"
                void main() {
                    string sDate = __DATE__;
                    string sTime = __TIME__;
                }
            "#,
            Some(&test_langspec()),
        )?;

        let artifacts = compile_script(&script, Some(&test_langspec()), CompileOptions::default())?;
        let instructions = decode_ncs_instructions(&artifacts.ncs)?;
        let string_constants = instructions
            .iter()
            .filter(|instruction| {
                instruction.opcode == NcsOpcode::Constant
                    && instruction.auxcode == crate::NcsAuxCode::TypeString
            })
            .map(|instruction| decode_string_constant(&instruction.extra))
            .collect::<Vec<_>>();

        assert!(
            string_constants.iter().any(|value| {
                let bytes = value.as_bytes();
                value.len() == 10
                    && bytes.get(4) == Some(&b'-')
                    && bytes.get(7) == Some(&b'-')
                    && value
                        .chars()
                        .enumerate()
                        .all(|(index, ch)| matches!(index, 4 | 7) || ch.is_ascii_digit())
            }),
            "__DATE__ should compile into an upstream-compatible YYYY-MM-DD string",
        );
        assert!(
            string_constants.iter().any(|value| {
                let bytes = value.as_bytes();
                value.len() == 8
                    && bytes.get(2) == Some(&b':')
                    && bytes.get(5) == Some(&b':')
                    && value
                        .chars()
                        .enumerate()
                        .all(|(index, ch)| matches!(index, 2 | 5) || ch.is_ascii_digit())
            }),
            "__TIME__ should compile into a macro-style time string",
        );
        Ok(())
    }

    #[test]
    fn compiles_conditional_expressions() -> Result<(), Box<dyn std::error::Error>> {
        let script = parse_text(
            SourceId::new(88),
            r#"
                int StartingConditional() {
                    int nCurHP = GetCurrentHitPoints();
                    return nCurHP > 0 ? TRUE : FALSE;
                }
            "#,
            Some(&test_langspec()),
        )?;

        let artifacts = compile_script(&script, Some(&test_langspec()), CompileOptions::default())?;
        let instructions = decode_ncs_instructions(&artifacts.ncs)?;

        assert!(
            instructions
                .iter()
                .any(|instruction| instruction.opcode == NcsOpcode::Jz),
            "conditional expression should branch on the computed condition",
        );
        assert!(
            instructions
                .iter()
                .any(|instruction| instruction.opcode == NcsOpcode::Jmp),
            "conditional expression should merge control flow after one arm executes",
        );
        Ok(())
    }

    #[test]
    fn compiles_nested_field_assignments() -> Result<(), Box<dyn std::error::Error>> {
        let script = parse_text(
            SourceId::new(89),
            r#"
                struct Inner { int value; };
                struct Outer { struct Inner inner; };
                void main() {
                    struct Outer outer;
                    outer.inner.value = 1;
                }
            "#,
            Some(&test_langspec()),
        )?;

        let artifacts = compile_script(&script, Some(&test_langspec()), CompileOptions::default())?;
        let instructions = decode_ncs_instructions(&artifacts.ncs)?;

        assert!(
            instructions
                .iter()
                .any(|instruction| instruction.opcode == NcsOpcode::Assignment),
            "nested field assignment should lower to an assignment opcode",
        );
        Ok(())
    }

    #[test]
    fn emits_native_short_circuit_and_switch_branches() -> Result<(), Box<dyn std::error::Error>> {
        let script = parse_text(
            SourceId::new(90),
            r#"
                int StartingConditional() {
                    int n = GetCurrentHitPoints();
                    switch (n) {
                        case 1:
                            return n == 1 || GetMaxHitPoints() > 0;
                        default:
                            return FALSE;
                    }
                    return FALSE;
                }
            "#,
            Some(&test_langspec()),
        )?;
        let artifacts = compile_script(&script, Some(&test_langspec()), CompileOptions::default())?;
        let instructions = decode_ncs_instructions(&artifacts.ncs)?;

        assert!(instructions.iter().any(|instruction| {
            instruction.opcode == NcsOpcode::LogicalOr
                && instruction.auxcode == NcsAuxCode::TypeTypeIntegerInteger
        }));
        assert!(
            instructions
                .iter()
                .any(|instruction| instruction.opcode == NcsOpcode::Jnz),
            "switch cases should branch directly with JNZ",
        );
        Ok(())
    }

    #[test]
    fn emits_native_direct_increment_opcodes() -> Result<(), Box<dyn std::error::Error>> {
        let script = parse_text(
            SourceId::new(91),
            "void main() { int n = 0; n++; ++n; }",
            Some(&test_langspec()),
        )?;
        let artifacts = compile_script(&script, Some(&test_langspec()), CompileOptions::default())?;
        let instructions = decode_ncs_instructions(&artifacts.ncs)?;

        assert_eq!(
            instructions
                .iter()
                .filter(|instruction| instruction.opcode == NcsOpcode::Increment)
                .count(),
            2,
        );
        let increment_offsets = instructions
            .iter()
            .filter(|instruction| instruction.opcode == NcsOpcode::Increment)
            .map(|instruction| {
                i32::from_be_bytes(
                    instruction
                        .extra
                        .get(..4)
                        .expect("increment offset should be four bytes")
                        .try_into()
                        .expect("increment offset should be four bytes"),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(increment_offsets, vec![-8, -4]);
        assert!(!instructions.iter().any(|instruction| {
            instruction.opcode == NcsOpcode::Add
                && instruction.auxcode == NcsAuxCode::TypeTypeIntegerInteger
        }));
        Ok(())
    }

    #[test]
    fn const_globals_do_not_create_a_runtime_global_frame() -> Result<(), Box<dyn std::error::Error>>
    {
        let script = parse_text(
            SourceId::new(92),
            "const int VALUE = 7; int StartingConditional() { return VALUE; }",
            Some(&test_langspec()),
        )?;
        let artifacts = compile_script(&script, Some(&test_langspec()), CompileOptions::default())?;
        let instructions = decode_ncs_instructions(&artifacts.ncs)?;

        assert!(
            !instructions
                .iter()
                .any(|instruction| instruction.opcode == NcsOpcode::SaveBasePointer),
        );
        assert_eq!(
            instructions
                .iter()
                .filter(|instruction| {
                    instruction.opcode == NcsOpcode::RunstackAdd
                        && instruction.auxcode == NcsAuxCode::TypeInteger
                })
                .count(),
            1,
            "only the loader conditional return slot should be allocated",
        );
        assert!(instructions.iter().any(|instruction| {
            instruction.opcode == NcsOpcode::Constant
                && instruction.auxcode == NcsAuxCode::TypeInteger
                && decode_integer_constant(&instruction.extra) == 7
        }));
        Ok(())
    }

    #[test]
    fn top_level_struct_retains_native_global_wrapper() -> Result<(), Box<dyn std::error::Error>> {
        let script = parse_text(
            SourceId::new(93),
            "struct Pair { int value; }; void main() {}",
            Some(&test_langspec()),
        )?;
        let artifacts = compile_script(&script, Some(&test_langspec()), CompileOptions::default())?;
        let instructions = decode_ncs_instructions(&artifacts.ncs)?;

        assert!(
            instructions
                .iter()
                .any(|instruction| instruction.opcode == NcsOpcode::SaveBasePointer),
        );
        Ok(())
    }
}
