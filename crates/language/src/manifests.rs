use std::{
    fs,
    path::{Path, PathBuf},
};

use nwnrs_nwpkg::{
    ManifestDocument, ManifestEntry, ManifestValueKind, NwpkgError, ProjectKind, ProjectLanguage,
    ProjectManifest, manifest_schema,
};
use toml_parser::{Source, lexer::TokenKind};

use crate::{
    LanguageDiagnostic, LanguageDiagnosticCode, LanguageDiagnosticSeverity, LanguageRange,
    LanguageSource, physical_diagnostic, utf16_range,
};

/// Presentation category for one manifest completion.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManifestCompletionKind {
    /// TOML table.
    Section,
    /// TOML property.
    Field,
    /// Project-kind enum member.
    ProjectKind,
    /// Project-language enum member.
    ProjectLanguage,
    /// Filesystem directory.
    Folder,
}

/// Compiler-owned manifest completion ready for editor presentation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManifestCompletion {
    /// Visible candidate label.
    pub label:         String,
    /// Short type or purpose.
    pub detail:        String,
    /// Full documentation.
    pub documentation: String,
    /// Text or snippet inserted by the editor.
    pub insert_text:   String,
    /// Exact UTF-16 replacement range.
    pub replacement:   LanguageRange,
    /// Presentation category.
    pub kind:          ManifestCompletionKind,
    /// Whether the editor should request another completion after insertion.
    pub retrigger:     bool,
}

/// Manifest hover result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManifestHover {
    /// Markdown-safe documentation text.
    pub documentation: String,
    /// Exact source range described by the hover.
    pub range:         LanguageRange,
}

/// Manifest go-to-definition target.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManifestDefinition {
    /// Exact target file.
    pub path:  PathBuf,
    /// Target selection range.
    pub range: LanguageRange,
}

/// Manifest Outline symbol category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManifestSymbolKind {
    /// Table namespace.
    Section,
    /// Key/value property.
    Field,
}

/// One manifest Outline symbol.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManifestSymbol {
    /// Symbol label.
    pub name:      String,
    /// Value preview or schema detail.
    pub detail:    String,
    /// Symbol category.
    pub kind:      ManifestSymbolKind,
    /// Full symbol range.
    pub range:     LanguageRange,
    /// Name selection range.
    pub selection: LanguageRange,
    /// Nested fields for a section.
    pub children:  Vec<ManifestSymbol>,
}

/// Produces recovering syntax plus strict semantic manifest diagnostics.
#[must_use]
pub fn analyze_manifest_diagnostics(path: &Path, source: &str) -> Vec<LanguageDiagnostic> {
    let document = ManifestDocument::parse(source);
    let mut diagnostics = document
        .diagnostics()
        .iter()
        .map(|diagnostic| {
            physical_diagnostic(
                path,
                source,
                LanguageDiagnosticSeverity::Error,
                Some(LanguageDiagnosticCode::Named("nwpkg.syntax".to_string())),
                diagnostic.message.clone(),
                diagnostic.span.start,
                diagnostic.span.end,
            )
        })
        .collect::<Vec<_>>();
    if diagnostics.is_empty() {
        match toml::from_str::<ProjectManifest>(source) {
            Ok(manifest) => {
                if let Some(root) = path.parent()
                    && let Err(error) = manifest.validate(root)
                {
                    diagnostics.push(LanguageDiagnostic {
                        severity: LanguageDiagnosticSeverity::Error,
                        code:     Some(LanguageDiagnosticCode::Named(
                            "nwpkg.validation".to_string(),
                        )),
                        message:  error.to_string(),
                        source:   LanguageSource::Physical(path.to_path_buf()),
                        range:    semantic_error_range(&document, source, &error),
                    });
                }
            }
            Err(error) => {
                let span = error.span().unwrap_or(source.len()..source.len());
                diagnostics.push(physical_diagnostic(
                    path,
                    source,
                    LanguageDiagnosticSeverity::Error,
                    Some(LanguageDiagnosticCode::Named("nwpkg.schema".to_string())),
                    error.to_string(),
                    span.start,
                    span.end,
                ));
            }
        }
    }
    diagnostics
}

/// Returns schema, value, and path completions for an incomplete manifest.
#[must_use]
pub fn manifest_completions(
    path: &Path,
    source: &str,
    byte_offset: usize,
) -> Vec<ManifestCompletion> {
    let document = ManifestDocument::parse(source);
    let offset = byte_offset.min(source.len());
    if is_table_context(source, offset) {
        return section_completions(source, offset);
    }
    if let Some((value, replacement)) = path_value_context(&document, source, offset) {
        return path_completions(path, source, &value, replacement);
    }
    if let Some(entry) = document.entry_at(offset)
        && entry.table == ["project"]
        && entry
            .value_span
            .is_some_and(|span| span.contains_cursor(offset))
    {
        if entry.key == ["kind"] {
            return project_kind_completions(source, entry);
        }
        if entry.key == ["language"] {
            return project_language_completions(source, entry);
        }
    }
    field_completions(source, offset, &document)
}

/// Returns schema documentation for the table or field under the cursor.
#[must_use]
pub fn manifest_hover(source: &str, byte_offset: usize) -> Option<ManifestHover> {
    let document = ManifestDocument::parse(source);
    let offset = byte_offset.min(source.len());
    if let Some(table) = document
        .tables()
        .iter()
        .find(|table| table.span.contains_cursor(offset))
        && let Some(schema) = manifest_schema()
            .iter()
            .find(|schema| schema.path == table.path)
    {
        return Some(ManifestHover {
            documentation: schema.documentation.to_string(),
            range:         utf16_range(source, table.span.start, table.span.end),
        });
    }
    let entry = document.entry_at(offset)?;
    let schema = manifest_schema()
        .iter()
        .find(|schema| schema.path == entry.table)?;
    if schema.dynamic_keys {
        return Some(ManifestHover {
            documentation: "Named include-package dependency. Use `path` for a live path relative \
                            to this nwpkg.toml, or `git` with at most one of `rev`, `tag`, or \
                            `branch`. Git dependencies are pinned to exact commits in nwpkg.lock."
                .to_string(),
            range:         utf16_range(source, entry.key_span.start, entry.key_span.end),
        });
    }
    let field = schema
        .fields
        .iter()
        .find(|field| entry.key.as_slice() == [field.name])?;
    Some(ManifestHover {
        documentation: field.documentation.to_string(),
        range:         utf16_range(source, entry.key_span.start, entry.key_span.end),
    })
}

/// Resolves source and dependency path values to exact files.
#[must_use]
pub fn manifest_definition(
    manifest_path: &Path,
    source: &str,
    byte_offset: usize,
) -> Option<ManifestDefinition> {
    let document = ManifestDocument::parse(source);
    let (value, _replacement) =
        path_value_context(&document, source, byte_offset.min(source.len()))?;
    let parent = manifest_path.parent()?;
    let resolved = parent.join(value);
    let metadata = fs::metadata(&resolved).ok()?;
    let target = if metadata.is_dir() {
        resolved.join(nwnrs_nwpkg::PROJECT_MANIFEST_FILENAME)
    } else {
        resolved
    };
    fs::metadata(&target)
        .ok()
        .is_some_and(|metadata| metadata.is_file())
        .then_some(ManifestDefinition {
            path:  target,
            range: LanguageRange::default(),
        })
}

/// Produces a hierarchical manifest Outline from parser spans.
#[must_use]
pub fn manifest_symbols(source: &str) -> Vec<ManifestSymbol> {
    let document = ManifestDocument::parse(source);
    let mut roots = Vec::new();
    for table in document.tables() {
        let mut children = document
            .entries()
            .iter()
            .filter(|entry| entry.table == table.path)
            .map(|entry| {
                let value_end = entry.value_span.map_or(entry.key_span.end, |span| span.end);
                ManifestSymbol {
                    name:      entry.key.join("."),
                    detail:    entry
                        .value_span
                        .and_then(|span| source.get(span.start..span.end))
                        .unwrap_or_default()
                        .trim()
                        .to_string(),
                    kind:      ManifestSymbolKind::Field,
                    range:     utf16_range(source, entry.key_span.start, value_end),
                    selection: utf16_range(source, entry.key_span.start, entry.key_span.end),
                    children:  Vec::new(),
                }
            })
            .collect::<Vec<_>>();
        let end = children.last().map_or(table.span.end, |child| {
            utf16_range_end_byte(source, child.range)
        });
        roots.push(ManifestSymbol {
            name:      table.path.join("."),
            detail:    "manifest section".to_string(),
            kind:      ManifestSymbolKind::Section,
            range:     utf16_range(source, table.span.start, end),
            selection: utf16_range(source, table.span.start, table.span.end),
            children:  std::mem::take(&mut children),
        });
    }
    roots
}

pub(crate) fn manifest_dependency_range(source: &str, name: &str) -> LanguageRange {
    let document = ManifestDocument::parse(source);
    document
        .entries()
        .iter()
        .find(|entry| entry.table == ["dependencies"] && entry.key == [name])
        .map_or_else(
            || utf16_range(source, 0, 0),
            |entry| utf16_range(source, entry.key_span.start, entry.key_span.end),
        )
}

fn semantic_error_range(
    document: &ManifestDocument,
    source: &str,
    error: &NwpkgError,
) -> LanguageRange {
    error
        .manifest_field_path()
        .and_then(|path| path.split_last())
        .and_then(|(key, table)| {
            document.entries().iter().find(|entry| {
                entry.table == table && entry.key.len() == 1 && entry.key.first() == Some(key)
            })
        })
        .map_or_else(
            || utf16_range(source, 0, 0),
            |entry| utf16_range(source, entry.key_span.start, entry.key_span.end),
        )
}

fn is_table_context(source: &str, offset: usize) -> bool {
    let line_start = source
        .get(..offset)
        .and_then(|prefix| prefix.rfind('\n'))
        .map_or(0, |index| index + 1);
    let line = source.get(line_start..offset).unwrap_or_default();
    let tokens = Source::new(line).lex().collect::<Vec<_>>();
    let significant = tokens.iter().filter(|token| {
        !matches!(
            token.kind(),
            TokenKind::Whitespace | TokenKind::Comment | TokenKind::Eof
        )
    });
    matches!(
        significant
            .map(|token| token.kind())
            .collect::<Vec<_>>()
            .as_slice(),
        [TokenKind::LeftSquareBracket] | [TokenKind::LeftSquareBracket, TokenKind::Atom]
    )
}

fn section_completions(source: &str, offset: usize) -> Vec<ManifestCompletion> {
    let range = utf16_range(source, offset, offset);
    manifest_schema()
        .iter()
        .map(|section| {
            let name = section.path.join(".");
            ManifestCompletion {
                label:         name.clone(),
                detail:        format!("[{name}]"),
                documentation: section.documentation.to_string(),
                insert_text:   format!("{name}]"),
                replacement:   range,
                kind:          ManifestCompletionKind::Section,
                retrigger:     false,
            }
        })
        .collect()
}

fn field_completions(
    source: &str,
    offset: usize,
    document: &ManifestDocument,
) -> Vec<ManifestCompletion> {
    let table = document.table_at(offset);
    let Some(schema) = manifest_schema().iter().find(|schema| schema.path == table) else {
        return Vec::new();
    };
    let range = utf16_range(source, offset, offset);
    if schema.dynamic_keys {
        return [
            (
                "path dependency",
                "Live local include dependency",
                "${1:package} = { path = \"${2:../include/package}\" }",
            ),
            (
                "Git dependency",
                "Git dependency following its default branch",
                "${1:package} = { git = \"${2:https://example.invalid/package.git}\" }",
            ),
            (
                "Git branch dependency",
                "Git dependency following a named branch",
                "${1:package} = { git = \"${2:https://example.invalid/package.git}\", branch = \
                 \"${3:main}\" }",
            ),
            (
                "Git tag dependency",
                "Git dependency following a named tag",
                "${1:package} = { git = \"${2:https://example.invalid/package.git}\", tag = \
                 \"${3:v1}\" }",
            ),
            (
                "Git revision dependency",
                "Git dependency pinned from an explicit revision",
                "${1:package} = { git = \"${2:https://example.invalid/package.git}\", rev = \
                 \"${3:commit}\" }",
            ),
        ]
        .into_iter()
        .map(|(label, detail, insert_text)| ManifestCompletion {
            label:         label.to_string(),
            detail:        detail.to_string(),
            documentation: schema.documentation.to_string(),
            insert_text:   insert_text.to_string(),
            replacement:   range,
            kind:          ManifestCompletionKind::Field,
            retrigger:     false,
        })
        .collect();
    }
    schema
        .fields
        .iter()
        .filter(|field| {
            !document
                .entries()
                .iter()
                .any(|entry| entry.table == table && entry.key == [field.name])
        })
        .map(|field| ManifestCompletion {
            label:         field.name.to_string(),
            detail:        field.documentation.to_string(),
            documentation: field.documentation.to_string(),
            insert_text:   match field.value_kind {
                ManifestValueKind::String => format!("{} = \"${{1:project}}\"", field.name),
                ManifestValueKind::ProjectKind => format!("{} = \"${{1:mod}}\"", field.name),
                ManifestValueKind::ProjectLanguage => {
                    format!("{} = \"${{1:english}}\"", field.name)
                }
                ManifestValueKind::Path => format!("{} = \"${{1:.}}\"", field.name),
                ManifestValueKind::Dependency => {
                    "${1:package} = { path = \"${2:../include/package}\" }".to_string()
                }
            },
            replacement:   range,
            kind:          ManifestCompletionKind::Field,
            retrigger:     false,
        })
        .collect()
}

fn project_language_completions(source: &str, entry: &ManifestEntry) -> Vec<ManifestCompletion> {
    let span = entry.value_span.unwrap_or(entry.key_span);
    let raw = source.get(span.start..span.end).unwrap_or_default();
    let quoted = raw.starts_with('"');
    let start = span.start + usize::from(quoted);
    let end = span
        .end
        .saturating_sub(usize::from(quoted && raw.ends_with('"')));
    let replacement = utf16_range(source, start, end);
    ProjectLanguage::all()
        .iter()
        .map(|language| ManifestCompletion {
            label: language.as_str().to_string(),
            detail: "nwpkg resource language".to_string(),
            documentation: format!(
                "Uses the `{}` native resource language policy.",
                language.as_str()
            ),
            insert_text: language.as_str().to_string(),
            replacement,
            kind: ManifestCompletionKind::ProjectLanguage,
            retrigger: false,
        })
        .collect()
}

fn project_kind_completions(source: &str, entry: &ManifestEntry) -> Vec<ManifestCompletion> {
    let span = entry.value_span.unwrap_or(entry.key_span);
    let raw = source.get(span.start..span.end).unwrap_or_default();
    let quoted = raw.starts_with('"');
    let start = span.start + usize::from(quoted);
    let end = span
        .end
        .saturating_sub(usize::from(quoted && raw.ends_with('"')));
    let replacement = utf16_range(source, start, end);
    ProjectKind::all()
        .iter()
        .map(|kind| ManifestCompletion {
            label: kind.as_str().to_string(),
            detail: "nwpkg project kind".to_string(),
            documentation: format!("Produces an nwpkg `{}` project.", kind.as_str()),
            insert_text: kind.as_str().to_string(),
            replacement,
            kind: ManifestCompletionKind::ProjectKind,
            retrigger: false,
        })
        .collect()
}

fn path_value_context(
    document: &ManifestDocument,
    source: &str,
    offset: usize,
) -> Option<(String, std::ops::Range<usize>)> {
    let entry = document.entry_at(offset)?;
    let is_path =
        (entry.table == ["source"] && entry.key == ["path"]) || entry.table == ["dependencies"];
    if !is_path {
        return None;
    }
    let span = entry.value_span?;
    let tokens = Source::new(source.get(span.start..span.end)?)
        .lex()
        .collect::<Vec<_>>();
    let token = tokens.into_iter().find(|token| {
        matches!(
            token.kind(),
            TokenKind::BasicString | TokenKind::LiteralString
        ) && {
            let absolute_start = span.start + token.span().start();
            let absolute_end = span.start + token.span().end();
            absolute_start <= offset && offset <= absolute_end
        }
    })?;
    let absolute = span.start + token.span().start()..span.start + token.span().end();
    let raw = source.get(absolute.clone())?;
    let unquoted_start = absolute.start + 1;
    let unquoted_end = absolute.end.saturating_sub(usize::from(
        raw.len() > 1 && raw.as_bytes().first() == raw.as_bytes().last(),
    ));
    let value = source.get(unquoted_start..unquoted_end)?.to_string();
    Some((value, unquoted_start..unquoted_end))
}

fn path_completions(
    manifest_path: &Path,
    source: &str,
    value: &str,
    value_span: std::ops::Range<usize>,
) -> Vec<ManifestCompletion> {
    let slash = value.rfind(['/', '\\']);
    let directory_part = slash.map_or("", |index| &value[..=index]);
    let name_part = slash.map_or(value, |index| &value[index + 1..]);
    let Some(parent) = manifest_path.parent() else {
        return Vec::new();
    };
    let directory = parent.join(if directory_part.is_empty() {
        "."
    } else {
        directory_part
    });
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    let replacement_start = value_span.end.saturating_sub(name_part.len());
    let replacement = utf16_range(source, replacement_start, value_span.end);
    let mut candidates = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.starts_with(name_part))
        .collect::<Vec<_>>();
    candidates.sort();
    candidates.truncate(200);
    candidates
        .into_iter()
        .map(|name| ManifestCompletion {
            label: name.clone(),
            detail: "Directory".to_string(),
            documentation: String::new(),
            insert_text: format!("{name}/"),
            replacement,
            kind: ManifestCompletionKind::Folder,
            retrigger: true,
        })
        .collect()
}

fn utf16_range_end_byte(source: &str, range: LanguageRange) -> usize {
    let mut line = 0_u32;
    let mut character = 0_u32;
    for (offset, value) in source.char_indices() {
        if line == range.end.line && character >= range.end.character {
            return offset;
        }
        if value == '\n' {
            line = line.saturating_add(1);
            character = 0;
        } else {
            character =
                character.saturating_add(u32::try_from(value.len_utf16()).unwrap_or(u32::MAX));
        }
    }
    source.len()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{
        ManifestCompletionKind, analyze_manifest_diagnostics, manifest_completions, manifest_hover,
        manifest_symbols,
    };

    #[test]
    fn kind_candidates_come_from_project_kind_schema() {
        let source = "[project]\nname = \"demo\"\nkind = \"m\"\n";
        let offset = source.rfind('m').expect("kind value") + 1;
        let completions = manifest_completions(Path::new("/tmp/nwpkg.toml"), source, offset);
        assert!(completions.iter().any(|item| item.label == "mod"));
        assert!(
            completions
                .iter()
                .all(|item| item.kind == ManifestCompletionKind::ProjectKind)
        );
    }

    #[test]
    fn hover_and_outline_use_parser_spans() {
        let source = "[\"project\"]\n\"name\" = \"demo\"\nkind = \"mod\"\n";
        let hover = manifest_hover(source, 3).expect("project hover");
        assert!(hover.documentation.contains("identity"));
        let symbols = manifest_symbols(source);
        let project = symbols.first().expect("project symbol");
        assert_eq!(project.name, "project");
        assert_eq!(
            project.children.first().expect("project name symbol").name,
            "name"
        );
    }

    #[test]
    fn schema_errors_use_exact_toml_span() {
        let source = "[project]\nname = \"demo\"\nkind = \"invalid\"\n[source]\npath = \".\"\n";
        let diagnostics = analyze_manifest_diagnostics(Path::new("/tmp/nwpkg.toml"), source);
        assert_eq!(diagnostics.len(), 1);
        assert!(
            diagnostics
                .first()
                .expect("schema diagnostic")
                .range
                .start
                .line
                >= 2
        );
    }

    #[test]
    fn semantic_validation_uses_typed_manifest_field_spans() {
        let source = "[project]\nname = \"\"\nkind = \"mod\"\n[source]\npath = \".\"\n";
        let diagnostics = analyze_manifest_diagnostics(Path::new("/tmp/nwpkg.toml"), source);
        let diagnostic = diagnostics.first().expect("validation diagnostic");
        assert_eq!(diagnostic.range.start.line, 1);
        assert!(diagnostic.range.start.character <= 4);
        assert_ne!(diagnostic.range, super::utf16_range(source, 0, 0));
    }
}
