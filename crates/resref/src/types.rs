use crate::is_valid_resref_part1;
use nwn_restype::{ResType, lookup_res_ext, lookup_res_type};
use nwn_util::ExpectationError;
use std::cmp::Ordering;
use std::error::Error;
use std::fmt;
use std::hash::{Hash, Hasher};

/// The maximum number of bytes in the name portion of a resource reference.
pub const RESREF_MAX_LENGTH: usize = 16;

/// Errors returned while constructing or resolving resource references.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResRefError {
    /// A format invariant was violated.
    Expectation(ExpectationError),
    /// The resource reference could not be interpreted.
    Message(String),
}

impl fmt::Display for ResRefError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Expectation(error) => error.fmt(f),
            Self::Message(message) => f.write_str(message),
        }
    }
}

impl Error for ResRefError {}

impl From<ExpectationError> for ResRefError {
    fn from(value: ExpectationError) -> Self {
        Self::Expectation(value)
    }
}

/// An NWN resource reference consisting of a name and resource type.
#[derive(Debug, Clone)]
pub struct ResRef {
    res_ref: String,
    res_type: ResType,
}

/// A resource reference that has been resolved to a concrete file extension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedResRef {
    base: ResRef,
    res_ext: String,
}

impl ResRef {
    /// Creates a new resource reference.
    pub fn new(res_ref: impl Into<String>, res_type: ResType) -> Result<Self, ResRefError> {
        let res_ref = res_ref.into();
        nwn_util::expect(
            is_valid_resref_part1(&res_ref),
            format!("'{}.{}' is not a valid resref", res_ref, res_type),
        )?;

        Ok(Self { res_ref, res_type })
    }

    /// Resolves this resource reference to a known file extension.
    pub fn resolve(&self) -> Option<ResolvedResRef> {
        let res_ext = lookup_res_ext(self.res_type)?;
        Some(ResolvedResRef {
            base: self.clone(),
            res_ext,
        })
    }

    /// Returns the resource name portion.
    pub fn res_ref(&self) -> &str {
        &self.res_ref
    }

    /// Returns the numeric resource type.
    pub fn res_type(&self) -> ResType {
        self.res_type
    }
}

impl PartialEq for ResRef {
    fn eq(&self, other: &Self) -> bool {
        self.res_type == other.res_type && self.res_ref.eq_ignore_ascii_case(&other.res_ref)
    }
}

impl Eq for ResRef {}

impl Hash for ResRef {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.res_ref.to_ascii_uppercase().hash(state);
        self.res_type.hash(state);
    }
}

impl Ord for ResRef {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.res_type.cmp(&other.res_type) {
            Ordering::Equal => self
                .res_ref
                .to_ascii_lowercase()
                .cmp(&other.res_ref.to_ascii_lowercase()),
            ordering => ordering,
        }
    }
}

impl PartialOrd for ResRef {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for ResRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.res_ref, self.res_type)
    }
}

impl ResolvedResRef {
    /// Creates a new resolved resource reference.
    pub fn new(res_ref: impl Into<String>, res_type: ResType) -> Result<Self, ResRefError> {
        let res_ref = res_ref.into();
        let resolved = ResRef::new(res_ref.clone(), res_type)?
            .resolve()
            .ok_or_else(|| {
                ResRefError::Message(format!(
                    "'{}.{}' is not a resolvable resref",
                    res_ref, res_type
                ))
            })?;

        Ok(resolved)
    }

    /// Attempts to resolve a `name.ext` filename into a resource reference.
    pub fn try_from_filename(filename: &str) -> Option<Self> {
        let normalized = filename.to_ascii_lowercase();
        let (base, ext) = normalized.rsplit_once('.')?;
        if !is_valid_resref_part1(base) {
            return None;
        }

        let res_type = lookup_res_type(ext)?;
        ResRef::new(base.to_string(), res_type).ok()?.resolve()
    }

    /// Resolves a `name.ext` filename into a resource reference.
    pub fn from_filename(filename: &str) -> Result<Self, ResRefError> {
        Self::try_from_filename(filename).ok_or_else(|| {
            ResRefError::Message(format!("'{}' is not a resolvable resref", filename))
        })
    }

    /// Returns the unresolved base resource reference.
    pub fn base(&self) -> &ResRef {
        &self.base
    }

    /// Returns the resource name portion.
    pub fn res_ref(&self) -> &str {
        self.base.res_ref()
    }

    /// Returns the numeric resource type.
    pub fn res_type(&self) -> ResType {
        self.base.res_type()
    }

    /// Returns the resolved file extension.
    pub fn res_ext(&self) -> &str {
        &self.res_ext
    }

    /// Formats the resolved reference as `name.ext`.
    pub fn to_file(&self) -> String {
        format!("{}.{}", self.base.res_ref, self.res_ext)
    }
}

impl fmt::Display for ResolvedResRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_file())
    }
}

impl From<ResolvedResRef> for ResRef {
    fn from(value: ResolvedResRef) -> Self {
        value.base
    }
}

impl From<&ResolvedResRef> for ResRef {
    fn from(value: &ResolvedResRef) -> Self {
        value.base.clone()
    }
}
