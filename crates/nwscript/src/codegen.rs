use std::{collections::BTreeMap, error::Error, fmt};

use serde::{Deserialize, Serialize};

use crate::{
    AssignmentOp, BinaryOp, BuiltinFunction, BuiltinType, BuiltinValue, HirBlock, HirCallTarget,
    HirExpr, HirExprKind, HirFunction, HirLocalId, HirLocalKind, HirModule, HirStmt, LangSpec,
    Literal, NCS_OPERATION_BASE_SIZE, NcsAuxCode, NcsInstruction, NcsOpcode, Ndb, NdbFile,
    NdbFunction, NdbLine, NdbStruct, NdbStructField, NdbType, NdbVariable, Script, SemanticOptions,
    SemanticType, SourceBundle, SourceId, SourceMap, UnaryOp, analyze_script_with_options,
    encode_ncs_instructions, lower_to_hir, nwscript_string_hash,
    opt::{
        ConstValue, build_constant_env, evaluate_const_expr, meld_instructions,
        optimization_needs_hir_passes, optimization_needs_post_codegen_passes, optimize_hir,
    },
    parse_source_bundle, write_ndb,
};

/// Optimization levels accepted by the pure-Rust compiler pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum OptimizationLevel {
    /// Unoptimized code generation.
    #[default]
    O0,
    /// Placeholder for future optimization work.
    O1,
    /// Placeholder for future optimization work.
    O2,
    /// Placeholder for future optimization work.
    O3,
}

/// One compilation request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CompileOptions {
    /// Entry-point validation policy forwarded to semantic analysis.
    pub semantic:     SemanticOptions,
    /// Optimization level for code generation.
    pub optimization: OptimizationLevel,
}

impl Default for CompileOptions {
    fn default() -> Self {
        Self {
            semantic:     SemanticOptions::default(),
            optimization: OptimizationLevel::O0,
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
    /// Optional source span associated with the failure.
    pub span:    Option<crate::Span>,
    /// Human-readable error text.
    pub message: String,
}

impl CodegenError {
    fn new(span: Option<crate::Span>, message: impl Into<String>) -> Self {
        Self {
            span,
            message: message.into(),
        }
    }
}

impl fmt::Display for CodegenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
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
            Self::Semantic(error) => error.fmt(f),
            Self::Hir(error) => error.fmt(f),
            Self::Codegen(error) => error.fmt(f),
        }
    }
}

impl Error for CompileError {}

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
pub fn compile_script(
    script: &Script,
    langspec: Option<&LangSpec>,
    options: CompileOptions,
) -> Result<CompileArtifacts, CompileError> {
    compile_script_with_debug(script, None, None, langspec, options)
}

/// Compiles one parsed script and emits `NDB` when a source map is available.
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
pub fn compile_source_bundle(
    bundle: &SourceBundle,
    langspec: Option<&LangSpec>,
    options: CompileOptions,
) -> Result<CompileArtifacts, CompileError> {
    let script = parse_source_bundle(bundle, langspec).map_err(|error| {
        CompileError::Codegen(CodegenError::new(
            None,
            format!("failed to parse source bundle during compile: {error}"),
        ))
    })?;
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
    if optimization_not_o0(options.optimization) {
        let ncs = compile_hir_to_ncs(&hir, langspec, options.optimization)?;
        return Ok(CompileArtifacts {
            ncs,
            ndb: None,
        });
    }

    let output = O0Compiler::new(&hir, langspec, source_map)?.compile()?;
    let ncs = encode_ncs_instructions(&output.instructions);
    let ndb = match (source_map, root_id) {
        (Some(source_map), Some(root_id)) => {
            let ndb = build_ndb(&hir, langspec, source_map, root_id, &output)?;
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
pub fn compile_hir_to_ncs(
    hir: &HirModule,
    langspec: Option<&LangSpec>,
    optimization: OptimizationLevel,
) -> Result<Vec<u8>, CodegenError> {
    let optimized_hir = if optimization_needs_hir_passes(optimization) {
        optimize_hir(hir, langspec, optimization)
    } else {
        hir.clone()
    };

    let mut instructions = O0Compiler::new(&optimized_hir, langspec, None)?
        .compile()?
        .instructions;
    if optimization_needs_post_codegen_passes(optimization) {
        instructions = meld_instructions(instructions);
    }
    Ok(encode_ncs_instructions(&instructions))
}

fn optimization_not_o0(optimization: OptimizationLevel) -> bool {
    optimization != OptimizationLevel::O0
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
                        CodegenError::new(None, format!("unresolved code label {:?}", target))
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
    hir:                  &'a HirModule,
    builtin_functions:    BTreeMap<String, (u16, &'a BuiltinFunction)>,
    builtin_constants:    BTreeMap<String, BuiltinValue>,
    constant_env:         BTreeMap<String, ConstValue>,
    structs:              BTreeMap<String, &'a crate::HirStruct>,
    functions:            BTreeMap<String, &'a HirFunction>,
    entry_function:       Option<&'a HirFunction>,
    global_layout:        BTreeMap<String, ValueLayout>,
    global_size:          usize,
    function_labels:      BTreeMap<String, LabelId>,
    function_exit_labels: BTreeMap<String, LabelId>,
    function_end_labels:  BTreeMap<String, LabelId>,
    globals_label:        Option<LabelId>,
    globals_end_label:    Option<LabelId>,
    variable_debug:       Vec<VariableDebugInfo>,
    line_debug:           LineDebugTracker,
    source_map:           Option<&'a SourceMap>,
    assembler:            Assembler,
}

#[derive(Clone)]
struct ValueLayout {
    ty:     SemanticType,
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
    return_layout: Option<ValueLayout>,
    locals:        BTreeMap<HirLocalId, ValueLayout>,
    locals_size:   usize,
}

struct FunctionEmitter<'a, 'b> {
    compiler:         &'b mut O0Compiler<'a>,
    function:         &'a HirFunction,
    layout:           FunctionLayout,
    temp_bytes:       usize,
    break_targets:    Vec<LabelId>,
    continue_targets: Vec<LabelId>,
    scope_stack:      Vec<Vec<usize>>,
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
        for global in &hir.globals {
            let size = size_of_type(&global.ty, &structs)?;
            global_layout.insert(
                global.name.clone(),
                ValueLayout {
                    ty: global.ty.clone(),
                    offset: global_size,
                    size,
                },
            );
            global_size += size;
        }

        let mut assembler = Assembler::default();
        let globals_label = (!hir.globals.is_empty()).then(|| assembler.new_label());
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
            builtin_functions,
            builtin_constants,
            constant_env,
            structs,
            functions,
            entry_function,
            global_layout,
            global_size,
            function_labels,
            function_exit_labels,
            function_end_labels,
            globals_label,
            globals_end_label,
            variable_debug: Vec::new(),
            line_debug: LineDebugTracker::default(),
            source_map,
            assembler,
        })
    }

    fn compile(mut self) -> Result<CodegenOutput, CodegenError> {
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

        let assembly = self.assembler.finalize()?;
        Ok(CodegenOutput {
            instructions:  assembly.instructions,
            label_offsets: assembly.offsets,
            functions:     self
                .function_labels
                .iter()
                .map(|(name, start)| {
                    let end = self.function_end_labels.get(name).copied().ok_or_else(|| {
                        CodegenError::new(
                            None,
                            format!("missing function end label for {:?}", name),
                        )
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
        if self.globals_label.is_some() {
            for global in &self.hir.globals {
                self.emit_stack_alloc(&global.ty)?;
                let start = self.assembler.new_label();
                self.assembler.place_label(start);
                let layout = self.global_layout.get(&global.name).ok_or_else(|| {
                    CodegenError::new(
                        Some(global.span),
                        format!("unknown global {:?}", global.name),
                    )
                })?;
                self.variable_debug.push(VariableDebugInfo {
                    name: global.name.clone(),
                    ty: global.ty.clone(),
                    start,
                    end: None,
                    stack_loc: usize_to_u32(layout.offset, "global stack location")?,
                });
            }
        }
        self.assembler
            .push(simple_instruction(NcsOpcode::SaveBasePointer));

        let mut emitter = GlobalEmitter {
            compiler:   self,
            temp_bytes: 0,
        };
        for global in &emitter.compiler.hir.globals {
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
        }

        emitter
            .compiler
            .assembler
            .push(simple_instruction(NcsOpcode::RestoreBasePointer));
        emitter
            .compiler
            .assembler
            .push(simple_instruction(NcsOpcode::Ret));
        Ok(())
    }

    fn emit_function(&mut self, function: &'a HirFunction) -> Result<(), CodegenError> {
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
            if function.return_type == SemanticType::Void {
                emitter.emit_function_epilogue();
            }
            emitter
                .compiler
                .assembler
                .push(simple_instruction(NcsOpcode::Ret));
            let final_line_end = emitter.compiler.assembler.new_label();
            emitter.compiler.assembler.place_label(final_line_end);
            emitter.compiler.end_line_end_at(body.span, final_line_end);
        } else if function.return_type == SemanticType::Void {
            emitter.compiler.assembler.place_label(exit);
            emitter.emit_function_epilogue();
            emitter
                .compiler
                .assembler
                .push(simple_instruction(NcsOpcode::Ret));
        } else {
            emitter.compiler.assembler.place_label(exit);
            emitter
                .compiler
                .assembler
                .push(simple_instruction(NcsOpcode::Ret));
        }
        Ok(())
    }

    fn function_layout(&self, function: &HirFunction) -> Result<FunctionLayout, CodegenError> {
        let mut offset = 0usize;
        let return_layout = if function.return_type != SemanticType::Void {
            let size = size_of_type(&function.return_type, &self.structs)?;
            let layout = ValueLayout {
                ty: function.return_type.clone(),
                offset,
                size,
            };
            offset += size;
            Some(layout)
        } else {
            None
        };

        let mut locals = BTreeMap::new();
        for parameter in &function.parameters {
            let size = size_of_type(&parameter.ty, &self.structs)?;
            locals.insert(
                parameter.local,
                ValueLayout {
                    ty: parameter.ty.clone(),
                    offset,
                    size,
                },
            );
            offset += size;
        }

        let frame_prefix = offset;
        for local in &function.locals {
            if local.kind != HirLocalKind::Local {
                continue;
            }
            let size = size_of_type(&local.ty, &self.structs)?;
            locals.insert(
                local.id,
                ValueLayout {
                    ty: local.ty.clone(),
                    offset,
                    size,
                },
            );
            offset += size;
        }

        Ok(FunctionLayout {
            return_layout,
            locals,
            locals_size: offset - frame_prefix,
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
                    CodegenError::new(None, format!("unknown structure {:?}", name))
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
}

struct GlobalEmitter<'a, 'b> {
    compiler:   &'b mut O0Compiler<'a>,
    temp_bytes: usize,
}

impl<'a> GlobalEmitter<'a, '_> {
    fn emit_expr(&mut self, expr: &HirExpr) -> Result<(), CodegenError> {
        emit_expr_common(self.compiler, &mut self.temp_bytes, None, expr)
    }

    fn emit_store_global(&mut self, name: &str, span: crate::Span) -> Result<(), CodegenError> {
        let layout =
            self.compiler.global_layout.get(name).ok_or_else(|| {
                CodegenError::new(Some(span), format!("unknown global {:?}", name))
            })?;
        let offset = usize_to_i32(layout.offset, "global offset")?
            - usize_to_i32(self.compiler.global_size, "global size")?;
        self.compiler.assembler.push(NcsInstruction {
            opcode:  NcsOpcode::AssignmentBase,
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

impl<'a> FunctionEmitter<'a, '_> {
    fn emit_prologue(&mut self) -> Result<(), CodegenError> {
        for local in &self.function.locals {
            if local.kind == HirLocalKind::Local {
                self.compiler.emit_stack_alloc(&local.ty)?;
            }
        }
        Ok(())
    }

    fn emit_block(&mut self, block: &HirBlock) -> Result<(), CodegenError> {
        self.scope_stack.push(Vec::new());
        for statement in &block.statements {
            self.emit_stmt(statement)?;
        }
        self.close_scope_variables();
        Ok(())
    }

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
                    let local = self.local_info(declarator.local, statement.span)?;
                    let end_index = self.compiler.variable_debug.len();
                    self.compiler.variable_debug.push(VariableDebugInfo {
                        name: local.name.clone(),
                        ty: local.ty.clone(),
                        start,
                        end: None,
                        stack_loc: self.local_stack_loc(declarator.local, statement.span)?,
                    });
                    self.current_scope_variables().push(end_index);
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
                self.emit_function_epilogue();
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
                self.break_targets.push(end_label);
                self.continue_targets.push(cond_label);
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
                self.break_targets.push(end_label);
                self.continue_targets.push(cond_label);
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
                self.break_targets.push(end_label);
                self.continue_targets.push(update_label);
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
                self.compiler.assembler.push_jump(NcsOpcode::Jmp, target);
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
                self.compiler.assembler.push_jump(NcsOpcode::Jmp, target);
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

    fn close_scope_variables(&mut self) {
        let Some(indices) = self.scope_stack.pop() else {
            return;
        };
        if indices.is_empty() {
            return;
        }
        let end = self.compiler.assembler.new_label();
        self.compiler.assembler.place_label(end);
        for index in indices {
            if let Some(variable) = self.compiler.variable_debug.get_mut(index) {
                variable.end = Some(end);
            }
        }
    }

    fn current_scope_variables(&mut self) -> &mut Vec<usize> {
        self.scope_stack
            .last_mut()
            .unwrap_or_else(|| unreachable!("function blocks should always have an active scope"))
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
            .ok_or_else(|| CodegenError::new(Some(span), format!("unknown local {:?}", local_id)))
    }

    fn local_stack_loc(
        &self,
        local_id: HirLocalId,
        span: crate::Span,
    ) -> Result<u32, CodegenError> {
        let slot = self.layout.locals.get(&local_id).ok_or_else(|| {
            CodegenError::new(Some(span), format!("unknown local slot {:?}", local_id))
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
            let after_compare = self.compiler.assembler.new_label();
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
            self.emit_branch_zero(after_compare)?;
            self.compiler.assembler.push_jump(NcsOpcode::Jmp, *label);
            self.compiler.assembler.place_label(after_compare);
        }
        if let Some((_, label)) = default_label {
            self.compiler.assembler.push_jump(NcsOpcode::Jmp, label);
        } else {
            self.compiler.assembler.push_jump(NcsOpcode::Jmp, body_end);
        }

        self.break_targets.push(body_end);
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
            CodegenError::new(Some(span), format!("unknown local slot {:?}", local))
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
        self.compiler.assembler.push(NcsInstruction {
            opcode,
            auxcode,
            extra: Vec::new(),
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

    fn emit_function_epilogue(&mut self) {
        let cleanup = self.layout.locals_size + self.temp_bytes;
        if cleanup > 0 {
            self.temp_bytes = 0;
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
        let locals = self
            .layout
            .locals
            .values()
            .map(|layout| layout.size)
            .sum::<usize>();
        let params_and_ret = self
            .layout
            .return_layout
            .as_ref()
            .map(|layout| layout.size)
            .unwrap_or(0)
            + self
                .function
                .parameters
                .iter()
                .map(|parameter| {
                    self.layout
                        .locals
                        .get(&parameter.local)
                        .map(|layout| layout.size)
                        .unwrap_or(0)
                })
                .sum::<usize>();
        params_and_ret + locals + self.temp_bytes
    }
}

fn emit_expr_common(
    compiler: &mut O0Compiler<'_>,
    temp_bytes: &mut usize,
    layout: Option<&FunctionLayout>,
    expr: &HirExpr,
) -> Result<(), CodegenError> {
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
                    CodegenError::new(Some(expr.span), format!("unknown local slot {:?}", local))
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
            crate::HirValueRef::Global(name) | crate::HirValueRef::ConstGlobal(name) => {
                let slot = compiler.global_layout.get(name).ok_or_else(|| {
                    CodegenError::new(Some(expr.span), format!("unknown global {:?}", name))
                })?;
                let offset = usize_to_i32(slot.offset, "global load offset")?
                    - usize_to_i32(compiler.global_size, "global size")?;
                compiler.assembler.push(NcsInstruction {
                    opcode:  NcsOpcode::RunstackCopyBase,
                    auxcode: NcsAuxCode::TypeVoid,
                    extra:   assignment_extra(offset, slot.size),
                });
                *temp_bytes += slot.size;
                Ok(())
            }
            crate::HirValueRef::BuiltinConstant(name) => {
                let value = compiler.builtin_constants.get(name).ok_or_else(|| {
                    CodegenError::new(
                        Some(expr.span),
                        format!("unknown builtin constant {:?}", name),
                    )
                })?;
                let literal = literal_from_builtin_value(value).ok_or_else(|| {
                    CodegenError::new(
                        Some(expr.span),
                        format!("unsupported builtin constant value for {:?}", name),
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
            compiler.assembler.push(NcsInstruction {
                opcode:  NcsOpcode::RunstackCopy,
                auxcode: NcsAuxCode::TypeVoid,
                extra:   assignment_extra(
                    usize_to_i32(field_layout.offset, "field offset")?
                        - usize_to_i32(base_size, "base size")?,
                    field_layout.size,
                ),
            });
            *temp_bytes += field_layout.size;
            *temp_bytes = temp_bytes.saturating_sub(base_size);
            compiler.assembler.push(NcsInstruction {
                opcode:  NcsOpcode::ModifyStackPointer,
                auxcode: NcsAuxCode::None,
                extra:   (-usize_to_i32(base_size, "base size")?)
                    .to_be_bytes()
                    .to_vec(),
            });
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
                emit_expr_common(compiler, temp_bytes, layout, inner)?;
                if matches!(op, UnaryOp::PostIncrement | UnaryOp::PostDecrement) {
                    emit_copy_top_bytes(compiler, temp_bytes, 4);
                }
                emit_push_literal(
                    compiler,
                    temp_bytes,
                    &Literal::Integer(1),
                    &SemanticType::Int,
                    Some(expr.span),
                )?;
                let opcode = match op {
                    UnaryOp::PreIncrement | UnaryOp::PostIncrement => NcsOpcode::Add,
                    UnaryOp::PreDecrement | UnaryOp::PostDecrement => NcsOpcode::Sub,
                    _ => unreachable!(),
                };
                *temp_bytes = temp_bytes.saturating_sub(8);
                *temp_bytes += 4;
                compiler.assembler.push(NcsInstruction {
                    opcode,
                    auxcode: NcsAuxCode::TypeTypeIntegerInteger,
                    extra: Vec::new(),
                });
                emit_store_target(compiler, temp_bytes, layout, inner, expr.span)?;
                if matches!(op, UnaryOp::PostIncrement | UnaryOp::PostDecrement) {
                    emit_drop_bytes(compiler, temp_bytes, 4);
                }
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
            emit_expr_common(compiler, temp_bytes, layout, right)?;
            let opcode = opcode_for_binary(*op);
            let aux = aux_for_binary(&left.ty, &right.ty, compiler.hir, &compiler.structs)?;
            let left_size = size_of_type(&left.ty, &compiler.structs)?;
            let right_size = size_of_type(&right.ty, &compiler.structs)?;
            let result_size = size_of_binary_result(*op, &left.ty, &right.ty, &compiler.structs)?;
            *temp_bytes = temp_bytes.saturating_sub(left_size + right_size);
            *temp_bytes += result_size;
            compiler.assembler.push(NcsInstruction {
                opcode,
                auxcode: aux,
                extra: Vec::new(),
            });
            Ok(())
        }
        HirExprKind::Conditional {
            ..
        } => Err(CodegenError::new(
            Some(expr.span),
            "conditional expression code generation is not implemented yet",
        )),
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
                        CodegenError::new(Some(expr.span), format!("unknown builtin {:?}", name))
                    })?;
            for (index, argument) in arguments.iter().enumerate() {
                if function
                    .parameters
                    .get(index)
                    .is_some_and(|parameter| matches!(parameter.ty, BuiltinType::Action))
                {
                    emit_action_parameter(compiler, temp_bytes, layout, argument)?;
                    continue;
                }
                emit_expr_common(compiler, temp_bytes, layout, argument)?;
            }
            for parameter in function.parameters.iter().skip(arguments.len()) {
                if matches!(parameter.ty, BuiltinType::Action) {
                    return Err(CodegenError::new(
                        Some(expr.span),
                        format!(
                            "builtin {:?} requires an action default that is not supported",
                            name
                        ),
                    ));
                }
                let default = parameter.default.as_ref().ok_or_else(|| {
                    CodegenError::new(
                        Some(expr.span),
                        format!("missing required parameter for builtin {:?}", name),
                    )
                })?;
                let literal = literal_from_builtin_value(default).ok_or_else(|| {
                    CodegenError::new(
                        Some(expr.span),
                        format!("unsupported builtin default value for {:?}", name),
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
                CodegenError::new(Some(expr.span), format!("unknown function {:?}", name))
            })?;
            if function.return_type != SemanticType::Void {
                compiler.emit_stack_alloc(&function.return_type)?;
                *temp_bytes += size_of_type(&function.return_type, &compiler.structs)?;
            }
            for argument in arguments {
                emit_expr_common(compiler, temp_bytes, layout, argument)?;
            }
            for parameter in function.parameters.iter().skip(arguments.len()) {
                let default = parameter.default.as_ref().ok_or_else(|| {
                    CodegenError::new(
                        Some(expr.span),
                        format!("missing required parameter for function {:?}", name),
                    )
                })?;
                emit_expr_common(compiler, temp_bytes, layout, default)?;
            }
            let label = compiler.function_labels.get(name).copied().ok_or_else(|| {
                CodegenError::new(
                    Some(expr.span),
                    format!("missing function label for {:?}", name),
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
    let stack_bytes = layout.map(function_frame_bytes).unwrap_or(0) + *temp_bytes;

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

fn emit_store_target(
    compiler: &mut O0Compiler<'_>,
    temp_bytes: &mut usize,
    layout: Option<&FunctionLayout>,
    target: &HirExpr,
    span: crate::Span,
) -> Result<(), CodegenError> {
    match &target.kind {
        HirExprKind::Value(crate::HirValueRef::Local(local)) => {
            let layout = layout.ok_or_else(|| {
                CodegenError::new(Some(span), "local assignment used outside a function")
            })?;
            let slot = layout.locals.get(local).ok_or_else(|| {
                CodegenError::new(Some(span), format!("unknown local slot {:?}", local))
            })?;
            let offset = usize_to_i32(slot.offset, "local assignment offset")?
                - usize_to_i32(
                    function_frame_bytes(layout) + *temp_bytes,
                    "local assignment frame size",
                )?;
            compiler.assembler.push(NcsInstruction {
                opcode:  NcsOpcode::Assignment,
                auxcode: NcsAuxCode::TypeVoid,
                extra:   assignment_extra(offset, slot.size),
            });
            Ok(())
        }
        HirExprKind::Value(crate::HirValueRef::Global(name))
        | HirExprKind::Value(crate::HirValueRef::ConstGlobal(name)) => {
            let slot = compiler.global_layout.get(name).ok_or_else(|| {
                CodegenError::new(Some(span), format!("unknown global {:?}", name))
            })?;
            let offset = usize_to_i32(slot.offset, "global assignment offset")?
                - usize_to_i32(compiler.global_size, "global size")?;
            compiler.assembler.push(NcsInstruction {
                opcode:  NcsOpcode::AssignmentBase,
                auxcode: NcsAuxCode::TypeVoid,
                extra:   assignment_extra(offset, slot.size),
            });
            Ok(())
        }
        HirExprKind::FieldAccess {
            base,
            field,
        } => match &base.kind {
            HirExprKind::Value(crate::HirValueRef::Local(local)) => {
                let layout = layout.ok_or_else(|| {
                    CodegenError::new(Some(span), "local field assignment used outside a function")
                })?;
                let slot = layout.locals.get(local).ok_or_else(|| {
                    CodegenError::new(Some(span), format!("unknown local slot {:?}", local))
                })?;
                let field_layout = field_layout(&slot.ty, field, &compiler.structs, Some(span))?;
                let offset = usize_to_i32(
                    slot.offset + field_layout.offset,
                    "local field assignment offset",
                )? - usize_to_i32(
                    function_frame_bytes(layout) + *temp_bytes,
                    "local field assignment frame size",
                )?;
                compiler.assembler.push(NcsInstruction {
                    opcode:  NcsOpcode::Assignment,
                    auxcode: NcsAuxCode::TypeVoid,
                    extra:   assignment_extra(offset, field_layout.size),
                });
                Ok(())
            }
            HirExprKind::Value(crate::HirValueRef::Global(name))
            | HirExprKind::Value(crate::HirValueRef::ConstGlobal(name)) => {
                let slot = compiler.global_layout.get(name).ok_or_else(|| {
                    CodegenError::new(Some(span), format!("unknown global {:?}", name))
                })?;
                let field_layout = field_layout(&slot.ty, field, &compiler.structs, Some(span))?;
                let offset = usize_to_i32(
                    slot.offset + field_layout.offset,
                    "global field assignment offset",
                )? - usize_to_i32(compiler.global_size, "global size")?;
                compiler.assembler.push(NcsInstruction {
                    opcode:  NcsOpcode::AssignmentBase,
                    auxcode: NcsAuxCode::TypeVoid,
                    extra:   assignment_extra(offset, field_layout.size),
                });
                Ok(())
            }
            _ => Err(CodegenError::new(
                Some(span),
                "field assignment code generation requires a direct local or global base",
            )),
        },
        _ => Err(CodegenError::new(
            Some(span),
            "assignment target code generation is not implemented yet",
        )),
    }
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
            extra:   string_extra(value)?,
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
        Literal::Magic(_) => {
            return Err(CodegenError::new(
                span,
                "magic literal code generation is not implemented yet",
            ));
        }
    }

    *temp_bytes += size_of_type(ty, &compiler.structs)?;
    Ok(())
}

fn literal_from_builtin_value(value: &BuiltinValue) -> Option<Literal> {
    match value {
        BuiltinValue::Int(value) => Some(Literal::Integer(*value)),
        BuiltinValue::Float(value) => Some(Literal::Float(*value)),
        BuiltinValue::String(value) => Some(Literal::String(value.clone())),
        BuiltinValue::ObjectId(value) => Some(Literal::Integer(*value)),
        BuiltinValue::ObjectSelf => Some(Literal::ObjectSelf),
        BuiltinValue::ObjectInvalid => Some(Literal::ObjectInvalid),
        BuiltinValue::LocationInvalid => Some(Literal::LocationInvalid),
        BuiltinValue::Json(value) => Some(Literal::Json(value.clone())),
        BuiltinValue::Vector(value) => Some(Literal::Vector(*value)),
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
        Some(ConstValue::String(value)) => Ok(nwscript_string_hash(&value)),
        Some(ConstValue::Float(_)) | None => Err(CodegenError::new(
            Some(expr.span),
            "switch case code generation requires a constant int or string",
        )),
    }
}

fn function_frame_bytes(layout: &FunctionLayout) -> usize {
    layout.locals.values().map(|slot| slot.size).sum::<usize>()
        + layout
            .return_layout
            .as_ref()
            .map(|layout| layout.size)
            .unwrap_or(0)
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
            format!(
                "unsupported binary operand pair for code generation: {:?} and {:?}",
                left, right
            ),
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
            format!("unsupported unary operand type {:?}", ty),
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
        .ok_or_else(|| CodegenError::new(None, format!("unknown engine structure {:?}", name)))?;

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
                format!("engine structure index out of range for {:?}", name),
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
                .ok_or_else(|| CodegenError::new(None, format!("unknown structure {:?}", name)))?;
            let mut size = 0usize;
            for field in &structure.fields {
                size += size_of_type(&field.ty, structs)?;
            }
            Ok(size)
        }
    }
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
                        format!("field {:?} does not exist on vector", field),
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
                .ok_or_else(|| CodegenError::new(span, format!("unknown structure {:?}", name)))?;
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
                format!("field {:?} does not exist on structure {:?}", field, name),
            ))
        }
        _ => Err(CodegenError::new(
            span,
            format!(
                "field access requires a vector or struct base, got {:?}",
                base
            ),
        )),
    }
}

fn string_extra(value: &str) -> Result<Vec<u8>, CodegenError> {
    let length = u16::try_from(value.len()).map_err(|_error| {
        CodegenError::new(None, "string constant exceeds NCS 16-bit length limit")
    })?;
    let mut bytes = Vec::with_capacity(2 + value.len());
    bytes.extend_from_slice(&length.to_be_bytes());
    bytes.extend_from_slice(value.as_bytes());
    Ok(bytes)
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

fn emit_drop_bytes(compiler: &mut O0Compiler<'_>, temp_bytes: &mut usize, size: usize) {
    if size > 0 {
        *temp_bytes = temp_bytes.saturating_sub(size);
        compiler.assembler.push(NcsInstruction {
            opcode:  NcsOpcode::ModifyStackPointer,
            auxcode: NcsAuxCode::None,
            extra:   (-i32::try_from(size).ok().unwrap_or(i32::MAX))
                .to_be_bytes()
                .to_vec(),
        });
    }
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
            binary_start: output
                .label_offsets
                .get(&line.start)
                .copied()
                .unwrap_or_default(),
            binary_end: output
                .label_offsets
                .get(&line.end)
                .copied()
                .unwrap_or_default(),
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
                binary_start: output
                    .label_offsets
                    .get(&info.start)
                    .copied()
                    .unwrap_or_default(),
                binary_end:   output
                    .label_offsets
                    .get(&info.end)
                    .copied()
                    .unwrap_or_default(),
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
                binary_start: output
                    .label_offsets
                    .get(&variable.start)
                    .copied()
                    .unwrap_or_default(),
                binary_end:   variable
                    .end
                    .and_then(|end| output.label_offsets.get(&end).copied())
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
                    CodegenError::new(None, format!("unknown debug structure {:?}", name))
                })?;
            NdbType::Struct(index)
        }
        SemanticType::Vector => NdbType::Unknown,
        SemanticType::Action => NdbType::Unknown,
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
                format!("engine structure index out of range for {:?}", name),
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
        .ok_or_else(|| CodegenError::new(None, format!("unknown engine structure {:?}", name)))
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
    use super::{CompileOptions, OptimizationLevel};
    use crate::{
        BuiltinConstant, BuiltinFunction, BuiltinParameter, BuiltinType, BuiltinValue, NcsOpcode,
        SourceId, SourceMap, compile_script, compile_script_with_source_map,
        decode_ncs_instructions, parse_text, read_ndb,
    };

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
                optimization: OptimizationLevel::O0,
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
            loader_retval.binary_start, 2,
            "loader #retval should begin after the loader RunstackAdd instruction",
        );
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
            first.binary_start, 10,
            "first global should begin after loader + first RunstackAdd"
        );
        assert_eq!(
            second.binary_start, 12,
            "second global should begin after loader + second RunstackAdd"
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
}
