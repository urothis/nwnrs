use std::{error::Error, fmt};

use super::*;

/// One parser error aligned to the upstream compiler's diagnostic space.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParserError {
    /// Stable upstream-aligned compiler error code.
    pub code:    CompilerErrorCode,
    /// Source span where parsing failed.
    pub span:    Span,
    /// Human-readable error message.
    pub message: String,
}

impl ParserError {
    pub(super) fn new(code: CompilerErrorCode, span: Span, message: impl Into<String>) -> Self {
        Self {
            code,
            span,
            message: message.into(),
        }
    }
}

impl fmt::Display for ParserError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.message, self.code.code())
    }
}

impl Error for ParserError {}

/// One parser failure.
#[derive(Debug)]
pub enum ParseError {
    /// Lexing failed before parsing could begin.
    Lex(LexerError),
    /// Syntactic parsing failed.
    Parse(ParserError),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Lex(error) => error.fmt(f),
            Self::Parse(error) => error.fmt(f),
        }
    }
}

impl Error for ParseError {}

impl From<LexerError> for ParseError {
    fn from(value: LexerError) -> Self {
        Self::Lex(value)
    }
}

impl From<ParserError> for ParseError {
    fn from(value: ParserError) -> Self {
        Self::Parse(value)
    }
}

/// One parser failure after source resolution and preprocessing.
#[derive(Debug)]
pub enum ResolvedParseError {
    /// Source loading or preprocessing failed.
    Preprocess(PreprocessError),
    /// Parsing failed after preprocessing.
    Parse(ParserError),
}

impl fmt::Display for ResolvedParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Preprocess(error) => error.fmt(f),
            Self::Parse(error) => error.fmt(f),
        }
    }
}

impl Error for ResolvedParseError {}

impl From<PreprocessError> for ResolvedParseError {
    fn from(value: PreprocessError) -> Self {
        Self::Preprocess(value)
    }
}

impl From<ParserError> for ResolvedParseError {
    fn from(value: ParserError) -> Self {
        Self::Parse(value)
    }
}

/// Parses one already-tokenized `NWScript` translation unit.
///
/// # Errors
///
/// Returns [`ParserError`] if the token stream is syntactically invalid.
pub fn parse_tokens(
    tokens: Vec<Token>,
    langspec: Option<&LangSpec>,
) -> Result<Script, ParserError> {
    parse_tokens_with_context(
        tokens,
        langspec,
        crate::CompilerResourceLimits::default(),
        None,
    )
}

/// Parses tokens with explicit resource bounds.
///
/// # Errors
///
/// Returns a parser diagnostic when syntax is invalid or a configured bound
/// is exceeded.
pub fn parse_tokens_with_limits(
    tokens: Vec<Token>,
    langspec: Option<&LangSpec>,
    limits: crate::CompilerResourceLimits,
) -> Result<Script, ParserError> {
    parse_tokens_with_context(tokens, langspec, limits, None)
}

/// Lexes and parses one source file.
///
/// # Errors
///
/// Returns [`ParseError`] if lexing or parsing fails.
pub fn parse_source(
    source: &SourceFile,
    langspec: Option<&LangSpec>,
) -> Result<Script, ParseError> {
    let limits = crate::CompilerResourceLimits::default();
    let tokens = lex_source_with_limits_and_cancellation(source, limits, None)?;
    parse_tokens_with_limits(tokens, langspec, limits).map_err(ParseError::from)
}

/// Lexes and parses a byte buffer associated with `source_id`.
///
/// # Errors
///
/// Returns [`ParseError`] if lexing or parsing fails.
pub fn parse_bytes(
    source_id: SourceId,
    input: &[u8],
    langspec: Option<&LangSpec>,
) -> Result<Script, ParseError> {
    let limits = crate::CompilerResourceLimits::default();
    if input.len() > limits.max_source_file_bytes {
        return Err(ParseError::Lex(LexerError {
            code:    CompilerErrorCode::UnknownStateInCompiler,
            span:    Span::new(source_id, 0, input.len()),
            message: format!(
                "source buffer contains {} bytes, exceeding configured maximum {}",
                input.len(),
                limits.max_source_file_bytes
            ),
        }));
    }
    let source = SourceFile::new(source_id, "<memory>", input.to_vec());
    parse_source(&source, langspec)
}

/// Lexes and parses a text buffer associated with `source_id`.
///
/// # Errors
///
/// Returns [`ParseError`] if lexing or parsing fails.
pub fn parse_text(
    source_id: SourceId,
    input: &str,
    langspec: Option<&LangSpec>,
) -> Result<Script, ParseError> {
    let bytes = nwnrs_types::encoding::encode_legacy(
        nwnrs_types::encoding::LegacyEncoding::Windows1252,
        input,
    )
    .map_err(|error| {
        ParseError::Lex(LexerError {
            code:    CompilerErrorCode::UnexpectedCharacter,
            span:    Span::new(source_id, 0, input.len()),
            message: format!("NWScript source is not representable as Windows-1252: {error}"),
        })
    })?;
    parse_bytes(source_id, &bytes, langspec)
}

/// Parses one already-loaded source bundle after include traversal and macro
/// expansion.
///
/// # Errors
///
/// Returns [`ResolvedParseError`] if preprocessing or parsing fails.
pub fn parse_source_bundle(
    bundle: &crate::SourceBundle,
    langspec: Option<&LangSpec>,
) -> Result<Script, ResolvedParseError> {
    let preprocessed = preprocess_source_bundle(bundle)?;
    parse_tokens_with_limits(preprocessed.tokens, langspec, bundle.resource_limits)
        .map_err(ResolvedParseError::from)
}

/// Parses one source bundle with cooperative preprocessing and phase
/// cancellation.
///
/// # Errors
///
/// Returns [`ResolvedParseError`] for ordinary parser failures or cancellation.
pub fn parse_source_bundle_with_cancellation(
    bundle: &crate::SourceBundle,
    langspec: Option<&LangSpec>,
    cancellation: &crate::CancellationToken,
) -> Result<Script, ResolvedParseError> {
    let preprocessed = crate::preprocess_source_bundle_with_cancellation(bundle, cancellation)?;
    cancellation
        .check()
        .map_err(crate::PreprocessError::from)
        .map_err(ResolvedParseError::from)?;
    let script = parse_bundle_tokens(
        preprocessed.tokens,
        langspec,
        bundle.resource_limits,
        cancellation,
    )?;
    cancellation
        .check()
        .map_err(crate::PreprocessError::from)
        .map_err(ResolvedParseError::from)?;
    Ok(script)
}

/// Parses one source bundle with caller-provided built-in macros and expansion
/// limits.
///
/// Source-defined declarative and procedural macros are collected into
/// `registry` during preprocessing, so callers can retain the resulting
/// definitions for later compilation units.
///
/// # Errors
///
/// Returns [`ResolvedParseError`] if preprocessing, macro expansion, or parsing
/// fails.
pub fn parse_source_bundle_with_macros(
    bundle: &crate::SourceBundle,
    langspec: Option<&LangSpec>,
    registry: &mut crate::MacroRegistry,
    options: crate::MacroExpansionOptions,
) -> Result<Script, ResolvedParseError> {
    let preprocessed = crate::preprocess_source_bundle_with_macros(bundle, registry, options)?;
    parse_tokens_with_limits(preprocessed.tokens, langspec, bundle.resource_limits)
        .map_err(ResolvedParseError::from)
}

/// Parses one source bundle with compiler macros and cooperative cancellation.
///
/// # Errors
///
/// Returns [`ResolvedParseError`] for ordinary parser failures or cancellation.
pub fn parse_source_bundle_with_macros_and_cancellation(
    bundle: &crate::SourceBundle,
    langspec: Option<&LangSpec>,
    registry: &mut crate::MacroRegistry,
    options: crate::MacroExpansionOptions,
    cancellation: &crate::CancellationToken,
) -> Result<Script, ResolvedParseError> {
    let preprocessed = crate::preprocess_source_bundle_with_macros_and_cancellation(
        bundle,
        registry,
        options,
        cancellation,
    )?;
    cancellation
        .check()
        .map_err(crate::PreprocessError::from)
        .map_err(ResolvedParseError::from)?;
    let script = parse_bundle_tokens(
        preprocessed.tokens,
        langspec,
        bundle.resource_limits,
        cancellation,
    )?;
    cancellation
        .check()
        .map_err(crate::PreprocessError::from)
        .map_err(ResolvedParseError::from)?;
    Ok(script)
}

/// Resolves, preprocesses, and parses one named root script.
///
/// # Errors
///
/// Returns [`ResolvedParseError`] if resolution, preprocessing, or parsing
/// fails.
pub fn parse_resolved_script<R: crate::ScriptResolver + ?Sized>(
    resolver: &R,
    root_name: &str,
    options: crate::SourceLoadOptions,
    langspec: Option<&LangSpec>,
) -> Result<Script, ResolvedParseError> {
    let bundle = crate::load_source_bundle(resolver, root_name, options)?;
    parse_source_bundle(&bundle, langspec)
}

pub(crate) fn parse_tokens_with_context<'a>(
    tokens: Vec<Token>,
    langspec: Option<&'a LangSpec>,
    limits: crate::CompilerResourceLimits,
    cancellation: Option<&'a crate::CancellationToken>,
) -> Result<Script, ParserError> {
    let fallback_span = tokens
        .first()
        .map_or_else(|| Span::new(SourceId::new(0), 0, 0), |token| token.span);
    limits.validate().map_err(|message| {
        ParserError::new(
            CompilerErrorCode::UnknownStateInCompiler,
            fallback_span,
            message,
        )
    })?;
    if tokens.len() > limits.max_tokens {
        return Err(ParserError::new(
            CompilerErrorCode::UnknownStateInCompiler,
            fallback_span,
            format!(
                "parser input contains {} tokens, exceeding configured maximum {}",
                tokens.len(),
                limits.max_tokens
            ),
        ));
    }
    let mut delimiter_depth = 0_usize;
    for (index, token) in tokens.iter().enumerate() {
        if index & 0x0fff == 0 && cancellation.is_some_and(crate::CancellationToken::is_cancelled) {
            return Err(ParserError::new(
                CompilerErrorCode::UnknownStateInCompiler,
                token.span,
                "operation cancelled",
            ));
        }
        match token.kind {
            TokenKind::LeftBrace | TokenKind::LeftParen | TokenKind::LeftSquareBracket => {
                delimiter_depth = delimiter_depth.saturating_add(1);
                if delimiter_depth > limits.max_syntax_nesting {
                    return Err(ParserError::new(
                        CompilerErrorCode::UnknownStateInCompiler,
                        token.span,
                        format!(
                            "delimiter nesting exceeded configured maximum of {}",
                            limits.max_syntax_nesting
                        ),
                    ));
                }
            }
            TokenKind::RightBrace | TokenKind::RightParen | TokenKind::RightSquareBracket => {
                delimiter_depth = delimiter_depth.saturating_sub(1);
            }
            _ => {}
        }
    }
    Parser::new(tokens, langspec, limits, cancellation).parse_script()
}

fn parse_bundle_tokens(
    tokens: Vec<Token>,
    langspec: Option<&LangSpec>,
    limits: crate::CompilerResourceLimits,
    cancellation: &crate::CancellationToken,
) -> Result<Script, ResolvedParseError> {
    match parse_tokens_with_context(tokens, langspec, limits, Some(cancellation)) {
        Ok(script) => Ok(script),
        Err(_error) if cancellation.is_cancelled() => Err(ResolvedParseError::Preprocess(
            PreprocessError::Cancelled(crate::Cancelled),
        )),
        Err(error) => Err(ResolvedParseError::Parse(error)),
    }
}
