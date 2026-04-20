use std::{collections::HashMap, error::Error, fmt};

use nwnrs_restype::prelude::*;
use serde::{Deserialize, Serialize};

use crate::CompilerErrorCode;

/// The built-in NWN resource type used for `NWScript` source files.
pub const NW_SCRIPT_SOURCE_RES_TYPE: ResType = ResType(2009);

/// The upstream default include-depth limit.
pub const DEFAULT_MAX_INCLUDE_DEPTH: usize = 16;

/// Errors returned while resolving or loading source files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceError {
    /// The source resolver itself failed.
    Resolver(String),
    /// The requested source file could not be loaded or violated a compiler
    /// rule.
    Compiler {
        /// Stable upstream-aligned compiler error code.
        code:        CompilerErrorCode,
        /// Logical script name associated with the failure.
        script_name: String,
        /// Human-readable error message.
        message:     String,
    },
}

impl SourceError {
    /// Returns the upstream compiler error code when this is a compiler
    /// failure.
    #[must_use]
    pub fn code(&self) -> Option<CompilerErrorCode> {
        match self {
            Self::Resolver(_) => None,
            Self::Compiler {
                code, ..
            } => Some(*code),
        }
    }

    /// Creates a source resolver error.
    pub fn resolver(message: impl Into<String>) -> Self {
        Self::Resolver(message.into())
    }

    /// Creates a file-not-found error.
    pub fn file_not_found(script_name: impl Into<String>) -> Self {
        let script_name = script_name.into();
        Self::Compiler {
            code: CompilerErrorCode::FileNotFound,
            message: format!("source file {script_name:?} was not found"),
            script_name,
        }
    }

    /// Creates an include-recursive error.
    pub fn include_recursive(script_name: impl Into<String>) -> Self {
        let script_name = script_name.into();
        Self::Compiler {
            code: CompilerErrorCode::IncludeRecursive,
            message: format!("recursive include detected for {script_name:?}"),
            script_name,
        }
    }

    /// Creates an include-too-many-levels error.
    pub fn include_too_many_levels(
        script_name: impl Into<String>,
        max_include_depth: usize,
    ) -> Self {
        let script_name = script_name.into();
        Self::Compiler {
            code: CompilerErrorCode::IncludeTooManyLevels,
            message: format!(
                "include depth exceeded the configured maximum of {max_include_depth} while \
                 loading {script_name:?}"
            ),
            script_name,
        }
    }
}

impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Resolver(message) => f.write_str(message),
            Self::Compiler {
                message,
                code,
                ..
            } => write!(f, "{message} ({})", code.code()),
        }
    }
}

impl Error for SourceError {}

/// Options controlling source loading and include traversal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SourceLoadOptions {
    /// Resource type requested from the source resolver.
    pub res_type:          ResType,
    /// Maximum recursive include depth.
    pub max_include_depth: usize,
}

impl Default for SourceLoadOptions {
    fn default() -> Self {
        Self {
            res_type:          NW_SCRIPT_SOURCE_RES_TYPE,
            max_include_depth: DEFAULT_MAX_INCLUDE_DEPTH,
        }
    }
}

/// Identifies one loaded `NWScript` source file within a compilation session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SourceId(u32);

impl SourceId {
    /// Creates a new source identifier from its stable numeric value.
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    /// Returns the stable numeric value for this source identifier.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl fmt::Display for SourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// A half-open byte span within one source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Span {
    /// The source file containing this span.
    pub source_id: SourceId,
    /// The inclusive starting byte offset.
    pub start:     usize,
    /// The exclusive ending byte offset.
    pub end:       usize,
}

impl Span {
    /// Creates a new span.
    #[must_use]
    pub const fn new(source_id: SourceId, start: usize, end: usize) -> Self {
        Self {
            source_id,
            start,
            end,
        }
    }

    /// Returns the byte length of this span.
    #[must_use]
    pub const fn len(self) -> usize {
        self.end.saturating_sub(self.start)
    }

    /// Returns `true` when this span is empty.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.start == self.end
    }
}

/// A one-based line and column location inside a source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SourceLocation {
    /// The absolute byte offset.
    pub offset: usize,
    /// The one-based source line.
    pub line:   usize,
    /// The one-based source column.
    pub column: usize,
}

/// One loaded source file plus its line-start table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceFile {
    /// Stable identifier used by spans and diagnostics.
    pub id:       SourceId,
    /// Logical filename used by diagnostics and include tracking.
    pub name:     String,
    /// Full source bytes.
    pub contents: Vec<u8>,
    line_starts:  Vec<usize>,
}

impl SourceFile {
    /// Creates a source file and precomputes line starts for span lookup.
    pub fn new(id: SourceId, name: impl Into<String>, contents: impl Into<Vec<u8>>) -> Self {
        let contents = contents.into();
        let line_starts = compute_line_starts(&contents);
        Self {
            id,
            name: name.into(),
            contents,
            line_starts,
        }
    }

    /// Returns the byte length of the source contents.
    #[must_use]
    pub fn len(&self) -> usize {
        self.contents.len()
    }

    /// Returns `true` when the source file is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.contents.is_empty()
    }

    /// Returns the raw source bytes.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.contents
    }

    /// Returns the source contents as UTF-8 when the file is valid UTF-8.
    #[must_use]
    pub fn text(&self) -> Option<&str> {
        std::str::from_utf8(&self.contents).ok()
    }

    /// Returns the raw bytes covered by `span` when it belongs to this file.
    #[must_use]
    pub fn span_bytes(&self, span: Span) -> Option<&[u8]> {
        if span.source_id != self.id || span.start > span.end || span.end > self.contents.len() {
            return None;
        }
        self.contents.get(span.start..span.end)
    }

    /// Returns the text covered by `span` when it belongs to this file.
    #[must_use]
    pub fn span_text(&self, span: Span) -> Option<&str> {
        let bytes = self.span_bytes(span)?;
        std::str::from_utf8(bytes).ok()
    }

    /// Resolves a byte offset to a one-based line and column.
    #[must_use]
    pub fn location(&self, offset: usize) -> Option<SourceLocation> {
        if offset > self.contents.len() {
            return None;
        }

        let line_index = match self.line_starts.binary_search(&offset) {
            Ok(index) => index,
            Err(index) => index.checked_sub(1)?,
        };
        let line_start = *self.line_starts.get(line_index)?;
        Some(SourceLocation {
            offset,
            line: line_index + 1,
            column: offset.saturating_sub(line_start) + 1,
        })
    }
}

/// A collection of loaded source files indexed by both id and normalized name.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceMap {
    files: Vec<SourceFile>,
    names: HashMap<String, SourceId>,
}

impl SourceMap {
    /// Creates an empty source map.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the next identifier that would be assigned to a new file.
    #[must_use]
    pub fn next_id(&self) -> SourceId {
        let id = u32::try_from(self.files.len()).ok().unwrap_or(u32::MAX);
        SourceId::new(id)
    }

    /// Inserts a fully constructed source file.
    pub fn insert_file(&mut self, file: SourceFile) -> SourceId {
        let normalized = normalize_script_name(&file.name);
        self.names.insert(normalized, file.id);
        let id = file.id;
        self.files.push(file);
        id
    }

    /// Creates and inserts a new source file.
    pub fn add_file(&mut self, name: impl Into<String>, contents: impl Into<Vec<u8>>) -> SourceId {
        let id = self.next_id();
        self.insert_file(SourceFile::new(id, name, contents))
    }

    /// Returns the file for `id`.
    #[must_use]
    pub fn get(&self, id: SourceId) -> Option<&SourceFile> {
        self.files.get(id.get() as usize)
    }

    /// Returns the file for `name`, using case-insensitive matching.
    #[must_use]
    pub fn get_by_name(&self, name: &str) -> Option<&SourceFile> {
        let id = self.names.get(&normalize_script_name(name))?;
        self.get(*id)
    }

    /// Returns `true` when a file with `name` has already been loaded.
    #[must_use]
    pub fn contains_name(&self, name: &str) -> bool {
        self.names.contains_key(&normalize_script_name(name))
    }

    /// Returns the number of loaded source files.
    #[must_use]
    pub fn len(&self) -> usize {
        self.files.len()
    }

    /// Returns `true` when there are no files.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Iterates over loaded source files in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = &SourceFile> {
        self.files.iter()
    }
}

/// Loads script source text by logical name and resource type.
pub trait ScriptResolver {
    /// Returns the source bytes for `script_name`, or `None` when it does not
    /// exist.
    ///
    /// # Errors
    ///
    /// Returns [`SourceError`] if the underlying resource lookup fails.
    fn resolve_script_bytes(
        &self,
        script_name: &str,
        res_type: ResType,
    ) -> Result<Option<Vec<u8>>, SourceError>;

    /// Returns the source contents for `script_name`, or `None` when it does
    /// not exist.
    ///
    /// # Errors
    ///
    /// Returns [`SourceError`] if the underlying resource lookup or UTF-8
    /// decoding fails.
    fn resolve_script(
        &self,
        script_name: &str,
        res_type: ResType,
    ) -> Result<Option<String>, SourceError> {
        match self.resolve_script_bytes(script_name, res_type)? {
            Some(bytes) => String::from_utf8(bytes).map(Some).map_err(|error| {
                SourceError::resolver(format!("source file is not valid UTF-8: {error}"))
            }),
            None => Ok(None),
        }
    }
}

/// An in-memory script resolver used for tests and fixture loading.
#[derive(Debug, Clone, Default)]
pub struct InMemoryScriptResolver {
    sources: HashMap<(ResType, String), Vec<u8>>,
}

impl InMemoryScriptResolver {
    /// Creates an empty in-memory resolver.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts one source file for an arbitrary resource type.
    pub fn insert(
        &mut self,
        script_name: impl Into<String>,
        res_type: ResType,
        contents: impl Into<Vec<u8>>,
    ) {
        self.sources.insert(
            (res_type, normalize_script_name(&script_name.into())),
            contents.into(),
        );
    }

    /// Inserts one standard `.nss` source file.
    pub fn insert_source(&mut self, script_name: impl Into<String>, contents: impl Into<Vec<u8>>) {
        self.insert(script_name, NW_SCRIPT_SOURCE_RES_TYPE, contents);
    }
}

impl ScriptResolver for InMemoryScriptResolver {
    fn resolve_script_bytes(
        &self,
        script_name: &str,
        res_type: ResType,
    ) -> Result<Option<Vec<u8>>, SourceError> {
        Ok(self
            .sources
            .get(&(res_type, normalize_script_name(script_name)))
            .cloned())
    }

    fn resolve_script(
        &self,
        script_name: &str,
        res_type: ResType,
    ) -> Result<Option<String>, SourceError> {
        match self.resolve_script_bytes(script_name, res_type)? {
            Some(bytes) => String::from_utf8(bytes).map(Some).map_err(|error| {
                SourceError::resolver(format!("source file is not valid UTF-8: {error}"))
            }),
            None => Ok(None),
        }
    }
}

pub(crate) fn normalize_script_name(input: &str) -> String {
    input.to_ascii_lowercase()
}

fn compute_line_starts(bytes: &[u8]) -> Vec<usize> {
    let mut starts = vec![0];
    let mut index = 0;
    while index < bytes.len() {
        match bytes.get(index) {
            Some(b'\n') => {
                starts.push(index + 1);
                index += 1;
            }
            Some(b'\r') => {
                if bytes.get(index + 1) == Some(&b'\n') {
                    starts.push(index + 2);
                    index += 2;
                } else {
                    starts.push(index + 1);
                    index += 1;
                }
            }
            _ => index += 1,
        }
    }
    starts
}

#[cfg(test)]
mod tests {
    use nwnrs_restype::prelude::*;

    use super::{
        InMemoryScriptResolver, NW_SCRIPT_SOURCE_RES_TYPE, ScriptResolver, SourceError, SourceFile,
        SourceId, SourceLoadOptions, SourceMap, Span,
    };
    use crate::CompilerErrorCode;

    #[test]
    fn computes_locations_across_mixed_line_endings() {
        let source = SourceFile::new(SourceId::new(7), "mixed.nss", "a\r\nbc\nd");

        let first = source.location(0);
        assert_eq!(
            first.map(|location| (location.line, location.column)),
            Some((1, 1))
        );

        let second = source.location(3);
        assert_eq!(
            second.map(|location| (location.line, location.column)),
            Some((2, 1))
        );

        let third = source.location(6);
        assert_eq!(
            third.map(|location| (location.line, location.column)),
            Some((3, 1))
        );
    }

    #[test]
    fn returns_span_text_for_same_source_file() {
        let source = SourceFile::new(SourceId::new(1), "test.nss", "void main()");
        let span = Span::new(source.id, 5, 9);

        assert_eq!(source.span_text(span), Some("main"));
    }

    #[test]
    fn source_map_tracks_names_case_insensitively() {
        let mut sources = SourceMap::new();
        let source_id = sources.add_file("Test.NSS", "void main() {}");

        assert_eq!(
            sources.get(source_id).map(|file| file.name.as_str()),
            Some("Test.NSS")
        );
        assert_eq!(
            sources.get_by_name("test.nss").map(|file| file.id),
            Some(source_id)
        );
        assert!(sources.contains_name("TEST.NSS"));
    }

    #[test]
    fn in_memory_resolver_matches_names_case_insensitively() {
        let mut resolver = InMemoryScriptResolver::new();
        resolver.insert_source("UTIL", "int X;");

        let resolved = resolver.resolve_script("util", NW_SCRIPT_SOURCE_RES_TYPE);
        assert_eq!(resolved.ok(), Some(Some("int X;".to_string())));
    }

    #[test]
    fn in_memory_resolver_preserves_non_utf8_source_bytes() {
        let mut resolver = InMemoryScriptResolver::new();
        resolver.insert_source("BYTES", b"\"a\x93\xff\"".to_vec());

        let resolved = resolver.resolve_script_bytes("bytes", NW_SCRIPT_SOURCE_RES_TYPE);
        assert_eq!(resolved.ok(), Some(Some(b"\"a\x93\xff\"".to_vec())));
    }

    #[test]
    fn source_load_options_default_to_nss_and_upstream_depth() {
        let options = SourceLoadOptions::default();

        assert_eq!(options.res_type, ResType(2009));
        assert_eq!(options.max_include_depth, 16);
    }

    #[test]
    fn source_error_exposes_upstream_code() {
        let error = SourceError::file_not_found("missing");

        assert_eq!(error.code(), Some(CompilerErrorCode::FileNotFound));
    }
}
