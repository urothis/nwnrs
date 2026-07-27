use super::*;

fn compiler_driver_error_span(error: &CompilerDriverError) -> Option<crate::Span> {
    let CompilerDriverError::Session(session_error) = error else {
        return None;
    };
    match session_error {
        CompilerSessionError::Preprocess(crate::PreprocessError::Lex(error)) => Some(error.span),
        CompilerSessionError::Preprocess(crate::PreprocessError::Macro(error)) => error.span,
        CompilerSessionError::Compile(compile_error) => match compile_error {
            crate::CompileError::Parse(crate::ResolvedParseError::Parse(error)) => Some(error.span),
            crate::CompileError::Parse(crate::ResolvedParseError::Preprocess(
                crate::PreprocessError::Lex(error),
            )) => Some(error.span),
            crate::CompileError::Parse(crate::ResolvedParseError::Preprocess(
                crate::PreprocessError::Macro(error),
            )) => error.span,
            crate::CompileError::Semantic(error) => Some(error.span),
            crate::CompileError::Hir(error) => Some(error.span),
            crate::CompileError::Codegen(error) => error.span,
            crate::CompileError::Parse(crate::ResolvedParseError::Preprocess(
                crate::PreprocessError::Cancelled(_) | crate::PreprocessError::Source(_),
            )) => None,
        },
        CompilerSessionError::Cancelled(_)
        | CompilerSessionError::LangSpec(_)
        | CompilerSessionError::Preprocess(crate::PreprocessError::Cancelled(_))
        | CompilerSessionError::Preprocess(crate::PreprocessError::Source(_))
        | CompilerSessionError::Source(_) => None,
    }
}

fn compiler_driver_error_code(error: &CompilerDriverError) -> Option<i32> {
    let CompilerDriverError::Session(session_error) = error else {
        return None;
    };
    let code = match session_error {
        CompilerSessionError::Preprocess(crate::PreprocessError::Lex(error)) => Some(error.code),
        CompilerSessionError::Compile(compile_error) => match compile_error {
            crate::CompileError::Parse(crate::ResolvedParseError::Parse(error)) => Some(error.code),
            crate::CompileError::Parse(crate::ResolvedParseError::Preprocess(
                crate::PreprocessError::Lex(error),
            )) => Some(error.code),
            crate::CompileError::Semantic(error) => Some(error.code),
            crate::CompileError::Codegen(error) => error.code,
            crate::CompileError::Parse(crate::ResolvedParseError::Preprocess(
                crate::PreprocessError::Cancelled(_)
                | crate::PreprocessError::Macro(_)
                | crate::PreprocessError::Source(_),
            ))
            | crate::CompileError::Hir(_) => None,
        },
        CompilerSessionError::Cancelled(_)
        | CompilerSessionError::LangSpec(_)
        | CompilerSessionError::Preprocess(
            crate::PreprocessError::Cancelled(_)
            | crate::PreprocessError::Macro(_)
            | crate::PreprocessError::Source(_),
        )
        | CompilerSessionError::Source(_) => None,
    }?;
    Some(code.code())
}

/// Converts a compiler-driver failure into structured, source-aware editor
/// diagnostic data.
pub fn source_aware_driver_diagnostic<R: ScriptResolver + ?Sized>(
    error: &CompilerDriverError,
    resolver: &R,
    input: &Path,
    source_load: crate::SourceLoadOptions,
) -> CompilerDiagnostic {
    let mut diagnostic = CompilerDiagnostic {
        code:         compiler_driver_error_code(error),
        message:      error.to_string(),
        file:         None,
        source:       None,
        start_line:   None,
        start_column: None,
        end_line:     None,
        end_column:   None,
        start_offset: None,
        end_offset:   None,
        source_bytes: None,
    };
    let Some(span) = compiler_driver_error_span(error) else {
        return diagnostic;
    };
    let Some(input_name) = input.to_str() else {
        diagnostic.message.push_str(&format!(
            "; additionally failed to resolve the source location: NWScript input path is not \
             valid UTF-8 ({})",
            crate::physical_path_display(input)
        ));
        return diagnostic;
    };
    let bundle = match crate::load_source_bundle(resolver, input_name, source_load) {
        Ok(bundle) => bundle,
        Err(location_error) => {
            diagnostic.message.push_str(&format!(
                "; additionally failed to resolve the source location: {location_error}"
            ));
            return diagnostic;
        }
    };
    let Some(file) = bundle.source_map.get(span.source_id) else {
        return diagnostic;
    };
    let span = visible_diagnostic_span(file, span);
    let Some(start) = file.location(span.start) else {
        return diagnostic;
    };
    let end_offset = if span.is_empty() {
        span.end.saturating_add(1).min(file.len())
    } else {
        span.end.min(file.len())
    };
    let end = file.location(end_offset).unwrap_or(start);
    diagnostic.file = Some(file.name.clone());
    diagnostic.source = resolver
        .resolve_script_origin(&file.name, crate::NW_SCRIPT_SOURCE_RES_TYPE)
        .ok()
        .flatten()
        .or_else(|| {
            Some(crate::ScriptSourceOrigin::Virtual(format!(
                "nwnrs-source:{}",
                file.name
            )))
        });
    diagnostic.start_line = Some(start.line);
    diagnostic.start_column = Some(start.column);
    diagnostic.end_line = Some(end.line);
    diagnostic.end_column = Some(end.column);
    diagnostic.start_offset = Some(span.start);
    diagnostic.end_offset = Some(end_offset);
    diagnostic.source_bytes = Some(file.contents.clone());
    diagnostic
}

/// Ensures editor diagnostics cover source text instead of a zero-width EOF
/// token or whitespace on the following line. Frontend phases should still
/// report the most precise span they have; this is the shared safety net for
/// every diagnostic producer.
pub(super) fn visible_diagnostic_span(file: &crate::SourceFile, span: crate::Span) -> crate::Span {
    let bytes = file.bytes();
    if span.source_id != file.id || span.start > span.end || span.end > bytes.len() {
        return span;
    }
    if bytes
        .get(span.start..span.end)
        .unwrap_or_default()
        .iter()
        .any(|byte| !byte.is_ascii_whitespace())
    {
        return span;
    }

    let mut end = span.start.min(bytes.len());
    while end > 0 && bytes.get(end - 1).is_some_and(u8::is_ascii_whitespace) {
        end -= 1;
    }
    if end > 0 {
        let start = diagnostic_unit_start(bytes, end);
        return crate::Span::new(span.source_id, start, end);
    }

    let mut start = span.end.min(bytes.len());
    while bytes.get(start).is_some_and(u8::is_ascii_whitespace) {
        start += 1;
    }
    if start < bytes.len() {
        let end = diagnostic_unit_end(bytes, start);
        return crate::Span::new(span.source_id, start, end);
    }

    span
}

fn diagnostic_unit_start(bytes: &[u8], end: usize) -> usize {
    let last = bytes
        .get(end.saturating_sub(1))
        .copied()
        .unwrap_or_default();
    if !is_diagnostic_word_byte(last) {
        return end - 1;
    }
    let mut start = end - 1;
    while start > 0
        && bytes
            .get(start - 1)
            .copied()
            .is_some_and(is_diagnostic_word_byte)
    {
        start -= 1;
    }
    start
}

fn diagnostic_unit_end(bytes: &[u8], start: usize) -> usize {
    if !bytes
        .get(start)
        .copied()
        .is_some_and(is_diagnostic_word_byte)
    {
        return start + 1;
    }
    let mut end = start + 1;
    while bytes.get(end).copied().is_some_and(is_diagnostic_word_byte) {
        end += 1;
    }
    end
}

const fn is_diagnostic_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

pub(super) fn format_compiler_diagnostic(diagnostic: &CompilerDiagnostic, input: &Path) -> String {
    match (
        diagnostic.file.as_deref(),
        diagnostic.start_line,
        diagnostic.start_column,
    ) {
        (Some(file), Some(line), Some(column)) => {
            format!("{file}:{line}:{column}: {}", diagnostic.message)
        }
        _ => format!(
            "failed to compile {}: {}",
            input.display(),
            diagnostic.message
        ),
    }
}

/// Formats a compiler-driver failure with its resolved source location when
/// the error carries a source span.
pub fn format_source_aware_driver_error<R: ScriptResolver + ?Sized>(
    error: &CompilerDriverError,
    resolver: &R,
    input: &Path,
    source_load: crate::SourceLoadOptions,
) -> String {
    let diagnostic = source_aware_driver_diagnostic(error, resolver, input, source_load);
    format_compiler_diagnostic(&diagnostic, input)
}
