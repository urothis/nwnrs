use std::{error::Error, fmt};

/// Text prefix of an `NCS V1.0` bytecode stream.
pub const NCS_HEADER: &str = "NCS V1.0";
/// Total size of the fixed binary header, including the encoded bytecode size.
pub const NCS_BINARY_HEADER_SIZE: usize = 13;
/// Size of the opcode-plus-auxcode instruction prefix.
pub const NCS_OPERATION_BASE_SIZE: usize = 2;
/// Byte offset of the opcode field inside an instruction.
pub const NCS_OPCODE_OFFSET: usize = 0;
/// Byte offset of the auxcode field inside an instruction.
pub const NCS_AUXCODE_OFFSET: usize = 1;
/// Byte offset of the extra-data section inside an instruction.
pub const NCS_EXTRA_DATA_OFFSET: usize = 2;

/// Fixed metadata extracted from the leading bytes of an `NCS` file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NcsHeader {
    /// Encoded bytecode payload size recorded in the binary header.
    pub code_size: u32,
}

/// Errors returned while decoding the fixed `NCS` header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NcsHeaderError {
    /// The provided byte slice was shorter than the fixed header.
    TooShort(usize),
    /// The file prefix did not match the expected `NCS V1.0` signature.
    InvalidMagic,
    /// The binary marker byte after the text header was not `B`.
    InvalidMarker(u8),
}

impl fmt::Display for NcsHeaderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort(len) => write!(f, "NCS header too short: expected 13 bytes, got {len}"),
            Self::InvalidMagic => f.write_str("invalid NCS header magic"),
            Self::InvalidMarker(marker) => write!(f, "invalid NCS binary marker: {marker:#04x}"),
        }
    }
}

impl Error for NcsHeaderError {}

/// Decodes the fixed binary header of an `NCS V1.0` file.
///
/// # Errors
///
/// Returns [`NcsHeaderError`] if the bytes are too short or the magic is
/// invalid.
pub fn decode_ncs_header(bytes: &[u8]) -> Result<NcsHeader, NcsHeaderError> {
    if bytes.len() < NCS_BINARY_HEADER_SIZE {
        return Err(NcsHeaderError::TooShort(bytes.len()));
    }
    if bytes.get(..NCS_HEADER.len()) != Some(NCS_HEADER.as_bytes()) {
        return Err(NcsHeaderError::InvalidMagic);
    }
    let Some(marker) = bytes.get(8).copied() else {
        return Err(NcsHeaderError::TooShort(bytes.len()));
    };
    if marker != b'B' {
        return Err(NcsHeaderError::InvalidMarker(marker));
    }
    let Some(code_size_bytes) = bytes.get(9..13) else {
        return Err(NcsHeaderError::TooShort(bytes.len()));
    };

    let code_size = <[u8; 4]>::try_from(code_size_bytes)
        .map(u32::from_be_bytes)
        .map_err(|_error| NcsHeaderError::TooShort(bytes.len()))?;
    Ok(NcsHeader {
        code_size,
    })
}

/// One decoded `NCS` instruction.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NcsInstruction {
    /// Operation code.
    pub opcode:  NcsOpcode,
    /// Auxiliary code.
    pub auxcode: NcsAuxCode,
    /// Encoded instruction payload after opcode and auxcode.
    pub extra:   Vec<u8>,
}

impl NcsInstruction {
    /// Returns the full encoded byte length of this instruction.
    #[must_use]
    pub fn encoded_len(&self) -> usize {
        NCS_OPERATION_BASE_SIZE + self.extra.len()
    }
}

/// One `NWScript` VM opcode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum NcsOpcode {
    /// `CPDOWNSP`
    Assignment = 0x01,
    /// `RSADD`
    RunstackAdd = 0x02,
    /// `CPTOPSP`
    RunstackCopy = 0x03,
    /// `CONST`
    Constant = 0x04,
    /// `ACTION`
    ExecuteCommand = 0x05,
    /// `LOGAND`
    LogicalAnd = 0x06,
    /// `LOGOR`
    LogicalOr = 0x07,
    /// `INCOR`
    InclusiveOr = 0x08,
    /// `EXCOR`
    ExclusiveOr = 0x09,
    /// `BOOLAND`
    BooleanAnd = 0x0a,
    /// `EQUAL`
    Equal = 0x0b,
    /// `NEQUAL`
    NotEqual = 0x0c,
    /// `GEQ`
    Geq = 0x0d,
    /// `GT`
    Gt = 0x0e,
    /// `LT`
    Lt = 0x0f,
    /// `LEQ`
    Leq = 0x10,
    /// `SHLEFT`
    ShiftLeft = 0x11,
    /// `SHRIGHT`
    ShiftRight = 0x12,
    /// `USHRIGHT`
    UShiftRight = 0x13,
    /// `ADD`
    Add = 0x14,
    /// `SUB`
    Sub = 0x15,
    /// `MUL`
    Mul = 0x16,
    /// `DIV`
    Div = 0x17,
    /// `MOD`
    Modulus = 0x18,
    /// `NEG`
    Negation = 0x19,
    /// `COMP`
    OnesComplement = 0x1a,
    /// `MOVSP`
    ModifyStackPointer = 0x1b,
    /// `STOREIP`
    StoreIp = 0x1c,
    /// `JMP`
    Jmp = 0x1d,
    /// `JSR`
    Jsr = 0x1e,
    /// `JZ`
    Jz = 0x1f,
    /// `RET`
    Ret = 0x20,
    /// `DESTRUCT`
    DeStruct = 0x21,
    /// `NOT`
    BooleanNot = 0x22,
    /// `DECSP`
    Decrement = 0x23,
    /// `INCSP`
    Increment = 0x24,
    /// `JNZ`
    Jnz = 0x25,
    /// `CPDOWNBP`
    AssignmentBase = 0x26,
    /// `CPTOPBP`
    RunstackCopyBase = 0x27,
    /// `DECBP`
    DecrementBase = 0x28,
    /// `INCBP`
    IncrementBase = 0x29,
    /// `SAVEBP`
    SaveBasePointer = 0x2a,
    /// `RESTOREBP`
    RestoreBasePointer = 0x2b,
    /// `STORESTATE`
    StoreState = 0x2c,
    /// `NOP`
    NoOperation = 0x2d,
}

impl NcsOpcode {
    /// Returns the canonical mnemonic used by the upstream assembler helper.
    #[must_use]
    pub fn canonical_name(self) -> &'static str {
        match self {
            Self::Assignment => "CPDOWNSP",
            Self::RunstackAdd => "RSADD",
            Self::RunstackCopy => "CPTOPSP",
            Self::Constant => "CONST",
            Self::ExecuteCommand => "ACTION",
            Self::LogicalAnd => "LOGAND",
            Self::LogicalOr => "LOGOR",
            Self::InclusiveOr => "INCOR",
            Self::ExclusiveOr => "EXCOR",
            Self::BooleanAnd => "BOOLAND",
            Self::Equal => "EQUAL",
            Self::NotEqual => "NEQUAL",
            Self::Geq => "GEQ",
            Self::Gt => "GT",
            Self::Lt => "LT",
            Self::Leq => "LEQ",
            Self::ShiftLeft => "SHLEFT",
            Self::ShiftRight => "SHRIGHT",
            Self::UShiftRight => "USHRIGHT",
            Self::Add => "ADD",
            Self::Sub => "SUB",
            Self::Mul => "MUL",
            Self::Div => "DIV",
            Self::Modulus => "MOD",
            Self::Negation => "NEG",
            Self::OnesComplement => "COMP",
            Self::ModifyStackPointer => "MOVSP",
            Self::StoreIp => "STOREIP",
            Self::Jmp => "JMP",
            Self::Jsr => "JSR",
            Self::Jz => "JZ",
            Self::Ret => "RET",
            Self::DeStruct => "DESTRUCT",
            Self::BooleanNot => "NOT",
            Self::Decrement => "DECSP",
            Self::Increment => "INCSP",
            Self::Jnz => "JNZ",
            Self::AssignmentBase => "CPDOWNBP",
            Self::RunstackCopyBase => "CPTOPBP",
            Self::DecrementBase => "DECBP",
            Self::IncrementBase => "INCBP",
            Self::SaveBasePointer => "SAVEBP",
            Self::RestoreBasePointer => "RESTOREBP",
            Self::StoreState => "STORESTATE",
            Self::NoOperation => "NOP",
        }
    }
}

impl fmt::Display for NcsOpcode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.canonical_name())
    }
}

/// One `NWScript` VM aux code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum NcsAuxCode {
    /// No auxcode.
    None = 0x00,
    /// `void`.
    TypeVoid = 0x01,
    /// command type.
    TypeCommand = 0x02,
    /// integer type.
    TypeInteger = 0x03,
    /// float type.
    TypeFloat = 0x04,
    /// string type.
    TypeString = 0x05,
    /// object type.
    TypeObject = 0x06,
    /// engine structure 0.
    TypeEngst0 = 0x10,
    /// engine structure 1.
    TypeEngst1 = 0x11,
    /// engine structure 2.
    TypeEngst2 = 0x12,
    /// engine structure 3.
    TypeEngst3 = 0x13,
    /// engine structure 4.
    TypeEngst4 = 0x14,
    /// engine structure 5.
    TypeEngst5 = 0x15,
    /// engine structure 6.
    TypeEngst6 = 0x16,
    /// engine structure 7.
    TypeEngst7 = 0x17,
    /// engine structure 8.
    TypeEngst8 = 0x18,
    /// engine structure 9.
    TypeEngst9 = 0x19,
    /// integer/integer operator specialization.
    TypeTypeIntegerInteger = 0x20,
    /// float/float operator specialization.
    TypeTypeFloatFloat = 0x21,
    /// object/object operator specialization.
    TypeTypeObjectObject = 0x22,
    /// string/string operator specialization.
    TypeTypeStringString = 0x23,
    /// struct/struct operator specialization.
    TypeTypeStructStruct = 0x24,
    /// integer/float operator specialization.
    TypeTypeIntegerFloat = 0x25,
    /// float/integer operator specialization.
    TypeTypeFloatInteger = 0x26,
    /// engst0/engst0 operator specialization.
    TypeTypeEngst0Engst0 = 0x30,
    /// engst1/engst1 operator specialization.
    TypeTypeEngst1Engst1 = 0x31,
    /// engst2/engst2 operator specialization.
    TypeTypeEngst2Engst2 = 0x32,
    /// engst3/engst3 operator specialization.
    TypeTypeEngst3Engst3 = 0x33,
    /// engst4/engst4 operator specialization.
    TypeTypeEngst4Engst4 = 0x34,
    /// engst5/engst5 operator specialization.
    TypeTypeEngst5Engst5 = 0x35,
    /// engst6/engst6 operator specialization.
    TypeTypeEngst6Engst6 = 0x36,
    /// engst7/engst7 operator specialization.
    TypeTypeEngst7Engst7 = 0x37,
    /// engst8/engst8 operator specialization.
    TypeTypeEngst8Engst8 = 0x38,
    /// engst9/engst9 operator specialization.
    TypeTypeEngst9Engst9 = 0x39,
    /// vector/vector operator specialization.
    TypeTypeVectorVector = 0x3a,
    /// vector/float operator specialization.
    TypeTypeVectorFloat = 0x3b,
    /// float/vector operator specialization.
    TypeTypeFloatVector = 0x3c,
    /// in-place evaluation marker.
    EvalInplace = 0x70,
    /// post-place evaluation marker.
    EvalPostplace = 0x71,
}

impl NcsAuxCode {
    /// Returns the canonical short suffix used by the upstream assembler
    /// helper.
    #[must_use]
    pub fn canonical_name(self) -> Option<&'static str> {
        match self {
            Self::None
            | Self::TypeVoid
            | Self::TypeCommand
            | Self::EvalInplace
            | Self::EvalPostplace => None,
            Self::TypeInteger => Some("I"),
            Self::TypeFloat => Some("F"),
            Self::TypeString => Some("S"),
            Self::TypeObject => Some("O"),
            Self::TypeEngst0 => Some("E0"),
            Self::TypeEngst1 => Some("E1"),
            Self::TypeEngst2 => Some("E2"),
            Self::TypeEngst3 => Some("E3"),
            Self::TypeEngst4 => Some("E4"),
            Self::TypeEngst5 => Some("E5"),
            Self::TypeEngst6 => Some("E6"),
            Self::TypeEngst7 => Some("E7"),
            Self::TypeEngst8 => Some("E8"),
            Self::TypeEngst9 => Some("E9"),
            Self::TypeTypeIntegerInteger => Some("II"),
            Self::TypeTypeFloatFloat => Some("FF"),
            Self::TypeTypeObjectObject => Some("OO"),
            Self::TypeTypeStringString => Some("SS"),
            Self::TypeTypeStructStruct => Some("TT"),
            Self::TypeTypeIntegerFloat => Some("IF"),
            Self::TypeTypeFloatInteger => Some("FI"),
            Self::TypeTypeEngst0Engst0 => Some("E0E0"),
            Self::TypeTypeEngst1Engst1 => Some("E1E1"),
            Self::TypeTypeEngst2Engst2 => Some("E2E2"),
            Self::TypeTypeEngst3Engst3 => Some("E3E3"),
            Self::TypeTypeEngst4Engst4 => Some("E4E4"),
            Self::TypeTypeEngst5Engst5 => Some("E5E5"),
            Self::TypeTypeEngst6Engst6 => Some("E6E6"),
            Self::TypeTypeEngst7Engst7 => Some("E7E7"),
            Self::TypeTypeEngst8Engst8 => Some("E8E8"),
            Self::TypeTypeEngst9Engst9 => Some("E9E9"),
            Self::TypeTypeVectorVector => Some("VV"),
            Self::TypeTypeVectorFloat => Some("VF"),
            Self::TypeTypeFloatVector => Some("FV"),
        }
    }
}

impl fmt::Display for NcsAuxCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(name) = self.canonical_name() {
            f.write_str(name)
        } else {
            write!(f, "{:#04x}", *self as u8)
        }
    }
}

/// An error returned when converting an opcode byte to [`NcsOpcode`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnknownNcsOpcode(pub u8);

impl fmt::Display for UnknownNcsOpcode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown NCS opcode: {:#04x}", self.0)
    }
}

impl Error for UnknownNcsOpcode {}

/// An error returned when converting an auxcode byte to [`NcsAuxCode`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnknownNcsAuxCode(pub u8);

impl fmt::Display for UnknownNcsAuxCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown NCS aux code: {:#04x}", self.0)
    }
}

impl Error for UnknownNcsAuxCode {}

/// Errors returned while decoding a full `NCS` bytecode stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NcsReadError {
    /// Fixed header decode failed.
    Header(NcsHeaderError),
    /// An opcode byte was unknown.
    Opcode(UnknownNcsOpcode),
    /// An auxcode byte was unknown.
    AuxCode(UnknownNcsAuxCode),
    /// One instruction payload was truncated.
    TruncatedInstruction {
        /// Byte offset of the truncated instruction.
        offset:         usize,
        /// Expected payload size after opcode and auxcode.
        expected_extra: usize,
        /// Actual available payload bytes.
        actual_extra:   usize,
    },
}

impl fmt::Display for NcsReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Header(error) => error.fmt(f),
            Self::Opcode(error) => error.fmt(f),
            Self::AuxCode(error) => error.fmt(f),
            Self::TruncatedInstruction {
                offset,
                expected_extra,
                actual_extra,
            } => write!(
                f,
                "truncated NCS instruction at byte {offset}: expected {expected_extra} payload \
                 bytes, got {actual_extra}"
            ),
        }
    }
}

impl Error for NcsReadError {}

impl From<NcsHeaderError> for NcsReadError {
    fn from(value: NcsHeaderError) -> Self {
        Self::Header(value)
    }
}

impl From<UnknownNcsOpcode> for NcsReadError {
    fn from(value: UnknownNcsOpcode) -> Self {
        Self::Opcode(value)
    }
}

impl From<UnknownNcsAuxCode> for NcsReadError {
    fn from(value: UnknownNcsAuxCode) -> Self {
        Self::AuxCode(value)
    }
}

impl TryFrom<u8> for NcsOpcode {
    type Error = UnknownNcsOpcode;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        let opcode = match value {
            0x01 => Self::Assignment,
            0x02 => Self::RunstackAdd,
            0x03 => Self::RunstackCopy,
            0x04 => Self::Constant,
            0x05 => Self::ExecuteCommand,
            0x06 => Self::LogicalAnd,
            0x07 => Self::LogicalOr,
            0x08 => Self::InclusiveOr,
            0x09 => Self::ExclusiveOr,
            0x0a => Self::BooleanAnd,
            0x0b => Self::Equal,
            0x0c => Self::NotEqual,
            0x0d => Self::Geq,
            0x0e => Self::Gt,
            0x0f => Self::Lt,
            0x10 => Self::Leq,
            0x11 => Self::ShiftLeft,
            0x12 => Self::ShiftRight,
            0x13 => Self::UShiftRight,
            0x14 => Self::Add,
            0x15 => Self::Sub,
            0x16 => Self::Mul,
            0x17 => Self::Div,
            0x18 => Self::Modulus,
            0x19 => Self::Negation,
            0x1a => Self::OnesComplement,
            0x1b => Self::ModifyStackPointer,
            0x1c => Self::StoreIp,
            0x1d => Self::Jmp,
            0x1e => Self::Jsr,
            0x1f => Self::Jz,
            0x20 => Self::Ret,
            0x21 => Self::DeStruct,
            0x22 => Self::BooleanNot,
            0x23 => Self::Decrement,
            0x24 => Self::Increment,
            0x25 => Self::Jnz,
            0x26 => Self::AssignmentBase,
            0x27 => Self::RunstackCopyBase,
            0x28 => Self::DecrementBase,
            0x29 => Self::IncrementBase,
            0x2a => Self::SaveBasePointer,
            0x2b => Self::RestoreBasePointer,
            0x2c => Self::StoreState,
            0x2d => Self::NoOperation,
            _ => return Err(UnknownNcsOpcode(value)),
        };
        Ok(opcode)
    }
}

impl TryFrom<u8> for NcsAuxCode {
    type Error = UnknownNcsAuxCode;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        let aux = match value {
            0x00 => Self::None,
            0x01 => Self::TypeVoid,
            0x02 => Self::TypeCommand,
            0x03 => Self::TypeInteger,
            0x04 => Self::TypeFloat,
            0x05 => Self::TypeString,
            0x06 => Self::TypeObject,
            0x10 => Self::TypeEngst0,
            0x11 => Self::TypeEngst1,
            0x12 => Self::TypeEngst2,
            0x13 => Self::TypeEngst3,
            0x14 => Self::TypeEngst4,
            0x15 => Self::TypeEngst5,
            0x16 => Self::TypeEngst6,
            0x17 => Self::TypeEngst7,
            0x18 => Self::TypeEngst8,
            0x19 => Self::TypeEngst9,
            0x20 => Self::TypeTypeIntegerInteger,
            0x21 => Self::TypeTypeFloatFloat,
            0x22 => Self::TypeTypeObjectObject,
            0x23 => Self::TypeTypeStringString,
            0x24 => Self::TypeTypeStructStruct,
            0x25 => Self::TypeTypeIntegerFloat,
            0x26 => Self::TypeTypeFloatInteger,
            0x30 => Self::TypeTypeEngst0Engst0,
            0x31 => Self::TypeTypeEngst1Engst1,
            0x32 => Self::TypeTypeEngst2Engst2,
            0x33 => Self::TypeTypeEngst3Engst3,
            0x34 => Self::TypeTypeEngst4Engst4,
            0x35 => Self::TypeTypeEngst5Engst5,
            0x36 => Self::TypeTypeEngst6Engst6,
            0x37 => Self::TypeTypeEngst7Engst7,
            0x38 => Self::TypeTypeEngst8Engst8,
            0x39 => Self::TypeTypeEngst9Engst9,
            0x3a => Self::TypeTypeVectorVector,
            0x3b => Self::TypeTypeVectorFloat,
            0x3c => Self::TypeTypeFloatVector,
            0x70 => Self::EvalInplace,
            0x71 => Self::EvalPostplace,
            _ => return Err(UnknownNcsAuxCode(value)),
        };
        Ok(aux)
    }
}

fn instruction_extra_size(opcode: NcsOpcode, auxcode: NcsAuxCode, bytes: &[u8]) -> usize {
    match opcode {
        NcsOpcode::Constant => match auxcode {
            NcsAuxCode::TypeInteger
            | NcsAuxCode::TypeFloat
            | NcsAuxCode::TypeObject
            | NcsAuxCode::TypeEngst2 => 4,
            NcsAuxCode::TypeString | NcsAuxCode::TypeEngst7 => {
                match bytes
                    .get(..2)
                    .and_then(|prefix| <[u8; 2]>::try_from(prefix).ok())
                {
                    Some(prefix) => 2 + usize::from(u16::from_be_bytes(prefix)),
                    None => 2,
                }
            }
            _ => 0,
        },
        NcsOpcode::Jmp
        | NcsOpcode::Jsr
        | NcsOpcode::Jz
        | NcsOpcode::Jnz
        | NcsOpcode::ModifyStackPointer
        | NcsOpcode::Decrement
        | NcsOpcode::Increment
        | NcsOpcode::DecrementBase
        | NcsOpcode::IncrementBase => 4,
        NcsOpcode::StoreState => 8,
        NcsOpcode::ExecuteCommand => 3,
        NcsOpcode::RunstackCopy
        | NcsOpcode::RunstackCopyBase
        | NcsOpcode::Assignment
        | NcsOpcode::AssignmentBase
        | NcsOpcode::DeStruct => 6,
        NcsOpcode::Equal | NcsOpcode::NotEqual if auxcode == NcsAuxCode::TypeTypeStructStruct => 2,
        _ => 0,
    }
}

/// Decodes a full `NCS V1.0` bytecode stream into individual instructions.
///
/// # Errors
///
/// Returns [`NcsReadError`] if the header is invalid or an instruction is
/// malformed.
pub fn decode_ncs_instructions(bytes: &[u8]) -> Result<Vec<NcsInstruction>, NcsReadError> {
    let header = decode_ncs_header(bytes)?;
    let mut offset = NCS_BINARY_HEADER_SIZE;
    let code_end = NCS_BINARY_HEADER_SIZE + header.code_size as usize;
    if bytes.len() < code_end {
        return Err(NcsReadError::TruncatedInstruction {
            offset,
            expected_extra: header.code_size as usize,
            actual_extra: bytes.len().saturating_sub(offset),
        });
    }

    let mut instructions = Vec::new();
    while offset < code_end {
        let opcode = NcsOpcode::try_from(*bytes.get(offset).ok_or(
            NcsReadError::TruncatedInstruction {
                offset,
                expected_extra: 1,
                actual_extra: bytes.len().saturating_sub(offset),
            },
        )?)?;
        let auxcode = NcsAuxCode::try_from(*bytes.get(offset + 1).ok_or(
            NcsReadError::TruncatedInstruction {
                offset,
                expected_extra: 2,
                actual_extra: bytes.len().saturating_sub(offset),
            },
        )?)?;
        let extra_window = bytes.get(offset + 2..code_end).unwrap_or(&[]);
        let extra_size = instruction_extra_size(opcode, auxcode, extra_window);
        let remaining = code_end.saturating_sub(offset + 2);
        if remaining < extra_size {
            return Err(NcsReadError::TruncatedInstruction {
                offset,
                expected_extra: extra_size,
                actual_extra: remaining,
            });
        }
        let extra = bytes
            .get(offset + 2..offset + 2 + extra_size)
            .unwrap_or(&[])
            .to_vec();
        instructions.push(NcsInstruction {
            opcode,
            auxcode,
            extra,
        });
        offset += NCS_OPERATION_BASE_SIZE + extra_size;
    }

    Ok(instructions)
}

/// Encodes one `NCS V1.0` instruction stream with the fixed binary header.
pub fn encode_ncs_instructions(instructions: &[NcsInstruction]) -> Vec<u8> {
    let code_size = u32::try_from(
        instructions
            .iter()
            .map(NcsInstruction::encoded_len)
            .sum::<usize>(),
    )
    .ok()
    .unwrap_or(u32::MAX);
    let mut bytes = Vec::with_capacity(
        NCS_BINARY_HEADER_SIZE + usize::try_from(code_size).ok().unwrap_or(usize::MAX),
    );
    bytes.extend_from_slice(NCS_HEADER.as_bytes());
    bytes.push(b'B');
    bytes.extend_from_slice(&code_size.to_be_bytes());
    for instruction in instructions {
        bytes.push(instruction.opcode as u8);
        bytes.push(instruction.auxcode as u8);
        bytes.extend_from_slice(&instruction.extra);
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_header_accepts_valid_prefix() -> Result<(), Box<dyn std::error::Error>> {
        let bytes = [
            b'N', b'C', b'S', b' ', b'V', b'1', b'.', b'0', b'B', 0x00, 0x00, 0x00, 0x2a,
        ];
        let header = decode_ncs_header(&bytes)?;
        assert_eq!(header.code_size, 42);
        Ok(())
    }

    #[test]
    fn decode_header_rejects_bad_marker() {
        let bytes = [
            b'N', b'C', b'S', b' ', b'V', b'1', b'.', b'0', b'X', 0x00, 0x00, 0x00, 0x2a,
        ];
        assert_eq!(
            decode_ncs_header(&bytes),
            Err(NcsHeaderError::InvalidMarker(b'X'))
        );
    }

    #[test]
    fn encode_and_decode_roundtrip_instruction_stream() -> Result<(), Box<dyn std::error::Error>> {
        let instructions = vec![
            NcsInstruction {
                opcode:  NcsOpcode::Constant,
                auxcode: NcsAuxCode::TypeInteger,
                extra:   42_i32.to_be_bytes().to_vec(),
            },
            NcsInstruction {
                opcode:  NcsOpcode::Ret,
                auxcode: NcsAuxCode::None,
                extra:   Vec::new(),
            },
        ];

        let bytes = encode_ncs_instructions(&instructions);
        let decoded = decode_ncs_instructions(&bytes)?;
        assert_eq!(decoded, instructions);
        Ok(())
    }
}
