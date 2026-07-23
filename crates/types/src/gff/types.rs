use std::{fmt, io};

use nwnrs_types::{io::prelude::*, localization::prelude::*};

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
///
/// # Examples
///
/// ```
/// let value = nwnrs_types::gff::GffCExoLocString::default();
/// assert_eq!(value.entries.len(), 0);
/// ```
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
    ///
    /// # Examples
    ///
    /// ```
    /// assert!(nwnrs_types::gff::GffFieldKind::CExoString.is_complex());
    /// assert!(!nwnrs_types::gff::GffFieldKind::Int.is_complex());
    /// ```
    #[must_use]
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
    ///
    /// # Examples
    ///
    /// ```
    /// let value = nwnrs_types::gff::GffValue::Int(42);
    /// assert_eq!(value.kind(), nwnrs_types::gff::GffFieldKind::Int);
    /// ```
    #[must_use]
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
    pub(crate) value:      GffValue,
    pub(crate) provenance: Option<GffFieldProvenance>,
}

impl GffField {
    /// Creates a field from a typed value.
    ///
    /// # Examples
    ///
    /// ```
    /// let field = nwnrs_types::gff::GffField::new(nwnrs_types::gff::GffValue::Byte(7));
    /// assert_eq!(field.kind(), nwnrs_types::gff::GffFieldKind::Byte);
    /// ```
    #[must_use]
    pub fn new(value: GffValue) -> Self {
        Self {
            value,
            provenance: None,
        }
    }

    pub(crate) fn with_provenance(value: GffValue, provenance: GffFieldProvenance) -> Self {
        Self {
            value,
            provenance: Some(provenance),
        }
    }

    /// Returns the kind of the stored value.
    #[must_use]
    pub fn kind(&self) -> GffFieldKind {
        self.value.kind()
    }

    /// Returns the stored field value.
    #[must_use]
    pub fn value(&self) -> &GffValue {
        &self.value
    }

    /// Returns mutable access to the stored value.
    ///
    /// Mutating a parsed field retains its on-disk provenance, allowing the
    /// writer to preserve every unaffected field while encoding the changed
    /// value canonically.
    pub fn value_mut(&mut self) -> &mut GffValue {
        &mut self.value
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GffFieldProvenance {
    pub(crate) label_bytes:        [u8; 16],
    pub(crate) original_value:     GffValue,
    pub(crate) raw_data_or_offset: i32,
    pub(crate) raw_field_data:     Option<Vec<u8>>,
}

/// A GFF structure containing labeled fields.
///
/// A structure is the fundamental record unit in `GFF V3.2`. Field labels are
/// unique within one structure, and field order is preserved explicitly because
/// authored order matters for stable rewriting and inspection.
///
/// # Examples
///
/// ```
/// let mut record = nwnrs_types::gff::GffStruct::new(42);
/// record.put_value("Tag", nwnrs_types::gff::GffValue::CExoString("demo".to_string()))?;
/// let tag = record.get_field("Tag").unwrap();
/// assert!(matches!(
///     tag.value(),
///     nwnrs_types::gff::GffValue::CExoString(value) if value == "demo"
/// ));
/// assert_eq!(
///     record.remove("Tag").unwrap().kind(),
///     nwnrs_types::gff::GffFieldKind::CExoString,
/// );
/// assert!(record.get_field("Tag").is_none());
/// # Ok::<(), nwnrs_types::gff::GffError>(())
/// ```
#[derive(Debug, Clone, PartialEq)]
/// Fields preserve insertion order and labels are unique within a structure.
pub struct GffStruct {
    /// The structure id stored in the document.
    pub id:                i32,
    pub(crate) fields:     Vec<(String, GffField)>,
    pub(crate) provenance: Option<GffStructProvenance>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GffStructProvenance {
    pub(crate) field_labels: Vec<String>,
}

impl GffStruct {
    /// Creates an empty structure with the given id.
    ///
    /// # Examples
    ///
    /// ```
    /// let record = nwnrs_types::gff::GffStruct::new(7);
    /// assert_eq!(record.id, 7);
    /// assert!(record.fields().is_empty());
    /// ```
    #[must_use]
    pub fn new(id: i32) -> Self {
        Self {
            id,
            fields: Vec::new(),
            provenance: None,
        }
    }

    /// Returns the fields in their stored order.
    #[must_use]
    pub fn fields(&self) -> &[(String, GffField)] {
        self.fields.as_slice()
    }

    /// Inserts or replaces a labeled field.
    ///
    /// # Errors
    ///
    /// Returns [`GffError`] if `label` is not a valid GFF label.
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

    /// Inserts a new field at a specific stored-order position.
    ///
    /// # Errors
    ///
    /// Returns [`GffError`] when the label is invalid or already exists.
    pub fn insert_field(
        &mut self,
        index: usize,
        label: impl Into<String>,
        field: GffField,
    ) -> GffResult<()> {
        let label = label.into();
        ensure_label(&label)?;
        if self.fields.iter().any(|(name, _)| *name == label) {
            return Err(GffError::msg(format!(
                "GFF structure already contains field {label}"
            )));
        }
        self.fields
            .insert(index.min(self.fields.len()), (label, field));
        Ok(())
    }

    /// Renames a field without changing its stored position or value.
    ///
    /// # Errors
    ///
    /// Returns [`GffError`] when either label is invalid, the source field is
    /// absent, or the destination label already exists.
    pub fn rename_field(&mut self, old_label: &str, new_label: impl Into<String>) -> GffResult<()> {
        let new_label = new_label.into();
        ensure_label(&new_label)?;
        if old_label != new_label && self.fields.iter().any(|(name, _)| *name == new_label) {
            return Err(GffError::msg(format!(
                "GFF structure already contains field {new_label}"
            )));
        }
        let (label, _) = self
            .fields
            .iter_mut()
            .find(|(name, _)| name == old_label)
            .ok_or_else(|| GffError::msg(format!("GFF field not found: {old_label}")))?;
        *label = new_label;
        Ok(())
    }

    /// Inserts or replaces a labeled value.
    ///
    /// # Errors
    ///
    /// Returns [`GffError`] if `label` is not a valid GFF label.
    pub fn put_value(&mut self, label: impl Into<String>, value: GffValue) -> GffResult<()> {
        self.put_field(label, GffField::new(value))
    }

    /// Returns a field by label.
    #[must_use]
    pub fn get_field(&self, label: &str) -> Option<&GffField> {
        self.fields
            .iter()
            .find_map(|(name, field)| (name == label).then_some(field))
    }

    /// Returns mutable access to a field by label.
    pub fn get_field_mut(&mut self, label: &str) -> Option<&mut GffField> {
        self.fields
            .iter_mut()
            .find_map(|(name, field)| (name == label).then_some(field))
    }

    /// Removes a field by label.
    pub fn remove(&mut self, label: &str) -> Option<GffField> {
        let idx = self.fields.iter().position(|(name, _)| name == label)?;
        Some(self.fields.remove(idx).1)
    }
}

/// A complete GFF document.
///
/// `GffRoot` pairs the top-level document header with the root structure. NWN
/// conventionally uses root structure id `-1`, but the type keeps that policy
/// explicit rather than implicit.
///
/// # Examples
///
/// ```
/// let mut root = nwnrs_types::gff::GffRoot::new("UTC ");
/// root.put_value("Tag", nwnrs_types::gff::GffValue::CExoString("demo".to_string()))?;
/// assert_eq!(root.fields().len(), 1);
/// # Ok::<(), nwnrs_types::gff::GffError>(())
/// ```
#[derive(Debug, Clone)]
/// NWN conventionally stores the root structure with id `-1`.
pub struct GffRoot {
    /// The four-byte document type tag.
    pub file_type:              String,
    /// The four-byte document version tag.
    pub file_version:           String,
    /// The root structure.
    pub root:                   GffStruct,
    pub(crate) source_bytes:    Option<Vec<u8>>,
    pub(crate) source_snapshot: Option<GffRootSnapshot>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GffRootSnapshot {
    pub(crate) file_type:    String,
    pub(crate) file_version: String,
    pub(crate) root:         GffStruct,
}

impl GffRoot {
    /// Creates a new root document with version `V3.2`.
    ///
    /// The returned root structure starts with id `-1`, which is the NWN
    /// convention for top-level GFF documents.
    ///
    /// # Examples
    ///
    /// ```
    /// let root = nwnrs_types::gff::GffRoot::new("GIT ");
    /// assert_eq!(root.file_type, "GIT ");
    /// assert_eq!(root.file_version, "V3.2");
    /// assert_eq!(root.root.id, -1);
    /// ```
    pub fn new(file_type: impl Into<String>) -> Self {
        Self {
            file_type:       file_type.into(),
            file_version:    "V3.2".to_string(),
            root:            GffStruct::new(-1),
            source_bytes:    None,
            source_snapshot: None,
        }
    }

    /// Returns the fields on the root structure.
    #[must_use]
    pub fn fields(&self) -> &[(String, GffField)] {
        self.root.fields()
    }

    /// Inserts or replaces a labeled value on the root structure.
    ///
    /// # Errors
    ///
    /// Returns [`GffError`] if `label` is not a valid GFF label.
    pub fn put_value(&mut self, label: impl Into<String>, value: GffValue) -> GffResult<()> {
        self.root.put_value(label, value)
    }

    pub(crate) fn snapshot(&self) -> GffRootSnapshot {
        GffRootSnapshot {
            file_type:    self.file_type.clone(),
            file_version: self.file_version.clone(),
            root:         self.root.clone(),
        }
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
    nwnrs_types::io::expect(
        label.len() <= 16,
        format!("invalid GFF label length for {label:?}"),
    )?;
    Ok(())
}

impl PartialEq for GffRoot {
    fn eq(&self, other: &Self) -> bool {
        self.file_type == other.file_type
            && self.file_version == other.file_version
            && self.root == other.root
    }
}
