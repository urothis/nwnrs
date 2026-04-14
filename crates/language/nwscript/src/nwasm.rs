use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
    fmt::Write,
};

use crate::{
    LangSpec, NcsAuxCode, NcsInstruction, NcsOpcode, NcsReadError, Ndb, NdbFunction, NdbLine,
    decode_ncs_instructions,
};

/// One decoded disassembly line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NcsAsmLine {
    /// Byte offset of the instruction within the NCS code section.
    pub offset:      usize,
    /// Optional synthetic label placed at this instruction offset.
    pub label:       Option<String>,
    /// Rendered instruction name.
    pub instruction: String,
    /// Rendered operand text.
    pub extra:       String,
}

#[derive(Debug, Clone)]
struct DecodedAsmLine {
    line:        NcsAsmLine,
    opcode:      NcsOpcode,
    jump_target: Option<usize>,
}

/// Options controlling NCS disassembly rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(clippy::struct_excessive_bools)]
pub struct NcsDisassemblyOptions {
    /// Render upstream internal enum names instead of canonical mnemonics.
    pub internal_names:    bool,
    /// Maximum string payload shown before truncation markers are appended.
    pub max_string_length: usize,
    /// Emit synthetic labels for jump targets.
    pub labels:            bool,
    /// Include instruction byte offsets in rendered text output.
    pub offsets:           bool,
    /// Include per-function local offsets when NDB debug info is available.
    pub local_offsets:     bool,
    /// Weave source lines into output when debug line info and source text are
    /// available.
    pub source_weave:      bool,
}

impl Default for NcsDisassemblyOptions {
    fn default() -> Self {
        Self {
            internal_names:    false,
            max_string_length: 15,
            labels:            true,
            offsets:           true,
            local_offsets:     true,
            source_weave:      true,
        }
    }
}

/// Errors returned while rendering or parsing NCS asm text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NcsAsmError {
    /// Binary decoding failed before text rendering could begin.
    Read(NcsReadError),
    /// One instruction payload did not match the expected opcode shape.
    InvalidExtra {
        /// Instruction byte offset in the NCS code section.
        offset:  usize,
        /// Opcode whose extra data was malformed.
        opcode:  NcsOpcode,
        /// Auxcode whose extra data was malformed.
        auxcode: NcsAuxCode,
        /// Human-readable explanation.
        message: String,
    },
    /// One line of textual asm could not be parsed.
    Parse {
        /// One-based line number in the text input.
        line:    usize,
        /// Human-readable explanation.
        message: String,
    },
}

impl fmt::Display for NcsAsmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(error) => error.fmt(f),
            Self::InvalidExtra {
                offset,
                opcode,
                auxcode,
                message,
            } => write!(
                f,
                "invalid {}.{} payload at byte {}: {}",
                opcode.internal_name(),
                auxcode.internal_name(),
                offset,
                message
            ),
            Self::Parse {
                line,
                message,
            } => {
                write!(f, "invalid NCS asm on line {line}: {message}")
            }
        }
    }
}

impl Error for NcsAsmError {}

impl From<NcsReadError> for NcsAsmError {
    fn from(value: NcsReadError) -> Self {
        Self::Read(value)
    }
}

impl NcsOpcode {
    /// Returns the upstream internal opcode constant name used by `nwasm`.
    #[must_use]
    pub fn internal_name(self) -> &'static str {
        match self {
            Self::Assignment => "ASSIGNMENT",
            Self::RunstackAdd => "RUNSTACK_ADD",
            Self::RunstackCopy => "RUNSTACK_COPY",
            Self::Constant => "CONSTANT",
            Self::ExecuteCommand => "EXECUTE_COMMAND",
            Self::LogicalAnd => "LOGICAL_AND",
            Self::LogicalOr => "LOGICAL_OR",
            Self::InclusiveOr => "INCLUSIVE_OR",
            Self::ExclusiveOr => "EXCLUSIVE_OR",
            Self::BooleanAnd => "BOOLEAN_AND",
            Self::Equal => "EQUAL",
            Self::NotEqual => "NOT_EQUAL",
            Self::Geq => "GEQ",
            Self::Gt => "GT",
            Self::Lt => "LT",
            Self::Leq => "LEQ",
            Self::ShiftLeft => "SHIFT_LEFT",
            Self::ShiftRight => "SHIFT_RIGHT",
            Self::UShiftRight => "USHIFT_RIGHT",
            Self::Add => "ADD",
            Self::Sub => "SUB",
            Self::Mul => "MUL",
            Self::Div => "DIV",
            Self::Modulus => "MODULUS",
            Self::Negation => "NEGATION",
            Self::OnesComplement => "ONES_COMPLEMENT",
            Self::ModifyStackPointer => "MODIFY_STACK_POINTER",
            Self::StoreIp => "STORE_IP",
            Self::Jmp => "JMP",
            Self::Jsr => "JSR",
            Self::Jz => "JZ",
            Self::Ret => "RET",
            Self::DeStruct => "DE_STRUCT",
            Self::BooleanNot => "BOOLEAN_NOT",
            Self::Decrement => "DECREMENT",
            Self::Increment => "INCREMENT",
            Self::Jnz => "JNZ",
            Self::AssignmentBase => "ASSIGNMENT_BASE",
            Self::RunstackCopyBase => "RUNSTACK_COPY_BASE",
            Self::DecrementBase => "DECREMENT_BASE",
            Self::IncrementBase => "INCREMENT_BASE",
            Self::SaveBasePointer => "SAVE_BASE_POINTER",
            Self::RestoreBasePointer => "RESTORE_BASE_POINTER",
            Self::StoreState => "STORE_STATE",
            Self::NoOperation => "NO_OPERATION",
        }
    }
}

impl NcsAuxCode {
    /// Returns the upstream internal auxcode constant name used by `nwasm`.
    #[must_use]
    pub fn internal_name(self) -> &'static str {
        match self {
            Self::None => "NONE",
            Self::TypeVoid => "TYPE_VOID",
            Self::TypeCommand => "TYPE_COMMAND",
            Self::TypeInteger => "TYPE_INTEGER",
            Self::TypeFloat => "TYPE_FLOAT",
            Self::TypeString => "TYPE_STRING",
            Self::TypeObject => "TYPE_OBJECT",
            Self::TypeEngst0 => "TYPE_ENGST0",
            Self::TypeEngst1 => "TYPE_ENGST1",
            Self::TypeEngst2 => "TYPE_ENGST2",
            Self::TypeEngst3 => "TYPE_ENGST3",
            Self::TypeEngst4 => "TYPE_ENGST4",
            Self::TypeEngst5 => "TYPE_ENGST5",
            Self::TypeEngst6 => "TYPE_ENGST6",
            Self::TypeEngst7 => "TYPE_ENGST7",
            Self::TypeEngst8 => "TYPE_ENGST8",
            Self::TypeEngst9 => "TYPE_ENGST9",
            Self::TypeTypeIntegerInteger => "TYPETYPE_INTEGER_INTEGER",
            Self::TypeTypeFloatFloat => "TYPETYPE_FLOAT_FLOAT",
            Self::TypeTypeObjectObject => "TYPETYPE_OBJECT_OBJECT",
            Self::TypeTypeStringString => "TYPETYPE_STRING_STRING",
            Self::TypeTypeStructStruct => "TYPETYPE_STRUCT_STRUCT",
            Self::TypeTypeIntegerFloat => "TYPETYPE_INTEGER_FLOAT",
            Self::TypeTypeFloatInteger => "TYPETYPE_FLOAT_INTEGER",
            Self::TypeTypeEngst0Engst0 => "TYPETYPE_ENGST0_ENGST0",
            Self::TypeTypeEngst1Engst1 => "TYPETYPE_ENGST1_ENGST1",
            Self::TypeTypeEngst2Engst2 => "TYPETYPE_ENGST2_ENGST2",
            Self::TypeTypeEngst3Engst3 => "TYPETYPE_ENGST3_ENGST3",
            Self::TypeTypeEngst4Engst4 => "TYPETYPE_ENGST4_ENGST4",
            Self::TypeTypeEngst5Engst5 => "TYPETYPE_ENGST5_ENGST5",
            Self::TypeTypeEngst6Engst6 => "TYPETYPE_ENGST6_ENGST6",
            Self::TypeTypeEngst7Engst7 => "TYPETYPE_ENGST7_ENGST7",
            Self::TypeTypeEngst8Engst8 => "TYPETYPE_ENGST8_ENGST8",
            Self::TypeTypeEngst9Engst9 => "TYPETYPE_ENGST9_ENGST9",
            Self::TypeTypeVectorVector => "TYPETYPE_VECTOR_VECTOR",
            Self::TypeTypeVectorFloat => "TYPETYPE_VECTOR_FLOAT",
            Self::TypeTypeFloatVector => "TYPETYPE_FLOAT_VECTOR",
            Self::EvalInplace => "EVAL_INPLACE",
            Self::EvalPostplace => "EVAL_POSTPLACE",
        }
    }
}

impl NcsInstruction {
    /// Returns the upstream `nwasm` instruction name.
    #[must_use]
    pub fn canonical_name(&self, internal: bool) -> String {
        let mut name = if internal {
            self.opcode.internal_name().to_string()
        } else {
            self.opcode.canonical_name().to_string()
        };
        let aux = if internal {
            Some(self.auxcode.internal_name())
        } else {
            self.auxcode.canonical_name()
        };
        if let Some(aux) = aux {
            name.push('.');
            name.push_str(aux);
        }
        name
    }

    /// Renders the decoded operand payload using upstream `nwasm` formatting
    /// rules.
    pub fn extra_string(&self, max_string_length: usize) -> Result<String, NcsAsmError> {
        extra_string_for_instruction(self, 0, None, &BTreeMap::new(), max_string_length)
    }
}

/// Decodes a full `NCS` stream into instruction-shaped disassembly lines.
pub fn disassemble_ncs(
    bytes: &[u8],
    langspec: Option<&LangSpec>,
    options: NcsDisassemblyOptions,
) -> Result<Vec<NcsAsmLine>, NcsAsmError> {
    Ok(decode_asm_lines(bytes, langspec, options)?
        .into_iter()
        .map(|line| line.line)
        .collect())
}

fn decode_asm_lines(
    bytes: &[u8],
    langspec: Option<&LangSpec>,
    options: NcsDisassemblyOptions,
) -> Result<Vec<DecodedAsmLine>, NcsAsmError> {
    let instructions = decode_ncs_instructions(bytes)?;
    let labels = if options.labels {
        collect_jump_labels(&instructions)
    } else {
        BTreeMap::new()
    };
    let mut offset = 0usize;
    let mut lines = Vec::with_capacity(instructions.len());

    for instruction in instructions {
        let jump_target = jump_target_for_instruction(&instruction, offset);
        let extra = extra_string_for_instruction(
            &instruction,
            offset,
            langspec,
            &labels,
            options.max_string_length,
        )?;
        lines.push(DecodedAsmLine {
            line: NcsAsmLine {
                offset,
                label: labels.get(&offset).cloned(),
                instruction: instruction.canonical_name(options.internal_names),
                extra,
            },
            opcode: instruction.opcode,
            jump_target,
        });
        offset += instruction.encoded_len();
    }

    Ok(lines)
}

/// Renders a full `NCS` stream into stable human-readable disassembly text.
pub fn render_ncs_disassembly(
    bytes: &[u8],
    langspec: Option<&LangSpec>,
    options: NcsDisassemblyOptions,
) -> Result<String, NcsAsmError> {
    let lines = decode_asm_lines(bytes, langspec, options)?;
    Ok(render_disassembly_lines(
        &lines.into_iter().map(|line| line.line).collect::<Vec<_>>(),
        options,
    ))
}

/// Renders a full `NCS` stream into NDB-aware disassembly text.
pub fn render_ncs_disassembly_with_ndb(
    bytes: &[u8],
    langspec: Option<&LangSpec>,
    ndb: Option<&Ndb>,
    source_files: Option<&BTreeMap<String, Vec<String>>>,
    options: NcsDisassemblyOptions,
) -> Result<String, NcsAsmError> {
    let lines = decode_asm_lines(bytes, langspec, options)?;
    Ok(render_disassembly_with_ndb_lines(
        &lines,
        ndb,
        source_files,
        options,
    ))
}

/// Parses assembleable NCS asm text into decoded instructions.
pub fn assemble_ncs_text(
    text: &str,
    langspec: Option<&LangSpec>,
) -> Result<Vec<NcsInstruction>, NcsAsmError> {
    let mut labels = BTreeMap::<String, usize>::new();
    let mut pending_labels = Vec::<String>::new();
    let mut parsed = Vec::<ParsedAsmInstruction>::new();

    for (index, raw_line) in text.lines().enumerate() {
        let line_number = index + 1;
        let line = strip_asm_line(raw_line);
        if line.is_empty() || is_function_header_line(line) {
            continue;
        }
        if let Some(label) = line.strip_suffix(':') {
            let label = label.trim();
            if label.is_empty() {
                return Err(NcsAsmError::Parse {
                    line:    line_number,
                    message: "empty label".to_string(),
                });
            }
            if labels.contains_key(label) || pending_labels.iter().any(|pending| pending == label) {
                return Err(NcsAsmError::Parse {
                    line:    line_number,
                    message: format!("duplicate label {label:?}"),
                });
            }
            pending_labels.push(label.to_string());
            continue;
        }

        let line = strip_rendered_offsets(line);
        let (instruction_name, extra) =
            split_instruction_line(line).ok_or_else(|| NcsAsmError::Parse {
                line:    line_number,
                message: "missing instruction mnemonic".to_string(),
            })?;
        let (opcode, auxcode) = parse_instruction_name(instruction_name, line_number)?;
        let operand = parse_instruction_operand(opcode, auxcode, extra, langspec, line_number)?;
        let instruction_index = parsed.len();
        for label in pending_labels.drain(..) {
            labels.insert(label, instruction_index);
        }
        parsed.push(ParsedAsmInstruction {
            opcode,
            auxcode,
            operand,
            line: line_number,
        });
    }

    if !pending_labels.is_empty() {
        return Err(NcsAsmError::Parse {
            line:    text.lines().count().max(1),
            message: format!(
                "label {:?} does not precede an instruction",
                pending_labels.first().cloned().unwrap_or_default()
            ),
        });
    }

    let instruction_offsets = parsed
        .iter()
        .scan(0usize, |offset, instruction| {
            let current = *offset;
            *offset += instruction.encoded_len();
            Some(current)
        })
        .collect::<Vec<_>>();

    let label_offsets = labels
        .into_iter()
        .map(|(label, index)| {
            let offset =
                instruction_offsets
                    .get(index)
                    .copied()
                    .ok_or_else(|| NcsAsmError::Parse {
                        line:    parsed.get(index).map_or(1, |entry| entry.line),
                        message: format!("label {label:?} resolved past end of instruction stream"),
                    })?;
            Ok((label, offset))
        })
        .collect::<Result<BTreeMap<_, _>, NcsAsmError>>()?;

    parsed
        .into_iter()
        .zip(instruction_offsets)
        .map(|(instruction, offset)| instruction.build(offset, &label_offsets))
        .collect()
}

/// Parses assembleable NCS asm text and encodes it into bytecode without the
/// fixed NCS file header.
pub fn assemble_ncs_bytes(text: &str, langspec: Option<&LangSpec>) -> Result<Vec<u8>, NcsAsmError> {
    Ok(crate::encode_ncs_instructions(&assemble_ncs_text(
        text, langspec,
    )?))
}

/// Renders already-decoded disassembly lines into plain text.
#[must_use]
pub fn render_disassembly_lines(lines: &[NcsAsmLine], options: NcsDisassemblyOptions) -> String {
    let mut rendered = Vec::new();

    for line in lines {
        if options.labels
            && let Some(label) = &line.label
        {
            rendered.push(format!("{label}:"));
        }

        let mut row = String::new();
        if options.offsets {
            let _ = write!(row, "{:04}", line.offset);
            row.push_str(": ");
        }

        row.push_str(&line.instruction);
        if !line.extra.is_empty() {
            row.push(' ');
            row.push_str(&line.extra);
        }
        rendered.push(row);
    }

    rendered.join("\n")
}

fn render_disassembly_with_ndb_lines(
    lines: &[DecodedAsmLine],
    ndb: Option<&Ndb>,
    source_files: Option<&BTreeMap<String, Vec<String>>>,
    options: NcsDisassemblyOptions,
) -> String {
    let Some(ndb) = ndb else {
        return render_disassembly_lines(
            &lines
                .iter()
                .map(|line| line.line.clone())
                .collect::<Vec<_>>(),
            options,
        );
    };

    let functions = sorted_functions(ndb);
    let mut rendered = Vec::new();
    let mut current_function: Option<usize> = None;
    let mut last_source: Option<(usize, usize)> = None;

    for decoded in lines {
        let line = &decoded.line;
        let function_index = functions
            .iter()
            .position(|function| line_in_function(line.offset, function));

        if function_index != current_function {
            current_function = function_index;
            last_source = None;

            if let Some(index) = current_function
                && let Some(function) = functions.get(index)
            {
                if !rendered.is_empty() {
                    rendered.push(String::new());
                }
                rendered.push(render_function_header(function, ndb));
            }
        }

        if options.labels
            && let Some(label) = &line.label
        {
            rendered.push(format!("{label}:"));
        }

        let mut row = String::new();
        if options.offsets {
            let _ = write!(row, "{:04}", line.offset);
            if options.local_offsets
                && let Some(index) = current_function
                && let Some(function) = functions.get(index)
            {
                let local = local_offset(line.offset, function);
                row.push(' ');
                let _ = write!(row, "{local:04}");
            }
            row.push_str(": ");
        }

        row.push_str(&line.instruction);
        let rendered_extra = render_ndb_aware_extra(decoded, &functions).unwrap_or(&line.extra);
        if !rendered_extra.is_empty() {
            row.push(' ');
            row.push_str(rendered_extra);
        }

        if options.source_weave
            && let Some((source_key, source_text)) =
                source_line_for_offset(line.offset, ndb, source_files, &mut last_source)
            && !source_text.is_empty()
        {
            row.push_str(" | ");
            row.push_str(&source_key);
            row.push_str(" | ");
            row.push_str(&source_text);
        }

        rendered.push(row);
    }

    rendered.join("\n")
}

#[derive(Debug, Clone)]
enum ParsedAsmOperand {
    None,
    Bytes(Vec<u8>),
    Jump(AsmJumpTarget),
}

#[derive(Debug, Clone)]
enum AsmJumpTarget {
    Offset(usize),
    Label(String),
}

#[derive(Debug, Clone)]
struct ParsedAsmInstruction {
    opcode:  NcsOpcode,
    auxcode: NcsAuxCode,
    operand: ParsedAsmOperand,
    line:    usize,
}

impl ParsedAsmInstruction {
    fn encoded_len(&self) -> usize {
        2 + match &self.operand {
            ParsedAsmOperand::None => 0,
            ParsedAsmOperand::Bytes(bytes) => bytes.len(),
            ParsedAsmOperand::Jump(_target) => 4,
        }
    }

    fn build(
        self,
        offset: usize,
        labels: &BTreeMap<String, usize>,
    ) -> Result<NcsInstruction, NcsAsmError> {
        let extra = match self.operand {
            ParsedAsmOperand::None => Vec::new(),
            ParsedAsmOperand::Bytes(bytes) => bytes,
            ParsedAsmOperand::Jump(target) => {
                let absolute_target = match target {
                    AsmJumpTarget::Offset(offset) => offset,
                    AsmJumpTarget::Label(label) => {
                        labels
                            .get(&label)
                            .copied()
                            .ok_or_else(|| NcsAsmError::Parse {
                                line:    self.line,
                                message: format!("unknown jump label {label:?}"),
                            })?
                    }
                };
                let relative = i64::try_from(absolute_target)
                    .ok()
                    .and_then(|target| i64::try_from(offset).ok().map(|origin| target - origin))
                    .and_then(|relative| i32::try_from(relative).ok())
                    .ok_or_else(|| NcsAsmError::Parse {
                        line:    self.line,
                        message: format!(
                            "jump target {absolute_target} is out of range for byte offset \
                             {offset}"
                        ),
                    })?;
                relative.to_be_bytes().to_vec()
            }
        };

        Ok(NcsInstruction {
            opcode: self.opcode,
            auxcode: self.auxcode,
            extra,
        })
    }
}

fn render_ndb_aware_extra<'a>(
    decoded: &'a DecodedAsmLine,
    functions: &[&'a NdbFunction],
) -> Option<&'a str> {
    if decoded.opcode != NcsOpcode::Jsr {
        return None;
    }
    let target = decoded.jump_target?;
    functions
        .iter()
        .find(|function| line_in_function(target, function))
        .map(|function| function.label.as_str())
}

fn extra_string_for_instruction(
    instruction: &NcsInstruction,
    offset: usize,
    langspec: Option<&LangSpec>,
    labels: &BTreeMap<usize, String>,
    max_string_length: usize,
) -> Result<String, NcsAsmError> {
    let extra = instruction.extra.as_slice();
    match instruction.opcode {
        NcsOpcode::Constant => match instruction.auxcode {
            NcsAuxCode::TypeString => {
                let value = decode_prefixed_string(extra, offset, instruction)?;
                Ok(truncate_nwasm_string(&value, max_string_length))
            }
            NcsAuxCode::TypeInteger => Ok(read_i32(extra, offset, instruction)?.to_string()),
            NcsAuxCode::TypeObject => {
                Ok(format!("0x{:08X}", read_i32(extra, offset, instruction)?))
            }
            NcsAuxCode::TypeFloat => Ok(read_f32(extra, offset, instruction)?.to_string()),
            NcsAuxCode::TypeEngst2 => Ok(read_u32(extra, offset, instruction)?.to_string()),
            NcsAuxCode::TypeEngst7 => {
                let value = decode_prefixed_string(extra, offset, instruction)?;
                Ok(value.escape_default().to_string())
            }
            _ => Ok(String::new()),
        },
        NcsOpcode::Jz | NcsOpcode::Jmp | NcsOpcode::Jsr | NcsOpcode::Jnz => {
            let target = jump_target(offset, read_i32(extra, offset, instruction)?);
            Ok(format_jump_target(instruction.opcode, target, labels))
        }
        NcsOpcode::StoreState => Ok(format!(
            "{}, {}",
            read_i32_part(extra, 0, offset, instruction)?,
            read_i32_part(extra, 4, offset, instruction)?,
        )),
        NcsOpcode::ModifyStackPointer
        | NcsOpcode::Increment
        | NcsOpcode::Decrement
        | NcsOpcode::IncrementBase
        | NcsOpcode::DecrementBase => Ok(read_i32(extra, offset, instruction)?.to_string()),
        NcsOpcode::ExecuteCommand => {
            let builtin_id = read_u16_part(extra, 0, offset, instruction)?;
            let argc = read_u8_part(extra, 2, offset, instruction)?;
            let _ = langspec;
            Ok(format!("{builtin_id}, {argc}"))
        }
        NcsOpcode::RunstackCopy | NcsOpcode::RunstackCopyBase => Ok(format!(
            "{}, {}",
            read_i32_part(extra, 0, offset, instruction)?,
            read_i16_part(extra, 4, offset, instruction)?,
        )),
        NcsOpcode::Assignment | NcsOpcode::AssignmentBase => Ok(format!(
            "{}, {}",
            read_i32_part(extra, 0, offset, instruction)?,
            read_u16_part(extra, 4, offset, instruction)?,
        )),
        NcsOpcode::DeStruct => Ok(format!(
            "{}, {}, {}",
            read_u16_part(extra, 0, offset, instruction)?,
            read_u16_part(extra, 2, offset, instruction)?,
            read_u16_part(extra, 4, offset, instruction)?,
        )),
        NcsOpcode::Equal | NcsOpcode::NotEqual
            if instruction.auxcode == NcsAuxCode::TypeTypeStructStruct =>
        {
            Ok(read_u16(extra, offset, instruction)?.to_string())
        }
        _ if extra.is_empty() => Ok(String::new()),
        _ => Err(NcsAsmError::InvalidExtra {
            offset,
            opcode: instruction.opcode,
            auxcode: instruction.auxcode,
            message: format!("unsupported extra payload {extra:?}"),
        }),
    }
}

fn strip_asm_line(line: &str) -> &str {
    line.split_once(" | ")
        .map_or(line, |(head, _tail)| head)
        .trim()
}

fn is_function_header_line(line: &str) -> bool {
    line.ends_with(']') && line.contains('(') && line.contains("):")
}

fn strip_rendered_offsets(line: &str) -> &str {
    let Some((prefix, rest)) = line.split_once(": ") else {
        return line;
    };
    if prefix
        .chars()
        .all(|ch| ch.is_ascii_digit() || ch.is_ascii_whitespace())
        && prefix.chars().any(|ch| ch.is_ascii_digit())
    {
        rest.trim_start()
    } else {
        line
    }
}

fn split_instruction_line(line: &str) -> Option<(&str, &str)> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some((instruction, rest)) = trimmed.split_once(char::is_whitespace) {
        Some((instruction, rest.trim()))
    } else {
        Some((trimmed, ""))
    }
}

fn parse_instruction_name(name: &str, line: usize) -> Result<(NcsOpcode, NcsAuxCode), NcsAsmError> {
    let (opcode_name, aux_name) = name
        .split_once('.')
        .map_or((name, None), |(opcode, aux)| (opcode, Some(aux)));
    let opcode = parse_opcode_name(opcode_name).ok_or_else(|| NcsAsmError::Parse {
        line,
        message: format!("unknown instruction mnemonic {opcode_name:?}"),
    })?;
    let auxcode = if let Some(aux_name) = aux_name {
        parse_aux_name(aux_name).ok_or_else(|| NcsAsmError::Parse {
            line,
            message: format!("unknown instruction auxcode {aux_name:?}"),
        })?
    } else {
        default_aux_for_opcode(opcode)
    };
    Ok((opcode, auxcode))
}

fn default_aux_for_opcode(opcode: NcsOpcode) -> NcsAuxCode {
    match opcode {
        NcsOpcode::Assignment
        | NcsOpcode::AssignmentBase
        | NcsOpcode::RunstackCopy
        | NcsOpcode::RunstackCopyBase => NcsAuxCode::TypeVoid,
        _ => NcsAuxCode::None,
    }
}

fn parse_opcode_name(name: &str) -> Option<NcsOpcode> {
    match name {
        "CPDOWNSP" | "ASSIGNMENT" => Some(NcsOpcode::Assignment),
        "RSADD" | "RUNSTACK_ADD" => Some(NcsOpcode::RunstackAdd),
        "CPTOPSP" | "RUNSTACK_COPY" => Some(NcsOpcode::RunstackCopy),
        "CONST" | "CONSTANT" => Some(NcsOpcode::Constant),
        "ACTION" | "EXECUTE_COMMAND" => Some(NcsOpcode::ExecuteCommand),
        "LOGAND" | "LOGICAL_AND" => Some(NcsOpcode::LogicalAnd),
        "LOGOR" | "LOGICAL_OR" => Some(NcsOpcode::LogicalOr),
        "INCOR" | "INCLUSIVE_OR" => Some(NcsOpcode::InclusiveOr),
        "EXCOR" | "EXCLUSIVE_OR" => Some(NcsOpcode::ExclusiveOr),
        "BOOLAND" | "BOOLEAN_AND" => Some(NcsOpcode::BooleanAnd),
        "EQUAL" => Some(NcsOpcode::Equal),
        "NEQUAL" | "NOT_EQUAL" => Some(NcsOpcode::NotEqual),
        "GEQ" => Some(NcsOpcode::Geq),
        "GT" => Some(NcsOpcode::Gt),
        "LT" => Some(NcsOpcode::Lt),
        "LEQ" => Some(NcsOpcode::Leq),
        "SHLEFT" | "SHIFT_LEFT" => Some(NcsOpcode::ShiftLeft),
        "SHRIGHT" | "SHIFT_RIGHT" => Some(NcsOpcode::ShiftRight),
        "USHRIGHT" | "USHIFT_RIGHT" => Some(NcsOpcode::UShiftRight),
        "ADD" => Some(NcsOpcode::Add),
        "SUB" => Some(NcsOpcode::Sub),
        "MUL" => Some(NcsOpcode::Mul),
        "DIV" => Some(NcsOpcode::Div),
        "MOD" | "MODULUS" => Some(NcsOpcode::Modulus),
        "NEG" | "NEGATION" => Some(NcsOpcode::Negation),
        "COMP" | "ONES_COMPLEMENT" => Some(NcsOpcode::OnesComplement),
        "MOVSP" | "MODIFY_STACK_POINTER" => Some(NcsOpcode::ModifyStackPointer),
        "STOREIP" | "STORE_IP" => Some(NcsOpcode::StoreIp),
        "JMP" => Some(NcsOpcode::Jmp),
        "JSR" => Some(NcsOpcode::Jsr),
        "JZ" => Some(NcsOpcode::Jz),
        "RET" => Some(NcsOpcode::Ret),
        "DESTRUCT" | "DE_STRUCT" => Some(NcsOpcode::DeStruct),
        "NOT" | "BOOLEAN_NOT" => Some(NcsOpcode::BooleanNot),
        "DECSP" | "DECREMENT" => Some(NcsOpcode::Decrement),
        "INCSP" | "INCREMENT" => Some(NcsOpcode::Increment),
        "JNZ" => Some(NcsOpcode::Jnz),
        "CPDOWNBP" | "ASSIGNMENT_BASE" => Some(NcsOpcode::AssignmentBase),
        "CPTOPBP" | "RUNSTACK_COPY_BASE" => Some(NcsOpcode::RunstackCopyBase),
        "DECBP" | "DECREMENT_BASE" => Some(NcsOpcode::DecrementBase),
        "INCBP" | "INCREMENT_BASE" => Some(NcsOpcode::IncrementBase),
        "SAVEBP" | "SAVE_BASE_POINTER" => Some(NcsOpcode::SaveBasePointer),
        "RESTOREBP" | "RESTORE_BASE_POINTER" => Some(NcsOpcode::RestoreBasePointer),
        "STORESTATE" | "STORE_STATE" => Some(NcsOpcode::StoreState),
        "NOP" | "NO_OPERATION" => Some(NcsOpcode::NoOperation),
        _ => None,
    }
}

fn parse_aux_name(name: &str) -> Option<NcsAuxCode> {
    match name {
        "NONE" => Some(NcsAuxCode::None),
        "TYPE_VOID" => Some(NcsAuxCode::TypeVoid),
        "TYPE_COMMAND" => Some(NcsAuxCode::TypeCommand),
        "I" | "TYPE_INTEGER" => Some(NcsAuxCode::TypeInteger),
        "F" | "TYPE_FLOAT" => Some(NcsAuxCode::TypeFloat),
        "S" | "TYPE_STRING" => Some(NcsAuxCode::TypeString),
        "O" | "TYPE_OBJECT" => Some(NcsAuxCode::TypeObject),
        "E0" | "TYPE_ENGST0" => Some(NcsAuxCode::TypeEngst0),
        "E1" | "TYPE_ENGST1" => Some(NcsAuxCode::TypeEngst1),
        "E2" | "TYPE_ENGST2" => Some(NcsAuxCode::TypeEngst2),
        "E3" | "TYPE_ENGST3" => Some(NcsAuxCode::TypeEngst3),
        "E4" | "TYPE_ENGST4" => Some(NcsAuxCode::TypeEngst4),
        "E5" | "TYPE_ENGST5" => Some(NcsAuxCode::TypeEngst5),
        "E6" | "TYPE_ENGST6" => Some(NcsAuxCode::TypeEngst6),
        "E7" | "TYPE_ENGST7" => Some(NcsAuxCode::TypeEngst7),
        "E8" | "TYPE_ENGST8" => Some(NcsAuxCode::TypeEngst8),
        "E9" | "TYPE_ENGST9" => Some(NcsAuxCode::TypeEngst9),
        "II" | "TYPETYPE_INTEGER_INTEGER" => Some(NcsAuxCode::TypeTypeIntegerInteger),
        "FF" | "TYPETYPE_FLOAT_FLOAT" => Some(NcsAuxCode::TypeTypeFloatFloat),
        "OO" | "TYPETYPE_OBJECT_OBJECT" => Some(NcsAuxCode::TypeTypeObjectObject),
        "SS" | "TYPETYPE_STRING_STRING" => Some(NcsAuxCode::TypeTypeStringString),
        "TT" | "TYPETYPE_STRUCT_STRUCT" => Some(NcsAuxCode::TypeTypeStructStruct),
        "IF" | "TYPETYPE_INTEGER_FLOAT" => Some(NcsAuxCode::TypeTypeIntegerFloat),
        "FI" | "TYPETYPE_FLOAT_INTEGER" => Some(NcsAuxCode::TypeTypeFloatInteger),
        "E0E0" | "TYPETYPE_ENGST0_ENGST0" => Some(NcsAuxCode::TypeTypeEngst0Engst0),
        "E1E1" | "TYPETYPE_ENGST1_ENGST1" => Some(NcsAuxCode::TypeTypeEngst1Engst1),
        "E2E2" | "TYPETYPE_ENGST2_ENGST2" => Some(NcsAuxCode::TypeTypeEngst2Engst2),
        "E3E3" | "TYPETYPE_ENGST3_ENGST3" => Some(NcsAuxCode::TypeTypeEngst3Engst3),
        "E4E4" | "TYPETYPE_ENGST4_ENGST4" => Some(NcsAuxCode::TypeTypeEngst4Engst4),
        "E5E5" | "TYPETYPE_ENGST5_ENGST5" => Some(NcsAuxCode::TypeTypeEngst5Engst5),
        "E6E6" | "TYPETYPE_ENGST6_ENGST6" => Some(NcsAuxCode::TypeTypeEngst6Engst6),
        "E7E7" | "TYPETYPE_ENGST7_ENGST7" => Some(NcsAuxCode::TypeTypeEngst7Engst7),
        "E8E8" | "TYPETYPE_ENGST8_ENGST8" => Some(NcsAuxCode::TypeTypeEngst8Engst8),
        "E9E9" | "TYPETYPE_ENGST9_ENGST9" => Some(NcsAuxCode::TypeTypeEngst9Engst9),
        "VV" | "TYPETYPE_VECTOR_VECTOR" => Some(NcsAuxCode::TypeTypeVectorVector),
        "VF" | "TYPETYPE_VECTOR_FLOAT" => Some(NcsAuxCode::TypeTypeVectorFloat),
        "FV" | "TYPETYPE_FLOAT_VECTOR" => Some(NcsAuxCode::TypeTypeFloatVector),
        "EVAL_INPLACE" => Some(NcsAuxCode::EvalInplace),
        "EVAL_POSTPLACE" => Some(NcsAuxCode::EvalPostplace),
        _ => None,
    }
}

fn parse_instruction_operand(
    opcode: NcsOpcode,
    auxcode: NcsAuxCode,
    extra: &str,
    langspec: Option<&LangSpec>,
    line: usize,
) -> Result<ParsedAsmOperand, NcsAsmError> {
    let extra = extra.trim();
    match opcode {
        NcsOpcode::Constant => parse_constant_operand(auxcode, extra, line),
        NcsOpcode::Jmp | NcsOpcode::Jsr | NcsOpcode::Jz | NcsOpcode::Jnz => {
            parse_jump_operand(extra, line)
        }
        NcsOpcode::StoreState => parse_store_state_operand(extra, line),
        NcsOpcode::ModifyStackPointer
        | NcsOpcode::Increment
        | NcsOpcode::Decrement
        | NcsOpcode::IncrementBase
        | NcsOpcode::DecrementBase => parse_single_number_bytes::<i32>(extra, line),
        NcsOpcode::ExecuteCommand => parse_action_operand(extra, langspec, line),
        NcsOpcode::RunstackCopy | NcsOpcode::RunstackCopyBase => {
            parse_runstack_copy_operand(extra, line)
        }
        NcsOpcode::Assignment | NcsOpcode::AssignmentBase => parse_assignment_operand(extra, line),
        NcsOpcode::DeStruct => parse_destruct_operand(extra, line),
        NcsOpcode::Equal | NcsOpcode::NotEqual if auxcode == NcsAuxCode::TypeTypeStructStruct => {
            parse_single_number_bytes::<u16>(extra, line)
        }
        _ if extra.is_empty() => Ok(ParsedAsmOperand::None),
        _ => Err(NcsAsmError::Parse {
            line,
            message: format!("instruction {opcode} does not accept operands {extra:?}"),
        }),
    }
}

fn parse_store_state_operand(extra: &str, line: usize) -> Result<ParsedAsmOperand, NcsAsmError> {
    let parts = split_csv(extra);
    let [first, second] = parts.as_slice() else {
        return Err(NcsAsmError::Parse {
            line,
            message: format!("expected `a, b`, found {extra:?}"),
        });
    };
    let first = parse_i32_like(first, line)?;
    let second = parse_i32_like(second, line)?;
    Ok(ParsedAsmOperand::Bytes(
        [
            first.to_be_bytes().as_slice(),
            second.to_be_bytes().as_slice(),
        ]
        .concat(),
    ))
}

fn parse_constant_operand(
    auxcode: NcsAuxCode,
    extra: &str,
    line: usize,
) -> Result<ParsedAsmOperand, NcsAsmError> {
    match auxcode {
        NcsAuxCode::TypeString | NcsAuxCode::TypeEngst7 => {
            let value = unescape_nwasm_string(extra, line)?;
            let length = u16::try_from(value.len()).map_err(|_error| NcsAsmError::Parse {
                line,
                message: format!("string operand too long: {} bytes", value.len()),
            })?;
            Ok(ParsedAsmOperand::Bytes(
                [length.to_be_bytes().as_slice(), value.as_bytes()].concat(),
            ))
        }
        NcsAuxCode::TypeInteger => parse_single_number_bytes::<i32>(extra, line),
        NcsAuxCode::TypeFloat => {
            let value = extra.parse::<f32>().map_err(|error| NcsAsmError::Parse {
                line,
                message: format!("invalid float operand {extra:?}: {error}"),
            })?;
            Ok(ParsedAsmOperand::Bytes(
                value.to_bits().to_be_bytes().to_vec(),
            ))
        }
        NcsAuxCode::TypeObject => {
            let value = parse_i32_like(extra, line)?;
            Ok(ParsedAsmOperand::Bytes(value.to_be_bytes().to_vec()))
        }
        NcsAuxCode::TypeEngst2 => {
            let value = parse_u32_like(extra, line)?;
            Ok(ParsedAsmOperand::Bytes(value.to_be_bytes().to_vec()))
        }
        _ if extra.is_empty() => Ok(ParsedAsmOperand::None),
        _ => Err(NcsAsmError::Parse {
            line,
            message: format!("unsupported CONST operand for auxcode {auxcode:?}"),
        }),
    }
}

fn parse_jump_operand(extra: &str, line: usize) -> Result<ParsedAsmOperand, NcsAsmError> {
    if extra.is_empty() {
        return Err(NcsAsmError::Parse {
            line,
            message: "missing jump target".to_string(),
        });
    }
    if let Some(offset) = parse_usize_like(extra) {
        return Ok(ParsedAsmOperand::Jump(AsmJumpTarget::Offset(offset)));
    }
    Ok(ParsedAsmOperand::Jump(AsmJumpTarget::Label(
        extra.to_string(),
    )))
}

fn parse_action_operand(
    extra: &str,
    langspec: Option<&LangSpec>,
    line: usize,
) -> Result<ParsedAsmOperand, NcsAsmError> {
    let parts = split_csv(extra);
    match parts.as_slice() {
        [id, argc] => {
            let id = parse_u16_like(id, line)?;
            let argc = parse_u8_like(argc, line)?;
            Ok(ParsedAsmOperand::Bytes(
                [id.to_be_bytes().as_slice(), &[argc]].concat(),
            ))
        }
        [name] if !name.is_empty() => {
            let Some(spec) = langspec else {
                return Err(NcsAsmError::Parse {
                    line,
                    message: format!(
                        "ACTION operand {name:?} requires langspec or explicit `id, argc`"
                    ),
                });
            };
            let (builtin_id, function) = spec
                .functions
                .iter()
                .enumerate()
                .find(|(_index, function)| function.name == *name)
                .ok_or_else(|| NcsAsmError::Parse {
                    line,
                    message: format!("unknown ACTION builtin {name:?}"),
                })?;
            let argc =
                u8::try_from(function.parameters.len()).map_err(|_error| NcsAsmError::Parse {
                    line,
                    message: format!(
                        "ACTION builtin {name:?} has too many parameters to infer argc"
                    ),
                })?;
            let builtin_id = u16::try_from(builtin_id).map_err(|_error| NcsAsmError::Parse {
                line,
                message: format!("ACTION builtin index out of range for {name:?}"),
            })?;
            Ok(ParsedAsmOperand::Bytes(
                [builtin_id.to_be_bytes().as_slice(), &[argc]].concat(),
            ))
        }
        _ => Err(NcsAsmError::Parse {
            line,
            message: format!("invalid ACTION operand {extra:?}"),
        }),
    }
}

fn parse_runstack_copy_operand(extra: &str, line: usize) -> Result<ParsedAsmOperand, NcsAsmError> {
    let parts = split_csv(extra);
    let [offset, size] = parts.as_slice() else {
        return Err(NcsAsmError::Parse {
            line,
            message: format!("expected `offset, size`, found {extra:?}"),
        });
    };
    let offset = parse_i32_like(offset, line)?;
    let size = parse_i16_like(size, line)?;
    Ok(ParsedAsmOperand::Bytes(
        [
            offset.to_be_bytes().as_slice(),
            size.to_be_bytes().as_slice(),
        ]
        .concat(),
    ))
}

fn parse_assignment_operand(extra: &str, line: usize) -> Result<ParsedAsmOperand, NcsAsmError> {
    let parts = split_csv(extra);
    let [offset, size] = parts.as_slice() else {
        return Err(NcsAsmError::Parse {
            line,
            message: format!("expected `offset, size`, found {extra:?}"),
        });
    };
    let offset = parse_i32_like(offset, line)?;
    let size = parse_u16_like(size, line)?;
    Ok(ParsedAsmOperand::Bytes(
        [
            offset.to_be_bytes().as_slice(),
            size.to_be_bytes().as_slice(),
        ]
        .concat(),
    ))
}

fn parse_destruct_operand(extra: &str, line: usize) -> Result<ParsedAsmOperand, NcsAsmError> {
    let parts = split_csv(extra);
    let [first, second, third] = parts.as_slice() else {
        return Err(NcsAsmError::Parse {
            line,
            message: format!("expected `a, b, c`, found {extra:?}"),
        });
    };
    let first = parse_u16_like(first, line)?;
    let second = parse_u16_like(second, line)?;
    let third = parse_u16_like(third, line)?;
    Ok(ParsedAsmOperand::Bytes(
        [
            first.to_be_bytes().as_slice(),
            second.to_be_bytes().as_slice(),
            third.to_be_bytes().as_slice(),
        ]
        .concat(),
    ))
}

fn parse_single_number_bytes<T>(extra: &str, line: usize) -> Result<ParsedAsmOperand, NcsAsmError>
where
    T: ParseAsmNumber,
{
    let value = T::parse(extra, line)?;
    Ok(ParsedAsmOperand::Bytes(value.to_be_bytes_vec()))
}

fn split_csv(input: &str) -> Vec<&str> {
    input.split(',').map(str::trim).collect()
}

trait ParseAsmNumber {
    fn parse(input: &str, line: usize) -> Result<Self, NcsAsmError>
    where
        Self: Sized;
    fn to_be_bytes_vec(self) -> Vec<u8>;
}

impl ParseAsmNumber for i32 {
    fn parse(input: &str, line: usize) -> Result<Self, NcsAsmError> {
        parse_i32_like(input, line)
    }

    fn to_be_bytes_vec(self) -> Vec<u8> {
        self.to_be_bytes().to_vec()
    }
}

impl ParseAsmNumber for u16 {
    fn parse(input: &str, line: usize) -> Result<Self, NcsAsmError> {
        parse_u16_like(input, line)
    }

    fn to_be_bytes_vec(self) -> Vec<u8> {
        self.to_be_bytes().to_vec()
    }
}

fn parse_i32_like(input: &str, line: usize) -> Result<i32, NcsAsmError> {
    let input = input.trim();
    if let Some(hex) = input
        .strip_prefix("0x")
        .or_else(|| input.strip_prefix("0X"))
    {
        u32::from_str_radix(hex, 16)
            .map(|value| i32::from_be_bytes(value.to_be_bytes()))
            .map_err(|error| NcsAsmError::Parse {
                line,
                message: format!("invalid hex i32 operand {input:?}: {error}"),
            })
    } else {
        input.parse::<i32>().map_err(|error| NcsAsmError::Parse {
            line,
            message: format!("invalid i32 operand {input:?}: {error}"),
        })
    }
}

fn parse_i16_like(input: &str, line: usize) -> Result<i16, NcsAsmError> {
    input
        .trim()
        .parse::<i16>()
        .map_err(|error| NcsAsmError::Parse {
            line,
            message: format!("invalid i16 operand {input:?}: {error}"),
        })
}

fn parse_u16_like(input: &str, line: usize) -> Result<u16, NcsAsmError> {
    let input = input.trim();
    if let Some(hex) = input
        .strip_prefix("0x")
        .or_else(|| input.strip_prefix("0X"))
    {
        u16::from_str_radix(hex, 16).map_err(|error| NcsAsmError::Parse {
            line,
            message: format!("invalid hex u16 operand {input:?}: {error}"),
        })
    } else {
        input.parse::<u16>().map_err(|error| NcsAsmError::Parse {
            line,
            message: format!("invalid u16 operand {input:?}: {error}"),
        })
    }
}

fn parse_u8_like(input: &str, line: usize) -> Result<u8, NcsAsmError> {
    input
        .trim()
        .parse::<u8>()
        .map_err(|error| NcsAsmError::Parse {
            line,
            message: format!("invalid u8 operand {input:?}: {error}"),
        })
}

fn parse_u32_like(input: &str, line: usize) -> Result<u32, NcsAsmError> {
    let input = input.trim();
    if let Some(hex) = input
        .strip_prefix("0x")
        .or_else(|| input.strip_prefix("0X"))
    {
        u32::from_str_radix(hex, 16).map_err(|error| NcsAsmError::Parse {
            line,
            message: format!("invalid hex u32 operand {input:?}: {error}"),
        })
    } else {
        input.parse::<u32>().map_err(|error| NcsAsmError::Parse {
            line,
            message: format!("invalid u32 operand {input:?}: {error}"),
        })
    }
}

fn parse_usize_like(input: &str) -> Option<usize> {
    let input = input.trim();
    input.parse::<usize>().ok().or_else(|| {
        input
            .strip_prefix("0x")
            .or_else(|| input.strip_prefix("0X"))
            .and_then(|hex| usize::from_str_radix(hex, 16).ok())
    })
}

fn unescape_nwasm_string(input: &str, line: usize) -> Result<String, NcsAsmError> {
    let mut chars = input.chars().peekable();
    let mut output = String::new();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            output.push(ch);
            continue;
        }
        let escaped = chars.next().ok_or_else(|| NcsAsmError::Parse {
            line,
            message: "dangling string escape".to_string(),
        })?;
        match escaped {
            '\\' => output.push('\\'),
            '\'' => output.push('\''),
            '"' => output.push('"'),
            'n' => output.push('\n'),
            'r' => output.push('\r'),
            't' => output.push('\t'),
            '0' => output.push('\0'),
            'x' => {
                let hi = chars.next().ok_or_else(|| NcsAsmError::Parse {
                    line,
                    message: "incomplete \\xNN string escape".to_string(),
                })?;
                let lo = chars.next().ok_or_else(|| NcsAsmError::Parse {
                    line,
                    message: "incomplete \\xNN string escape".to_string(),
                })?;
                let value = u8::from_str_radix(&format!("{hi}{lo}"), 16).map_err(|error| {
                    NcsAsmError::Parse {
                        line,
                        message: format!("invalid \\x escape: {error}"),
                    }
                })?;
                output.push(char::from(value));
            }
            'u' => {
                if chars.next() != Some('{') {
                    return Err(NcsAsmError::Parse {
                        line,
                        message: "expected `\\u{...}` escape".to_string(),
                    });
                }
                let mut digits = String::new();
                loop {
                    let next = chars.next().ok_or_else(|| NcsAsmError::Parse {
                        line,
                        message: "unterminated `\\u{...}` escape".to_string(),
                    })?;
                    if next == '}' {
                        break;
                    }
                    digits.push(next);
                }
                let value =
                    u32::from_str_radix(&digits, 16).map_err(|error| NcsAsmError::Parse {
                        line,
                        message: format!("invalid unicode escape: {error}"),
                    })?;
                let ch = char::from_u32(value).ok_or_else(|| NcsAsmError::Parse {
                    line,
                    message: format!("invalid unicode scalar value {value:#x}"),
                })?;
                output.push(ch);
            }
            other => {
                return Err(NcsAsmError::Parse {
                    line,
                    message: format!("unsupported string escape \\{other}"),
                });
            }
        }
    }
    Ok(output)
}

fn truncate_nwasm_string(value: &str, max_string_length: usize) -> String {
    let escaped = value
        .chars()
        .take(max_string_length)
        .flat_map(char::escape_default)
        .collect::<String>();
    if value.len() > max_string_length {
        format!("{escaped}..{}", value.len())
    } else {
        escaped
    }
}

fn collect_jump_labels(instructions: &[NcsInstruction]) -> BTreeMap<usize, String> {
    let mut offset = 0usize;
    let mut targets = BTreeSet::new();
    let mut labels = BTreeMap::new();

    for instruction in instructions {
        if let Some(target) = jump_target_for_instruction(instruction, offset) {
            targets.insert((instruction.opcode as u8, target));
        }
        offset += instruction.encoded_len();
    }

    for (opcode, target) in targets {
        let prefix = if opcode == NcsOpcode::Jsr as u8 {
            "sub"
        } else {
            "loc"
        };
        labels
            .entry(target)
            .or_insert_with(|| format!("{prefix}_{target:04}"));
    }

    labels
}

fn jump_target_for_instruction(instruction: &NcsInstruction, offset: usize) -> Option<usize> {
    if !matches!(
        instruction.opcode,
        NcsOpcode::Jmp | NcsOpcode::Jsr | NcsOpcode::Jz | NcsOpcode::Jnz
    ) {
        return None;
    }
    read_i32(&instruction.extra, offset, instruction)
        .ok()
        .map(|relative| jump_target(offset, relative))
}

fn jump_target(offset: usize, relative: i32) -> usize {
    let base = i64::try_from(offset).ok().unwrap_or(i64::MAX);
    let target = base.saturating_add(i64::from(relative));
    usize::try_from(target.max(0)).ok().unwrap_or(0)
}

fn format_jump_target(
    opcode: NcsOpcode,
    target: usize,
    labels: &BTreeMap<usize, String>,
) -> String {
    if let Some(label) = labels.get(&target) {
        label.clone()
    } else if opcode == NcsOpcode::Jsr {
        format!("sub_{target:04}")
    } else {
        format!("loc_{target:04}")
    }
}

fn sorted_functions(ndb: &Ndb) -> Vec<&NdbFunction> {
    let mut functions = ndb.functions.iter().collect::<Vec<_>>();
    functions.sort_by_key(|function| function.binary_start);
    functions
}

fn line_in_function(offset: usize, function: &NdbFunction) -> bool {
    let start = function.binary_start.saturating_sub(13) as usize;
    let end = function.binary_end.saturating_sub(13) as usize;
    (start..end).contains(&offset)
}

fn local_offset(offset: usize, function: &NdbFunction) -> usize {
    let start = function.binary_start.saturating_sub(13) as usize;
    offset.saturating_sub(start)
}

fn render_function_header(function: &NdbFunction, ndb: &Ndb) -> String {
    let args = function
        .args
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    let start = function.binary_start.saturating_sub(13);
    let end = function.binary_end.saturating_sub(13);
    let location = source_location_for_function(function, ndb).map_or_else(
        || " ".to_string(),
        |(file, line)| format!(" {file}.nss:{} ", line.saturating_sub(1)),
    );

    format!(
        "{} {}({}):{}[{}:{}]",
        function.return_type, function.label, args, location, start, end
    )
}

fn source_location_for_function(function: &NdbFunction, ndb: &Ndb) -> Option<(String, usize)> {
    ndb.lines
        .iter()
        .find(|line| line.binary_start == function.binary_start)
        .and_then(|line| {
            ndb.files
                .get(line.file_num)
                .map(|file| (file.name.clone(), line.line_num))
        })
}

fn source_line_for_offset(
    offset: usize,
    ndb: &Ndb,
    source_files: Option<&BTreeMap<String, Vec<String>>>,
    last_source: &mut Option<(usize, usize)>,
) -> Option<(String, String)> {
    let line = unique_line_for_offset(offset, &ndb.lines)?;
    let key = (line.file_num, line.line_num);
    if last_source.as_ref() == Some(&key) {
        return None;
    }
    *last_source = Some(key);

    let file = ndb.files.get(line.file_num)?;
    let source_files = source_files?;
    let lines = source_files.get(&file.name)?;
    let text = lines
        .get(line.line_num.saturating_sub(1))?
        .trim()
        .to_string();
    Some((
        format!("{}.nss:{}", file.name, line.line_num.saturating_sub(1)),
        text,
    ))
}

fn unique_line_for_offset(offset: usize, lines: &[NdbLine]) -> Option<&NdbLine> {
    let matches = lines
        .iter()
        .filter(|line| {
            let start = line.binary_start.saturating_sub(13) as usize;
            let end = line.binary_end.saturating_sub(13) as usize;
            (start..end).contains(&offset)
        })
        .collect::<Vec<_>>();
    if matches.len() == 1 {
        matches.into_iter().next()
    } else {
        None
    }
}

fn decode_prefixed_string(
    extra: &[u8],
    offset: usize,
    instruction: &NcsInstruction,
) -> Result<String, NcsAsmError> {
    let length = usize::from(read_u16(extra, offset, instruction)?);
    let payload = extra
        .get(2..2 + length)
        .ok_or_else(|| NcsAsmError::InvalidExtra {
            offset,
            opcode: instruction.opcode,
            auxcode: instruction.auxcode,
            message: format!("expected {length} string bytes"),
        })?;
    Ok(String::from_utf8_lossy(payload).to_string())
}

fn read_u8_part(
    extra: &[u8],
    start: usize,
    offset: usize,
    instruction: &NcsInstruction,
) -> Result<u8, NcsAsmError> {
    extra
        .get(start)
        .copied()
        .ok_or_else(|| NcsAsmError::InvalidExtra {
            offset,
            opcode: instruction.opcode,
            auxcode: instruction.auxcode,
            message: format!("expected byte at offset {start}"),
        })
}

fn read_u16(extra: &[u8], offset: usize, instruction: &NcsInstruction) -> Result<u16, NcsAsmError> {
    read_u16_part(extra, 0, offset, instruction)
}

fn read_u16_part(
    extra: &[u8],
    start: usize,
    offset: usize,
    instruction: &NcsInstruction,
) -> Result<u16, NcsAsmError> {
    let bytes: [u8; 2] = extra
        .get(start..start + 2)
        .ok_or_else(|| NcsAsmError::InvalidExtra {
            offset,
            opcode: instruction.opcode,
            auxcode: instruction.auxcode,
            message: format!("expected 2 bytes at offset {start}"),
        })?
        .try_into()
        .map_err(|_error| NcsAsmError::InvalidExtra {
            offset,
            opcode: instruction.opcode,
            auxcode: instruction.auxcode,
            message: format!("expected 2 bytes at offset {start}"),
        })?;
    Ok(u16::from_be_bytes(bytes))
}

fn read_i16_part(
    extra: &[u8],
    start: usize,
    offset: usize,
    instruction: &NcsInstruction,
) -> Result<i16, NcsAsmError> {
    let bytes: [u8; 2] = extra
        .get(start..start + 2)
        .ok_or_else(|| NcsAsmError::InvalidExtra {
            offset,
            opcode: instruction.opcode,
            auxcode: instruction.auxcode,
            message: format!("expected 2 bytes at offset {start}"),
        })?
        .try_into()
        .map_err(|_error| NcsAsmError::InvalidExtra {
            offset,
            opcode: instruction.opcode,
            auxcode: instruction.auxcode,
            message: format!("expected 2 bytes at offset {start}"),
        })?;
    Ok(i16::from_be_bytes(bytes))
}

fn read_i32(extra: &[u8], offset: usize, instruction: &NcsInstruction) -> Result<i32, NcsAsmError> {
    read_i32_part(extra, 0, offset, instruction)
}

fn read_i32_part(
    extra: &[u8],
    start: usize,
    offset: usize,
    instruction: &NcsInstruction,
) -> Result<i32, NcsAsmError> {
    let bytes: [u8; 4] = extra
        .get(start..start + 4)
        .ok_or_else(|| NcsAsmError::InvalidExtra {
            offset,
            opcode: instruction.opcode,
            auxcode: instruction.auxcode,
            message: format!("expected 4 bytes at offset {start}"),
        })?
        .try_into()
        .map_err(|_error| NcsAsmError::InvalidExtra {
            offset,
            opcode: instruction.opcode,
            auxcode: instruction.auxcode,
            message: format!("expected 4 bytes at offset {start}"),
        })?;
    Ok(i32::from_be_bytes(bytes))
}

fn read_u32(extra: &[u8], offset: usize, instruction: &NcsInstruction) -> Result<u32, NcsAsmError> {
    let bytes: [u8; 4] = extra
        .get(..4)
        .ok_or_else(|| NcsAsmError::InvalidExtra {
            offset,
            opcode: instruction.opcode,
            auxcode: instruction.auxcode,
            message: "expected 4 bytes".to_string(),
        })?
        .try_into()
        .map_err(|_error| NcsAsmError::InvalidExtra {
            offset,
            opcode: instruction.opcode,
            auxcode: instruction.auxcode,
            message: "expected 4 bytes".to_string(),
        })?;
    Ok(u32::from_be_bytes(bytes))
}

fn read_f32(extra: &[u8], offset: usize, instruction: &NcsInstruction) -> Result<f32, NcsAsmError> {
    Ok(f32::from_bits(read_u32(extra, offset, instruction)?))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{
        NcsAsmLine, NcsDisassemblyOptions, assemble_ncs_bytes, assemble_ncs_text, disassemble_ncs,
        render_ncs_disassembly, render_ncs_disassembly_with_ndb,
    };
    use crate::{
        BuiltinFunction, BuiltinType, LangSpec, NcsAuxCode, NcsInstruction, NcsOpcode, Ndb,
        NdbFile, NdbFunction, NdbLine, NdbType, encode_ncs_instructions,
    };

    #[test]
    fn instruction_names_match_upstream_nwasm() {
        let instruction = NcsInstruction {
            opcode:  NcsOpcode::LogicalAnd,
            auxcode: NcsAuxCode::TypeTypeIntegerInteger,
            extra:   b"not-printed".to_vec(),
        };

        assert_eq!(instruction.canonical_name(false), "LOGAND.II");
        assert_eq!(
            instruction.canonical_name(true),
            "LOGICAL_AND.TYPETYPE_INTEGER_INTEGER"
        );
    }

    #[test]
    fn disassembles_action_names_with_langspec() -> Result<(), Box<dyn std::error::Error>> {
        let bytes = encode_ncs_instructions(&[
            NcsInstruction {
                opcode:  NcsOpcode::ExecuteCommand,
                auxcode: NcsAuxCode::None,
                extra:   [1_u16.to_be_bytes().as_slice(), &[2_u8]].concat(),
            },
            NcsInstruction {
                opcode:  NcsOpcode::Ret,
                auxcode: NcsAuxCode::None,
                extra:   Vec::new(),
            },
        ]);
        let spec = LangSpec {
            engine_num_structures: 0,
            engine_structures:     Vec::new(),
            constants:             Vec::new(),
            functions:             vec![
                BuiltinFunction {
                    name:        "First".to_string(),
                    return_type: BuiltinType::Void,
                    parameters:  Vec::new(),
                },
                BuiltinFunction {
                    name:        "DelayCommand".to_string(),
                    return_type: BuiltinType::Void,
                    parameters:  Vec::new(),
                },
            ],
        };

        let lines = disassemble_ncs(&bytes, Some(&spec), NcsDisassemblyOptions::default())?;
        assert_eq!(
            lines,
            vec![
                NcsAsmLine {
                    offset:      0,
                    label:       None,
                    instruction: "ACTION".to_string(),
                    extra:       "1, 2".to_string(),
                },
                NcsAsmLine {
                    offset:      5,
                    label:       None,
                    instruction: "RET".to_string(),
                    extra:       String::new(),
                },
            ]
        );
        Ok(())
    }

    #[test]
    fn disassembles_string_constants_like_upstream() -> Result<(), Box<dyn std::error::Error>> {
        let extra = [5_u16.to_be_bytes().as_slice(), b"hello"].concat();
        let bytes = encode_ncs_instructions(&[NcsInstruction {
            opcode: NcsOpcode::Constant,
            auxcode: NcsAuxCode::TypeString,
            extra,
        }]);
        let lines = disassemble_ncs(&bytes, None, NcsDisassemblyOptions::default())?;

        assert_eq!(lines.len(), 1);
        let line = lines
            .first()
            .expect("string disassembly should produce one line");
        assert_eq!(line.instruction, "CONST.S");
        assert_eq!(line.extra, "hello");
        Ok(())
    }

    #[test]
    fn renders_jump_targets_with_labels() -> Result<(), Box<dyn std::error::Error>> {
        let bytes = encode_ncs_instructions(&[
            NcsInstruction {
                opcode:  NcsOpcode::Jz,
                auxcode: NcsAuxCode::None,
                extra:   8_i32.to_be_bytes().to_vec(),
            },
            NcsInstruction {
                opcode:  NcsOpcode::Ret,
                auxcode: NcsAuxCode::None,
                extra:   Vec::new(),
            },
            NcsInstruction {
                opcode:  NcsOpcode::Ret,
                auxcode: NcsAuxCode::None,
                extra:   Vec::new(),
            },
        ]);

        let rendered = render_ncs_disassembly(&bytes, None, NcsDisassemblyOptions::default())?;
        assert!(rendered.contains("0000: JZ loc_0008"));
        assert!(rendered.contains("loc_0008:"));
        Ok(())
    }

    #[test]
    fn renders_function_headers_and_source_weaving_with_ndb()
    -> Result<(), Box<dyn std::error::Error>> {
        let bytes = encode_ncs_instructions(&[NcsInstruction {
            opcode:  NcsOpcode::Ret,
            auxcode: NcsAuxCode::None,
            extra:   Vec::new(),
        }]);
        let ndb = Ndb {
            files:     vec![NdbFile {
                name:    "test".to_string(),
                is_root: true,
            }],
            structs:   Vec::new(),
            functions: vec![NdbFunction {
                label:        "main".to_string(),
                binary_start: 13,
                binary_end:   15,
                return_type:  NdbType::Void,
                args:         Vec::new(),
            }],
            variables: Vec::new(),
            lines:     vec![NdbLine {
                file_num:     0,
                line_num:     1,
                binary_start: 13,
                binary_end:   15,
            }],
        };
        let mut sources = BTreeMap::new();
        sources.insert("test".to_string(), vec!["void main() {}".to_string()]);

        let rendered = render_ncs_disassembly_with_ndb(
            &bytes,
            None,
            Some(&ndb),
            Some(&sources),
            NcsDisassemblyOptions::default(),
        )?;

        assert!(rendered.contains("v main(): test.nss:0 [0:2]"));
        assert!(rendered.contains("0000 0000: RET | test.nss:0 | void main() {}"));
        Ok(())
    }

    #[test]
    fn renders_jsr_targets_as_function_labels_with_ndb() -> Result<(), Box<dyn std::error::Error>> {
        let bytes = encode_ncs_instructions(&[
            NcsInstruction {
                opcode:  NcsOpcode::Jsr,
                auxcode: NcsAuxCode::None,
                extra:   6_i32.to_be_bytes().to_vec(),
            },
            NcsInstruction {
                opcode:  NcsOpcode::Ret,
                auxcode: NcsAuxCode::None,
                extra:   Vec::new(),
            },
            NcsInstruction {
                opcode:  NcsOpcode::Ret,
                auxcode: NcsAuxCode::None,
                extra:   Vec::new(),
            },
        ]);
        let ndb = Ndb {
            files:     vec![NdbFile {
                name:    "test".to_string(),
                is_root: true,
            }],
            structs:   Vec::new(),
            functions: vec![NdbFunction {
                label:        "helper".to_string(),
                binary_start: 19,
                binary_end:   21,
                return_type:  NdbType::Void,
                args:         Vec::new(),
            }],
            variables: Vec::new(),
            lines:     Vec::new(),
        };

        let rendered = render_ncs_disassembly_with_ndb(
            &bytes,
            None,
            Some(&ndb),
            None,
            NcsDisassemblyOptions::default(),
        )?;

        assert!(rendered.contains("0000: JSR helper"));
        Ok(())
    }

    #[test]
    fn assemble_roundtrips_rendered_disassembly_to_identical_bytes()
    -> Result<(), Box<dyn std::error::Error>> {
        let bytes = encode_ncs_instructions(&[
            NcsInstruction {
                opcode:  NcsOpcode::Constant,
                auxcode: NcsAuxCode::TypeString,
                extra:   [6_u16.to_be_bytes().as_slice(), b"hi\\n[]"].concat(),
            },
            NcsInstruction {
                opcode:  NcsOpcode::AssignmentBase,
                auxcode: NcsAuxCode::TypeVoid,
                extra:   [
                    (-12_i32).to_be_bytes().as_slice(),
                    4_u16.to_be_bytes().as_slice(),
                ]
                .concat(),
            },
            NcsInstruction {
                opcode:  NcsOpcode::ExecuteCommand,
                auxcode: NcsAuxCode::None,
                extra:   [7_u16.to_be_bytes().as_slice(), &[1_u8]].concat(),
            },
            NcsInstruction {
                opcode:  NcsOpcode::Jsr,
                auxcode: NcsAuxCode::None,
                extra:   12_i32.to_be_bytes().to_vec(),
            },
            NcsInstruction {
                opcode:  NcsOpcode::Equal,
                auxcode: NcsAuxCode::TypeTypeStructStruct,
                extra:   12_u16.to_be_bytes().to_vec(),
            },
            NcsInstruction {
                opcode:  NcsOpcode::Ret,
                auxcode: NcsAuxCode::None,
                extra:   Vec::new(),
            },
            NcsInstruction {
                opcode:  NcsOpcode::Ret,
                auxcode: NcsAuxCode::None,
                extra:   Vec::new(),
            },
        ]);

        let rendered = render_ncs_disassembly(
            &bytes,
            None,
            NcsDisassemblyOptions {
                max_string_length: usize::MAX,
                ..NcsDisassemblyOptions::default()
            },
        )?;
        let reassembled = assemble_ncs_bytes(&rendered, None)?;

        assert_eq!(reassembled, bytes);
        Ok(())
    }

    #[test]
    fn assemble_accepts_internal_names_and_explicit_auxcodes()
    -> Result<(), Box<dyn std::error::Error>> {
        let text = "\
0000: ASSIGNMENT_BASE.TYPE_VOID -8, 4
0008: EXECUTE_COMMAND.NONE 1, 2
0013: RET.NONE
";
        let instructions = assemble_ncs_text(text, None)?;

        assert_eq!(instructions.len(), 3);
        let first = instructions.first().expect("expected first instruction");
        let second = instructions.get(1).expect("expected second instruction");
        assert_eq!(first.opcode, NcsOpcode::AssignmentBase);
        assert_eq!(first.auxcode, NcsAuxCode::TypeVoid);
        assert_eq!(second.opcode, NcsOpcode::ExecuteCommand);
        assert_eq!(second.auxcode, NcsAuxCode::None);
        Ok(())
    }
}
