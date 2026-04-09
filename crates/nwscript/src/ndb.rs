use std::{
    error::Error,
    fmt,
    io::{self, BufRead, Write},
};

use nwnrs_util::prelude::*;
use serde::{Deserialize, Serialize};

/// A type abbreviation used in an `NDB V1.0` file.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NdbType {
    /// `f`
    Float,
    /// `i`
    Int,
    /// `v`
    Void,
    /// `o`
    Object,
    /// `s`
    String,
    /// `e0`..`e9`
    EngineStructure(u8),
    /// `t0000`-style user-structure indices.
    Struct(usize),
    /// `?`
    Unknown,
    /// Any unrecognized type tag preserved verbatim.
    Raw(String),
}

impl NdbType {
    fn parse(input: &str) -> Self {
        match input {
            "f" => Self::Float,
            "i" => Self::Int,
            "v" => Self::Void,
            "o" => Self::Object,
            "s" => Self::String,
            "?" => Self::Unknown,
            _ if input.len() == 2 && input.starts_with('e') => input[1..]
                .parse::<u8>()
                .map(Self::EngineStructure)
                .unwrap_or_else(|_| Self::Raw(input.to_string())),
            _ if input.len() == 5 && input.starts_with('t') => input[1..]
                .parse::<usize>()
                .map(Self::Struct)
                .unwrap_or_else(|_| Self::Raw(input.to_string())),
            _ => Self::Raw(input.to_string()),
        }
    }
}

impl fmt::Display for NdbType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Float => f.write_str("f"),
            Self::Int => f.write_str("i"),
            Self::Void => f.write_str("v"),
            Self::Object => f.write_str("o"),
            Self::String => f.write_str("s"),
            Self::EngineStructure(index) => write!(f, "e{index}"),
            Self::Struct(index) => write!(f, "t{index:04}"),
            Self::Unknown => f.write_str("?"),
            Self::Raw(raw) => f.write_str(raw),
        }
    }
}

/// One file entry in an `NDB` file table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NdbFile {
    /// File name as written in the debug table.
    pub name:    String,
    /// Whether this file is the root script file (`N`) rather than an include
    /// (`n`).
    pub is_root: bool,
}

/// One struct field entry in an `NDB` file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NdbStructField {
    /// Field name.
    pub label: String,
    /// Field type.
    pub ty:    NdbType,
}

/// One struct entry in an `NDB` file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NdbStruct {
    /// Struct name.
    pub label:  String,
    /// Declared fields in write order.
    pub fields: Vec<NdbStructField>,
}

/// One function entry in an `NDB` file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NdbFunction {
    /// Function name.
    pub label:        String,
    /// Start byte offset in the emitted `NCS`.
    pub binary_start: u32,
    /// End byte offset in the emitted `NCS`.
    pub binary_end:   u32,
    /// Return type abbreviation.
    pub return_type:  NdbType,
    /// Parameter types in declaration order.
    pub args:         Vec<NdbType>,
}

/// One variable entry in an `NDB` file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NdbVariable {
    /// Variable name.
    pub label:        String,
    /// Variable type abbreviation.
    pub ty:           NdbType,
    /// Start byte offset in the emitted `NCS`.
    pub binary_start: u32,
    /// End byte offset in the emitted `NCS`.
    pub binary_end:   u32,
    /// Stack location as recorded by the compiler.
    pub stack_loc:    u32,
}

/// One line mapping entry in an `NDB` file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NdbLine {
    /// File-table index.
    pub file_num:     usize,
    /// One-based source line number.
    pub line_num:     usize,
    /// Start byte offset in the emitted `NCS`.
    pub binary_start: u32,
    /// End byte offset in the emitted `NCS`.
    pub binary_end:   u32,
}

/// Parsed contents of an `NDB V1.0` file.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Ndb {
    /// File table entries.
    pub files:     Vec<NdbFile>,
    /// Declared structs.
    pub structs:   Vec<NdbStruct>,
    /// Function debug entries.
    pub functions: Vec<NdbFunction>,
    /// Variable debug entries.
    pub variables: Vec<NdbVariable>,
    /// Source line mappings.
    pub lines:     Vec<NdbLine>,
}

/// Errors returned while parsing or writing `NDB V1.0`.
#[derive(Debug)]
pub enum NdbError {
    /// An underlying I/O operation failed.
    Io(io::Error),
    /// The file violated a structural expectation.
    Expectation(ExpectationError),
    /// A line could not be parsed.
    Parse(String),
}

impl NdbError {
    fn parse(message: impl Into<String>) -> Self {
        Self::Parse(message.into())
    }
}

impl fmt::Display for NdbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::Expectation(error) => error.fmt(f),
            Self::Parse(message) => f.write_str(message),
        }
    }
}

impl Error for NdbError {}

impl From<io::Error> for NdbError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<ExpectationError> for NdbError {
    fn from(value: ExpectationError) -> Self {
        Self::Expectation(value)
    }
}

/// Parses `NDB V1.0` from a buffered reader.
pub fn read_ndb<R: BufRead>(reader: &mut R) -> Result<Ndb, NdbError> {
    let mut header = String::new();
    reader.read_line(&mut header)?;
    expect(
        header.trim_end_matches(['\r', '\n']) == "NDB V1.0",
        "invalid NDB header",
    )?;

    let mut counts_line = String::new();
    reader.read_line(&mut counts_line)?;
    let counts = counts_line
        .split_whitespace()
        .map(|part| {
            part.parse::<usize>().map_err(|error| {
                NdbError::parse(format!("invalid NDB section count {part:?}: {error}"))
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    expect(
        counts.len() == 5,
        format!("expected 5 NDB section counts, found {}", counts.len()),
    )?;
    let expected_files = counts.first().copied().unwrap_or(0);
    let expected_structs = counts.get(1).copied().unwrap_or(0);
    let expected_functions = counts.get(2).copied().unwrap_or(0);
    let expected_variables = counts.get(3).copied().unwrap_or(0);
    let expected_lines = counts.get(4).copied().unwrap_or(0);

    let mut result = Ndb::default();
    let mut line = String::new();
    while reader.read_line(&mut line)? != 0 {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            line.clear();
            continue;
        }

        let parts = trimmed.split_whitespace().collect::<Vec<_>>();
        expect(!parts.is_empty(), "encountered empty NDB record")?;
        let part = |index| {
            parts.get(index).copied().ok_or_else(|| {
                NdbError::parse(format!("missing field {index} in NDB record: {trimmed}"))
            })
        };
        match part(0)? {
            tag if tag.starts_with('N') || tag.starts_with('n') => {
                expect(
                    parts.len() == 2,
                    format!("invalid NDB file record: {trimmed}"),
                )?;
                let index = parse_usize(&tag[1..], trimmed)?;
                expect(
                    index == result.files.len(),
                    format!("out-of-order NDB file record index {index} in {trimmed}"),
                )?;
                result.files.push(NdbFile {
                    name:    part(1)?.to_string(),
                    is_root: tag.starts_with('N'),
                });
            }
            "s" => {
                expect(
                    parts.len() == 3,
                    format!("invalid NDB struct record: {trimmed}"),
                )?;
                result.structs.push(NdbStruct {
                    label:  part(2)?.to_string(),
                    fields: Vec::with_capacity(parse_usize(part(1)?, trimmed)?),
                });
            }
            "sf" => {
                expect(
                    parts.len() == 3,
                    format!("invalid NDB struct field record: {trimmed}"),
                )?;
                let structure = result.structs.last_mut().ok_or_else(|| {
                    NdbError::parse(format!("struct field without struct header: {trimmed}"))
                })?;
                structure.fields.push(NdbStructField {
                    label: part(2)?.to_string(),
                    ty:    NdbType::parse(part(1)?),
                });
            }
            "f" => {
                expect(
                    parts.len() == 6,
                    format!("invalid NDB function record: {trimmed}"),
                )?;
                result.functions.push(NdbFunction {
                    label:        part(5)?.to_string(),
                    binary_start: parse_hex_u32(part(1)?, trimmed)?,
                    binary_end:   parse_hex_u32(part(2)?, trimmed)?,
                    return_type:  NdbType::parse(part(4)?),
                    args:         Vec::with_capacity(parse_usize(part(3)?, trimmed)?),
                });
            }
            "fp" => {
                expect(
                    parts.len() == 2,
                    format!("invalid NDB function parameter record: {trimmed}"),
                )?;
                let function = result.functions.last_mut().ok_or_else(|| {
                    NdbError::parse(format!(
                        "function parameter without function header: {trimmed}"
                    ))
                })?;
                function.args.push(NdbType::parse(part(1)?));
            }
            "v" => {
                expect(
                    parts.len() == 6,
                    format!("invalid NDB variable record: {trimmed}"),
                )?;
                result.variables.push(NdbVariable {
                    label:        part(5)?.to_string(),
                    ty:           NdbType::parse(part(4)?),
                    binary_start: parse_hex_u32(part(1)?, trimmed)?,
                    binary_end:   parse_hex_u32(part(2)?, trimmed)?,
                    stack_loc:    parse_hex_u32(part(3)?, trimmed)?,
                });
            }
            tag if tag.starts_with('l') => {
                expect(
                    parts.len() == 4,
                    format!("invalid NDB line record: {trimmed}"),
                )?;
                result.lines.push(NdbLine {
                    file_num:     parse_usize(&tag[1..], trimmed)?,
                    line_num:     parse_usize(part(1)?, trimmed)?,
                    binary_start: parse_hex_u32(part(2)?, trimmed)?,
                    binary_end:   parse_hex_u32(part(3)?, trimmed)?,
                });
            }
            _ => {
                return Err(NdbError::parse(format!(
                    "unrecognized NDB record: {trimmed}"
                )));
            }
        }

        line.clear();
    }

    expect(
        result.files.len() == expected_files,
        format!(
            "expected {} file entries, found {}",
            expected_files,
            result.files.len()
        ),
    )?;
    expect(
        result.structs.len() == expected_structs,
        format!(
            "expected {} struct entries, found {}",
            expected_structs,
            result.structs.len()
        ),
    )?;
    expect(
        result.functions.len() == expected_functions,
        format!(
            "expected {} function entries, found {}",
            expected_functions,
            result.functions.len()
        ),
    )?;
    expect(
        result.variables.len() == expected_variables,
        format!(
            "expected {} variable entries, found {}",
            expected_variables,
            result.variables.len()
        ),
    )?;
    expect(
        result.lines.len() == expected_lines,
        format!(
            "expected {} line entries, found {}",
            expected_lines,
            result.lines.len()
        ),
    )?;

    Ok(result)
}

/// Parses `NDB V1.0` from a string slice.
pub fn parse_ndb_str(input: &str) -> Result<Ndb, NdbError> {
    let mut reader = io::Cursor::new(input.as_bytes());
    read_ndb(&mut reader)
}

/// Writes an `NDB V1.0` file in upstream-compatible textual form.
pub fn write_ndb<W: Write>(writer: &mut W, ndb: &Ndb) -> Result<(), NdbError> {
    writeln!(writer, "NDB V1.0")?;
    writeln!(
        writer,
        "{:07} {:07} {:07} {:07} {:07}",
        ndb.files.len(),
        ndb.structs.len(),
        ndb.functions.len(),
        ndb.variables.len(),
        ndb.lines.len()
    )?;

    for (index, file) in ndb.files.iter().enumerate() {
        let prefix = if file.is_root { 'N' } else { 'n' };
        writeln!(writer, "{prefix}{index:02} {}", file.name)?;
    }

    for structure in &ndb.structs {
        writeln!(
            writer,
            "s {:02} {}",
            structure.fields.len(),
            structure.label
        )?;
        for field in &structure.fields {
            writeln!(writer, "sf {} {}", field.ty, field.label)?;
        }
    }

    for function in &ndb.functions {
        writeln!(
            writer,
            "f {:08x} {:08x} {:03} {} {}",
            function.binary_start,
            function.binary_end,
            function.args.len(),
            function.return_type,
            function.label
        )?;
        for arg in &function.args {
            writeln!(writer, "fp {arg}")?;
        }
    }

    for variable in &ndb.variables {
        writeln!(
            writer,
            "v {:08x} {:08x} {:08x} {} {}",
            variable.binary_start,
            variable.binary_end,
            variable.stack_loc,
            variable.ty,
            variable.label
        )?;
    }

    for line in &ndb.lines {
        writeln!(
            writer,
            "l{:02} {:07} {:08x} {:08x}",
            line.file_num, line.line_num, line.binary_start, line.binary_end
        )?;
    }

    Ok(())
}

fn parse_usize(input: &str, line: &str) -> Result<usize, NdbError> {
    input
        .parse::<usize>()
        .map_err(|error| NdbError::parse(format!("invalid integer {input:?} in {line:?}: {error}")))
}

fn parse_hex_u32(input: &str, line: &str) -> Result<u32, NdbError> {
    u32::from_str_radix(input, 16)
        .map_err(|error| NdbError::parse(format!("invalid hex {input:?} in {line:?}: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_write_roundtrip_minimal_ndb() -> Result<(), Box<dyn std::error::Error>> {
        let source = concat!(
            "NDB V1.0\n",
            "0000001 0000001 0000001 0000001 0000001\n",
            "N00 test\n",
            "s 01 vector\n",
            "sf f x\n",
            "f 00000010 00000020 002 e0 main\n",
            "fp i\n",
            "fp t0000\n",
            "v 00000010 00000020 00000004 e2 nValue\n",
            "l00 0000001 00000010 00000020\n",
        );

        let parsed = parse_ndb_str(source)?;
        assert_eq!(
            parsed.files.first().map(|file| file.name.as_str()),
            Some("test")
        );
        assert_eq!(parsed.files.first().map(|file| file.is_root), Some(true));
        assert_eq!(
            parsed
                .structs
                .first()
                .and_then(|structure| structure.fields.first())
                .map(|field| field.label.as_str()),
            Some("x")
        );
        assert_eq!(
            parsed
                .functions
                .first()
                .map(|function| function.return_type.clone()),
            Some(NdbType::EngineStructure(0))
        );
        assert_eq!(
            parsed
                .functions
                .first()
                .map(|function| function.args.clone()),
            Some(vec![NdbType::Int, NdbType::Struct(0)])
        );
        assert_eq!(
            parsed.variables.first().map(|variable| variable.ty.clone()),
            Some(NdbType::EngineStructure(2))
        );

        let mut output = Vec::new();
        write_ndb(&mut output, &parsed)?;
        let written = String::from_utf8(output)?;
        assert_eq!(written, source);
        Ok(())
    }
}
