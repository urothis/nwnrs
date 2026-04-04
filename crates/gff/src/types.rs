use std::{fmt, io};

use nwnrs_core::prelude::*;
use nwnrs_util::prelude::*;

type GffByte = u8;
type GffChar = i8;
type GffWord = u16;
type GffShort = i16;
type GffDword = u32;
type GffInt = i32;
type GffFloat = f32;
type GffDword64 = u64;
type GffInt64 = i64;
type GffDouble = f64;
type GffCExoString = String;
type GffResRef = String;
type GffVoid = Vec<u8>;
type GffList = Vec<GffStruct>;

pub(crate) const HEADER_SIZE: usize = 56;

/// A `CExoLocString` value.
#[derive(Debug, Clone, PartialEq)]
/// A localized string may either reference a TLK entry via
/// [`str_ref`](Self::str_ref) or carry inline language-specific overrides in
/// [`entries`](Self::entries).
pub struct GffCExoLocString {
    /// The fallback TLK string reference.
    pub str_ref: StrRef,
    /// The inline language-specific strings.
    pub entries: Vec<(i32, String)>,
}

impl Default for GffCExoLocString {
    fn default() -> Self {
        Self {
            str_ref: BAD_STRREF,
            entries: Vec::new(),
        }
    }
}

/// The primitive and compound value kinds supported by GFF.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
/// These correspond directly to the numeric field type ids stored in the binary
/// format.
pub enum GffFieldKind {
    /// An unsigned 8-bit integer.
    Byte = 0,
    /// A signed 8-bit integer.
    Char = 1,
    /// An unsigned 16-bit integer.
    Word = 2,
    /// A signed 16-bit integer.
    Short = 3,
    /// An unsigned 32-bit integer.
    Dword = 4,
    /// A signed 32-bit integer.
    Int = 5,
    /// An unsigned 64-bit integer.
    Dword64 = 6,
    /// A signed 64-bit integer.
    Int64 = 7,
    /// A 32-bit float.
    Float = 8,
    /// A 64-bit float.
    Double = 9,
    /// A counted string.
    CExoString = 10,
    /// A resource reference string.
    ResRef = 11,
    /// A localized string table.
    CExoLocString = 12,
    /// An opaque byte blob.
    Void = 13,
    /// A nested structure.
    Struct = 14,
    /// A list of nested structures.
    List = 15,
}

impl GffFieldKind {
    /// Returns `true` if this kind is stored out-of-line in the binary format.
    pub fn is_complex(self) -> bool {
        matches!(
            self,
            Self::Dword64
                | Self::Int64
                | Self::Double
                | Self::CExoString
                | Self::ResRef
                | Self::CExoLocString
                | Self::Void
                | Self::Struct
                | Self::List
        )
    }

    pub(crate) fn from_u32(value: u32) -> Option<Self> {
        Some(match value {
            0 => Self::Byte,
            1 => Self::Char,
            2 => Self::Word,
            3 => Self::Short,
            4 => Self::Dword,
            5 => Self::Int,
            6 => Self::Dword64,
            7 => Self::Int64,
            8 => Self::Float,
            9 => Self::Double,
            10 => Self::CExoString,
            11 => Self::ResRef,
            12 => Self::CExoLocString,
            13 => Self::Void,
            14 => Self::Struct,
            15 => Self::List,
            _ => return None,
        })
    }
}

/// A typed GFF field value.
#[derive(Debug, Clone, PartialEq)]
/// The enum variants mirror the canonical `GFF V3.2` field kinds.
pub enum GffValue {
    /// An unsigned 8-bit integer.
    Byte(GffByte),
    /// A signed 8-bit integer.
    Char(GffChar),
    /// An unsigned 16-bit integer.
    Word(GffWord),
    /// A signed 16-bit integer.
    Short(GffShort),
    /// An unsigned 32-bit integer.
    Dword(GffDword),
    /// A signed 32-bit integer.
    Int(GffInt),
    /// A 32-bit float.
    Float(GffFloat),
    /// An unsigned 64-bit integer.
    Dword64(GffDword64),
    /// A signed 64-bit integer.
    Int64(GffInt64),
    /// A 64-bit float.
    Double(GffDouble),
    /// A counted string.
    CExoString(GffCExoString),
    /// A resource reference string.
    ResRef(GffResRef),
    /// A localized string table.
    CExoLocString(GffCExoLocString),
    /// An opaque byte blob.
    Void(GffVoid),
    /// A nested structure.
    Struct(GffStruct),
    /// A list of nested structures.
    List(GffList),
}

impl GffValue {
    /// Returns the field kind for this value.
    pub fn kind(&self) -> GffFieldKind {
        match self {
            Self::Byte(_) => GffFieldKind::Byte,
            Self::Char(_) => GffFieldKind::Char,
            Self::Word(_) => GffFieldKind::Word,
            Self::Short(_) => GffFieldKind::Short,
            Self::Dword(_) => GffFieldKind::Dword,
            Self::Int(_) => GffFieldKind::Int,
            Self::Float(_) => GffFieldKind::Float,
            Self::Dword64(_) => GffFieldKind::Dword64,
            Self::Int64(_) => GffFieldKind::Int64,
            Self::Double(_) => GffFieldKind::Double,
            Self::CExoString(_) => GffFieldKind::CExoString,
            Self::ResRef(_) => GffFieldKind::ResRef,
            Self::CExoLocString(_) => GffFieldKind::CExoLocString,
            Self::Void(_) => GffFieldKind::Void,
            Self::Struct(_) => GffFieldKind::Struct,
            Self::List(_) => GffFieldKind::List,
        }
    }
}

/// A labeled GFF field.
#[derive(Debug, Clone, PartialEq)]
/// Labels are stored on the containing [`GffStruct`]; this type only wraps the
/// typed value.
pub struct GffField {
    value: GffValue,
}

impl GffField {
    /// Creates a field from a typed value.
    pub fn new(value: GffValue) -> Self {
        Self { value }
    }

    /// Returns the kind of the stored value.
    pub fn kind(&self) -> GffFieldKind {
        self.value.kind()
    }

    /// Returns the stored field value.
    pub fn value(&self) -> &GffValue {
        &self.value
    }
}

/// A GFF structure containing labeled fields.
#[derive(Debug, Clone, PartialEq)]
/// Fields preserve insertion order and labels are unique within a structure.
pub struct GffStruct {
    /// The structure id stored in the document.
    pub id: i32,
    fields: Vec<(String, GffField)>,
}

impl GffStruct {
    /// Creates an empty structure with the given id.
    pub fn new(id: i32) -> Self {
        Self {
            id,
            fields: Vec::new(),
        }
    }

    /// Returns the fields in their stored order.
    pub fn fields(&self) -> &[(String, GffField)] {
        self.fields.as_slice()
    }

    /// Inserts or replaces a labeled field.
    pub fn put_field(&mut self, label: impl Into<String>, field: GffField) -> GffResult<()> {
        let label = label.into();
        ensure_label(&label)?;

        if let Some((_, existing)) = self.fields.iter_mut().find(|(name, _)| *name == label) {
            *existing = field;
        } else {
            self.fields.push((label, field));
        }

        Ok(())
    }

    /// Inserts or replaces a labeled value.
    pub fn put_value(&mut self, label: impl Into<String>, value: GffValue) -> GffResult<()> {
        self.put_field(label, GffField::new(value))
    }

    /// Returns a field by label.
    pub fn get_field(&self, label: &str) -> Option<&GffField> {
        self.fields
            .iter()
            .find_map(|(name, field)| (name == label).then_some(field))
    }

    /// Removes a field by label.
    pub fn remove(&mut self, label: &str) -> Option<GffField> {
        let idx = self.fields.iter().position(|(name, _)| name == label)?;
        Some(self.fields.remove(idx).1)
    }
}

/// A complete GFF document.
#[derive(Debug, Clone, PartialEq)]
/// NWN conventionally stores the root structure with id `-1`.
pub struct GffRoot {
    /// The four-byte document type tag.
    pub file_type: String,
    /// The four-byte document version tag.
    pub file_version: String,
    /// The root structure.
    pub root: GffStruct,
}

impl GffRoot {
    /// Creates a new root document with version `V3.2`.
    pub fn new(file_type: impl Into<String>) -> Self {
        Self {
            file_type: file_type.into(),
            file_version: "V3.2".to_string(),
            root: GffStruct::new(-1),
        }
    }

    /// Returns the fields on the root structure.
    pub fn fields(&self) -> &[(String, GffField)] {
        self.root.fields()
    }

    /// Inserts or replaces a labeled value on the root structure.
    pub fn put_value(&mut self, label: impl Into<String>, value: GffValue) -> GffResult<()> {
        self.root.put_value(label, value)
    }
}

/// Errors returned by GFF readers and writers.
#[derive(Debug)]
pub enum GffError {
    /// An underlying IO error occurred.
    Io(io::Error),
    /// A format invariant was violated.
    Expectation(ExpectationError),
    /// The document could not be interpreted or encoded.
    Message(String),
}

impl GffError {
    pub(crate) fn msg(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }
}

impl fmt::Display for GffError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::Expectation(error) => error.fmt(f),
            Self::Message(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for GffError {}

impl From<io::Error> for GffError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<ExpectationError> for GffError {
    fn from(value: ExpectationError) -> Self {
        Self::Expectation(value)
    }
}

/// A result alias for GFF operations.
pub type GffResult<T> = Result<T, GffError>;

pub(crate) fn ensure_label(label: &str) -> GffResult<()> {
    nwnrs_util::expect(
        label.len() <= 16,
        format!("invalid GFF label length for {:?}", label),
    )?;
    Ok(())
}

/// Creates an empty GFF structure with the given structure id.
pub fn new_gff_struct(id: i32) -> GffStruct {
    GffStruct::new(id)
}

/// Creates a new root document with the given four-byte file type tag.
pub fn new_gff_root(file_type: &str) -> GffRoot {
    GffRoot::new(file_type)
}

/// Creates an empty localized string with [`BAD_STRREF`] and no inline entries.
pub fn new_c_exo_loc_string() -> GffCExoLocString {
    GffCExoLocString::default()
}
