use std::{
    error::Error,
    fmt,
    io::{self, BufRead, Write},
};

use nwnrs_types::{
    encoding::{LegacyEncoding, decode_legacy_preserving, encode_legacy, encode_legacy_preserving},
    io::prelude::*,
};
use serde::{Deserialize, Serialize};

use crate::{CancellationToken, Cancelled};

/// Generous defensive bounds for parsing untrusted NDB debug data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct NdbParseLimits {
    /// Maximum encoded NDB bytes consumed.
    pub max_bytes:         usize,
    /// Maximum bytes in any one textual record.
    pub max_line_bytes:    usize,
    /// Maximum records across the five top-level sections.
    pub max_records:       usize,
    /// Maximum struct-field records across the file.
    pub max_struct_fields: usize,
    /// Maximum function-parameter records across the file.
    pub max_function_args: usize,
}

impl Default for NdbParseLimits {
    fn default() -> Self {
        Self {
            max_bytes:         512 * 1024 * 1024,
            max_line_bytes:    16 * 1024 * 1024,
            max_records:       2_000_000,
            max_struct_fields: 4_000_000,
            max_function_args: 4_000_000,
        }
    }
}

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
                .map_or_else(|_| Self::Raw(input.to_string()), Self::EngineStructure),
            _ if input.len() == 5 && input.starts_with('t') => input[1..]
                .parse::<usize>()
                .map_or_else(|_| Self::Raw(input.to_string()), Self::Struct),
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
    /// Start byte offset from the beginning of the emitted `NCS` file.
    pub binary_start: u32,
    /// End byte offset from the beginning of the emitted `NCS` file.
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
    /// Start byte offset from the beginning of the emitted `NCS` file.
    pub binary_start: u32,
    /// End byte offset from the beginning of the emitted `NCS` file.
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
    /// Start byte offset from the beginning of the emitted `NCS` file.
    pub binary_start: u32,
    /// End byte offset from the beginning of the emitted `NCS` file.
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
    /// The caller cancelled parsing.
    Cancelled(Cancelled),
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
            Self::Cancelled(error) => error.fmt(f),
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

impl From<Cancelled> for NdbError {
    fn from(value: Cancelled) -> Self {
        Self::Cancelled(value)
    }
}

/// Parses `NDB V1.0` from a buffered reader.
///
/// # Errors
///
/// Returns [`NdbError`] if the header is invalid or a record is malformed.
pub fn read_ndb<R: BufRead>(reader: &mut R) -> Result<Ndb, NdbError> {
    read_ndb_internal(reader, NdbParseLimits::default(), None)
}

/// Parses `NDB V1.0` with caller-selected defensive resource bounds.
///
/// # Errors
///
/// Returns [`NdbError`] if the header or records are malformed or a configured
/// resource bound is exceeded.
pub fn read_ndb_with_limits<R: BufRead>(
    reader: &mut R,
    limits: NdbParseLimits,
) -> Result<Ndb, NdbError> {
    read_ndb_internal(reader, limits, None)
}

/// Parses `NDB V1.0` with resource bounds and cooperative cancellation.
///
/// # Errors
///
/// Returns [`NdbError`] for malformed data, exceeded resource bounds, I/O
/// failures, or caller cancellation.
pub fn read_ndb_with_limits_and_cancellation<R: BufRead>(
    reader: &mut R,
    limits: NdbParseLimits,
    cancellation: &CancellationToken,
) -> Result<Ndb, NdbError> {
    read_ndb_internal(reader, limits, Some(cancellation))
}

fn read_ndb_internal<R: BufRead>(
    reader: &mut R,
    limits: NdbParseLimits,
    cancellation: Option<&CancellationToken>,
) -> Result<Ndb, NdbError> {
    validate_ndb_limits(limits)?;
    let mut bytes_read = 0_usize;
    let mut header = String::new();
    read_bounded_line(reader, &mut header, limits, &mut bytes_read, cancellation)?;
    expect(
        header.trim_end_matches(['\r', '\n']) == "NDB V1.0",
        "invalid NDB header",
    )?;

    let mut counts_line = String::new();
    read_bounded_line(
        reader,
        &mut counts_line,
        limits,
        &mut bytes_read,
        cancellation,
    )?;
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
    let expected_records = counts.iter().try_fold(0_usize, |total, count| {
        total
            .checked_add(*count)
            .ok_or_else(|| NdbError::parse("NDB top-level record count overflowed"))
    })?;
    expect(
        expected_records <= limits.max_records,
        format!(
            "NDB declares {expected_records} top-level records, exceeding configured maximum {}",
            limits.max_records
        ),
    )?;

    let mut result = Ndb::default();
    let mut declared_struct_fields = Vec::new();
    let mut declared_function_args = Vec::new();
    let mut total_struct_fields = 0_usize;
    let mut total_function_args = 0_usize;
    let mut line = String::new();
    while read_bounded_line(reader, &mut line, limits, &mut bytes_read, cancellation)? != 0 {
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
                let declared = parse_usize(part(1)?, trimmed)?;
                total_struct_fields = total_struct_fields
                    .checked_add(declared)
                    .ok_or_else(|| NdbError::parse("NDB declared struct-field count overflowed"))?;
                expect(
                    total_struct_fields <= limits.max_struct_fields,
                    format!(
                        "NDB declares {total_struct_fields} struct fields, exceeding configured \
                         maximum {}",
                        limits.max_struct_fields
                    ),
                )?;
                let mut fields = Vec::new();
                fields.try_reserve(declared).map_err(|error| {
                    NdbError::parse(format!(
                        "failed to reserve storage for {declared} NDB struct fields: {error}"
                    ))
                })?;
                declared_struct_fields.push(declared);
                result.structs.push(NdbStruct {
                    label: part(2)?.to_string(),
                    fields,
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
                let declared = declared_struct_fields.last().copied().unwrap_or(0);
                expect(
                    structure.fields.len() < declared,
                    format!("struct contains more than its declared {declared} fields: {trimmed}"),
                )?;
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
                let declared = parse_usize(part(3)?, trimmed)?;
                total_function_args =
                    total_function_args.checked_add(declared).ok_or_else(|| {
                        NdbError::parse("NDB declared function-parameter count overflowed")
                    })?;
                expect(
                    total_function_args <= limits.max_function_args,
                    format!(
                        "NDB declares {total_function_args} function parameters, exceeding \
                         configured maximum {}",
                        limits.max_function_args
                    ),
                )?;
                let mut args = Vec::new();
                args.try_reserve(declared).map_err(|error| {
                    NdbError::parse(format!(
                        "failed to reserve storage for {declared} NDB function parameters: {error}"
                    ))
                })?;
                declared_function_args.push(declared);
                result.functions.push(NdbFunction {
                    label: part(5)?.to_string(),
                    binary_start: parse_hex_u32(part(1)?, trimmed)?,
                    binary_end: parse_hex_u32(part(2)?, trimmed)?,
                    return_type: NdbType::parse(part(4)?),
                    args,
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
                let declared = declared_function_args.last().copied().unwrap_or(0);
                expect(
                    function.args.len() < declared,
                    format!(
                        "function contains more than its declared {declared} parameters: {trimmed}"
                    ),
                )?;
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
            "expected {} file records, found {}",
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
    for (index, (structure, declared)) in result
        .structs
        .iter()
        .zip(&declared_struct_fields)
        .enumerate()
    {
        expect(
            structure.fields.len() == *declared,
            format!(
                "struct {index} declares {declared} fields but contains {}",
                structure.fields.len()
            ),
        )?;
    }
    expect(
        result.functions.len() == expected_functions,
        format!(
            "expected {} function entries, found {}",
            expected_functions,
            result.functions.len()
        ),
    )?;
    for (index, (function, declared)) in result
        .functions
        .iter()
        .zip(&declared_function_args)
        .enumerate()
    {
        expect(
            function.args.len() == *declared,
            format!(
                "function {index} declares {declared} parameters but contains {}",
                function.args.len()
            ),
        )?;
    }
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

    validate_ndb_identifiers(&result)?;
    Ok(result)
}

fn validate_ndb_limits(limits: NdbParseLimits) -> Result<(), NdbError> {
    expect(limits.max_bytes > 0, "NDB max_bytes must be non-zero")?;
    expect(
        limits.max_line_bytes > 0,
        "NDB max_line_bytes must be non-zero",
    )?;
    expect(
        limits.max_line_bytes <= limits.max_bytes,
        "NDB max_line_bytes must not exceed max_bytes",
    )?;
    Ok(())
}

fn read_bounded_line<R: BufRead>(
    reader: &mut R,
    output: &mut String,
    limits: NdbParseLimits,
    total: &mut usize,
    cancellation: Option<&CancellationToken>,
) -> Result<usize, NdbError> {
    output.clear();
    let mut bytes = Vec::new();
    loop {
        if let Some(cancellation) = cancellation {
            cancellation.check()?;
        }
        let available = reader.fill_buf()?;
        if available.is_empty() {
            break;
        }
        let count = available
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(available.len(), |position| position + 1);
        let next_line_size = bytes
            .len()
            .checked_add(count)
            .ok_or_else(|| NdbError::parse("NDB line length overflowed"))?;
        let next_total = total
            .checked_add(count)
            .ok_or_else(|| NdbError::parse("NDB byte count overflowed"))?;
        expect(
            next_line_size <= limits.max_line_bytes,
            format!(
                "NDB line/record exceeds configured maximum of {} bytes",
                limits.max_line_bytes
            ),
        )?;
        expect(
            next_total <= limits.max_bytes,
            format!(
                "NDB input exceeds configured maximum of {} bytes",
                limits.max_bytes
            ),
        )?;
        let ended_with_newline = available.get(count.saturating_sub(1)) == Some(&b'\n');
        let chunk = available
            .get(..count)
            .ok_or_else(|| NdbError::parse("NDB buffered reader returned an invalid byte count"))?;
        bytes.extend_from_slice(chunk);
        reader.consume(count);
        *total = next_total;
        if ended_with_newline {
            break;
        }
    }
    if bytes.is_empty() {
        return Ok(0);
    }
    *output = decode_legacy_preserving(LegacyEncoding::Windows1252, &bytes);
    Ok(output.len())
}

/// Parses `NDB V1.0` from a string slice.
///
/// # Errors
///
/// Returns [`NdbError`] if the header is invalid or a record is malformed.
pub fn parse_ndb_str(input: &str) -> Result<Ndb, NdbError> {
    let bytes = encode_legacy(LegacyEncoding::Windows1252, input)
        .map_err(|error| NdbError::parse(format!("NDB text is not Windows-1252: {error}")))?;
    let mut reader = io::Cursor::new(bytes);
    read_ndb(&mut reader)
}

/// Writes an `NDB V1.0` file in upstream-compatible textual form.
///
/// # Errors
///
/// Returns [`NdbError`] if the write fails.
pub fn write_ndb<W: Write>(writer: &mut W, ndb: &Ndb) -> Result<(), NdbError> {
    write_ndb_internal(writer, ndb, None)
}

/// Writes NDB with cooperative cancellation.
pub fn write_ndb_with_cancellation<W: Write>(
    writer: &mut W,
    ndb: &Ndb,
    cancellation: &CancellationToken,
) -> Result<(), NdbError> {
    write_ndb_internal(writer, ndb, Some(cancellation))
}

fn write_ndb_internal<W: Write>(
    writer: &mut W,
    ndb: &Ndb,
    cancellation: Option<&CancellationToken>,
) -> Result<(), NdbError> {
    let mut rendered = Vec::new();
    write_ndb_records(&mut rendered, ndb, cancellation)?;
    let rendered = String::from_utf8(rendered).map_err(|error| {
        NdbError::parse(format!("internal NDB renderer emitted UTF-8: {error}"))
    })?;
    let encoded =
        encode_legacy_preserving(LegacyEncoding::Windows1252, &rendered).map_err(|error| {
            NdbError::parse(format!(
                "NDB text is not representable as Windows-1252: {error}"
            ))
        })?;
    writer.write_all(&encoded)?;
    Ok(())
}

fn write_ndb_records<W: Write>(
    writer: &mut W,
    ndb: &Ndb,
    cancellation: Option<&CancellationToken>,
) -> Result<(), NdbError> {
    check_ndb_cancellation(cancellation)?;
    validate_ndb_identifiers(ndb)?;
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
        check_ndb_cancellation(cancellation)?;
        let prefix = if file.is_root { 'N' } else { 'n' };
        writeln!(writer, "{prefix}{index:02} {}", file.name)?;
    }

    for structure in &ndb.structs {
        check_ndb_cancellation(cancellation)?;
        writeln!(
            writer,
            "s {:02} {}",
            structure.fields.len(),
            structure.label
        )?;
        for field in &structure.fields {
            check_ndb_cancellation(cancellation)?;
            writeln!(writer, "sf {} {}", field.ty, field.label)?;
        }
    }

    for function in &ndb.functions {
        check_ndb_cancellation(cancellation)?;
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
            check_ndb_cancellation(cancellation)?;
            writeln!(writer, "fp {arg}")?;
        }
    }

    for variable in &ndb.variables {
        check_ndb_cancellation(cancellation)?;
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
        check_ndb_cancellation(cancellation)?;
        writeln!(
            writer,
            "l{:02} {:07} {:08x} {:08x}",
            line.file_num, line.line_num, line.binary_start, line.binary_end
        )?;
    }

    Ok(())
}

fn validate_ndb_identifiers(ndb: &Ndb) -> Result<(), NdbError> {
    let file_names_valid = ndb.files.iter().all(|value| {
        !value.name.chars().any(char::is_whitespace)
            && encode_legacy_preserving(LegacyEncoding::Windows1252, &value.name).is_ok()
    });
    let identifiers_valid = ndb.structs.iter().all(|value| {
        value.label.is_ascii()
            && value
                .fields
                .iter()
                .all(|field| field.label.is_ascii() && field.ty.to_string().is_ascii())
    }) && ndb.functions.iter().all(|value| {
        value.label.is_ascii()
            && value.return_type.to_string().is_ascii()
            && value
                .args
                .iter()
                .all(|argument| argument.to_string().is_ascii())
    }) && ndb
        .variables
        .iter()
        .all(|value| value.label.is_ascii() && value.ty.to_string().is_ascii());
    expect(
        file_names_valid,
        "NDB source names must be single Windows-1252 tokens",
    )?;
    expect(identifiers_valid, "NDB identifiers must be ASCII")?;
    Ok(())
}

fn check_ndb_cancellation(cancellation: Option<&CancellationToken>) -> Result<(), NdbError> {
    cancellation.map_or(Ok(()), CancellationToken::check)?;
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

    #[test]
    fn source_names_round_trip_exact_windows_1252_and_opaque_bytes()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = b"NDB V1.0\n0000001 0000000 0000000 0000000 0000000\nN00 caf\xe9\x81.nss\n";
        let mut parsed = read_ndb(&mut io::Cursor::new(source))?;
        assert!(
            parsed
                .files
                .first()
                .is_some_and(|file| file.name.contains("café"))
        );

        let mut output = Vec::new();
        write_ndb(&mut output, &parsed)?;
        assert_eq!(output, source);

        parsed.files.first_mut().expect("NDB source file").name = "renamedé.nss".to_string();
        output.clear();
        write_ndb(&mut output, &parsed)?;
        assert!(output.windows(8).any(|window| window == b"renamed\xe9"));
        Ok(())
    }

    #[test]
    fn parser_rejects_declared_counts_that_do_not_match_records() {
        let source = concat!(
            "NDB V1.0\n",
            "0000002 0000000 0000000 0000000 0000000\n",
            "N00 only_one\n",
        );
        let error = parse_ndb_str(source).expect_err("mismatched counts must fail");
        assert!(error.to_string().contains("file records"));
    }

    #[test]
    fn bounded_parser_rejects_oversized_lines() {
        let source = "NDB V1.0\n0000000 0000000 0000000 0000000 0000000\n";
        let limits = NdbParseLimits {
            max_line_bytes: 8,
            ..NdbParseLimits::default()
        };
        let error = read_ndb_with_limits(&mut io::Cursor::new(source), limits)
            .expect_err("oversized count line must fail");
        assert!(error.to_string().contains("line"));
    }

    #[test]
    fn bounded_parser_observes_precancellation() {
        let source = "NDB V1.0\n0000000 0000000 0000000 0000000 0000000\n";
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        let error = read_ndb_with_limits_and_cancellation(
            &mut io::Cursor::new(source),
            NdbParseLimits::default(),
            &cancellation,
        )
        .expect_err("cancelled parse must fail");
        assert_eq!(error.to_string(), "operation cancelled");
    }
}
