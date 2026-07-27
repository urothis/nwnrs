use toml_parser::{
    ParseError, Source, Span,
    parser::{Event, EventKind, parse_document},
};

/// Half-open UTF-8 byte range in a manifest source.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ManifestSpan {
    /// First included UTF-8 byte.
    pub start: usize,
    /// First excluded UTF-8 byte.
    pub end:   usize,
}

impl ManifestSpan {
    fn from_parser(span: Span) -> Self {
        Self {
            start: span.start(),
            end:   span.end(),
        }
    }

    /// Returns whether the range contains a byte offset, including its end for
    /// cursor-oriented editor queries.
    #[must_use]
    pub fn contains_cursor(self, offset: usize) -> bool {
        self.start <= offset && offset <= self.end
    }
}

/// One recoverable TOML syntax diagnostic.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManifestSyntaxDiagnostic {
    /// Human-readable parser message.
    pub message: String,
    /// Most precise range reported by the recovering parser.
    pub span:    ManifestSpan,
}

/// One table header in the physical manifest.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManifestTable {
    /// Decoded dotted table path.
    pub path: Vec<String>,
    /// Complete header range, including brackets.
    pub span: ManifestSpan,
}

/// One key/value assignment in the physical manifest.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManifestEntry {
    /// Table containing the assignment.
    pub table:      Vec<String>,
    /// Decoded dotted key path.
    pub key:        Vec<String>,
    /// Range covering the physical key.
    pub key_span:   ManifestSpan,
    /// Range covering the value when the recovering parser found one.
    pub value_span: Option<ManifestSpan>,
}

/// Error-recovering, span-preserving view of an `nwpkg.toml` document.
///
/// This model is for editor queries only. Strict deserialization into
/// [`crate::ProjectManifest`] remains authoritative for builds and packaging.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManifestDocument {
    source:      String,
    tables:      Vec<ManifestTable>,
    entries:     Vec<ManifestEntry>,
    diagnostics: Vec<ManifestSyntaxDiagnostic>,
}

impl ManifestDocument {
    /// Parses a complete or incomplete manifest without discarding valid
    /// surrounding structure.
    #[must_use]
    pub fn parse(source: &str) -> Self {
        let parser_source = Source::new(source);
        let tokens = parser_source.lex().collect::<Vec<_>>();
        let mut events = Vec::<Event>::new();
        let mut errors = Vec::<ParseError>::new();
        parse_document(&tokens, &mut events, &mut errors);

        let (tables, entries, decode_errors) = collect_structure(&parser_source, &events);
        errors.extend(decode_errors);
        let mut diagnostics = errors
            .into_iter()
            .map(|error| ManifestSyntaxDiagnostic {
                message: error.description().to_string(),
                span:    ManifestSpan::from_parser(
                    error
                        .unexpected()
                        .or(error.context())
                        .unwrap_or_else(|| Span::new_unchecked(source.len(), source.len())),
                ),
            })
            .collect::<Vec<_>>();
        if diagnostics.is_empty()
            && let Err(error) = toml::from_str::<toml::Value>(source)
        {
            let span = error.span().unwrap_or(source.len()..source.len());
            diagnostics.push(ManifestSyntaxDiagnostic {
                message: error.to_string(),
                span:    ManifestSpan {
                    start: span.start,
                    end:   span.end,
                },
            });
        }
        Self {
            source: source.to_string(),
            tables,
            entries,
            diagnostics,
        }
    }

    /// Returns the exact source text used to build the document.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Returns physical table headers in source order.
    #[must_use]
    pub fn tables(&self) -> &[ManifestTable] {
        &self.tables
    }

    /// Returns physical key/value assignments in source order.
    #[must_use]
    pub fn entries(&self) -> &[ManifestEntry] {
        &self.entries
    }

    /// Returns recoverable syntax diagnostics in parser order.
    #[must_use]
    pub fn diagnostics(&self) -> &[ManifestSyntaxDiagnostic] {
        &self.diagnostics
    }

    /// Returns the deepest table that owns the cursor.
    #[must_use]
    pub fn table_at(&self, offset: usize) -> &[String] {
        self.tables
            .iter()
            .take_while(|table| table.span.start <= offset)
            .last()
            .map_or(&[], |table| table.path.as_slice())
    }

    /// Returns the assignment whose key or value contains the cursor.
    #[must_use]
    pub fn entry_at(&self, offset: usize) -> Option<&ManifestEntry> {
        self.entries.iter().find(|entry| {
            entry.key_span.contains_cursor(offset)
                || entry
                    .value_span
                    .is_some_and(|span| span.contains_cursor(offset))
        })
    }
}

#[derive(Default)]
struct PendingKey {
    parts: Vec<String>,
    start: Option<usize>,
    end:   usize,
}

impl PendingKey {
    fn push(&mut self, source: &Source<'_>, event: Event, errors: &mut Vec<ParseError>) {
        let mut decoded = String::new();
        if let Some(raw) = source.get(event) {
            raw.decode_key(&mut decoded, errors);
        }
        if decoded.is_empty()
            && let Some(raw) = source.get(event.span())
        {
            decoded = raw.as_str().trim_matches(['"', '\'']).to_string();
        }
        self.parts.push(decoded);
        self.start.get_or_insert(event.span().start());
        self.end = event.span().end();
    }

    fn clear(&mut self) {
        self.parts.clear();
        self.start = None;
        self.end = 0;
    }
}

fn collect_structure(
    source: &Source<'_>,
    events: &[Event],
) -> (Vec<ManifestTable>, Vec<ManifestEntry>, Vec<ParseError>) {
    let mut tables = Vec::new();
    let mut entries = Vec::new();
    let mut current_table = Vec::new();
    let mut pending = PendingKey::default();
    let mut table_start = None;
    let mut assignment: Option<ManifestEntry> = None;
    let mut value_depth = 0_usize;
    let mut errors = Vec::new();

    for event in events {
        match event.kind() {
            EventKind::StdTableOpen | EventKind::ArrayTableOpen => {
                table_start = Some(event.span().start());
                pending.clear();
            }
            EventKind::SimpleKey if table_start.is_some() => {
                pending.push(source, *event, &mut errors);
            }
            EventKind::StdTableClose | EventKind::ArrayTableClose => {
                let path = pending.parts.clone();
                let span = ManifestSpan {
                    start: table_start.take().unwrap_or(event.span().start()),
                    end:   event.span().end(),
                };
                current_table.clone_from(&path);
                tables.push(ManifestTable {
                    path,
                    span,
                });
                pending.clear();
            }
            EventKind::SimpleKey if assignment.is_none() => {
                pending.push(source, *event, &mut errors);
            }
            EventKind::KeyValSep if assignment.is_none() && pending.start.is_some() => {
                assignment = Some(ManifestEntry {
                    table:      current_table.clone(),
                    key:        pending.parts.clone(),
                    key_span:   ManifestSpan {
                        start: pending.start.unwrap_or(event.span().start()),
                        end:   pending.end,
                    },
                    value_span: None,
                });
                pending.clear();
            }
            EventKind::ArrayOpen | EventKind::InlineTableOpen if assignment.is_some() => {
                value_depth = value_depth.saturating_add(1);
                extend_value_span(&mut assignment, event.span());
            }
            EventKind::ArrayClose | EventKind::InlineTableClose if assignment.is_some() => {
                extend_value_span(&mut assignment, event.span());
                value_depth = value_depth.saturating_sub(1);
            }
            EventKind::Scalar if assignment.is_some() => {
                extend_value_span(&mut assignment, event.span());
            }
            EventKind::Newline if assignment.is_some() && value_depth == 0 => {
                if let Some(entry) = assignment.take() {
                    entries.push(entry);
                }
            }
            EventKind::Error if assignment.is_some() => {
                extend_value_span(&mut assignment, event.span());
            }
            _ => {}
        }
    }
    if let Some(entry) = assignment {
        entries.push(entry);
    }
    (tables, entries, errors)
}

fn extend_value_span(assignment: &mut Option<ManifestEntry>, span: Span) {
    if let Some(entry) = assignment {
        entry.value_span = Some(entry.value_span.map_or(
            ManifestSpan::from_parser(span),
            |existing| ManifestSpan {
                start: existing.start.min(span.start()),
                end:   existing.end.max(span.end()),
            },
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::ManifestDocument;

    #[test]
    fn preserves_quoted_and_dotted_key_spans() {
        let source = "[project]\n\"na.me\" = \"value\"\n[source.paths]\npath = [\n  \"src\",\n]\n";
        let document = ManifestDocument::parse(source);
        let project = document.tables().first().expect("project table");
        let name = document.entries().first().expect("project name");
        assert_eq!(project.path, ["project"]);
        assert_eq!(name.key, ["na.me"]);
        assert_eq!(&source[name.key_span.start..name.key_span.end], "\"na.me\"");
        let source_paths = document.tables().get(1).expect("source paths table");
        let path = document.entries().get(1).expect("source path");
        assert_eq!(source_paths.path, ["source", "paths"]);
        let value_span = path.value_span.expect("multiline value span");
        assert_eq!(
            &source[value_span.start..value_span.end],
            "[\n  \"src\",\n]"
        );
    }

    #[test]
    fn decodes_quoted_table_names() {
        let source = "[\"project\"]\n\"name\" = \"demo\"\n";
        let document = ManifestDocument::parse(source);
        let project = document.tables().first().expect("project table");
        let name = document.entries().first().expect("project name");
        assert_eq!(project.path, ["project"]);
        assert_eq!(name.table, ["project"]);
        assert_eq!(name.key, ["name"]);
    }

    #[test]
    fn retains_valid_structure_around_incomplete_values() {
        let source = "[project]\nname = \"unfinished\nkind = \"mod\"\n[source]\npath = \".\"\n";
        let document = ManifestDocument::parse(source);
        assert!(!document.diagnostics().is_empty());
        assert!(
            document
                .tables()
                .iter()
                .any(|table| table.path == ["source"])
        );
        assert!(document.entries().iter().any(|entry| entry.key == ["path"]));
    }
}
