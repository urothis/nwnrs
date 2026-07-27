use std::{collections::HashMap, error::Error, fmt};

use crate::{
    CompilerErrorCode, NCS_BINARY_HEADER_SIZE, NcsAuxCode, NcsInstruction, NcsOpcode, NcsReadError,
    Ndb, NdbFunction, NdbType, ScriptString, Span, decode_ncs_instructions,
};

mod instructions;
mod runtime;

use instructions::*;

const HARD_VM_STACK_CELL_LIMIT: usize = 1_000_000;

/// One opaque object id visible to the VM runtime.
pub type VmObjectId = u32;

/// One engine-structure payload carried by a stack value.
#[derive(Debug, Clone, PartialEq)]
pub enum VmEngineStructureValue {
    /// One 32-bit opaque payload.
    Word(u32),
    /// One textual payload used by some engine structures.
    Text(String),
}

/// One runtime stack value.
#[derive(Debug, Clone, PartialEq)]
pub enum VmValue {
    /// Integer value.
    Int(i32),
    /// Floating-point value.
    Float(f32),
    /// Encoding-neutral NWScript string value.
    String(ScriptString),
    /// Object id value.
    Object(VmObjectId),
    /// One opaque engine-structure value.
    EngineStructure {
        /// Upstream engine-structure index.
        index: u8,
        /// Runtime payload.
        value: VmEngineStructureValue,
    },
    /// One user-defined structure represented by its declared field values.
    Struct(Vec<VmValue>),
}

impl VmValue {
    /// Returns a short display name for diagnostics.
    #[must_use]
    pub fn kind_name(&self) -> &'static str {
        match self {
            Self::Int(_) => "int",
            Self::Float(_) => "float",
            Self::String(_) => "string",
            Self::Object(_) => "object",
            Self::EngineStructure {
                ..
            } => "engine structure",
            Self::Struct(_) => "struct",
        }
    }
}

impl VmEngineStructureValue {
    /// Returns the contained 32-bit payload when this value is `Word`.
    #[must_use]
    pub fn as_word(&self) -> Option<u32> {
        match self {
            Self::Word(value) => Some(*value),
            Self::Text(_) => None,
        }
    }

    /// Returns the contained string slice when this value is `Text`.
    #[must_use]
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Word(_) => None,
            Self::Text(value) => Some(value),
        }
    }
}

/// Errors returned while executing `NCS` bytecode.
#[derive(Debug)]
pub enum VmError {
    /// Decoding the bytecode stream failed before execution began.
    Read(NcsReadError),
    /// One instruction requested a feature this VM does not yet implement.
    Unsupported {
        /// Byte offset of the instruction within the code section.
        offset:  usize,
        /// Opcode that failed.
        opcode:  NcsOpcode,
        /// Auxcode that failed.
        auxcode: NcsAuxCode,
        /// Human-readable explanation.
        message: String,
    },
    /// One stack access ran past the available values.
    StackUnderflow {
        /// Human-readable explanation.
        message: String,
    },
    /// One instruction expected a value of a different runtime type.
    TypeMismatch {
        /// Byte offset of the instruction within the code section.
        offset:   usize,
        /// Human-readable explanation.
        message:  String,
        /// Optional expected type description.
        expected: Option<&'static str>,
        /// Actual runtime value kind.
        actual:   &'static str,
    },
    /// One jump or return target did not point at a valid instruction.
    InvalidInstructionPointer {
        /// Byte offset that could not be resolved.
        offset: usize,
    },
    /// One opcode payload was malformed.
    InvalidExtra {
        /// Byte offset of the instruction within the code section.
        offset:  usize,
        /// Opcode whose payload failed.
        opcode:  NcsOpcode,
        /// Auxcode whose payload failed.
        auxcode: NcsAuxCode,
        /// Human-readable explanation.
        message: String,
    },
    /// One command id was invoked without a registered handler.
    InvalidCommand {
        /// Byte offset of the `ACTION` instruction.
        offset:  usize,
        /// Unhandled command id.
        command: u16,
    },
    /// One host-side setup or invocation request was invalid before execution.
    Setup {
        /// Human-readable explanation.
        message: String,
    },
    /// A compiler-hosted VM command reported a diagnostic for source tokens.
    MacroDiagnostic {
        /// Original macro-input source span.
        span:    Span,
        /// Human-readable diagnostic.
        message: String,
    },
    /// One VM run exceeded the configured instruction budget.
    InstructionLimitExceeded {
        /// Byte offset of the instruction that would execute next.
        offset: usize,
        /// Maximum instruction count allowed for the run.
        limit:  usize,
    },
    /// One VM run exceeded the configured call-depth budget.
    RecursionLimitExceeded {
        /// Current return-frame depth.
        depth: usize,
        /// Maximum permitted return-frame depth.
        limit: usize,
    },
    /// One VM run exceeded the configured runtime-stack budget.
    StackLimitExceeded {
        /// Current runtime stack size in cells.
        cells: usize,
        /// Maximum permitted runtime stack size in cells.
        limit: usize,
    },
    /// An arithmetic instruction attempted division or modulus by zero.
    DivideByZero {
        /// Byte offset of the failing instruction.
        offset: usize,
    },
    /// The caller cancelled VM execution.
    Cancelled,
}

impl VmError {
    /// Returns the closest upstream-aligned VM/compiler error code when one
    /// exists.
    #[must_use]
    pub fn code(&self) -> Option<CompilerErrorCode> {
        match self {
            Self::Read(NcsReadError::Opcode(_)) => Some(CompilerErrorCode::VmInvalidOpCode),
            Self::Read(NcsReadError::AuxCode(_)) => Some(CompilerErrorCode::VmInvalidAuxCode),
            Self::Read(
                NcsReadError::Header(_)
                | NcsReadError::TruncatedInstruction {
                    ..
                }
                | NcsReadError::TrailingBytes {
                    ..
                },
            )
            | Self::InvalidExtra {
                ..
            } => Some(CompilerErrorCode::VmInvalidExtraDataOnOpCode),
            Self::Unsupported {
                ..
            } => None,
            Self::StackUnderflow {
                ..
            } => Some(CompilerErrorCode::VmStackUnderflow),
            Self::TypeMismatch {
                ..
            } => Some(CompilerErrorCode::VmUnknownTypeOnRunTimeStack),
            Self::InvalidInstructionPointer {
                ..
            } => Some(CompilerErrorCode::VmIpOutOfCodeSegment),
            Self::InvalidCommand {
                ..
            } => Some(CompilerErrorCode::VmInvalidCommand),
            Self::Setup {
                ..
            }
            | Self::MacroDiagnostic {
                ..
            } => None,
            Self::InstructionLimitExceeded {
                ..
            } => Some(CompilerErrorCode::VmTooManyInstructions),
            Self::RecursionLimitExceeded {
                ..
            } => Some(CompilerErrorCode::VmTooManyLevelsOfRecursion),
            Self::StackLimitExceeded {
                ..
            } => Some(CompilerErrorCode::VmStackOverflow),
            Self::DivideByZero {
                ..
            } => Some(CompilerErrorCode::VmDivideByZero),
            Self::Cancelled => None,
        }
    }
}

impl fmt::Display for VmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(error) => error.fmt(f),
            Self::Unsupported {
                offset,
                opcode,
                auxcode,
                message,
            } => write!(
                f,
                "unsupported VM instruction {}.{} at byte {}: {}",
                opcode, auxcode, offset, message
            ),
            Self::StackUnderflow {
                message,
            } => write!(f, "VM stack underflow: {message}"),
            Self::TypeMismatch {
                offset,
                message,
                expected,
                actual,
            } => match expected {
                Some(expected) => write!(
                    f,
                    "VM type mismatch at byte {}: {} (expected {}, got {})",
                    offset, message, expected, actual
                ),
                None => write!(
                    f,
                    "VM type mismatch at byte {}: {} ({})",
                    offset, message, actual
                ),
            },
            Self::InvalidInstructionPointer {
                offset,
            } => write!(
                f,
                "VM instruction pointer left the code segment at byte {offset}"
            ),
            Self::InvalidExtra {
                offset,
                opcode,
                auxcode,
                message,
            } => write!(
                f,
                "invalid {}.{} payload at byte {}: {}",
                opcode, auxcode, offset, message
            ),
            Self::InvalidCommand {
                offset,
                command,
            } => {
                write!(f, "invalid VM command {} at byte {}", command, offset)
            }
            Self::Setup {
                message,
            } => f.write_str(message),
            Self::MacroDiagnostic {
                message, ..
            } => f.write_str(message),
            Self::InstructionLimitExceeded {
                offset,
                limit,
            } => write!(
                f,
                "VM instruction limit of {} exceeded before byte {}",
                limit, offset
            ),
            Self::RecursionLimitExceeded {
                depth,
                limit,
            } => write!(f, "VM recursion depth {depth} exceeds limit {limit}"),
            Self::StackLimitExceeded {
                cells,
                limit,
            } => write!(f, "VM stack size {cells} exceeds limit {limit}"),
            Self::DivideByZero {
                offset,
            } => write!(f, "VM division by zero at byte {offset}"),
            Self::Cancelled => f.write_str("operation cancelled"),
        }
    }
}

impl Error for VmError {}

impl From<NcsReadError> for VmError {
    fn from(value: NcsReadError) -> Self {
        Self::Read(value)
    }
}

/// One instruction-dispatch trace event emitted by the VM.
#[derive(Debug, Clone)]
pub struct VmTraceEvent {
    /// Script byte offset of the instruction about to execute.
    pub offset:      usize,
    /// Current instruction pointer in code-section bytes.
    pub ip:          usize,
    /// Current stack pointer in stack cells.
    pub sp:          usize,
    /// Current base pointer in stack cells.
    pub bp:          usize,
    /// One cloned instruction payload.
    pub instruction: NcsInstruction,
}

#[derive(Debug, Clone)]
struct VmProgramInstruction {
    offset:      usize,
    instruction: NcsInstruction,
}

#[derive(Debug, Clone)]
struct VmFunctionDebug {
    label: String,
    start: usize,
    end:   usize,
}

#[derive(Debug, Clone)]
struct VmSourceLineDebug {
    file_name:   String,
    is_root:     bool,
    line_number: usize,
    start:       usize,
    end:         usize,
}

#[derive(Debug, Clone, Copy)]
struct VmReturnFrame {
    target: usize,
}

#[derive(Debug, Clone)]
struct VmProgram {
    instructions:     Vec<VmProgramInstruction>,
    offsets_to_index: HashMap<usize, usize>,
    functions:        Vec<VmFunctionDebug>,
    source_lines:     Vec<VmSourceLineDebug>,
}

impl VmProgram {
    fn decode(bytes: &[u8]) -> Result<Self, VmError> {
        let instructions = decode_ncs_instructions(bytes)?;
        Ok(Self::from_instructions(instructions))
    }

    fn from_instructions(instructions: Vec<NcsInstruction>) -> Self {
        let mut decoded = Vec::with_capacity(instructions.len());
        let mut offsets_to_index = HashMap::with_capacity(instructions.len());
        let mut offset = 0usize;
        for (index, instruction) in instructions.into_iter().enumerate() {
            let encoded_len = instruction.encoded_len();
            offsets_to_index.insert(offset, index);
            decoded.push(VmProgramInstruction {
                offset,
                instruction,
            });
            offset += encoded_len;
        }
        Self {
            instructions: decoded,
            offsets_to_index,
            functions: Vec::new(),
            source_lines: Vec::new(),
        }
    }

    fn instruction_at(&self, offset: usize) -> Option<&VmProgramInstruction> {
        self.offsets_to_index
            .get(&offset)
            .and_then(|index| self.instructions.get(*index))
    }

    fn attach_ndb(&mut self, ndb: &Ndb) -> Result<(), VmError> {
        self.functions.clear();
        self.source_lines.clear();
        for function in &ndb.functions {
            let start = ndb_code_offset(
                function.binary_start,
                &format!("function {:?} start", function.label),
            )?;
            let end = ndb_code_offset(
                function.binary_end,
                &format!("function {:?} end", function.label),
            )?;
            self.functions.push(VmFunctionDebug {
                label: function.label.to_string(),
                start,
                end,
            });
        }
        for line in &ndb.lines {
            let start = ndb_code_offset(
                line.binary_start,
                &format!("line mapping {}:{} start", line.file_num, line.line_num),
            )?;
            let end = ndb_code_offset(
                line.binary_end,
                &format!("line mapping {}:{} end", line.file_num, line.line_num),
            )?;
            let file = ndb.files.get(line.file_num).ok_or_else(|| VmError::Setup {
                message: format!(
                    "line mapping {}:{} references missing file index {}",
                    line.file_num, line.line_num, line.file_num
                ),
            })?;
            self.source_lines.push(VmSourceLineDebug {
                file_name: file.name.clone(),
                is_root: file.is_root,
                line_number: line.line_num,
                start,
                end,
            });
        }
        Ok(())
    }

    fn function_at(&self, offset: usize) -> Option<&VmFunctionDebug> {
        self.functions
            .iter()
            .find(|function| contains_debug_offset(offset, function.start, function.end))
    }

    fn source_line_at(&self, offset: usize) -> Option<&VmSourceLineDebug> {
        self.source_lines
            .iter()
            .find(|line| contains_debug_offset(offset, line.start, line.end))
    }
}

fn contains_debug_offset(offset: usize, start: usize, end: usize) -> bool {
    if end <= start {
        offset == start
    } else {
        (start..end).contains(&offset)
    }
}

fn ndb_code_offset(offset: u32, description: &str) -> Result<usize, VmError> {
    usize::try_from(offset)
        .ok()
        .and_then(|offset| offset.checked_sub(NCS_BINARY_HEADER_SIZE))
        .ok_or_else(|| VmError::Setup {
            message: format!("{description} offset {offset} is before the NCS code section"),
        })
}

/// One debugger-visible source location derived from attached `NDB` metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmSourceLocation {
    /// File name as recorded in the attached `NDB` table.
    pub file_name:   String,
    /// Whether this file is the root script file.
    pub is_root:     bool,
    /// One-based source line number.
    pub line_number: usize,
}

/// One debugger-visible function range derived from attached `NDB` metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmFunctionInfo {
    /// Function name as recorded in the attached `NDB` table.
    pub name:  String,
    /// Start byte offset in the code section.
    pub start: usize,
    /// End byte offset in the code section.
    pub end:   usize,
}

/// One deferred NWScript action captured by `STORESTATE`.
#[derive(Debug, Clone)]
pub struct VmSituation {
    label:   String,
    program: VmProgram,
    ip:      usize,
    sp:      usize,
    bp:      usize,
    stack:   Vec<VmValue>,
}

impl VmSituation {
    /// Returns the user-facing label associated with this saved situation.
    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Returns the saved instruction pointer in code-section bytes.
    #[must_use]
    pub fn ip(&self) -> usize {
        self.ip
    }

    /// Returns the saved stack pointer in stack cells.
    #[must_use]
    pub fn sp(&self) -> usize {
        self.sp
    }

    /// Returns the saved base pointer in stack cells.
    #[must_use]
    pub fn bp(&self) -> usize {
        self.bp
    }

    /// Returns the saved stack snapshot.
    #[must_use]
    pub fn stack(&self) -> &[VmValue] {
        &self.stack
    }

    /// Rehydrates this situation into a runnable script snapshot.
    #[must_use]
    pub fn to_script(&self) -> VmScript {
        VmScript {
            label:           self.label.clone(),
            program:         self.program.clone(),
            ip:              self.ip,
            sp:              self.sp,
            bp:              self.bp,
            ret:             Vec::new(),
            stack:           self.stack.clone(),
            save_ip:         0,
            save_sp:         0,
            save_bp:         0,
            saved_situation: None,
            abort_requested: false,
            aborted:         false,
        }
    }
}

/// One executable `NCS` script plus its VM runtime state.
#[derive(Debug, Clone)]
pub struct VmScript {
    label:           String,
    program:         VmProgram,
    ip:              usize,
    sp:              usize,
    bp:              usize,
    ret:             Vec<VmReturnFrame>,
    stack:           Vec<VmValue>,
    save_ip:         usize,
    save_sp:         usize,
    save_bp:         usize,
    saved_situation: Option<VmSituation>,
    abort_requested: bool,
    aborted:         bool,
}

impl VmScript {
    /// Decodes one `NCS V1.0` bytecode stream into a runnable script.
    ///
    /// # Errors
    ///
    /// Returns [`VmError`] if the bytecode is malformed.
    pub fn from_bytes(bytes: &[u8], label: impl Into<String>) -> Result<Self, VmError> {
        Ok(Self {
            label:           label.into(),
            program:         VmProgram::decode(bytes)?,
            ip:              0,
            sp:              0,
            bp:              0,
            ret:             Vec::new(),
            stack:           Vec::new(),
            save_ip:         0,
            save_sp:         0,
            save_bp:         0,
            saved_situation: None,
            abort_requested: false,
            aborted:         false,
        })
    }

    /// Builds a runnable script from decoded instructions.
    #[must_use]
    pub fn from_instructions(instructions: Vec<NcsInstruction>, label: impl Into<String>) -> Self {
        Self {
            label:           label.into(),
            program:         VmProgram::from_instructions(instructions),
            ip:              0,
            sp:              0,
            bp:              0,
            ret:             Vec::new(),
            stack:           Vec::new(),
            save_ip:         0,
            save_sp:         0,
            save_bp:         0,
            saved_situation: None,
            abort_requested: false,
            aborted:         false,
        }
    }

    /// Attaches one `NDB` debug table for function and source-line debugging.
    ///
    /// # Errors
    ///
    /// Returns [`VmError`] if one debug record contains an invalid code offset.
    pub fn attach_ndb(&mut self, ndb: &Ndb) -> Result<(), VmError> {
        self.program.attach_ndb(ndb)
    }

    /// Decodes one `NCS V1.0` bytecode stream and attaches one `NDB` debug
    /// table.
    ///
    /// # Errors
    ///
    /// Returns [`VmError`] if the bytecode or attached debug metadata is
    /// malformed.
    pub fn from_bytes_with_ndb(
        bytes: &[u8],
        label: impl Into<String>,
        ndb: &Ndb,
    ) -> Result<Self, VmError> {
        let mut script = Self::from_bytes(bytes, label)?;
        script.attach_ndb(ndb)?;
        Ok(script)
    }

    /// Executes this script using the supplied VM command table.
    ///
    /// # Errors
    ///
    /// Returns [`VmError`] if execution fails.
    pub fn run(&mut self, vm: &Vm) -> Result<(), VmError> {
        vm.run(self)
    }

    /// Returns the user-facing label associated with this script.
    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Returns the current instruction pointer in code-section bytes.
    #[must_use]
    pub fn ip(&self) -> usize {
        self.ip
    }

    /// Returns the current stack pointer in stack cells.
    #[must_use]
    pub fn sp(&self) -> usize {
        self.sp
    }

    /// Returns the current base pointer in stack cells.
    #[must_use]
    pub fn bp(&self) -> usize {
        self.bp
    }

    /// Returns the decoded instruction at the current instruction pointer.
    #[must_use]
    pub fn current_instruction(&self) -> Option<&NcsInstruction> {
        self.program
            .instruction_at(self.ip)
            .map(|decoded| &decoded.instruction)
    }

    /// Returns the decoded instruction at one byte offset, if present.
    #[must_use]
    pub fn instruction_at(&self, offset: usize) -> Option<&NcsInstruction> {
        self.program
            .instruction_at(offset)
            .map(|decoded| &decoded.instruction)
    }

    /// Returns the attached debug function that contains the current
    /// instruction pointer.
    #[must_use]
    pub fn current_function(&self) -> Option<VmFunctionInfo> {
        self.function_at(self.ip)
    }

    /// Returns the attached debug function that contains one byte offset.
    #[must_use]
    pub fn function_at(&self, offset: usize) -> Option<VmFunctionInfo> {
        self.program
            .function_at(offset)
            .map(|function| VmFunctionInfo {
                name:  function.label.clone(),
                start: function.start,
                end:   function.end,
            })
    }

    /// Returns the attached source location for the current instruction
    /// pointer.
    #[must_use]
    pub fn current_source_location(&self) -> Option<VmSourceLocation> {
        self.source_location_at(self.ip)
    }

    /// Returns the attached source location for one byte offset.
    #[must_use]
    pub fn source_location_at(&self, offset: usize) -> Option<VmSourceLocation> {
        self.program
            .source_line_at(offset)
            .map(|line| VmSourceLocation {
                file_name:   line.file_name.clone(),
                is_root:     line.is_root,
                line_number: line.line_number,
            })
    }

    /// Returns the instruction pointer saved by `STOREIP` or `STORESTATE`.
    #[must_use]
    pub fn save_ip(&self) -> usize {
        self.save_ip
    }

    /// Returns the stack pointer saved by `STORESTATE`.
    #[must_use]
    pub fn save_sp(&self) -> usize {
        self.save_sp
    }

    /// Returns the base pointer saved by `STORESTATE`.
    #[must_use]
    pub fn save_bp(&self) -> usize {
        self.save_bp
    }

    /// Returns the current stack values.
    #[must_use]
    pub fn stack(&self) -> &[VmValue] {
        &self.stack
    }

    /// Returns a compact debugger-oriented stack rendering with `^` at the base
    /// pointer and `*` at the top stack cell.
    #[must_use]
    pub fn stack_string(&self) -> String {
        let mut rendered = String::new();
        for (index, value) in self.stack.iter().enumerate() {
            if index > 0 {
                rendered.push(' ');
            }
            if index == self.bp {
                rendered.push('^');
            }
            if index + 1 == self.sp {
                rendered.push('*');
            }
            rendered.push_str(&format!("{value:?}"));
        }
        rendered
    }

    /// Returns the current return-frame depth.
    #[must_use]
    pub fn return_depth(&self) -> usize {
        self.ret.len()
    }

    /// Returns the last deferred action snapshot captured by `STORESTATE`.
    #[must_use]
    pub fn saved_situation(&self) -> Option<&VmSituation> {
        self.saved_situation.as_ref()
    }

    /// Removes and returns the last deferred action snapshot captured by
    /// `STORESTATE`.
    pub fn take_saved_situation(&mut self) -> Option<VmSituation> {
        self.saved_situation.take()
    }

    /// Configures this script to call one named user function directly.
    ///
    /// This bypasses the compiler-emitted loader.
    ///
    /// If the script uses globals, the caller must initialize the loader/global
    /// frame first before invoking a named function directly.
    ///
    /// # Errors
    ///
    /// Returns [`VmError`] if the function cannot be found or the argument
    /// types are unsupported.
    pub fn prepare_function_call(
        &mut self,
        ndb: &Ndb,
        name: &str,
        args: &[VmValue],
    ) -> Result<(), VmError> {
        self.attach_ndb(ndb)?;
        let function = ndb
            .functions
            .iter()
            .find(|function| function.label == name)
            .ok_or_else(|| VmError::Setup {
                message: format!("unknown NDB function {name:?}"),
            })?;
        expect_argument_count(function, args.len())?;

        self.ip = ndb_code_offset(function.binary_start, &format!("function {name:?} start"))?;
        let preserved_sp = self.sp;
        let preserved_bp = self.bp;
        self.sp = preserved_sp;
        self.bp = preserved_bp;
        self.ret.clear();
        self.ret.push(VmReturnFrame {
            target: usize::MAX
        });
        self.save_ip = 0;
        self.save_sp = 0;
        self.save_bp = 0;
        self.saved_situation = None;
        self.abort_requested = false;
        self.aborted = false;

        if preserved_sp == 0 {
            self.stack.clear();
            self.bp = 0;
        }
        if function.return_type != NdbType::Void {
            for value in default_values_for_ndb_type(ndb, &function.return_type)? {
                self.push(value);
            }
        }
        for (expected, actual) in function.args.iter().zip(args) {
            for value in flatten_entry_argument(ndb, expected, actual)? {
                self.push(value);
            }
        }
        Ok(())
    }

    /// Reads one return value, including recursively represented structures,
    /// after a direct function call.
    ///
    /// # Errors
    ///
    /// Returns [`VmError`] if the function cannot be found or uses an
    /// unsupported return type.
    pub fn function_return_value(&self, ndb: &Ndb, name: &str) -> Result<Option<VmValue>, VmError> {
        let function = ndb
            .functions
            .iter()
            .find(|function| function.label == name)
            .ok_or_else(|| VmError::Setup {
                message: format!("unknown NDB function {name:?}"),
            })?;
        if function.return_type == NdbType::Void {
            return Ok(None);
        }
        let width = cells_for_ndb_type(ndb, &function.return_type)?;
        let start = self
            .sp
            .checked_sub(width)
            .ok_or_else(|| VmError::StackUnderflow {
                message: format!("function {name:?} return slot is missing"),
            })?;
        let cells = self
            .stack
            .get(start..self.sp)
            .ok_or_else(|| VmError::StackUnderflow {
                message: format!("function {name:?} return slot is incomplete"),
            })?;
        let mut cells = cells.iter();
        inflate_ndb_value(ndb, &function.return_type, &mut cells).map(Some)
    }

    /// Requests that this script abort once control returns to the VM
    /// dispatcher.
    pub fn abort(&mut self) {
        self.abort_requested = true;
    }

    /// Returns whether the last VM run terminated via `abort()`.
    #[must_use]
    pub fn aborted(&self) -> bool {
        self.aborted
    }

    /// Pushes one raw stack value.
    pub fn push(&mut self, value: VmValue) {
        self.stack.push(value);
        self.sp = self.stack.len();
    }

    /// Pushes one integer value.
    pub fn push_int(&mut self, value: i32) {
        self.push(VmValue::Int(value));
    }

    /// Pushes one floating-point value.
    pub fn push_float(&mut self, value: f32) {
        self.push(VmValue::Float(value));
    }

    /// Pushes one string value.
    pub fn push_string(&mut self, value: impl Into<ScriptString>) {
        self.push(VmValue::String(value.into()));
    }

    /// Pushes one object id.
    pub fn push_object(&mut self, value: VmObjectId) {
        self.push(VmValue::Object(value));
    }

    /// Pushes one engine-structure value.
    pub fn push_engine_structure(&mut self, index: u8, value: VmEngineStructureValue) {
        self.push(VmValue::EngineStructure {
            index,
            value,
        });
    }

    /// Pushes one vector value as three stack cells in `x, y, z` order.
    pub fn push_vector(&mut self, value: [f32; 3]) {
        for component in value {
            self.push_float(component);
        }
    }

    /// Pops one raw stack value.
    ///
    /// # Errors
    ///
    /// Returns [`VmError`] if the stack is empty.
    pub fn pop(&mut self) -> Result<VmValue, VmError> {
        let value = self.stack.pop().ok_or_else(|| VmError::StackUnderflow {
            message: "attempted to pop from an empty stack".to_string(),
        })?;
        self.sp = self.stack.len();
        Ok(value)
    }

    /// Pops one integer value.
    ///
    /// # Errors
    ///
    /// Returns [`VmError`] if the stack top is not an integer.
    pub fn pop_int(&mut self) -> Result<i32, VmError> {
        match self.pop()? {
            VmValue::Int(value) => Ok(value),
            other => Err(VmError::TypeMismatch {
                offset:   self.ip,
                message:  "expected integer on stack top".to_string(),
                expected: Some("int"),
                actual:   other.kind_name(),
            }),
        }
    }

    /// Pops one floating-point value.
    ///
    /// # Errors
    ///
    /// Returns [`VmError`] if the stack top is not a float.
    pub fn pop_float(&mut self) -> Result<f32, VmError> {
        match self.pop()? {
            VmValue::Float(value) => Ok(value),
            other => Err(VmError::TypeMismatch {
                offset:   self.ip,
                message:  "expected float on stack top".to_string(),
                expected: Some("float"),
                actual:   other.kind_name(),
            }),
        }
    }

    /// Pops one string value.
    ///
    /// # Errors
    ///
    /// Returns [`VmError`] if the stack top is not a string.
    pub fn pop_string(&mut self) -> Result<ScriptString, VmError> {
        match self.pop()? {
            VmValue::String(value) => Ok(value),
            other => Err(VmError::TypeMismatch {
                offset:   self.ip,
                message:  "expected string on stack top".to_string(),
                expected: Some("string"),
                actual:   other.kind_name(),
            }),
        }
    }

    /// Pops one object id.
    ///
    /// # Errors
    ///
    /// Returns [`VmError`] if the stack top is not an object.
    pub fn pop_object(&mut self) -> Result<VmObjectId, VmError> {
        match self.pop()? {
            VmValue::Object(value) => Ok(value),
            other => Err(VmError::TypeMismatch {
                offset:   self.ip,
                message:  "expected object on stack top".to_string(),
                expected: Some("object"),
                actual:   other.kind_name(),
            }),
        }
    }

    /// Pops one engine-structure value.
    ///
    /// # Errors
    ///
    /// Returns [`VmError`] if the stack top is not an engine structure.
    pub fn pop_engine_structure(&mut self) -> Result<(u8, VmEngineStructureValue), VmError> {
        match self.pop()? {
            VmValue::EngineStructure {
                index,
                value,
            } => Ok((index, value)),
            other => Err(VmError::TypeMismatch {
                offset:   self.ip,
                message:  "expected engine structure on stack top".to_string(),
                expected: Some("engine structure"),
                actual:   other.kind_name(),
            }),
        }
    }

    /// Pops one engine-structure value and checks its engine-structure index.
    ///
    /// # Errors
    ///
    /// Returns [`VmError`] if the stack top is not the requested engine
    /// structure.
    pub fn pop_engine_structure_index(
        &mut self,
        expected_index: u8,
    ) -> Result<VmEngineStructureValue, VmError> {
        let (index, value) = self.pop_engine_structure()?;
        if index != expected_index {
            return Err(VmError::TypeMismatch {
                offset:   self.ip,
                message:  format!(
                    "expected engine structure {} on stack top, found {}",
                    expected_index, index
                ),
                expected: Some("engine structure"),
                actual:   "engine structure",
            });
        }
        Ok(value)
    }

    /// Pops one vector value from three float stack cells.
    ///
    /// # Errors
    ///
    /// Returns [`VmError`] if the top three cells are not floats.
    pub fn pop_vector(&mut self) -> Result<[f32; 3], VmError> {
        let z = self.pop_float()?;
        let y = self.pop_float()?;
        let x = self.pop_float()?;
        Ok([x, y, z])
    }

    fn set_stack_pointer(&mut self, pointer: usize) -> Result<(), VmError> {
        if pointer > self.stack.len() {
            return Err(VmError::StackUnderflow {
                message: format!(
                    "attempted to move stack pointer to {}, but stack has {} values",
                    pointer,
                    self.stack.len()
                ),
            });
        }
        self.stack.truncate(pointer);
        self.sp = pointer;
        Ok(())
    }

    fn assign_cell(&mut self, src: usize, dst: usize) -> Result<(), VmError> {
        let Some(value) = self.stack.get(src).cloned() else {
            return Err(VmError::StackUnderflow {
                message: format!("attempted to copy from missing stack cell {src}"),
            });
        };
        if dst >= self.stack.len() {
            self.stack.push(value);
            self.sp = self.stack.len();
        } else {
            let Some(target) = self.stack.get_mut(dst) else {
                return Err(VmError::StackUnderflow {
                    message: format!("attempted to write missing stack cell {dst}"),
                });
            };
            *target = value;
        }
        Ok(())
    }
}

/// One immutable `ACTION` handler.
pub type VmCommandHandler = dyn Fn(&mut VmScript, u16, u8) -> Result<(), VmError> + 'static;

/// One engine-structure default factory used by `RSADD`.
pub type VmEngineStructureFactory = dyn Fn(u8) -> VmEngineStructureValue + 'static;

/// One engine-structure equality hook used by `EQ` and `NEQ`.
pub type VmEngineStructureComparer =
    dyn Fn(u8, &VmEngineStructureValue, &VmEngineStructureValue) -> bool + 'static;

/// One instruction trace hook invoked before the VM executes each opcode.
pub type VmTraceHook = dyn Fn(&VmScript, &VmTraceEvent) + 'static;

/// One set of optional execution controls for a VM run.
#[derive(Debug, Clone, Copy)]
pub struct VmRunOptions {
    /// Maximum number of instructions that may execute before the VM aborts
    /// with an error.
    pub max_instructions:    Option<usize>,
    /// Maximum number of active return frames.
    pub max_recursion_depth: Option<usize>,
    /// Maximum runtime stack size in 32-bit cells.
    pub max_stack_cells:     Option<usize>,
}

impl Default for VmRunOptions {
    fn default() -> Self {
        Self {
            max_instructions:    None,
            max_recursion_depth: None,
            max_stack_cells:     Some(1_000_000),
        }
    }
}

/// One result returned after executing exactly one instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmStepOutcome {
    /// One instruction executed and the script can continue.
    Running,
    /// The script returned from the outermost frame.
    Halted,
    /// A host action handler requested abort and the VM stopped cleanly.
    Aborted,
}

/// One command-dispatch table used to execute `ACTION` opcodes.
#[derive(Default)]
pub struct Vm {
    commands:                   Vec<Option<Box<VmCommandHandler>>>,
    engine_structures:          Vec<Option<Box<VmEngineStructureFactory>>>,
    engine_structure_comparers: Vec<Option<Box<VmEngineStructureComparer>>>,
    trace_hook:                 Option<Box<VmTraceHook>>,
    cancellation:               Option<crate::CancellationToken>,
}

impl fmt::Debug for Vm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Vm")
            .field("registered_commands", &self.commands.len())
            .field(
                "registered_engine_structures",
                &self.engine_structures.len(),
            )
            .field(
                "registered_engine_structure_comparers",
                &self.engine_structure_comparers.len(),
            )
            .field("has_trace_hook", &self.trace_hook.is_some())
            .field("has_cancellation", &self.cancellation.is_some())
            .finish()
    }
}

fn script_has_globals(ndb: &Ndb) -> bool {
    ndb.variables
        .iter()
        .any(|variable| variable.binary_end == u32::MAX && variable.label != "#retval")
}

fn entry_function_name(ndb: &Ndb) -> Option<&'static str> {
    if ndb
        .functions
        .iter()
        .any(|function| function.label == "main")
    {
        Some("main")
    } else if ndb
        .functions
        .iter()
        .any(|function| function.label == "StartingConditional")
    {
        Some("StartingConditional")
    } else {
        None
    }
}

fn global_stack_cells(ndb: &Ndb) -> Result<usize, VmError> {
    ndb.variables
        .iter()
        .filter(|variable| variable.binary_end == u32::MAX && variable.label != "#retval")
        .try_fold(0usize, |cells, variable| {
            let start =
                usize::try_from(variable.stack_loc / 4).map_err(|_error| VmError::Setup {
                    message: format!(
                        "global {:?} stack location exceeds usize range",
                        variable.label
                    ),
                })?;
            let width = cells_for_ndb_type(ndb, &variable.ty)?;
            Ok(cells.max(start + width))
        })
}

fn loader_prefix_cells(ndb: &Ndb) -> Result<usize, VmError> {
    let Some(retval) = ndb
        .variables
        .iter()
        .find(|variable| variable.binary_end == u32::MAX && variable.label == "#retval")
    else {
        return Ok(0);
    };
    cells_for_ndb_type(ndb, &retval.ty)
}

fn expect_argument_count(function: &NdbFunction, actual: usize) -> Result<(), VmError> {
    if function.args.len() != actual {
        return Err(VmError::Setup {
            message: format!(
                "function {:?} expects {} arguments, got {}",
                function.label,
                function.args.len(),
                actual
            ),
        });
    }
    Ok(())
}

fn flatten_entry_argument(
    ndb: &Ndb,
    expected: &NdbType,
    actual: &VmValue,
) -> Result<Vec<VmValue>, VmError> {
    match (expected, actual) {
        (NdbType::Int, VmValue::Int(_))
        | (NdbType::Float, VmValue::Float(_))
        | (NdbType::String, VmValue::String(_))
        | (NdbType::Object, VmValue::Object(_)) => Ok(vec![actual.clone()]),
        (
            NdbType::EngineStructure(expected),
            VmValue::EngineStructure {
                index, ..
            },
        ) if expected == index => Ok(vec![actual.clone()]),
        (NdbType::Struct(struct_index), VmValue::Struct(values)) => {
            let structure = ndb
                .structs
                .get(*struct_index)
                .ok_or_else(|| VmError::Setup {
                    message: format!("NDB references missing structure t{struct_index:04}"),
                })?;
            if structure.fields.len() != values.len() {
                return Err(VmError::Setup {
                    message: format!(
                        "struct {} expects {} fields, got {}",
                        structure.label,
                        structure.fields.len(),
                        values.len()
                    ),
                });
            }
            let mut flattened = Vec::new();
            for (field, value) in structure.fields.iter().zip(values) {
                flattened.extend(flatten_entry_argument(ndb, &field.ty, value)?);
            }
            Ok(flattened)
        }
        _ => Err(VmError::Setup {
            message: format!(
                "argument type mismatch: expected {}, got {}",
                expected,
                actual.kind_name()
            ),
        }),
    }
}

fn default_values_for_ndb_type(ndb: &Ndb, ty: &NdbType) -> Result<Vec<VmValue>, VmError> {
    Ok(match ty {
        NdbType::Float => vec![VmValue::Float(0.0)],
        NdbType::Int => vec![VmValue::Int(0)],
        NdbType::Void => {
            return Err(VmError::Setup {
                message: "void return slots are not materialized".to_string(),
            });
        }
        NdbType::Object => vec![VmValue::Object(0)],
        NdbType::String => vec![VmValue::String(ScriptString::default())],
        NdbType::EngineStructure(index) => vec![VmValue::EngineStructure {
            index: *index,
            value: default_engine_structure_value(*index),
        }],
        NdbType::Struct(struct_index) => {
            let structure = ndb
                .structs
                .get(*struct_index)
                .ok_or_else(|| VmError::Setup {
                    message: format!("NDB references missing structure t{struct_index:04}"),
                })?;
            let mut values = Vec::new();
            for field in &structure.fields {
                values.extend(default_values_for_ndb_type(ndb, &field.ty)?);
            }
            values
        }
        NdbType::Unknown | NdbType::Raw(_) => {
            return Err(VmError::Setup {
                message: format!("unsupported NDB runtime type {ty}"),
            });
        }
    })
}

fn inflate_ndb_value<'a>(
    ndb: &Ndb,
    ty: &NdbType,
    cells: &mut impl Iterator<Item = &'a VmValue>,
) -> Result<VmValue, VmError> {
    if let NdbType::Struct(struct_index) = ty {
        let structure = ndb
            .structs
            .get(*struct_index)
            .ok_or_else(|| VmError::Setup {
                message: format!("NDB references missing structure t{struct_index:04}"),
            })?;
        let mut values = Vec::with_capacity(structure.fields.len());
        for field in &structure.fields {
            values.push(inflate_ndb_value(ndb, &field.ty, cells)?);
        }
        return Ok(VmValue::Struct(values));
    }

    let value = cells
        .next()
        .cloned()
        .ok_or_else(|| VmError::StackUnderflow {
            message: format!("missing runtime cell for NDB type {ty}"),
        })?;
    flatten_entry_argument(ndb, ty, &value)?;
    Ok(value)
}

#[cfg(test)]
mod tests;
