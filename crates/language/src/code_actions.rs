use std::{fs, path::PathBuf};

use nwnrs_nwscript::{CancellationToken, Keyword, Lexer, SourceFile, SourceId, Token, TokenKind};

use crate::{LanguageResult, NwScriptDefinitionQuery, NwScriptProjectIndex, NwScriptSourceRange};

/// One compiler diagnostic relevant to NWScript quick fixes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NwScriptCodeActionDiagnostic {
    /// Stable compiler diagnostic number.
    pub code:  i32,
    /// Exact editor range already mapped by the language core.
    pub range: NwScriptSourceRange,
}

/// One compiler-produced source edit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NwScriptTextEdit {
    /// Exact physical source to edit.
    pub path:     PathBuf,
    /// Exact replacement/insertion range.
    pub range:    NwScriptSourceRange,
    /// Replacement text.
    pub new_text: String,
}

/// One compiler-owned editor quick fix.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NwScriptCodeAction {
    /// User-facing action title.
    pub title:        String,
    /// Whether the action is the unambiguous preferred correction.
    pub is_preferred: bool,
    /// Exact edits applied by the action.
    pub edits:        Vec<NwScriptTextEdit>,
}

pub(crate) fn code_actions(
    index: &mut NwScriptProjectIndex,
    query: &NwScriptDefinitionQuery,
    diagnostics: &[NwScriptCodeActionDiagnostic],
    cancellation: Option<&CancellationToken>,
) -> LanguageResult<Vec<NwScriptCodeAction>> {
    let bytes = query
        .source_overlays
        .iter()
        .find_map(|(path, bytes)| {
            nwnrs_nwscript::physical_sources_equal(path, &query.source_path)
                .then_some(bytes.clone())
        })
        .map_or_else(
            || {
                fs::read(&query.source_path).map_err(|error| {
                    format!(
                        "failed to read code-action source {}: {error}",
                        query.source_path.display()
                    )
                })
            },
            Ok,
        )?;
    let source = SourceFile::new(
        SourceId::new(0),
        nwnrs_nwscript::physical_path_display(&query.source_path),
        bytes,
    );
    let tokens = Lexer::new(source.id, source.bytes())
        .lex_all()
        .map_err(|error| format!("failed to lex code-action source: {error}"))?;
    let mut actions = Vec::new();
    for diagnostic in diagnostics {
        if let Some(cancellation) = cancellation {
            cancellation.check().map_err(|error| error.to_string())?;
        }
        match diagnostic.code {
            -573 => actions.push(NwScriptCodeAction {
                title:        "Insert missing semicolon".to_string(),
                is_preferred: true,
                edits:        vec![NwScriptTextEdit {
                    path:     query.source_path.clone(),
                    range:    NwScriptSourceRange {
                        start_line:   diagnostic.range.end_line,
                        start_column: diagnostic.range.end_column,
                        end_line:     diagnostic.range.end_line,
                        end_column:   diagnostic.range.end_column,
                    },
                    new_text: ";".to_string(),
                }],
            }),
            -622 => {
                if let Some(action) = missing_include_action(
                    index,
                    query,
                    diagnostic,
                    &source,
                    &tokens,
                    cancellation,
                )? {
                    actions.push(action);
                }
            }
            _ => {}
        }
    }
    Ok(actions)
}

fn missing_include_action(
    index: &mut NwScriptProjectIndex,
    query: &NwScriptDefinitionQuery,
    diagnostic: &NwScriptCodeActionDiagnostic,
    source: &SourceFile,
    tokens: &[Token],
    cancellation: Option<&CancellationToken>,
) -> LanguageResult<Option<NwScriptCodeAction>> {
    let offset = offset_for_position(
        source.bytes(),
        diagnostic.range.start_line,
        diagnostic.range.start_column,
    )
    .ok_or_else(|| "undefined-identifier diagnostic range is outside its source".to_string())?;
    let Some(symbol) = tokens
        .iter()
        .find(|token| {
            token.kind == TokenKind::Identifier
                && offset >= token.span.start
                && offset <= token.span.end
        })
        .map(|token| token.text.clone())
    else {
        return Ok(None);
    };
    let mut candidate_query = query.clone();
    candidate_query.symbol = symbol;
    candidate_query.qualifier = None;
    let candidates = index.include_candidates(&candidate_query, cancellation)?;
    let [candidate] = candidates.as_slice() else {
        return Ok(None);
    };
    let mut last_include = None;
    let mut existing = Vec::new();
    for pair in tokens.windows(2) {
        let Some(include) = pair.first() else {
            continue;
        };
        let Some(path) = pair.get(1) else {
            continue;
        };
        if include.kind == TokenKind::Keyword(Keyword::Include) && path.kind == TokenKind::String {
            last_include = Some(path.span.end);
            existing.push(path.text.to_ascii_lowercase());
        }
    }
    if existing
        .iter()
        .any(|include| include == &candidate.include_name.to_ascii_lowercase())
    {
        return Ok(None);
    }
    let (offset, new_text) = match last_include {
        Some(include_end) => {
            let line_end = source
                .bytes()
                .get(include_end..)
                .and_then(|tail| tail.iter().position(|byte| matches!(byte, b'\r' | b'\n')))
                .map_or(source.len(), |relative| {
                    include_end.saturating_add(relative)
                });
            (
                line_end,
                format!("\n#include \"{}\"", candidate.include_name),
            )
        }
        None => (0, format!("#include \"{}\"\n\n", candidate.include_name)),
    };
    let location = source
        .location(offset)
        .ok_or_else(|| "include insertion offset is outside its source".to_string())?;
    let range = NwScriptSourceRange {
        start_line:   location.line,
        start_column: location.column,
        end_line:     location.line,
        end_column:   location.column,
    };
    Ok(Some(NwScriptCodeAction {
        title:        format!("Add #include \"{}\"", candidate.include_name),
        is_preferred: true,
        edits:        vec![NwScriptTextEdit {
            path: query.source_path.clone(),
            range,
            new_text,
        }],
    }))
}

fn offset_for_position(bytes: &[u8], line: usize, column: usize) -> Option<usize> {
    if line == 0 || column == 0 {
        return None;
    }
    let mut current_line = 1_usize;
    let mut line_start = 0_usize;
    for (index, byte) in bytes.iter().enumerate() {
        if current_line == line {
            break;
        }
        if *byte == b'\n' {
            current_line = current_line.saturating_add(1);
            line_start = index.saturating_add(1);
        }
    }
    (current_line == line)
        .then(|| line_start.saturating_add(column.saturating_sub(1)))
        .filter(|offset| *offset <= bytes.len())
}
