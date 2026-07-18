use std::{
    cmp::Ordering,
    error::Error,
    fmt,
    hash::{Hash, Hasher},
};

use nwnrs_types::io::prelude::*;
use serde::{Deserialize, Serialize};

use crate::resman::{ResType, is_valid_resref_part1, lookup_res_ext, lookup_res_type};

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
///
/// Equality, ordering, and hashing treat the name portion case-insensitively
/// while preserving its authored spelling for formatting and display.
///
/// # Examples
///
/// ```
/// let rr = nwnrs_types::resman::ResRef::new("appearance", nwnrs_types::resman::ResType(2017))?;
/// assert_eq!(rr.to_string(), "appearance.2da");
/// assert_eq!(rr.res_ref(), "appearance");
/// # Ok::<(), nwnrs_types::resman::ResRefError>(())
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResRef {
    res_ref:  String,
    res_type: ResType,
}

/// A resource reference that has been resolved to a concrete file extension.
///
/// This type keeps the typed [`ResRef`] together with the conventional
/// extension implied by [`ResType`]. It does not attempt to prove that a
/// corresponding file exists in storage.
///
/// # Examples
///
/// ```
/// let rr = nwnrs_types::resman::ResolvedResRef::from_filename("appearance.2da")?;
/// assert_eq!(rr.res_ext(), "2da");
/// # Ok::<(), nwnrs_types::resman::ResRefError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedResRef {
    base:    ResRef,
    res_ext: String,
}

impl ResRef {
    /// Creates a new resource reference.
    ///
    /// # Errors
    ///
    /// Returns [`ResRefError`] if `res_ref` is not a valid resource reference
    /// name.
    ///
    /// # Examples
    ///
    /// ```
    /// let rr = nwnrs_types::resman::ResRef::new("nwscript", nwnrs_types::resman::ResType(2009))?;
    /// assert_eq!(rr.res_type(), nwnrs_types::resman::ResType(2009));
    /// # Ok::<(), nwnrs_types::resman::ResRefError>(())
    /// ```
    pub fn new(res_ref: impl Into<String>, res_type: ResType) -> Result<Self, ResRefError> {
        let res_ref = res_ref.into();
        nwnrs_types::io::expect(
            is_valid_resref_part1(&res_ref),
            format!("'{res_ref}.{res_type}' is not a valid resref"),
        )?;

        Ok(Self {
            res_ref,
            res_type,
        })
    }

    /// Resolves this resource reference to a known file extension.
    ///
    /// # Examples
    ///
    /// ```
    /// let rr = nwnrs_types::resman::ResRef::new("appearance", nwnrs_types::resman::ResType(2017))?;
    /// let resolved = rr.resolve().unwrap();
    /// assert_eq!(resolved.to_file(), "appearance.2da");
    /// # Ok::<(), nwnrs_types::resman::ResRefError>(())
    /// ```
    #[must_use]
    pub fn resolve(&self) -> Option<ResolvedResRef> {
        let res_ext = lookup_res_ext(self.res_type)?;
        Some(ResolvedResRef {
            base: self.clone(),
            res_ext,
        })
    }

    /// Returns the resource name portion.
    #[must_use]
    pub fn res_ref(&self) -> &str {
        &self.res_ref
    }

    /// Returns the numeric resource type.
    #[must_use]
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
    ///
    /// # Errors
    ///
    /// Returns [`ResRefError`] if `res_ref` is invalid or `res_type` cannot be
    /// resolved to a file extension.
    ///
    /// # Examples
    ///
    /// ```
    /// let rr = nwnrs_types::resman::ResolvedResRef::new("dialog", nwnrs_types::resman::ResType(2029))?;
    /// assert_eq!(rr.to_string(), "dialog.dlg");
    /// # Ok::<(), nwnrs_types::resman::ResRefError>(())
    /// ```
    pub fn new(res_ref: impl Into<String>, res_type: ResType) -> Result<Self, ResRefError> {
        let res_ref = res_ref.into();
        let resolved = ResRef::new(res_ref.clone(), res_type)?
            .resolve()
            .ok_or_else(|| {
                ResRefError::Message(format!("'{res_ref}.{res_type}' is not a resolvable resref"))
            })?;

        Ok(resolved)
    }

    /// Attempts to resolve a `name.ext` filename into a resource reference.
    ///
    /// # Examples
    ///
    /// ```
    /// let rr = nwnrs_types::resman::ResolvedResRef::try_from_filename("dialog.dlg").unwrap();
    /// assert_eq!(rr.res_type(), nwnrs_types::resman::ResType(2029));
    /// ```
    #[must_use]
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
    ///
    /// # Errors
    ///
    /// Returns [`ResRefError`] if `filename` cannot be parsed as a resolvable
    /// resource reference.
    ///
    /// # Examples
    ///
    /// ```
    /// let rr = nwnrs_types::resman::ResolvedResRef::from_filename("spell.ssf")?;
    /// assert_eq!(rr.res_ext(), "ssf");
    /// # Ok::<(), nwnrs_types::resman::ResRefError>(())
    /// ```
    pub fn from_filename(filename: &str) -> Result<Self, ResRefError> {
        Self::try_from_filename(filename)
            .ok_or_else(|| ResRefError::Message(format!("'{filename}' is not a resolvable resref")))
    }

    /// Resolves a filename while preserving the resource-name casing.
    ///
    /// This is intended for package metadata that must reproduce the original
    /// archive spelling. Normal resource lookup should use
    /// [`Self::from_filename`].
    ///
    /// # Errors
    ///
    /// Returns [`ResRefError`] if `filename` cannot be parsed as a resolvable
    /// resource reference.
    pub fn from_filename_preserving_case(filename: &str) -> Result<Self, ResRefError> {
        let (base, extension) = filename.rsplit_once('.').ok_or_else(|| {
            ResRefError::Message(format!("'{filename}' is not a resolvable resref"))
        })?;
        let res_type = lookup_res_type(&extension.to_ascii_lowercase()).ok_or_else(|| {
            ResRefError::Message(format!("'{filename}' is not a resolvable resref"))
        })?;
        Self::new(base.to_string(), res_type)
    }

    /// Returns the unresolved base resource reference.
    #[must_use]
    pub fn base(&self) -> &ResRef {
        &self.base
    }

    /// Returns the resource name portion.
    #[must_use]
    pub fn res_ref(&self) -> &str {
        self.base.res_ref()
    }

    /// Returns the numeric resource type.
    #[must_use]
    pub fn res_type(&self) -> ResType {
        self.base.res_type()
    }

    /// Returns the resolved file extension.
    #[must_use]
    pub fn res_ext(&self) -> &str {
        &self.res_ext
    }

    /// Formats the resolved reference as `name.ext`.
    ///
    /// # Examples
    ///
    /// ```
    /// let rr = nwnrs_types::resman::ResolvedResRef::from_filename("portraits.txi")?;
    /// assert_eq!(rr.to_file(), "portraits.txi");
    /// # Ok::<(), nwnrs_types::resman::ResRefError>(())
    /// ```
    #[must_use]
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

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use crate::resman::{ResRef, ResType, ResolvedResRef};

    #[test]
    fn resrefs_compare_and_hash_case_insensitively() {
        let lower = ResRef::new("alpha", ResType(2017)).unwrap_or_else(|error| {
            panic!("lower rr: {error}");
        });
        let upper = ResRef::new("ALPHA", ResType(2017)).unwrap_or_else(|error| {
            panic!("upper rr: {error}");
        });
        assert_eq!(lower, upper);

        let mut set = HashSet::new();
        set.insert(lower);
        assert!(set.contains(&upper));
    }

    #[test]
    fn filenames_resolve_to_canonical_lowercase_names() {
        let resolved = ResolvedResRef::from_filename("HELLO.2DA").unwrap_or_else(|error| {
            panic!("resolve filename: {error}");
        });
        assert_eq!(resolved.res_ref(), "hello");
        assert_eq!(resolved.to_file(), "hello.2da");
        assert!(ResolvedResRef::try_from_filename("invalid.nope").is_none());
    }

    #[test]
    fn metadata_filename_resolution_preserves_resource_case() {
        let resolved = ResolvedResRef::from_filename_preserving_case("Repute.FAC")
            .unwrap_or_else(|error| panic!("resolve filename: {error}"));
        assert_eq!(resolved.res_ref(), "Repute");
        assert_eq!(resolved.to_file(), "Repute.fac");
    }
}
