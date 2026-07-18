use std::{collections::HashMap, error::Error, fmt};

use crate::{
    CompilerErrorCode, NCS_BINARY_HEADER_SIZE, NcsAuxCode, NcsInstruction, NcsOpcode, NcsReadError,
    Ndb, NdbFunction, NdbType, ScriptString, decode_ncs_instructions,
};

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
        self.sp += 1;
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
        self.sp -= 1;
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
            self.sp += 1;
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
#[derive(Debug, Clone, Copy, Default)]
pub struct VmRunOptions {
    /// Maximum number of instructions that may execute before the VM aborts
    /// with an error.
    pub max_instructions:    Option<usize>,
    /// Maximum number of active return frames.
    pub max_recursion_depth: Option<usize>,
    /// Maximum runtime stack size in 32-bit cells.
    pub max_stack_cells:     Option<usize>,
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
            .finish()
    }
}

impl Vm {
    /// Creates an empty VM command table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers one command handler by its numeric action id.
    pub fn define_command<F>(&mut self, command: u16, handler: F)
    where
        F: Fn(&mut VmScript, u16, u8) -> Result<(), VmError> + 'static,
    {
        let index = usize::from(command);
        if self.commands.len() <= index {
            self.commands.resize_with(index + 1, || None);
        }
        if let Some(slot) = self.commands.get_mut(index) {
            *slot = Some(Box::new(handler));
        }
    }

    /// Registers one zero-metadata command handler.
    pub fn define_simple_command<F>(&mut self, command: u16, handler: F)
    where
        F: Fn(&mut VmScript) -> Result<(), VmError> + 'static,
    {
        self.define_command(command, move |script, _command, _argc| handler(script));
    }

    /// Registers one engine-structure default factory by its numeric type
    /// index.
    pub fn define_engine_structure<F>(&mut self, index: u8, factory: F)
    where
        F: Fn(u8) -> VmEngineStructureValue + 'static,
    {
        let index = usize::from(index);
        if self.engine_structures.len() <= index {
            self.engine_structures.resize_with(index + 1, || None);
        }
        if let Some(slot) = self.engine_structures.get_mut(index) {
            *slot = Some(Box::new(factory));
        }
    }

    /// Registers one fixed default engine-structure value by its numeric type
    /// index.
    pub fn define_engine_structure_default(&mut self, index: u8, value: VmEngineStructureValue) {
        self.define_engine_structure(index, move |_index| value.clone());
    }

    /// Registers one engine-structure equality hook by its numeric type index.
    pub fn define_engine_structure_comparer<F>(&mut self, index: u8, comparer: F)
    where
        F: Fn(u8, &VmEngineStructureValue, &VmEngineStructureValue) -> bool + 'static,
    {
        let index = usize::from(index);
        if self.engine_structure_comparers.len() <= index {
            self.engine_structure_comparers
                .resize_with(index + 1, || None);
        }
        if let Some(slot) = self.engine_structure_comparers.get_mut(index) {
            *slot = Some(Box::new(comparer));
        }
    }

    /// Registers one instruction trace hook invoked before each opcode
    /// executes.
    pub fn define_trace_hook<F>(&mut self, hook: F)
    where
        F: Fn(&VmScript, &VmTraceEvent) + 'static,
    {
        self.trace_hook = Some(Box::new(hook));
    }

    /// Removes the currently registered instruction trace hook.
    pub fn clear_trace_hook(&mut self) {
        self.trace_hook = None;
    }

    /// Executes one script until it returns from the outermost frame.
    ///
    /// # Errors
    ///
    /// Returns [`VmError`] if execution fails.
    pub fn run(&self, script: &mut VmScript) -> Result<(), VmError> {
        self.run_with_options(script, VmRunOptions::default())
    }

    /// Executes exactly one instruction.
    ///
    /// # Errors
    ///
    /// Returns [`VmError`] if decoding or execution fails.
    pub fn step(&self, script: &mut VmScript) -> Result<VmStepOutcome, VmError> {
        const HALT_IP: usize = usize::MAX;

        script.aborted = false;

        if script.ret.is_empty() {
            script.ret.push(VmReturnFrame {
                target: HALT_IP
            });
        }

        if consume_abort_request(script) {
            return Ok(VmStepOutcome::Aborted);
        }

        let decoded = script
            .program
            .instruction_at(script.ip)
            .ok_or(VmError::InvalidInstructionPointer {
                offset: script.ip
            })?
            .clone();
        self.emit_trace(script, &decoded);
        let next_ip = decoded.offset + decoded.instruction.encoded_len();

        match decoded.instruction.opcode {
            NcsOpcode::NoOperation => {
                script.ip = next_ip;
            }
            NcsOpcode::Jmp => {
                script.ip = jump_target(decoded.offset, read_i32(&decoded, 0)?)?;
            }
            NcsOpcode::Jsr => {
                let target = jump_target(decoded.offset, read_i32(&decoded, 0)?)?;
                script.ret.push(VmReturnFrame {
                    target: next_ip
                });
                script.ip = target;
            }
            NcsOpcode::Jz => {
                if script.pop_int()? == 0 {
                    script.ip = jump_target(decoded.offset, read_i32(&decoded, 0)?)?;
                } else {
                    script.ip = next_ip;
                }
            }
            NcsOpcode::Jnz => {
                if script.pop_int()? != 0 {
                    script.ip = jump_target(decoded.offset, read_i32(&decoded, 0)?)?;
                } else {
                    script.ip = next_ip;
                }
            }
            NcsOpcode::Ret => {
                let frame = script.ret.pop().ok_or_else(|| VmError::StackUnderflow {
                    message: "attempted to return without a return frame".to_string(),
                })?;
                if frame.target == HALT_IP {
                    return Ok(VmStepOutcome::Halted);
                }
                script.ip = frame.target;
            }
            NcsOpcode::SaveBasePointer => {
                script.push_int(
                    i32::try_from(script.bp).map_err(|_error| {
                        invalid_extra(&decoded, "base pointer exceeds i32 range")
                    })?,
                );
                script.bp = script.sp.saturating_sub(1);
                script.ip = next_ip;
            }
            NcsOpcode::RestoreBasePointer => {
                script.bp = usize::try_from(script.pop_int()?)
                    .map_err(|_error| invalid_extra(&decoded, "negative base pointer restore"))?;
                script.ip = next_ip;
            }
            NcsOpcode::RunstackAdd => {
                push_default_value(script, &decoded, self)?;
                script.ip = next_ip;
            }
            NcsOpcode::RunstackCopy | NcsOpcode::RunstackCopyBase => {
                let base = if decoded.instruction.opcode == NcsOpcode::RunstackCopyBase {
                    script.bp
                } else {
                    script.sp
                };
                let src = relative_stack_cell(&decoded, base, read_i32(&decoded, 0)?)?;
                let cells = usize::from(read_u16(&decoded, 4)?) / 4;
                for index in 0..cells {
                    script.assign_cell(src + index, script.sp + index)?;
                }
                script.ip = next_ip;
            }
            NcsOpcode::Assignment | NcsOpcode::AssignmentBase => {
                let cells = usize::from(read_u16(&decoded, 4)?) / 4;
                let dst = if decoded.instruction.opcode == NcsOpcode::AssignmentBase {
                    relative_stack_cell(&decoded, script.bp, read_i32(&decoded, 0)?)?
                } else {
                    let encoded_offset = read_i32(&decoded, 0)?;
                    match relative_stack_cell(&decoded, script.sp, encoded_offset) {
                        Ok(dst) => dst,
                        Err(VmError::StackUnderflow {
                            ..
                        }) => relative_stack_cell(&decoded, script.sp + cells, encoded_offset)?,
                        Err(error) => return Err(error),
                    }
                };
                for index in 0..cells {
                    script.assign_cell(script.sp.saturating_sub(cells) + index, dst + index)?;
                }
                script.ip = next_ip;
            }
            NcsOpcode::Constant => {
                push_constant_value(script, &decoded)?;
                script.ip = next_ip;
            }
            NcsOpcode::ModifyStackPointer => {
                let byte_delta = read_i32(&decoded, 0)?;
                if byte_delta > 0 {
                    let cells = usize::try_from(byte_delta / 4)
                        .map_err(|_error| invalid_extra(&decoded, "invalid MOVSP payload"))?;
                    for _ in 0..cells {
                        script.push_int(0);
                    }
                    script.ip = next_ip;
                    return Ok(VmStepOutcome::Running);
                }
                let cells = usize::try_from((-byte_delta) / 4)
                    .map_err(|_error| invalid_extra(&decoded, "invalid MOVSP payload"))?;
                script.set_stack_pointer(script.sp.saturating_sub(cells))?;
                script.ip = next_ip;
            }
            NcsOpcode::Increment
            | NcsOpcode::Decrement
            | NcsOpcode::IncrementBase
            | NcsOpcode::DecrementBase => {
                let base = if matches!(
                    decoded.instruction.opcode,
                    NcsOpcode::IncrementBase | NcsOpcode::DecrementBase
                ) {
                    script.bp
                } else {
                    script.sp
                };
                let dst = relative_stack_cell(&decoded, base, read_i32(&decoded, 0)?)?;
                let delta = if matches!(
                    decoded.instruction.opcode,
                    NcsOpcode::Increment | NcsOpcode::IncrementBase
                ) {
                    1
                } else {
                    -1
                };
                let value = script
                    .stack
                    .get_mut(dst)
                    .ok_or_else(|| VmError::StackUnderflow {
                        message: format!("attempted to update missing stack cell {dst}"),
                    })?;
                match value {
                    VmValue::Int(int_value) => *int_value += delta,
                    other => {
                        return Err(VmError::TypeMismatch {
                            offset:   decoded.offset,
                            message:  "increment/decrement requires integer target".to_string(),
                            expected: Some("int"),
                            actual:   other.kind_name(),
                        });
                    }
                }
                script.ip = next_ip;
            }
            NcsOpcode::Negation => {
                match decoded.instruction.auxcode {
                    NcsAuxCode::TypeInteger => {
                        let value = script.pop_int()?;
                        script.push_int(-value);
                    }
                    NcsAuxCode::TypeFloat => {
                        let value = script.pop_float()?;
                        script.push_float(-value);
                    }
                    NcsAuxCode::TypeTypeVectorVector => {
                        let [x, y, z] = script.pop_vector()?;
                        script.push_vector([-x, -y, -z]);
                    }
                    _ => {
                        return unsupported(
                            &decoded,
                            "NEG only supports integer, float, and vector",
                        );
                    }
                }
                script.ip = next_ip;
            }
            NcsOpcode::Equal
            | NcsOpcode::NotEqual
            | NcsOpcode::Lt
            | NcsOpcode::Gt
            | NcsOpcode::Leq
            | NcsOpcode::Geq => {
                apply_comparison(script, &decoded, self)?;
                script.ip = next_ip;
            }
            NcsOpcode::LogicalOr => {
                let rhs = script.pop_int()? != 0;
                let lhs = script.pop_int()? != 0;
                script.push_int(bool_to_int(lhs || rhs));
                script.ip = next_ip;
            }
            NcsOpcode::LogicalAnd => {
                let rhs = script.pop_int()? != 0;
                let lhs = script.pop_int()? != 0;
                script.push_int(bool_to_int(lhs && rhs));
                script.ip = next_ip;
            }
            NcsOpcode::InclusiveOr => {
                let rhs = script.pop_int()?;
                let lhs = script.pop_int()?;
                script.push_int(lhs | rhs);
                script.ip = next_ip;
            }
            NcsOpcode::ExclusiveOr => {
                let rhs = script.pop_int()?;
                let lhs = script.pop_int()?;
                script.push_int(lhs ^ rhs);
                script.ip = next_ip;
            }
            NcsOpcode::BooleanAnd => {
                let rhs = script.pop_int()?;
                let lhs = script.pop_int()?;
                script.push_int(lhs & rhs);
                script.ip = next_ip;
            }
            NcsOpcode::BooleanNot => {
                let value = script.pop_int()? == 0;
                script.push_int(bool_to_int(value));
                script.ip = next_ip;
            }
            NcsOpcode::OnesComplement => {
                let value = script.pop_int()?;
                script.push_int(!value);
                script.ip = next_ip;
            }
            NcsOpcode::ShiftLeft => {
                let rhs = script.pop_int()?;
                let lhs = script.pop_int()?;
                script.push_int(lhs.wrapping_shl(rhs as u32));
                script.ip = next_ip;
            }
            NcsOpcode::ShiftRight => {
                let rhs = script.pop_int()?;
                let lhs = script.pop_int()?;
                script.push_int(lhs.wrapping_shr(rhs as u32));
                script.ip = next_ip;
            }
            NcsOpcode::UShiftRight => {
                let rhs = script.pop_int()?;
                let lhs = script.pop_int()?;
                script.push_int(((lhs as u32).wrapping_shr(rhs as u32)) as i32);
                script.ip = next_ip;
            }
            NcsOpcode::Add => {
                apply_add(script, &decoded)?;
                script.ip = next_ip;
            }
            NcsOpcode::Sub => {
                apply_sub(script, &decoded)?;
                script.ip = next_ip;
            }
            NcsOpcode::Mul => {
                apply_mul(script, &decoded)?;
                script.ip = next_ip;
            }
            NcsOpcode::Div => {
                apply_div(script, &decoded)?;
                script.ip = next_ip;
            }
            NcsOpcode::Modulus => {
                let rhs = script.pop_int()?;
                let lhs = script.pop_int()?;
                if rhs == 0 {
                    return Err(VmError::DivideByZero {
                        offset: decoded.offset,
                    });
                }
                script.push_int(lhs % rhs);
                script.ip = next_ip;
            }
            NcsOpcode::DeStruct => {
                let size_orig = usize::from(read_u16(&decoded, 0)?) / 4;
                let start = usize::from(read_u16(&decoded, 2)?) / 4;
                let size = usize::from(read_u16(&decoded, 4)?) / 4;

                if size + start < size_orig {
                    let new_sp = script.sp.saturating_sub(size_orig) + size + start;
                    script.set_stack_pointer(new_sp)?;
                }

                if start > 0 {
                    let from = script.sp.saturating_sub(size + start);
                    let to = script.sp.saturating_sub(start);
                    for index in from..to {
                        script.assign_cell(index + start, index)?;
                    }
                    script.set_stack_pointer(script.sp.saturating_sub(start))?;
                }
                script.ip = next_ip;
            }
            NcsOpcode::StoreIp | NcsOpcode::StoreState => {
                script.save_ip =
                    jump_target(decoded.offset, i32::from(decoded.instruction.auxcode as u8))?;
                if decoded.instruction.opcode == NcsOpcode::StoreState {
                    let situation = capture_saved_situation(script, &decoded, script.save_ip)?;
                    script.save_bp = situation.bp;
                    script.save_sp = situation.sp;
                    script.saved_situation = Some(situation);
                }
                script.ip = next_ip;
            }
            NcsOpcode::ExecuteCommand => {
                let command = read_u16(&decoded, 0)?;
                let argc = read_u8(&decoded, 2)?;
                let Some(handler) = self
                    .commands
                    .get(usize::from(command))
                    .and_then(Option::as_ref)
                else {
                    return Err(VmError::InvalidCommand {
                        offset: decoded.offset,
                        command,
                    });
                };
                handler(script, command, argc)?;
                if consume_abort_request(script) {
                    return Ok(VmStepOutcome::Aborted);
                }
                script.ip = next_ip;
            }
        }

        Ok(VmStepOutcome::Running)
    }

    /// Continues execution until the script is about to execute the instruction
    /// at `offset`.
    ///
    /// If the script reaches the requested offset, this returns
    /// [`VmStepOutcome::Running`] without executing that instruction.
    ///
    /// # Errors
    ///
    /// Returns [`VmError`] if execution fails or exceeds the configured
    /// instruction budget.
    pub fn run_until_offset(
        &self,
        script: &mut VmScript,
        offset: usize,
        options: VmRunOptions,
    ) -> Result<VmStepOutcome, VmError> {
        let mut instructions_executed = 0usize;
        loop {
            if script.ip == offset {
                return Ok(VmStepOutcome::Running);
            }
            check_run_limits(script, options)?;
            if let Some(limit) = options.max_instructions
                && instructions_executed >= limit
            {
                return Err(VmError::InstructionLimitExceeded {
                    offset: script.ip,
                    limit,
                });
            }
            instructions_executed += 1;
            match self.step(script)? {
                VmStepOutcome::Running => {}
                outcome => return Ok(outcome),
            }
        }
    }

    /// Executes the current instruction, stepping over user-function calls.
    ///
    /// For non-`JSR` instructions this behaves like [`Vm::step`]. For `JSR`, it
    /// runs until control returns to the caller, preserving a debugger-friendly
    /// "step over" behavior.
    ///
    /// # Errors
    ///
    /// Returns [`VmError`] if execution fails or exceeds the configured
    /// instruction budget.
    pub fn step_over(
        &self,
        script: &mut VmScript,
        options: VmRunOptions,
    ) -> Result<VmStepOutcome, VmError> {
        let Some(instruction) = script.current_instruction().cloned() else {
            return Err(VmError::InvalidInstructionPointer {
                offset: script.ip
            });
        };
        if instruction.opcode != NcsOpcode::Jsr {
            return self.step(script);
        }

        let return_offset = script.ip + instruction.encoded_len();
        let depth = script.ret.len();
        check_run_limits(script, options)?;
        match self.step(script)? {
            VmStepOutcome::Running => {}
            outcome => return Ok(outcome),
        }

        let mut instructions_executed = 0usize;
        loop {
            if script.ip == return_offset && script.ret.len() == depth {
                return Ok(VmStepOutcome::Running);
            }
            check_run_limits(script, options)?;
            if let Some(limit) = options.max_instructions
                && instructions_executed >= limit
            {
                return Err(VmError::InstructionLimitExceeded {
                    offset: script.ip,
                    limit,
                });
            }
            instructions_executed += 1;
            match self.step(script)? {
                VmStepOutcome::Running => {}
                outcome => return Ok(outcome),
            }
        }
    }

    /// Continues execution until the current function returns to its caller.
    ///
    /// If the script is currently at top level, this runs until the script
    /// halts or aborts.
    ///
    /// # Errors
    ///
    /// Returns [`VmError`] if execution fails or exceeds the configured
    /// instruction budget.
    pub fn step_out(
        &self,
        script: &mut VmScript,
        options: VmRunOptions,
    ) -> Result<VmStepOutcome, VmError> {
        let depth = script.ret.len();
        let mut instructions_executed = 0usize;
        loop {
            check_run_limits(script, options)?;
            if let Some(limit) = options.max_instructions
                && instructions_executed >= limit
            {
                return Err(VmError::InstructionLimitExceeded {
                    offset: script.ip,
                    limit,
                });
            }
            instructions_executed += 1;
            match self.step(script)? {
                VmStepOutcome::Running => {
                    if script.ret.len() < depth {
                        return Ok(VmStepOutcome::Running);
                    }
                }
                outcome => return Ok(outcome),
            }
        }
    }

    /// Continues execution until the script is about to execute one instruction
    /// mapped to `file_name:line_number` in attached `NDB` metadata.
    ///
    /// If the script reaches the requested line, this returns
    /// [`VmStepOutcome::Running`] without executing that instruction.
    ///
    /// # Errors
    ///
    /// Returns [`VmError`] if the script has no matching debug mapping,
    /// execution fails, or the instruction budget is exceeded.
    pub fn run_until_line(
        &self,
        script: &mut VmScript,
        file_name: &str,
        line_number: usize,
        options: VmRunOptions,
    ) -> Result<VmStepOutcome, VmError> {
        let target = script
            .program
            .source_lines
            .iter()
            .find(|line| {
                line.file_name.eq_ignore_ascii_case(file_name) && line.line_number == line_number
            })
            .map(|line| line.start)
            .ok_or_else(|| VmError::Setup {
                message: format!("no attached source line mapping for {file_name}:{line_number}"),
            })?;
        self.run_until_offset(script, target, options)
    }

    /// Continues execution until the script is about to execute one instruction
    /// inside the named function from attached `NDB` metadata.
    ///
    /// If the script reaches the requested function, this returns
    /// [`VmStepOutcome::Running`] without executing that instruction.
    ///
    /// # Errors
    ///
    /// Returns [`VmError`] if the script has no matching debug mapping,
    /// execution fails, or the instruction budget is exceeded.
    pub fn run_until_function(
        &self,
        script: &mut VmScript,
        name: &str,
        options: VmRunOptions,
    ) -> Result<VmStepOutcome, VmError> {
        let target = script
            .program
            .functions
            .iter()
            .find(|function| function.label == name)
            .map(|function| function.start)
            .ok_or_else(|| VmError::Setup {
                message: format!("no attached function mapping for {name:?}"),
            })?;
        self.run_until_offset(script, target, options)
    }

    /// Executes one script with explicit runtime controls until it returns from
    /// the outermost frame.
    ///
    /// # Errors
    ///
    /// Returns [`VmError`] if execution fails or exceeds the configured
    /// instruction budget.
    pub fn run_with_options(
        &self,
        script: &mut VmScript,
        options: VmRunOptions,
    ) -> Result<(), VmError> {
        let mut instructions_executed = 0usize;

        loop {
            check_run_limits(script, options)?;
            if let Some(limit) = options.max_instructions
                && instructions_executed >= limit
            {
                return Err(VmError::InstructionLimitExceeded {
                    offset: script.ip,
                    limit,
                });
            }
            instructions_executed += 1;
            match self.step(script)? {
                VmStepOutcome::Running => {}
                VmStepOutcome::Halted | VmStepOutcome::Aborted => break,
            }
        }

        Ok(())
    }

    /// Executes one previously captured NWScript action situation.
    ///
    /// # Errors
    ///
    /// Returns [`VmError`] if execution fails.
    pub fn run_situation(&self, situation: &VmSituation) -> Result<VmScript, VmError> {
        self.run_situation_with_options(situation, VmRunOptions::default())
    }

    /// Executes one previously captured NWScript action situation with explicit
    /// runtime controls.
    ///
    /// # Errors
    ///
    /// Returns [`VmError`] if execution fails.
    pub fn run_situation_with_options(
        &self,
        situation: &VmSituation,
        options: VmRunOptions,
    ) -> Result<VmScript, VmError> {
        let mut script = situation.to_script();
        self.run_with_options(&mut script, options)?;
        Ok(script)
    }

    /// Decodes one script, runs it, and returns the finished runtime state.
    ///
    /// # Errors
    ///
    /// Returns [`VmError`] if decoding or execution fails.
    pub fn run_bytes(&self, bytes: &[u8], label: impl Into<String>) -> Result<VmScript, VmError> {
        self.run_bytes_with_options(bytes, label, VmRunOptions::default())
    }

    /// Decodes one script, runs it with explicit runtime controls, and returns
    /// the finished runtime state.
    ///
    /// # Errors
    ///
    /// Returns [`VmError`] if decoding or execution fails.
    pub fn run_bytes_with_options(
        &self,
        bytes: &[u8],
        label: impl Into<String>,
        options: VmRunOptions,
    ) -> Result<VmScript, VmError> {
        let mut script = VmScript::from_bytes(bytes, label)?;
        self.run_with_options(&mut script, options)?;
        Ok(script)
    }

    /// Decodes one script, attaches one `NDB` table, runs it, and returns the
    /// finished runtime state.
    ///
    /// Normal user-function calls do not require `NDB`; this variant adds
    /// function names and source locations for debugger operations.
    ///
    /// # Errors
    ///
    /// Returns [`VmError`] if decoding, metadata attachment, or execution
    /// fails.
    pub fn run_bytes_with_ndb(
        &self,
        bytes: &[u8],
        label: impl Into<String>,
        ndb: &Ndb,
    ) -> Result<VmScript, VmError> {
        self.run_bytes_with_ndb_and_options(bytes, label, ndb, VmRunOptions::default())
    }

    /// Decodes one script, attaches one `NDB` table, and runs it with explicit
    /// runtime controls.
    ///
    /// # Errors
    ///
    /// Returns [`VmError`] if decoding, metadata attachment, or execution
    /// fails.
    pub fn run_bytes_with_ndb_and_options(
        &self,
        bytes: &[u8],
        label: impl Into<String>,
        ndb: &Ndb,
        options: VmRunOptions,
    ) -> Result<VmScript, VmError> {
        let mut script = VmScript::from_bytes_with_ndb(bytes, label, ndb)?;
        self.run_with_options(&mut script, options)?;
        Ok(script)
    }

    /// Decodes one script, prepares a named direct function call, and runs it.
    ///
    /// This bypasses the compiler-emitted loader and initializes globals
    /// automatically for `main()`-style entry loaders when needed.
    ///
    /// # Errors
    ///
    /// Returns [`VmError`] if decoding fails, the function cannot be invoked
    /// directly, or execution fails.
    pub fn run_function_bytes(
        &self,
        bytes: &[u8],
        label: impl Into<String>,
        ndb: &Ndb,
        name: &str,
        args: &[VmValue],
    ) -> Result<VmScript, VmError> {
        self.run_function_bytes_with_options(bytes, label, ndb, name, args, VmRunOptions::default())
    }

    /// Decodes one script, prepares a named direct function call, and runs it
    /// with explicit runtime controls.
    ///
    /// # Errors
    ///
    /// Returns [`VmError`] if decoding fails, the function cannot be invoked
    /// directly, execution fails, or the instruction budget is exceeded.
    pub fn run_function_bytes_with_options(
        &self,
        bytes: &[u8],
        label: impl Into<String>,
        ndb: &Ndb,
        name: &str,
        args: &[VmValue],
        options: VmRunOptions,
    ) -> Result<VmScript, VmError> {
        let mut script = VmScript::from_bytes_with_ndb(bytes, label, ndb)?;
        if script_has_globals(ndb) {
            self.bootstrap_globals_for_direct_call(&mut script, ndb, options)?;
        }
        script.prepare_function_call(ndb, name, args)?;
        self.run_with_options(&mut script, options)?;
        Ok(script)
    }

    fn default_engine_structure_value(&self, index: u8) -> VmEngineStructureValue {
        self.engine_structures
            .get(usize::from(index))
            .and_then(Option::as_ref)
            .map_or_else(
                || default_engine_structure_value(index),
                |factory| factory(index),
            )
    }

    fn compare_engine_structure_values(
        &self,
        index: u8,
        lhs: &VmEngineStructureValue,
        rhs: &VmEngineStructureValue,
    ) -> bool {
        self.engine_structure_comparers
            .get(usize::from(index))
            .and_then(Option::as_ref)
            .map_or_else(|| lhs == rhs, |comparer| comparer(index, lhs, rhs))
    }

    fn emit_trace(&self, script: &VmScript, decoded: &VmProgramInstruction) {
        let Some(hook) = self.trace_hook.as_ref() else {
            return;
        };
        hook(
            script,
            &VmTraceEvent {
                offset:      decoded.offset,
                ip:          script.ip,
                sp:          script.sp,
                bp:          script.bp,
                instruction: decoded.instruction.clone(),
            },
        );
    }

    fn bootstrap_globals_for_direct_call(
        &self,
        script: &mut VmScript,
        ndb: &Ndb,
        options: VmRunOptions,
    ) -> Result<(), VmError> {
        if !script_has_globals(ndb) {
            return Ok(());
        }

        let entry_name = entry_function_name(ndb).ok_or_else(|| VmError::Setup {
            message: "script entry function is missing".to_string(),
        })?;
        let entry = ndb
            .functions
            .iter()
            .find(|function| function.label == entry_name)
            .ok_or_else(|| VmError::Setup {
                message: format!("missing NDB entry for {entry_name:?}"),
            })?;
        let entry_start = ndb_code_offset(
            entry.binary_start,
            &format!("entry function {entry_name:?} start"),
        )?;

        let mut bootstrap =
            VmScript::from_instructions(Vec::new(), format!("{}#bootstrap", script.label));
        bootstrap.program = script.program.clone();
        let entry_index = *bootstrap.program.offsets_to_index.get(&entry_start).ok_or(
            VmError::InvalidInstructionPointer {
                offset: entry_start,
            },
        )?;
        let Some(instruction) = bootstrap.program.instructions.get_mut(entry_index) else {
            return Err(VmError::InvalidInstructionPointer {
                offset: entry_start,
            });
        };
        instruction.instruction = NcsInstruction {
            opcode:  NcsOpcode::Ret,
            auxcode: NcsAuxCode::None,
            extra:   Vec::new(),
        };
        let global_cleanup = bootstrap
            .program
            .instructions
            .windows(2)
            .position(|window| {
                let [restore, cleanup] = window else {
                    return false;
                };
                restore.offset < entry_start
                    && restore.instruction.opcode == NcsOpcode::RestoreBasePointer
                    && cleanup.instruction.opcode == NcsOpcode::ModifyStackPointer
            })
            .map(|index| index + 1)
            .ok_or_else(|| VmError::Setup {
                message: "global loader is missing its post-entry stack cleanup".to_string(),
            })?;
        let cleanup = bootstrap
            .program
            .instructions
            .get_mut(global_cleanup)
            .ok_or_else(|| VmError::Setup {
                message: "global loader cleanup index is out of bounds".to_string(),
            })?;
        cleanup.instruction = NcsInstruction {
            opcode:  NcsOpcode::Ret,
            auxcode: NcsAuxCode::None,
            extra:   Vec::new(),
        };
        self.run_with_options(&mut bootstrap, options)?;

        let globals_cells = global_stack_cells(ndb)?;
        let prefix_cells = loader_prefix_cells(ndb)?;
        let globals_end = prefix_cells + globals_cells;
        let globals = bootstrap
            .stack
            .get(prefix_cells..globals_end)
            .ok_or_else(|| VmError::StackUnderflow {
                message: format!(
                    "bootstrapped globals frame expected cells {}..{}, but stack only has {}",
                    prefix_cells,
                    globals_end,
                    bootstrap.stack.len()
                ),
            })?
            .to_vec();
        script.ip = 0;
        script.sp = globals.len();
        script.bp = 0;
        script.ret.clear();
        script.stack = globals;
        if globals_cells > 0 {
            script.push_int(0);
            script.bp = globals_cells;
        }
        script.save_ip = 0;
        script.save_sp = 0;
        script.save_bp = 0;
        script.saved_situation = None;
        script.abort_requested = false;
        script.aborted = false;
        Ok(())
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

fn read_u8(decoded: &VmProgramInstruction, start: usize) -> Result<u8, VmError> {
    decoded
        .instruction
        .extra
        .get(start)
        .copied()
        .ok_or_else(|| invalid_extra(decoded, "payload ended early"))
}

fn read_u16(decoded: &VmProgramInstruction, start: usize) -> Result<u16, VmError> {
    let window = decoded
        .instruction
        .extra
        .get(start..start + 2)
        .ok_or_else(|| invalid_extra(decoded, "payload ended early"))?;
    let bytes =
        <[u8; 2]>::try_from(window).map_err(|_error| invalid_extra(decoded, "bad u16 payload"))?;
    Ok(u16::from_be_bytes(bytes))
}

fn read_i32(decoded: &VmProgramInstruction, start: usize) -> Result<i32, VmError> {
    let window = decoded
        .instruction
        .extra
        .get(start..start + 4)
        .ok_or_else(|| invalid_extra(decoded, "payload ended early"))?;
    let bytes =
        <[u8; 4]>::try_from(window).map_err(|_error| invalid_extra(decoded, "bad i32 payload"))?;
    Ok(i32::from_be_bytes(bytes))
}

fn read_u32(decoded: &VmProgramInstruction, start: usize) -> Result<u32, VmError> {
    let window = decoded
        .instruction
        .extra
        .get(start..start + 4)
        .ok_or_else(|| invalid_extra(decoded, "payload ended early"))?;
    let bytes =
        <[u8; 4]>::try_from(window).map_err(|_error| invalid_extra(decoded, "bad u32 payload"))?;
    Ok(u32::from_be_bytes(bytes))
}

fn read_f32(decoded: &VmProgramInstruction, start: usize) -> Result<f32, VmError> {
    Ok(f32::from_bits(read_u32(decoded, start)?))
}

fn read_ncs_string(decoded: &VmProgramInstruction) -> Result<ScriptString, VmError> {
    let length = usize::from(read_u16(decoded, 0)?);
    let window = decoded
        .instruction
        .extra
        .get(2..2 + length)
        .ok_or_else(|| invalid_extra(decoded, "string payload shorter than declared length"))?;
    Ok(ScriptString::new(window.to_vec()))
}

fn read_ncs_text(decoded: &VmProgramInstruction) -> Result<String, VmError> {
    let value = read_ncs_string(decoded)?;
    value
        .as_str()
        .map(str::to_owned)
        .map_err(|error| invalid_extra(decoded, &format!("invalid UTF-8 text payload: {error}")))
}

fn relative_stack_cell(
    decoded: &VmProgramInstruction,
    base: usize,
    encoded_offset: i32,
) -> Result<usize, VmError> {
    if encoded_offset > 0 || encoded_offset % 4 != 0 {
        return Err(invalid_extra(
            decoded,
            &format!("expected negative 4-byte-aligned stack offset, got {encoded_offset}"),
        ));
    }
    let cells = usize::try_from((-encoded_offset) / 4)
        .map_err(|_error| invalid_extra(decoded, "invalid negative stack offset"))?;
    base.checked_sub(cells)
        .ok_or_else(|| VmError::StackUnderflow {
            message: format!("stack offset {encoded_offset} underflowed base {base}"),
        })
}

fn jump_target(current: usize, delta: i32) -> Result<usize, VmError> {
    if delta >= 0 {
        current
            .checked_add(usize::try_from(delta).ok().unwrap_or(usize::MAX))
            .ok_or(VmError::InvalidInstructionPointer {
                offset: current
            })
    } else {
        current
            .checked_sub(usize::try_from(-delta).ok().unwrap_or(usize::MAX))
            .ok_or(VmError::InvalidInstructionPointer {
                offset: current
            })
    }
}

fn capture_saved_situation(
    script: &VmScript,
    decoded: &VmProgramInstruction,
    target_ip: usize,
) -> Result<VmSituation, VmError> {
    let global_bytes = usize::try_from(read_u32(decoded, 0)?)
        .map_err(|_error| invalid_extra(decoded, "global size exceeds usize range"))?;
    let stack_bytes = usize::try_from(read_u32(decoded, 4)?)
        .map_err(|_error| invalid_extra(decoded, "stack size exceeds usize range"))?;
    if global_bytes % 4 != 0 || stack_bytes % 4 != 0 {
        return Err(invalid_extra(
            decoded,
            "saved state sizes must be 4-byte aligned",
        ));
    }

    let saved_sp = (global_bytes + stack_bytes) / 4;
    if saved_sp > script.sp {
        return Err(VmError::StackUnderflow {
            message: format!(
                "saved situation requested {} cells, but stack only has {}",
                saved_sp, script.sp
            ),
        });
    }

    Ok(VmSituation {
        label:   script.label.clone(),
        program: script.program.clone(),
        ip:      target_ip,
        sp:      saved_sp,
        bp:      script.bp,
        stack:   script
            .stack
            .get(..saved_sp)
            .ok_or_else(|| VmError::StackUnderflow {
                message: format!(
                    "saved situation requested {} cells, but stack only has {}",
                    saved_sp,
                    script.stack.len()
                ),
            })?
            .to_vec(),
    })
}

fn default_engine_structure_value(index: u8) -> VmEngineStructureValue {
    if index == 7 {
        VmEngineStructureValue::Text(String::new())
    } else {
        VmEngineStructureValue::Word(0)
    }
}

fn consume_abort_request(script: &mut VmScript) -> bool {
    if script.abort_requested {
        script.abort_requested = false;
        script.aborted = true;
        script.ret.clear();
        return true;
    }
    false
}

fn cells_for_ndb_type(ndb: &Ndb, ty: &NdbType) -> Result<usize, VmError> {
    Ok(match ty {
        NdbType::Float
        | NdbType::Int
        | NdbType::Object
        | NdbType::String
        | NdbType::EngineStructure(_) => 1,
        NdbType::Void => 0,
        NdbType::Struct(struct_index) => ndb
            .structs
            .get(*struct_index)
            .ok_or_else(|| VmError::Setup {
                message: format!("NDB references missing structure t{struct_index:04}"),
            })?
            .fields
            .iter()
            .try_fold(0usize, |total, field| {
                cells_for_ndb_type(ndb, &field.ty).map(|width| total + width)
            })?,
        NdbType::Unknown | NdbType::Raw(_) => {
            return Err(VmError::Setup {
                message: format!("unsupported VM function metadata type {ty}"),
            });
        }
    })
}

fn check_run_limits(script: &VmScript, options: VmRunOptions) -> Result<(), VmError> {
    if let Some(limit) = options.max_recursion_depth
        && script.ret.len() > limit
    {
        return Err(VmError::RecursionLimitExceeded {
            depth: script.ret.len(),
            limit,
        });
    }
    if let Some(limit) = options.max_stack_cells
        && script.stack.len() > limit
    {
        return Err(VmError::StackLimitExceeded {
            cells: script.stack.len(),
            limit,
        });
    }
    Ok(())
}

fn push_default_value(
    script: &mut VmScript,
    decoded: &VmProgramInstruction,
    vm: &Vm,
) -> Result<(), VmError> {
    match decoded.instruction.auxcode {
        NcsAuxCode::TypeInteger => script.push_int(0),
        NcsAuxCode::TypeFloat => script.push_float(0.0),
        NcsAuxCode::TypeString => script.push_string(ScriptString::default()),
        NcsAuxCode::TypeObject => script.push_object(0),
        aux => {
            let Some(index) = engine_structure_index(aux) else {
                return unsupported(decoded, "RSADD does not support this auxcode");
            };
            script.push_engine_structure(index, vm.default_engine_structure_value(index));
        }
    }
    Ok(())
}

fn push_constant_value(
    script: &mut VmScript,
    decoded: &VmProgramInstruction,
) -> Result<(), VmError> {
    match decoded.instruction.auxcode {
        NcsAuxCode::TypeInteger => script.push_int(read_i32(decoded, 0)?),
        NcsAuxCode::TypeFloat => script.push_float(read_f32(decoded, 0)?),
        NcsAuxCode::TypeString => script.push_string(read_ncs_string(decoded)?),
        NcsAuxCode::TypeObject => script.push_object(read_u32(decoded, 0)?),
        aux => {
            let Some(index) = engine_structure_index(aux) else {
                return unsupported(decoded, "CONST does not support this auxcode");
            };
            let value = if index == 7 {
                VmEngineStructureValue::Text(read_ncs_text(decoded)?)
            } else {
                VmEngineStructureValue::Word(read_u32(decoded, 0)?)
            };
            script.push(VmValue::EngineStructure {
                index,
                value,
            });
        }
    }
    Ok(())
}

fn engine_structure_index(auxcode: NcsAuxCode) -> Option<u8> {
    match auxcode {
        NcsAuxCode::TypeEngst0 => Some(0),
        NcsAuxCode::TypeEngst1 => Some(1),
        NcsAuxCode::TypeEngst2 => Some(2),
        NcsAuxCode::TypeEngst3 => Some(3),
        NcsAuxCode::TypeEngst4 => Some(4),
        NcsAuxCode::TypeEngst5 => Some(5),
        NcsAuxCode::TypeEngst6 => Some(6),
        NcsAuxCode::TypeEngst7 => Some(7),
        NcsAuxCode::TypeEngst8 => Some(8),
        NcsAuxCode::TypeEngst9 => Some(9),
        _ => None,
    }
}

fn apply_comparison(
    script: &mut VmScript,
    decoded: &VmProgramInstruction,
    vm: &Vm,
) -> Result<(), VmError> {
    if decoded.instruction.auxcode == NcsAuxCode::TypeTypeVectorVector {
        let rhs = script.pop_vector()?;
        let lhs = script.pop_vector()?;
        let result = match decoded.instruction.opcode {
            NcsOpcode::Equal => lhs == rhs,
            NcsOpcode::NotEqual => lhs != rhs,
            _ => return unsupported(decoded, "ordered comparisons are not valid for vectors"),
        };
        script.push_int(bool_to_int(result));
        return Ok(());
    }

    if decoded.instruction.auxcode == NcsAuxCode::TypeTypeStructStruct {
        let size_bytes = usize::from(read_u16(decoded, 0)?);
        let cell_count = size_bytes / 4;
        let rhs_start =
            script
                .sp
                .checked_sub(cell_count)
                .ok_or_else(|| VmError::StackUnderflow {
                    message: format!("missing {} cells for rhs struct comparison", cell_count),
                })?;
        let lhs_start =
            rhs_start
                .checked_sub(cell_count)
                .ok_or_else(|| VmError::StackUnderflow {
                    message: format!("missing {} cells for lhs struct comparison", cell_count),
                })?;
        let equal =
            script.stack.get(lhs_start..rhs_start) == script.stack.get(rhs_start..script.sp);
        script.set_stack_pointer(lhs_start)?;
        let result = match decoded.instruction.opcode {
            NcsOpcode::Equal => equal,
            NcsOpcode::NotEqual => !equal,
            _ => return unsupported(decoded, "ordered comparisons are not valid for structs"),
        };
        script.push_int(bool_to_int(result));
        return Ok(());
    }

    let rhs = script.pop()?;
    let lhs = script.pop()?;
    let result = match (&lhs, &rhs) {
        (VmValue::Int(lhs), VmValue::Int(rhs)) => {
            compare_ordered(decoded.instruction.opcode, lhs, rhs)
        }
        (VmValue::Int(lhs), VmValue::Float(rhs)) => {
            compare_ordered(decoded.instruction.opcode, &(*lhs as f32), rhs)
        }
        (VmValue::Float(lhs), VmValue::Int(rhs)) => {
            compare_ordered(decoded.instruction.opcode, lhs, &(*rhs as f32))
        }
        (VmValue::Float(lhs), VmValue::Float(rhs)) => {
            compare_ordered(decoded.instruction.opcode, lhs, rhs)
        }
        (VmValue::String(lhs), VmValue::String(rhs)) => {
            compare_ordered(decoded.instruction.opcode, lhs, rhs)
        }
        (VmValue::Object(lhs), VmValue::Object(rhs)) => compare_equality(decoded, lhs, rhs)?,
        (
            VmValue::EngineStructure {
                index: lhs_index,
                value: lhs_value,
            },
            VmValue::EngineStructure {
                index: rhs_index,
                value: rhs_value,
            },
        ) if lhs_index == rhs_index => {
            compare_engine_structure_equality(decoded, vm, *lhs_index, lhs_value, rhs_value)?
        }
        _ => {
            return unsupported(
                decoded,
                &format!(
                    "comparison between {} and {} is not implemented",
                    lhs.kind_name(),
                    rhs.kind_name()
                ),
            );
        }
    };
    script.push_int(bool_to_int(result));
    Ok(())
}

fn apply_add(script: &mut VmScript, decoded: &VmProgramInstruction) -> Result<(), VmError> {
    if decoded.instruction.auxcode == NcsAuxCode::TypeTypeVectorVector {
        let rhs = script.pop_vector()?;
        let lhs = script.pop_vector()?;
        script.push_vector([lhs[0] + rhs[0], lhs[1] + rhs[1], lhs[2] + rhs[2]]);
        return Ok(());
    }
    let rhs = script.pop()?;
    let lhs = script.pop()?;
    match (lhs, rhs) {
        (VmValue::Int(lhs), VmValue::Int(rhs)) => script.push_int(lhs.wrapping_add(rhs)),
        (VmValue::Int(lhs), VmValue::Float(rhs)) => script.push_float(lhs as f32 + rhs),
        (VmValue::Float(lhs), VmValue::Int(rhs)) => script.push_float(lhs + rhs as f32),
        (VmValue::Float(lhs), VmValue::Float(rhs)) => script.push_float(lhs + rhs),
        (VmValue::String(lhs), VmValue::String(rhs)) => {
            script.push_string(lhs.concat(&rhs));
        }
        _ => {
            return unsupported(
                decoded,
                "ADD currently supports int, float, string, and vector",
            );
        }
    }
    Ok(())
}

fn apply_sub(script: &mut VmScript, decoded: &VmProgramInstruction) -> Result<(), VmError> {
    if decoded.instruction.auxcode == NcsAuxCode::TypeTypeVectorVector {
        let rhs = script.pop_vector()?;
        let lhs = script.pop_vector()?;
        script.push_vector([lhs[0] - rhs[0], lhs[1] - rhs[1], lhs[2] - rhs[2]]);
        return Ok(());
    }
    let rhs = script.pop()?;
    let lhs = script.pop()?;
    match (lhs, rhs) {
        (VmValue::Int(lhs), VmValue::Int(rhs)) => script.push_int(lhs.wrapping_sub(rhs)),
        (VmValue::Int(lhs), VmValue::Float(rhs)) => script.push_float(lhs as f32 - rhs),
        (VmValue::Float(lhs), VmValue::Int(rhs)) => script.push_float(lhs - rhs as f32),
        (VmValue::Float(lhs), VmValue::Float(rhs)) => script.push_float(lhs - rhs),
        _ => return unsupported(decoded, "SUB currently supports int, float, and vector"),
    }
    Ok(())
}

fn apply_mul(script: &mut VmScript, decoded: &VmProgramInstruction) -> Result<(), VmError> {
    match decoded.instruction.auxcode {
        NcsAuxCode::TypeTypeVectorFloat => {
            let rhs = script.pop_float()?;
            let lhs = script.pop_vector()?;
            script.push_vector([lhs[0] * rhs, lhs[1] * rhs, lhs[2] * rhs]);
            return Ok(());
        }
        NcsAuxCode::TypeTypeFloatVector => {
            let rhs = script.pop_vector()?;
            let lhs = script.pop_float()?;
            script.push_vector([lhs * rhs[0], lhs * rhs[1], lhs * rhs[2]]);
            return Ok(());
        }
        _ => {}
    }
    let rhs = script.pop()?;
    let lhs = script.pop()?;
    match (lhs, rhs) {
        (VmValue::Int(lhs), VmValue::Int(rhs)) => script.push_int(lhs.wrapping_mul(rhs)),
        (VmValue::Int(lhs), VmValue::Float(rhs)) => script.push_float(lhs as f32 * rhs),
        (VmValue::Float(lhs), VmValue::Int(rhs)) => script.push_float(lhs * rhs as f32),
        (VmValue::Float(lhs), VmValue::Float(rhs)) => script.push_float(lhs * rhs),
        _ => return unsupported(decoded, "MUL currently supports int, float, and vector"),
    }
    Ok(())
}

fn apply_div(script: &mut VmScript, decoded: &VmProgramInstruction) -> Result<(), VmError> {
    if decoded.instruction.auxcode == NcsAuxCode::TypeTypeVectorFloat {
        let rhs = script.pop_float()?;
        if rhs == 0.0 {
            return Err(VmError::DivideByZero {
                offset: decoded.offset,
            });
        }
        let lhs = script.pop_vector()?;
        script.push_vector([lhs[0] / rhs, lhs[1] / rhs, lhs[2] / rhs]);
        return Ok(());
    }
    let rhs = script.pop()?;
    let lhs = script.pop()?;
    match (lhs, rhs) {
        (VmValue::Int(lhs), VmValue::Int(rhs)) => {
            if rhs == 0 {
                return Err(VmError::DivideByZero {
                    offset: decoded.offset,
                });
            }
            script.push_int(lhs / rhs);
        }
        (VmValue::Int(lhs), VmValue::Float(rhs)) => {
            if rhs == 0.0 {
                return Err(VmError::DivideByZero {
                    offset: decoded.offset,
                });
            }
            script.push_float(lhs as f32 / rhs);
        }
        (VmValue::Float(lhs), VmValue::Int(rhs)) => {
            if rhs == 0 {
                return Err(VmError::DivideByZero {
                    offset: decoded.offset,
                });
            }
            script.push_float(lhs / rhs as f32);
        }
        (VmValue::Float(lhs), VmValue::Float(rhs)) => {
            if rhs == 0.0 {
                return Err(VmError::DivideByZero {
                    offset: decoded.offset,
                });
            }
            script.push_float(lhs / rhs);
        }
        _ => return unsupported(decoded, "DIV currently supports int, float, and vector"),
    }
    Ok(())
}

fn compare_engine_structure_equality(
    decoded: &VmProgramInstruction,
    vm: &Vm,
    index: u8,
    lhs: &VmEngineStructureValue,
    rhs: &VmEngineStructureValue,
) -> Result<bool, VmError> {
    let equal = vm.compare_engine_structure_values(index, lhs, rhs);
    match decoded.instruction.opcode {
        NcsOpcode::Equal => Ok(equal),
        NcsOpcode::NotEqual => Ok(!equal),
        _ => unsupported(
            decoded,
            "ordered comparison is not valid for engine structures",
        ),
    }
}

fn compare_equality<T: PartialEq>(
    decoded: &VmProgramInstruction,
    lhs: &T,
    rhs: &T,
) -> Result<bool, VmError> {
    match decoded.instruction.opcode {
        NcsOpcode::Equal => Ok(lhs == rhs),
        NcsOpcode::NotEqual => Ok(lhs != rhs),
        _ => unsupported(
            decoded,
            "ordered comparison is not valid for this runtime type",
        ),
    }
}

fn compare_ordered<T: PartialEq + PartialOrd>(opcode: NcsOpcode, lhs: &T, rhs: &T) -> bool {
    match opcode {
        NcsOpcode::Equal => lhs == rhs,
        NcsOpcode::NotEqual => lhs != rhs,
        NcsOpcode::Lt => lhs < rhs,
        NcsOpcode::Gt => lhs > rhs,
        NcsOpcode::Leq => lhs <= rhs,
        NcsOpcode::Geq => lhs >= rhs,
        _ => false,
    }
}

fn bool_to_int(value: bool) -> i32 {
    if value { 1 } else { 0 }
}

fn invalid_extra(decoded: &VmProgramInstruction, message: &str) -> VmError {
    VmError::InvalidExtra {
        offset:  decoded.offset,
        opcode:  decoded.instruction.opcode,
        auxcode: decoded.instruction.auxcode,
        message: message.to_string(),
    }
}

fn unsupported<T>(decoded: &VmProgramInstruction, message: &str) -> Result<T, VmError> {
    Err(VmError::Unsupported {
        offset:  decoded.offset,
        opcode:  decoded.instruction.opcode,
        auxcode: decoded.instruction.auxcode,
        message: message.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, collections::HashMap, io::Cursor, rc::Rc};

    use super::{Vm, VmEngineStructureValue, VmRunOptions, VmScript, VmStepOutcome, VmValue};
    use crate::{
        CompileOptions, InMemoryScriptResolver, NcsAuxCode, NcsInstruction, NcsOpcode, SourceId,
        SourceMap, compile_script, compile_script_with_source_map, load_source_bundle,
        parse_langspec, parse_text, read_ndb,
    };

    fn compile_debug_script(
        file_name: &str,
        source: &[u8],
    ) -> Result<(Vec<u8>, crate::Ndb), Box<dyn std::error::Error>> {
        let mut source_map = SourceMap::new();
        let root_id = source_map.add_file(file_name.to_string(), source.to_vec());
        let script = parse_text(root_id, std::str::from_utf8(source)?, None)?;
        let artifacts = compile_script_with_source_map(
            &script,
            &source_map,
            root_id,
            None,
            CompileOptions::default(),
        )?;
        let ndb_bytes = artifacts
            .ndb
            .ok_or("compile_script_with_source_map did not emit NDB bytes")?;
        let mut reader = Cursor::new(ndb_bytes);
        let ndb = read_ndb(&mut reader)?;
        Ok((artifacts.ncs, ndb))
    }

    #[test]
    fn runs_manual_integer_program() -> Result<(), Box<dyn std::error::Error>> {
        let instructions = vec![
            NcsInstruction {
                opcode:  NcsOpcode::Constant,
                auxcode: NcsAuxCode::TypeInteger,
                extra:   7_i32.to_be_bytes().to_vec(),
            },
            NcsInstruction {
                opcode:  NcsOpcode::Constant,
                auxcode: NcsAuxCode::TypeInteger,
                extra:   35_i32.to_be_bytes().to_vec(),
            },
            NcsInstruction {
                opcode:  NcsOpcode::Add,
                auxcode: NcsAuxCode::TypeTypeIntegerInteger,
                extra:   Vec::new(),
            },
            NcsInstruction {
                opcode:  NcsOpcode::Ret,
                auxcode: NcsAuxCode::None,
                extra:   Vec::new(),
            },
        ];
        let mut script = VmScript::from_instructions(instructions, "arith");

        script.run(&Vm::new())?;

        assert_eq!(script.stack(), &[VmValue::Int(42)]);
        Ok(())
    }

    #[test]
    fn invokes_registered_action_command() -> Result<(), Box<dyn std::error::Error>> {
        let langspec = parse_langspec("nwscript", "void PrintInteger(int n);")?;
        let script = parse_text(
            SourceId::new(0),
            "void main() { PrintInteger(42); }",
            Some(&langspec),
        )?;
        let artifacts = compile_script(&script, Some(&langspec), CompileOptions::default())?;

        let called = Rc::new(RefCell::new(None));
        let mut vm = Vm::new();
        {
            let called = Rc::clone(&called);
            vm.define_command(0, move |script, _command, argc| {
                assert_eq!(argc, 1);
                *called.borrow_mut() = Some(script.pop_int()?);
                Ok(())
            });
        }

        let mut runtime = VmScript::from_bytes(&artifacts.ncs, "compiled-main")?;
        runtime.run(&vm)?;

        assert_eq!(*called.borrow(), Some(42));
        Ok(())
    }

    #[test]
    fn preserves_non_utf8_script_strings_through_compile_and_vm()
    -> Result<(), Box<dyn std::error::Error>> {
        let langspec = parse_langspec("nwscript", "void Capture(string sValue);")?;
        let script = parse_text(
            SourceId::new(101),
            r#"void main() { Capture("\xFF\x80"); }"#,
            Some(&langspec),
        )?;
        let artifacts = compile_script(&script, Some(&langspec), CompileOptions::default())?;

        let captured = Rc::new(RefCell::new(None));
        let mut vm = Vm::new();
        {
            let captured = Rc::clone(&captured);
            vm.define_simple_command(0, move |script| {
                *captured.borrow_mut() = Some(script.pop_string()?.into_bytes());
                Ok(())
            });
        }
        let mut runtime = VmScript::from_bytes(&artifacts.ncs, "raw-string")?;
        runtime.run(&vm)?;

        assert_eq!(*captured.borrow(), Some(vec![0xff, 0x80]));
        Ok(())
    }

    #[test]
    fn loads_and_runs_script_through_source_bundle_pipeline()
    -> Result<(), Box<dyn std::error::Error>> {
        let langspec = parse_langspec("nwscript", "void PrintInteger(int n);")?;
        let mut resolver = InMemoryScriptResolver::new();
        resolver.insert_source("main", "void main() { PrintInteger(7); }");
        let bundle = load_source_bundle(&resolver, "main", crate::SourceLoadOptions::default())?;
        let artifacts =
            crate::compile_source_bundle(&bundle, Some(&langspec), CompileOptions::default())?;

        let seen = Rc::new(RefCell::new(Vec::new()));
        let mut vm = Vm::new();
        {
            let seen = Rc::clone(&seen);
            vm.define_simple_command(0, move |script| {
                seen.borrow_mut().push(script.pop_int()?);
                Ok(())
            });
        }

        let mut runtime = VmScript::from_bytes(&artifacts.ncs, "bundle-main")?;
        runtime.run(&vm)?;

        assert_eq!(&*seen.borrow(), &[7]);
        Ok(())
    }

    #[test]
    fn executes_compiled_vector_arithmetic() -> Result<(), Box<dyn std::error::Error>> {
        let langspec = parse_langspec("nwscript", "void PrintVector(vector v);")?;
        let script = parse_text(
            SourceId::new(0),
            "void main() { PrintVector(([1.0, 2.0, 3.0] + [4.0, 5.0, 6.0]) * 2.0); }",
            Some(&langspec),
        )?;
        let artifacts = compile_script(&script, Some(&langspec), CompileOptions::default())?;

        let seen = Rc::new(RefCell::new(Vec::new()));
        let mut vm = Vm::new();
        {
            let seen = Rc::clone(&seen);
            vm.define_simple_command(0, move |script| {
                seen.borrow_mut().push(script.pop_vector()?);
                Ok(())
            });
        }

        let mut runtime = VmScript::from_bytes(&artifacts.ncs, "vector-main")?;
        runtime.run(&vm)?;

        assert_eq!(&*seen.borrow(), &[[10.0, 14.0, 18.0]]);
        Ok(())
    }

    #[test]
    fn executes_compiled_mixed_numeric_arithmetic() -> Result<(), Box<dyn std::error::Error>> {
        let langspec = parse_langspec("nwscript", "void PrintFloat(float f);")?;
        let script = parse_text(
            SourceId::new(0),
            "void main() { PrintFloat(1 + 2.5); PrintFloat(5.5 - 2); PrintFloat(3 * 1.5); \
             PrintFloat(9.0 / 2); }",
            Some(&langspec),
        )?;
        let artifacts = compile_script(&script, Some(&langspec), CompileOptions::default())?;

        let seen = Rc::new(RefCell::new(Vec::new()));
        let mut vm = Vm::new();
        {
            let seen = Rc::clone(&seen);
            vm.define_simple_command(0, move |script| {
                seen.borrow_mut().push(script.pop_float()?);
                Ok(())
            });
        }

        let mut runtime = VmScript::from_bytes(&artifacts.ncs, "mixed-numeric-main")?;
        runtime.run(&vm)?;

        assert_eq!(&*seen.borrow(), &[3.5, 3.5, 4.5, 4.5]);
        Ok(())
    }

    #[test]
    fn executes_compiled_mixed_numeric_comparisons() -> Result<(), Box<dyn std::error::Error>> {
        let instructions = vec![
            NcsInstruction {
                opcode:  NcsOpcode::Constant,
                auxcode: NcsAuxCode::TypeInteger,
                extra:   1_i32.to_be_bytes().to_vec(),
            },
            NcsInstruction {
                opcode:  NcsOpcode::Constant,
                auxcode: NcsAuxCode::TypeFloat,
                extra:   2.5_f32.to_bits().to_be_bytes().to_vec(),
            },
            NcsInstruction {
                opcode:  NcsOpcode::Lt,
                auxcode: NcsAuxCode::TypeTypeIntegerFloat,
                extra:   Vec::new(),
            },
            NcsInstruction {
                opcode:  NcsOpcode::Constant,
                auxcode: NcsAuxCode::TypeFloat,
                extra:   2.5_f32.to_bits().to_be_bytes().to_vec(),
            },
            NcsInstruction {
                opcode:  NcsOpcode::Constant,
                auxcode: NcsAuxCode::TypeInteger,
                extra:   1_i32.to_be_bytes().to_vec(),
            },
            NcsInstruction {
                opcode:  NcsOpcode::Gt,
                auxcode: NcsAuxCode::TypeTypeFloatInteger,
                extra:   Vec::new(),
            },
            NcsInstruction {
                opcode:  NcsOpcode::Constant,
                auxcode: NcsAuxCode::TypeInteger,
                extra:   3_i32.to_be_bytes().to_vec(),
            },
            NcsInstruction {
                opcode:  NcsOpcode::Constant,
                auxcode: NcsAuxCode::TypeFloat,
                extra:   3.0_f32.to_bits().to_be_bytes().to_vec(),
            },
            NcsInstruction {
                opcode:  NcsOpcode::Equal,
                auxcode: NcsAuxCode::TypeTypeIntegerFloat,
                extra:   Vec::new(),
            },
            NcsInstruction {
                opcode:  NcsOpcode::Constant,
                auxcode: NcsAuxCode::TypeFloat,
                extra:   4.5_f32.to_bits().to_be_bytes().to_vec(),
            },
            NcsInstruction {
                opcode:  NcsOpcode::Constant,
                auxcode: NcsAuxCode::TypeInteger,
                extra:   4_i32.to_be_bytes().to_vec(),
            },
            NcsInstruction {
                opcode:  NcsOpcode::NotEqual,
                auxcode: NcsAuxCode::TypeTypeFloatInteger,
                extra:   Vec::new(),
            },
            NcsInstruction {
                opcode:  NcsOpcode::Ret,
                auxcode: NcsAuxCode::None,
                extra:   Vec::new(),
            },
        ];
        let mut runtime = VmScript::from_instructions(instructions, "mixed-compare-manual");
        runtime.run(&Vm::new())?;

        assert_eq!(
            runtime.stack(),
            &[
                VmValue::Int(1),
                VmValue::Int(1),
                VmValue::Int(1),
                VmValue::Int(1),
            ]
        );
        Ok(())
    }

    #[test]
    fn grows_stack_with_positive_movsp() -> Result<(), Box<dyn std::error::Error>> {
        let instructions = vec![
            NcsInstruction {
                opcode:  NcsOpcode::ModifyStackPointer,
                auxcode: NcsAuxCode::None,
                extra:   4_i32.to_be_bytes().to_vec(),
            },
            NcsInstruction {
                opcode:  NcsOpcode::Ret,
                auxcode: NcsAuxCode::None,
                extra:   Vec::new(),
            },
        ];
        let mut script = VmScript::from_instructions(instructions, "movsp-grow");

        script.run(&Vm::new())?;

        assert_eq!(script.stack(), &[VmValue::Int(0)]);
        Ok(())
    }

    #[test]
    fn steps_manual_program_instruction_by_instruction() -> Result<(), Box<dyn std::error::Error>> {
        let instructions = vec![
            NcsInstruction {
                opcode:  NcsOpcode::Constant,
                auxcode: NcsAuxCode::TypeInteger,
                extra:   7_i32.to_be_bytes().to_vec(),
            },
            NcsInstruction {
                opcode:  NcsOpcode::Constant,
                auxcode: NcsAuxCode::TypeInteger,
                extra:   35_i32.to_be_bytes().to_vec(),
            },
            NcsInstruction {
                opcode:  NcsOpcode::Add,
                auxcode: NcsAuxCode::TypeTypeIntegerInteger,
                extra:   Vec::new(),
            },
            NcsInstruction {
                opcode:  NcsOpcode::Ret,
                auxcode: NcsAuxCode::None,
                extra:   Vec::new(),
            },
        ];
        let mut script = VmScript::from_instructions(instructions, "step-arith");
        let vm = Vm::new();

        assert_eq!(
            script
                .current_instruction()
                .map(|instruction| instruction.opcode),
            Some(NcsOpcode::Constant)
        );
        assert_eq!(vm.step(&mut script)?, VmStepOutcome::Running);
        assert_eq!(script.stack(), &[VmValue::Int(7)]);

        assert_eq!(vm.step(&mut script)?, VmStepOutcome::Running);
        assert_eq!(script.stack(), &[VmValue::Int(7), VmValue::Int(35)]);

        assert_eq!(vm.step(&mut script)?, VmStepOutcome::Running);
        assert_eq!(script.stack(), &[VmValue::Int(42)]);

        assert_eq!(vm.step(&mut script)?, VmStepOutcome::Halted);
        assert_eq!(script.stack(), &[VmValue::Int(42)]);
        Ok(())
    }

    #[test]
    fn steps_over_and_out_of_compiled_user_calls() -> Result<(), Box<dyn std::error::Error>> {
        let source = br#"int AddOne(int x) {
    return x + 1;
}

int Twice(int x) {
    int first = AddOne(x);
    int second = AddOne(x);
    return first + second;
}
"#;
        let (ncs, ndb) = compile_debug_script("debug_calls.nss", source)?;
        let vm = Vm::new();
        let mut script = VmScript::from_bytes_with_ndb(&ncs, "debug-calls", &ndb)?;
        script.prepare_function_call(&ndb, "Twice", &[VmValue::Int(2)])?;

        for _ in 0..32 {
            if script
                .current_instruction()
                .map(|instruction| instruction.opcode)
                == Some(NcsOpcode::Jsr)
            {
                break;
            }
            assert_eq!(vm.step(&mut script)?, VmStepOutcome::Running);
        }

        let before = script.ip();
        assert_eq!(
            script.current_function().map(|function| function.name),
            Some("Twice".to_string())
        );
        assert_eq!(
            vm.step_over(&mut script, VmRunOptions::default())?,
            VmStepOutcome::Running
        );
        assert!(script.ip() > before);
        assert_eq!(
            script.current_function().map(|function| function.name),
            Some("Twice".to_string())
        );

        assert_eq!(
            vm.run_until_function(&mut script, "AddOne", VmRunOptions::default())?,
            VmStepOutcome::Running
        );
        assert_eq!(
            script.current_function().map(|function| function.name),
            Some("AddOne".to_string())
        );

        assert_eq!(
            vm.step_out(&mut script, VmRunOptions::default())?,
            VmStepOutcome::Running
        );
        assert_eq!(
            script.current_function().map(|function| function.name),
            Some("Twice".to_string())
        );
        Ok(())
    }

    #[test]
    fn exposes_source_locations_and_runs_until_lines() -> Result<(), Box<dyn std::error::Error>> {
        let source = br#"int AddOne(int x) {
    return x + 1;
}

int Twice(int x) {
    int first = AddOne(x);
    int second = AddOne(x);
    return first + second;
}
"#;
        let (ncs, ndb) = compile_debug_script("debug_lines.nss", source)?;
        let vm = Vm::new();
        let mut script = VmScript::from_bytes_with_ndb(&ncs, "debug-lines", &ndb)?;
        script.prepare_function_call(&ndb, "Twice", &[VmValue::Int(2)])?;
        let root_file_index = ndb
            .files
            .iter()
            .position(|file| file.name == "debug_lines.nss")
            .ok_or("missing root file entry")?;
        let mut root_lines = ndb
            .lines
            .iter()
            .filter(|line| line.file_num == root_file_index)
            .map(|line| line.line_num)
            .collect::<Vec<_>>();
        root_lines.sort_unstable();
        root_lines.dedup();
        let first_line = *root_lines.first().ok_or("missing root source lines")?;
        let second_line = *root_lines
            .iter()
            .find(|line| **line > first_line)
            .ok_or("missing second root source line")?;

        assert_eq!(
            vm.run_until_line(
                &mut script,
                "debug_lines.nss",
                first_line,
                VmRunOptions::default()
            )?,
            VmStepOutcome::Running
        );
        let location = script
            .current_source_location()
            .ok_or("missing first source location")?;
        assert_eq!(location.file_name, "debug_lines.nss");
        assert!(location.is_root);
        assert_eq!(location.line_number, first_line);

        assert_eq!(
            vm.run_until_line(
                &mut script,
                "debug_lines.nss",
                second_line,
                VmRunOptions::default()
            )?,
            VmStepOutcome::Running
        );
        let location = script
            .current_source_location()
            .ok_or("missing second source location")?;
        assert_eq!(location.line_number, second_line);
        assert_eq!(location.file_name, "debug_lines.nss");
        Ok(())
    }

    #[test]
    fn executes_compiled_struct_equality() -> Result<(), Box<dyn std::error::Error>> {
        let langspec = parse_langspec("nwscript", "void PrintInteger(int n);")?;
        let source = r#"
            struct Pair {
                int a;
                int b;
            };

            void main() {
                struct Pair left;
                struct Pair right;
                left.a = 1;
                left.b = 2;
                right.a = 1;
                right.b = 2;
                PrintInteger(left == right);
            }
        "#;
        let script = parse_text(SourceId::new(0), source, Some(&langspec))?;
        let artifacts = compile_script(&script, Some(&langspec), CompileOptions::default())?;

        let seen = Rc::new(RefCell::new(Vec::new()));
        let mut vm = Vm::new();
        {
            let seen = Rc::clone(&seen);
            vm.define_simple_command(0, move |script| {
                seen.borrow_mut().push(script.pop_int()?);
                Ok(())
            });
        }

        let mut runtime = VmScript::from_bytes(&artifacts.ncs, "struct-main")?;
        runtime.run(&vm)?;

        assert_eq!(&*seen.borrow(), &[1]);
        Ok(())
    }

    #[test]
    fn executes_compiled_user_function_calls_without_ndb() -> Result<(), Box<dyn std::error::Error>>
    {
        let source = br#"
            int AddOne(int nValue) {
                return nValue + 1;
            }

            void main() {
                PrintInteger(AddOne(41));
            }
        "#;
        let langspec = parse_langspec("nwscript", "void PrintInteger(int n);")?;
        let script = parse_text(
            SourceId::new(0),
            std::str::from_utf8(source)?,
            Some(&langspec),
        )?;
        let artifacts = compile_script(&script, Some(&langspec), CompileOptions::default())?;
        assert!(artifacts.ndb.is_none());

        let seen = Rc::new(RefCell::new(Vec::new()));
        let mut vm = Vm::new();
        {
            let seen = Rc::clone(&seen);
            vm.define_simple_command(0, move |script| {
                seen.borrow_mut().push(script.pop_int()?);
                Ok(())
            });
        }

        vm.run_bytes(&artifacts.ncs, "user-call-main")?;

        assert_eq!(&*seen.borrow(), &[42]);
        Ok(())
    }

    #[test]
    fn compiled_builtin_arguments_follow_native_stack_order()
    -> Result<(), Box<dyn std::error::Error>> {
        let langspec = parse_langspec("nwscript", "void Assert(int condition, string message);")?;
        let script = parse_text(
            SourceId::new(0),
            "void main() { Assert(7, \"seven\"); }",
            Some(&langspec),
        )?;
        let artifacts = compile_script(&script, Some(&langspec), CompileOptions::default())?;

        let mut vm = Vm::new();
        vm.define_command(0, |script, _command, argc| {
            assert_eq!(argc, 2);
            assert_eq!(script.pop_int()?, 7);
            assert_eq!(script.pop_string()?.as_bytes(), b"seven");
            Ok(())
        });

        let mut runtime = VmScript::from_bytes(&artifacts.ncs, "builtin-argument-order")?;
        runtime.run(&vm)?;
        Ok(())
    }

    #[test]
    fn captures_and_executes_saved_action_situations() -> Result<(), Box<dyn std::error::Error>> {
        let langspec = parse_langspec(
            "nwscript",
            "void DelayCommand(float seconds, action aAction);\nvoid PrintInteger(int n);",
        )?;
        let script = parse_text(
            SourceId::new(0),
            "void main() { int value = 42; DelayCommand(1.0, PrintInteger(value + 1)); }",
            Some(&langspec),
        )?;
        let artifacts = compile_script(&script, Some(&langspec), CompileOptions::default())?;

        let delayed = Rc::new(RefCell::new(None));
        let printed = Rc::new(RefCell::new(None));
        let mut vm = Vm::new();
        {
            let delayed = Rc::clone(&delayed);
            vm.define_command(0, move |script, _command, argc| {
                assert_eq!(argc, 2);
                let seconds = script.pop_float()?;
                assert_eq!(seconds, 1.0);
                *delayed.borrow_mut() = script.saved_situation().cloned();
                Ok(())
            });
        }
        {
            let printed = Rc::clone(&printed);
            vm.define_command(1, move |script, _command, argc| {
                assert_eq!(argc, 1);
                *printed.borrow_mut() = Some(script.pop_int()?);
                Ok(())
            });
        }

        let mut runtime = VmScript::from_bytes(&artifacts.ncs, "compiled-delay")?;
        runtime.run(&vm)?;

        let saved = delayed
            .borrow()
            .clone()
            .ok_or("missing saved action situation")?;
        vm.run_situation(&saved)?;

        assert_eq!(*printed.borrow(), Some(43));
        Ok(())
    }

    #[test]
    fn roundtrips_engine_structures_through_action_handlers()
    -> Result<(), Box<dyn std::error::Error>> {
        let langspec = parse_langspec(
            "nwscript",
            "effect EffectDamage(int nAmount);\nvoid PrintEffect(effect eValue);",
        )?;
        let script = parse_text(
            SourceId::new(0),
            "void main() { PrintEffect(EffectDamage(42)); }",
            Some(&langspec),
        )?;
        let artifacts = compile_script(&script, Some(&langspec), CompileOptions::default())?;

        let seen = Rc::new(RefCell::new(Vec::new()));
        let mut vm = Vm::new();
        vm.define_command(0, move |script, _command, argc| {
            assert_eq!(argc, 1);
            let amount = script.pop_int()?;
            script.push_engine_structure(0, VmEngineStructureValue::Word(amount as u32));
            Ok(())
        });
        {
            let seen = Rc::clone(&seen);
            vm.define_command(1, move |script, _command, argc| {
                assert_eq!(argc, 1);
                let value = script.pop_engine_structure_index(0)?;
                let Some(value) = value.as_word() else {
                    return Err(super::VmError::TypeMismatch {
                        offset:   script.ip(),
                        message:  "expected word-backed effect value".to_string(),
                        expected: Some("engine structure"),
                        actual:   "engine structure",
                    });
                };
                seen.borrow_mut().push(value);
                Ok(())
            });
        }

        let mut runtime = VmScript::from_bytes(&artifacts.ncs, "effect-main")?;
        runtime.run(&vm)?;

        assert_eq!(&*seen.borrow(), &[42]);
        Ok(())
    }

    #[test]
    fn uses_host_defined_engine_structure_defaults_for_rsadd()
    -> Result<(), Box<dyn std::error::Error>> {
        let langspec = parse_langspec(
            "nwscript",
            "#define ENGINE_NUM_STRUCTURES 8\n#define ENGINE_STRUCTURE_7 json\nvoid \
             PrintJson(json jValue);",
        )?;
        let script = parse_text(
            SourceId::new(0),
            "void main() { json jValue; PrintJson(jValue); }",
            Some(&langspec),
        )?;
        let artifacts = compile_script(&script, Some(&langspec), CompileOptions::default())?;

        let seen = Rc::new(RefCell::new(Vec::new()));
        let mut vm = Vm::new();
        vm.define_engine_structure_default(7, VmEngineStructureValue::Text("{\"ok\":true}".into()));
        {
            let seen = Rc::clone(&seen);
            vm.define_command(0, move |script, _command, argc| {
                assert_eq!(argc, 1);
                let value = script.pop_engine_structure_index(7)?;
                let Some(value) = value.as_text() else {
                    return Err(super::VmError::TypeMismatch {
                        offset:   script.ip(),
                        message:  "expected text-backed json value".to_string(),
                        expected: Some("engine structure"),
                        actual:   "engine structure",
                    });
                };
                seen.borrow_mut().push(value.to_string());
                Ok(())
            });
        }

        let mut runtime = VmScript::from_bytes(&artifacts.ncs, "json-main")?;
        runtime.run(&vm)?;

        assert_eq!(&*seen.borrow(), &["{\"ok\":true}".to_string()]);
        Ok(())
    }

    #[test]
    fn uses_host_defined_engine_structure_comparison() -> Result<(), Box<dyn std::error::Error>> {
        let langspec = parse_langspec(
            "nwscript",
            "effect EffectDamage(int nAmount);\nvoid PrintInteger(int n);",
        )?;
        let script = parse_text(
            SourceId::new(0),
            "void main() { PrintInteger(EffectDamage(5) == EffectDamage(5)); }",
            Some(&langspec),
        )?;
        let artifacts = compile_script(&script, Some(&langspec), CompileOptions::default())?;

        let next_handle = Rc::new(RefCell::new(100_u32));
        let effect_values = Rc::new(RefCell::new(HashMap::<u32, i32>::new()));
        let seen = Rc::new(RefCell::new(Vec::new()));
        let mut vm = Vm::new();
        {
            let next_handle = Rc::clone(&next_handle);
            let effect_values = Rc::clone(&effect_values);
            vm.define_command(0, move |script, _command, argc| {
                assert_eq!(argc, 1);
                let amount = script.pop_int()?;
                let handle = *next_handle.borrow();
                *next_handle.borrow_mut() = handle + 1;
                effect_values.borrow_mut().insert(handle, amount);
                script.push_engine_structure(0, VmEngineStructureValue::Word(handle));
                Ok(())
            });
        }
        {
            let effect_values = Rc::clone(&effect_values);
            vm.define_engine_structure_comparer(0, move |_index, lhs, rhs| {
                let Some(lhs) = lhs.as_word() else {
                    return false;
                };
                let Some(rhs) = rhs.as_word() else {
                    return false;
                };
                effect_values.borrow().get(&lhs) == effect_values.borrow().get(&rhs)
            });
        }
        {
            let seen = Rc::clone(&seen);
            vm.define_command(1, move |script, _command, argc| {
                assert_eq!(argc, 1);
                seen.borrow_mut().push(script.pop_int()?);
                Ok(())
            });
        }

        let mut runtime = VmScript::from_bytes(&artifacts.ncs, "effect-compare-main")?;
        runtime.run(&vm)?;

        assert_eq!(&*seen.borrow(), &[1]);
        Ok(())
    }

    #[test]
    fn aborts_script_after_action_handler_requests_abort() -> Result<(), Box<dyn std::error::Error>>
    {
        let langspec = parse_langspec("nwscript", "void StopNow();\nvoid PrintInteger(int n);")?;
        let script = parse_text(
            SourceId::new(0),
            "void main() { StopNow(); PrintInteger(42); }",
            Some(&langspec),
        )?;
        let artifacts = compile_script(&script, Some(&langspec), CompileOptions::default())?;

        let seen = Rc::new(RefCell::new(Vec::new()));
        let mut vm = Vm::new();
        vm.define_command(0, move |script, _command, argc| {
            assert_eq!(argc, 0);
            script.abort();
            Ok(())
        });
        {
            let seen = Rc::clone(&seen);
            vm.define_command(1, move |script, _command, argc| {
                assert_eq!(argc, 1);
                seen.borrow_mut().push(script.pop_int()?);
                Ok(())
            });
        }

        let mut runtime = VmScript::from_bytes(&artifacts.ncs, "abort-main")?;
        runtime.run(&vm)?;

        assert!(runtime.aborted());
        assert!(seen.borrow().is_empty());
        Ok(())
    }

    #[test]
    fn emits_instruction_trace_events_during_execution() -> Result<(), Box<dyn std::error::Error>> {
        let langspec = parse_langspec("nwscript", "void PrintInteger(int n);")?;
        let script = parse_text(
            SourceId::new(0),
            "void main() { PrintInteger(7); }",
            Some(&langspec),
        )?;
        let artifacts = compile_script(&script, Some(&langspec), CompileOptions::default())?;

        let seen = Rc::new(RefCell::new(Vec::new()));
        let traces = Rc::new(RefCell::new(Vec::new()));
        let mut vm = Vm::new();
        {
            let traces = Rc::clone(&traces);
            vm.define_trace_hook(move |_script, event| {
                traces.borrow_mut().push((
                    event.offset,
                    event.instruction.opcode,
                    event.instruction.auxcode,
                    event.sp,
                    event.bp,
                ));
            });
        }
        {
            let seen = Rc::clone(&seen);
            vm.define_command(0, move |script, _command, argc| {
                assert_eq!(argc, 1);
                seen.borrow_mut().push(script.pop_int()?);
                Ok(())
            });
        }

        let mut runtime = VmScript::from_bytes(&artifacts.ncs, "trace-main")?;
        runtime.run(&vm)?;

        assert_eq!(&*seen.borrow(), &[7]);
        let trace = traces.borrow();
        assert!(!trace.is_empty());
        assert_eq!(trace.first().map(|entry| entry.1), Some(NcsOpcode::Jsr));
        assert!(
            trace
                .iter()
                .any(|(_, opcode, auxcode, _, _)| *opcode == NcsOpcode::Constant
                    && *auxcode == NcsAuxCode::TypeInteger)
        );
        assert!(
            trace
                .iter()
                .any(|(_, opcode, _, _, _)| *opcode == NcsOpcode::ExecuteCommand)
        );
        Ok(())
    }

    #[test]
    fn rejects_runs_that_exceed_instruction_budget() -> Result<(), Box<dyn std::error::Error>> {
        let langspec = parse_langspec("nwscript", "void PrintInteger(int n);")?;
        let script = parse_text(
            SourceId::new(0),
            "void main() { while (1) {} }",
            Some(&langspec),
        )?;
        let artifacts = compile_script(&script, Some(&langspec), CompileOptions::default())?;

        let vm = Vm::new();
        let error = vm
            .run_bytes_with_options(
                &artifacts.ncs,
                "budget-main",
                VmRunOptions {
                    max_instructions: Some(16),
                    ..VmRunOptions::default()
                },
            )
            .expect_err("infinite loop should exceed instruction budget");

        assert!(matches!(
            &error,
            super::VmError::InstructionLimitExceeded {
                limit: 16,
                ..
            }
        ));
        assert_eq!(
            error.code(),
            Some(crate::CompilerErrorCode::VmTooManyInstructions)
        );
        Ok(())
    }

    #[test]
    fn maps_division_by_zero_to_native_error_code() -> Result<(), Box<dyn std::error::Error>> {
        let instructions = vec![
            NcsInstruction {
                opcode:  NcsOpcode::Constant,
                auxcode: NcsAuxCode::TypeInteger,
                extra:   1_i32.to_be_bytes().to_vec(),
            },
            NcsInstruction {
                opcode:  NcsOpcode::Constant,
                auxcode: NcsAuxCode::TypeInteger,
                extra:   0_i32.to_be_bytes().to_vec(),
            },
            NcsInstruction {
                opcode:  NcsOpcode::Div,
                auxcode: NcsAuxCode::TypeTypeIntegerInteger,
                extra:   Vec::new(),
            },
        ];
        let mut script = VmScript::from_instructions(instructions, "divide-zero");
        let error = script
            .run(&Vm::new())
            .expect_err("division by zero should fail");
        assert!(matches!(error, super::VmError::DivideByZero { .. }));
        assert_eq!(error.code(), Some(crate::CompilerErrorCode::VmDivideByZero));
        Ok(())
    }

    #[test]
    fn maps_stack_and_recursion_budgets_to_native_error_codes()
    -> Result<(), Box<dyn std::error::Error>> {
        let stack_program = vec![
            NcsInstruction {
                opcode:  NcsOpcode::Constant,
                auxcode: NcsAuxCode::TypeInteger,
                extra:   1_i32.to_be_bytes().to_vec(),
            },
            NcsInstruction {
                opcode:  NcsOpcode::Ret,
                auxcode: NcsAuxCode::None,
                extra:   Vec::new(),
            },
        ];
        let mut stack_script = VmScript::from_instructions(stack_program, "stack-limit");
        let stack_error = Vm::new()
            .run_with_options(
                &mut stack_script,
                VmRunOptions {
                    max_stack_cells: Some(0),
                    ..VmRunOptions::default()
                },
            )
            .expect_err("stack limit should fail");
        assert_eq!(
            stack_error.code(),
            Some(crate::CompilerErrorCode::VmStackOverflow)
        );

        let recursive = parse_text(
            SourceId::new(0),
            "void recurse() { recurse(); } void main() { recurse(); }",
            None,
        )?;
        let artifacts = compile_script(&recursive, None, CompileOptions::default())?;
        let recursion_error = Vm::new()
            .run_bytes_with_options(
                &artifacts.ncs,
                "recursion-limit",
                VmRunOptions {
                    max_recursion_depth: Some(1),
                    ..VmRunOptions::default()
                },
            )
            .expect_err("recursion limit should fail");
        assert_eq!(
            recursion_error.code(),
            Some(crate::CompilerErrorCode::VmTooManyLevelsOfRecursion)
        );
        Ok(())
    }

    #[test]
    fn runs_named_function_calls_from_ndb_without_globals() -> Result<(), Box<dyn std::error::Error>>
    {
        let source = br#"
            int AddOne(int nValue) {
                return nValue + 1;
            }

            void main() {
                return;
            }
        "#;
        let mut source_map = SourceMap::new();
        let root_id = source_map.add_file("direct_call.nss".to_string(), source.to_vec());
        let langspec = parse_langspec("nwscript", "void Dummy();")?;
        let script = parse_text(root_id, std::str::from_utf8(source)?, Some(&langspec))?;
        let artifacts = compile_script_with_source_map(
            &script,
            &source_map,
            root_id,
            Some(&langspec),
            CompileOptions::default(),
        )?;
        let ndb = read_ndb(&mut std::io::Cursor::new(
            artifacts.ndb.clone().ok_or("missing ndb")?,
        ))?;

        let vm = Vm::new();
        let runtime = vm.run_function_bytes(
            &artifacts.ncs,
            "direct-call",
            &ndb,
            "AddOne",
            &[VmValue::Int(41)],
        )?;

        assert_eq!(
            runtime.function_return_value(&ndb, "AddOne")?,
            Some(VmValue::Int(42))
        );
        Ok(())
    }

    #[test]
    fn runs_struct_arguments_and_returns_from_ndb() -> Result<(), Box<dyn std::error::Error>> {
        let source = br#"
            struct Pair {
                int first;
                int second;
            };

            struct Pair Swap(struct Pair value) {
                struct Pair result;
                result.first = value.second;
                result.second = value.first;
                return result;
            }

            void main() {}
        "#;
        let mut source_map = SourceMap::new();
        let root_id = source_map.add_file("direct_struct.nss".to_string(), source.to_vec());
        let script = parse_text(root_id, std::str::from_utf8(source)?, None)?;
        let artifacts = compile_script_with_source_map(
            &script,
            &source_map,
            root_id,
            None,
            CompileOptions::default(),
        )?;
        let ndb = read_ndb(&mut std::io::Cursor::new(
            artifacts.ndb.clone().ok_or("missing ndb")?,
        ))?;

        let vm = Vm::new();
        let runtime = vm.run_function_bytes(
            &artifacts.ncs,
            "direct-struct",
            &ndb,
            "Swap",
            &[VmValue::Struct(vec![VmValue::Int(10), VmValue::Int(20)])],
        )?;

        assert_eq!(
            runtime.function_return_value(&ndb, "Swap")?,
            Some(VmValue::Struct(vec![VmValue::Int(20), VmValue::Int(10)]))
        );
        Ok(())
    }

    #[test]
    fn runs_direct_function_calls_after_bootstrapping_globals()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = br#"
            int GLOBAL = 1;
            int SECOND = GLOBAL + 2;

            int ReadGlobal() {
                return GLOBAL + SECOND;
            }

            void main() {
                return;
            }
        "#;
        let mut source_map = SourceMap::new();
        let root_id = source_map.add_file("globals_direct_call.nss".to_string(), source.to_vec());
        let langspec = parse_langspec("nwscript", "void Dummy();")?;
        let script = parse_text(root_id, std::str::from_utf8(source)?, Some(&langspec))?;
        let artifacts = compile_script_with_source_map(
            &script,
            &source_map,
            root_id,
            Some(&langspec),
            CompileOptions::default(),
        )?;
        let ndb = read_ndb(&mut std::io::Cursor::new(
            artifacts.ndb.clone().ok_or("missing ndb")?,
        ))?;

        let vm = Vm::new();
        let runtime = vm.run_function_bytes(
            &artifacts.ncs,
            "globals-direct-call",
            &ndb,
            "ReadGlobal",
            &[],
        )?;

        assert_eq!(
            runtime.function_return_value(&ndb, "ReadGlobal")?,
            Some(VmValue::Int(4))
        );
        Ok(())
    }

    #[test]
    fn runs_direct_function_calls_for_globals_with_non_void_entry_loader()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = br#"
            int GLOBAL = 1;

            int ReadGlobal() {
                return GLOBAL;
            }

            int StartingConditional() {
                return 0;
            }
        "#;
        let mut source_map = SourceMap::new();
        let root_id = source_map.add_file(
            "globals_loader_conditional.nss".to_string(),
            source.to_vec(),
        );
        let langspec = parse_langspec("nwscript", "void Dummy();")?;
        let script = parse_text(root_id, std::str::from_utf8(source)?, Some(&langspec))?;
        let artifacts = compile_script_with_source_map(
            &script,
            &source_map,
            root_id,
            Some(&langspec),
            CompileOptions::default(),
        )?;
        let ndb = read_ndb(&mut std::io::Cursor::new(
            artifacts.ndb.clone().ok_or("missing ndb")?,
        ))?;
        let vm = Vm::new();
        let runtime = vm.run_function_bytes(
            &artifacts.ncs,
            "globals-conditional",
            &ndb,
            "ReadGlobal",
            &[],
        )?;
        assert_eq!(
            runtime.function_return_value(&ndb, "ReadGlobal")?,
            Some(VmValue::Int(1))
        );
        Ok(())
    }
}
