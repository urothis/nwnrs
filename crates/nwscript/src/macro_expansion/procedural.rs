use super::*;

/// Built-in identity macro used to validate and bootstrap the expansion host.
#[derive(Debug, Clone, Copy, Default)]
pub struct IdentityMacro;

impl BangMacro for IdentityMacro {
    fn expand(
        &self,
        invocation: &MacroInvocation,
        _context: MacroContext<'_>,
    ) -> Result<MacroOutput, MacroExpansionError> {
        Ok(MacroOutput::expanded(invocation.input.clone()))
    }
}

/// Built-in compiler macro that lowers `quote!{...}` to tokenstream runtime
/// construction calls.
///
/// `$name` inserts a compiler-time `tokenstream` variable. Rust-style repeated
/// interpolation accepts `tokenstream_list` values, including nested lists.
/// Static syntax is passed to [`QUOTE_STATIC_FUNCTION`] and combined using
/// [`QUOTE_CONCAT_FUNCTION`].
#[derive(Debug, Clone, Copy, Default)]
pub struct QuoteMacro;

impl BangMacro for QuoteMacro {
    fn expand(
        &self,
        invocation: &MacroInvocation,
        _context: MacroContext<'_>,
    ) -> Result<MacroOutput, MacroExpansionError> {
        lower_quote_expression(&invocation.input, invocation.span).map(MacroOutput::expanded)
    }
}

/// Registers the built-in macros used while compiling procedural macro
/// implementations.
///
/// # Errors
///
/// Returns an error only if one of the compiler-owned macro paths is invalid.
pub fn register_compiler_macros(registry: &mut MacroRegistry) -> Result<(), MacroExpansionError> {
    registry.register_path("quote", QuoteMacro)?;
    Ok(())
}

/// One procedural bang macro implemented and executed in `NWScript`.
///
/// The entry function must have the exact shape
/// `tokenstream Entry(tokenstream input)`. Its translation unit may contain
/// ordinary helper functions and source-defined declarative macros. Calls to
/// [`QuoteMacro`] are lowered before the unit is passed to the normal compiler.
#[derive(Debug, Clone)]
pub struct NwScriptMacro {
    entry: String,
    ncs:   Vec<u8>,
    ndb:   crate::Ndb,
}

impl NwScriptMacro {
    /// Compiles one procedural macro implementation from `NWScript` source.
    ///
    /// # Errors
    ///
    /// Returns an expansion diagnostic when the source cannot be lexed,
    /// expanded, parsed, compiled, or does not expose the required entry
    /// function signature.
    pub fn compile(
        source_name: &str,
        entry: &str,
        source: &str,
    ) -> Result<Self, MacroExpansionError> {
        let source_bytes = nwnrs_types::encoding::encode_legacy(
            nwnrs_types::encoding::LegacyEncoding::Windows1252,
            source,
        )
        .map_err(|error| {
            MacroExpansionError::without_span(format!(
                "procedural macro {source_name:?} is not representable as Windows-1252: {error}"
            ))
        })?;
        Self::compile_bytes_impl(source_name, entry, &source_bytes, None)
    }

    /// Compiles one procedural macro from exact Windows-1252 source bytes.
    ///
    /// # Errors
    ///
    /// Returns an expansion diagnostic when the source or macro entry is
    /// invalid.
    pub fn compile_bytes(
        source_name: &str,
        entry: &str,
        source: &[u8],
    ) -> Result<Self, MacroExpansionError> {
        Self::compile_bytes_impl(source_name, entry, source, None)
    }

    fn compile_bytes_impl(
        source_name: &str,
        entry: &str,
        source: &[u8],
        cancellation: Option<&crate::CancellationToken>,
    ) -> Result<Self, MacroExpansionError> {
        check_macro_cancellation(cancellation)?;
        if !valid_identifier(entry) {
            return Err(MacroExpansionError::without_span(format!(
                "invalid procedural macro entry function {entry:?}"
            )));
        }
        let source_id = SourceId::new(0);
        let source_bytes = source.to_vec();
        let source_file = crate::SourceFile::new(source_id, source_name, source_bytes.clone());
        let limits = crate::CompilerResourceLimits::default();
        let tokens =
            crate::lex_source_with_limits_and_cancellation(&source_file, limits, cancellation)
                .map_err(|error| {
                    MacroExpansionError::without_span(format!(
                        "failed to lex procedural macro {source_name:?}: {error}"
                    ))
                })?;
        let mut registry = MacroRegistry::new();
        register_compiler_macros(&mut registry)?;
        let tokens = match cancellation {
            Some(cancellation) => expand_source_macros_with_cancellation(
                tokens,
                &mut registry,
                MacroExpansionOptions::default(),
                cancellation,
            )?,
            None => expand_source_macros(tokens, &mut registry, MacroExpansionOptions::default())?,
        };
        check_macro_cancellation(cancellation)?;
        let langspec = crate::parse_langspec("nwnrs_macro", MACRO_LANGSPEC).map_err(|error| {
            MacroExpansionError::without_span(format!(
                "failed to load the procedural macro language specification: {error}"
            ))
        })?;
        let script =
            crate::parser::parse_tokens_with_context(tokens, Some(&langspec), limits, cancellation)
                .map_err(|error| {
                    MacroExpansionError::without_span(format!(
                        "failed to parse procedural macro {source_name:?}: {error}"
                    ))
                })?;
        let mut source_map = crate::SourceMap::new();
        let root_id = source_map.add_file(source_name.to_string(), source_bytes);
        let artifacts = cancellation
            .map_or_else(
                || {
                    crate::compile_script_with_source_map(
                        &script,
                        &source_map,
                        root_id,
                        Some(&langspec),
                        crate::CompileOptions::default(),
                    )
                },
                |cancellation| {
                    crate::codegen::compile_script_with_cancellation(
                        &script,
                        Some(&source_map),
                        Some(root_id),
                        Some(&langspec),
                        crate::CompileOptions::default(),
                        cancellation,
                    )
                },
            )
            .map_err(|error| {
                MacroExpansionError::without_span(format!(
                    "failed to compile procedural macro {source_name:?}: {error}"
                ))
            })?;
        let Some(ndb_bytes) = artifacts.ndb else {
            return Err(MacroExpansionError::without_span(format!(
                "procedural macro {source_name:?} did not emit required NDB metadata"
            )));
        };
        let ndb = match cancellation {
            Some(cancellation) => crate::read_ndb_with_limits_and_cancellation(
                &mut Cursor::new(ndb_bytes),
                crate::NdbParseLimits::default(),
                cancellation,
            ),
            None => crate::read_ndb(&mut Cursor::new(ndb_bytes)),
        }
        .map_err(|error| {
            MacroExpansionError::without_span(format!(
                "failed to read procedural macro metadata for {source_name:?}: {error}"
            ))
        })?;
        validate_macro_entry(source_name, entry, &ndb)?;
        Ok(Self {
            entry: entry.to_string(),
            ncs: artifacts.ncs,
            ndb,
        })
    }
}

impl BangMacro for NwScriptMacro {
    fn expand(
        &self,
        invocation: &MacroInvocation,
        context: MacroContext<'_>,
    ) -> Result<MacroOutput, MacroExpansionError> {
        let arena = Rc::new(RefCell::new(
            TokenStreamArena::with_input(invocation.input.clone(), context.options())
                .map_err(|error| MacroExpansionError::new(invocation.span, error.to_string()))?,
        ));
        let mut vm = macro_vm(Rc::clone(&arena), GENERATED_MACRO_SOURCE_ID);
        if let Some(cancellation) = context.cancellation() {
            vm.set_cancellation_token(cancellation.clone());
        }
        let runtime = vm
            .run_function_bytes_with_options(
                &self.ncs,
                format!("macro {}", self.entry),
                &self.ndb,
                &self.entry,
                &[crate::VmValue::EngineStructure {
                    index: MACRO_TOKENSTREAM_INDEX,
                    value: crate::VmEngineStructureValue::Word(0),
                }],
                crate::VmRunOptions {
                    max_instructions:    Some(DEFAULT_PROCEDURAL_MACRO_INSTRUCTION_LIMIT),
                    max_recursion_depth: Some(DEFAULT_PROCEDURAL_MACRO_RECURSION_LIMIT),
                    max_stack_cells:     Some(DEFAULT_PROCEDURAL_MACRO_STACK_LIMIT),
                },
            )
            .map_err(|error| match error {
                crate::VmError::MacroDiagnostic {
                    span,
                    message,
                } => MacroExpansionError::new(
                    if span.source_id == GENERATED_MACRO_SOURCE_ID {
                        invocation.span
                    } else {
                        span
                    },
                    message,
                ),
                error => MacroExpansionError::new(
                    invocation.span,
                    format!("procedural macro `{}` failed: {error}", invocation.path),
                ),
            })?;
        let output = runtime
            .function_return_value(&self.ndb, &self.entry)
            .map_err(|error| {
                MacroExpansionError::new(
                    invocation.span,
                    format!(
                        "could not read procedural macro `{}` output: {error}",
                        invocation.path
                    ),
                )
            })?;
        let Some(crate::VmValue::EngineStructure {
            index: MACRO_TOKENSTREAM_INDEX,
            value: crate::VmEngineStructureValue::Word(handle),
        }) = output
        else {
            return Err(MacroExpansionError::new(
                invocation.span,
                format!(
                    "procedural macro `{}` returned an invalid tokenstream value",
                    invocation.path
                ),
            ));
        };
        let mut stream = arena.borrow().balanced(handle).map_err(|error| {
            MacroExpansionError::new(
                invocation.span,
                format!(
                    "procedural macro `{}` returned invalid tokens: {error}",
                    invocation.path
                ),
            )
        })?;
        rebase_source_id(&mut stream, GENERATED_MACRO_SOURCE_ID, invocation.span);
        Ok(MacroOutput::expanded(stream))
    }
}

/// Compiles and registers one source-implemented procedural macro.
///
/// # Errors
///
/// Returns an expansion diagnostic if `path` is invalid or the implementation
/// cannot be compiled and validated.
pub fn register_nwscript_macro(
    registry: &mut MacroRegistry,
    path: &str,
    source_name: &str,
    entry: &str,
    source: &str,
) -> Result<(), MacroExpansionError> {
    let implementation = NwScriptMacro::compile(source_name, entry, source)?;
    registry.register_path(path, implementation)
}

/// Collects top-level `proc_macro! path { ... }` definitions, removes them
/// from `stream`, and registers their compiled `NWScript` implementations.
///
/// The function named by the final path segment is the implementation entry
/// point. For example, `proc_macro! project::wrap { tokenstream
/// wrap(tokenstream input) { ... } }` registers `project::wrap!`.
///
/// # Errors
///
/// Returns an expansion diagnostic for malformed definitions, duplicate paths,
/// or implementation compile and signature failures.
pub fn collect_nwscript_macros(
    stream: &mut NwTokenStream,
    registry: &mut MacroRegistry,
) -> Result<(), MacroExpansionError> {
    collect_nwscript_macros_impl(stream, registry, None)
}

pub(crate) fn collect_nwscript_macros_with_cancellation(
    stream: &mut NwTokenStream,
    registry: &mut MacroRegistry,
    cancellation: &crate::CancellationToken,
) -> Result<(), MacroExpansionError> {
    collect_nwscript_macros_impl(stream, registry, Some(cancellation))
}

fn collect_nwscript_macros_impl(
    stream: &mut NwTokenStream,
    registry: &mut MacroRegistry,
    cancellation: Option<&crate::CancellationToken>,
) -> Result<(), MacroExpansionError> {
    let mut output = Vec::new();
    let mut position = 0;
    while position < stream.trees.len() {
        check_macro_cancellation(cancellation)?;
        let Some(NwTokenTree::Token(keyword)) = stream.trees.get(position) else {
            if let Some(tree) = stream.trees.get(position).cloned() {
                output.push(tree);
            }
            position += 1;
            continue;
        };
        if keyword.kind != TokenKind::Identifier || keyword.text != "proc_macro" {
            output.push(NwTokenTree::Token(keyword.clone()));
            position += 1;
            continue;
        }
        let Some(NwTokenTree::Token(bang)) = stream.trees.get(position + 1) else {
            return Err(MacroExpansionError::new(
                keyword.span,
                "`proc_macro` must be followed by `!`, a path, and a body",
            ));
        };
        if bang.kind != TokenKind::BooleanNot {
            return Err(MacroExpansionError::new(
                bang.span,
                "expected `!` after `proc_macro`",
            ));
        }

        let path_start = position + 2;
        let (path, body_position) = parse_definition_path(&stream.trees, path_start)?;
        let Some(NwTokenTree::Group(body)) = stream.trees.get(body_position) else {
            return Err(MacroExpansionError::new(
                keyword.span,
                "procedural macro path must be followed by a braced implementation",
            ));
        };
        if body.delimiter != NwDelimiter::Brace {
            return Err(MacroExpansionError::new(
                body.span(),
                "procedural macro implementation must use braces",
            ));
        }
        if registry.contains(&path) {
            return Err(MacroExpansionError::new(
                keyword.span,
                format!("macro `{path}` is already defined"),
            ));
        }
        let Some(entry) = path.segments.last() else {
            return Err(MacroExpansionError::new(
                keyword.span,
                "procedural macro path is empty",
            ));
        };
        let source_name = format!("{path}.macro.nss");
        let source = crate::render_nwscript_tokens_bytes(&body.stream);
        let implementation =
            NwScriptMacro::compile_bytes_impl(&source_name, entry, &source, cancellation)?;
        registry.register(path, implementation);
        position = body_position + 1;
        if let Some(NwTokenTree::Token(semicolon)) = stream.trees.get(position)
            && semicolon.kind == TokenKind::Semicolon
        {
            position += 1;
        }
    }
    stream.trees = output;
    Ok(())
}

pub(super) fn check_macro_cancellation(
    cancellation: Option<&crate::CancellationToken>,
) -> Result<(), MacroExpansionError> {
    if cancellation.is_some_and(crate::CancellationToken::is_cancelled) {
        return Err(MacroExpansionError::without_span("operation cancelled"));
    }
    Ok(())
}

fn parse_definition_path(
    trees: &[NwTokenTree],
    start: usize,
) -> Result<(MacroPath, usize), MacroExpansionError> {
    let Some(NwTokenTree::Token(first)) = trees.get(start) else {
        return Err(MacroExpansionError::without_span(
            "procedural macro definition requires a path",
        ));
    };
    if first.kind != TokenKind::Identifier {
        return Err(MacroExpansionError::new(
            first.span,
            "procedural macro path must begin with an identifier",
        ));
    }
    let mut segments = vec![first.text.clone()];
    let mut position = start + 1;
    while let (
        Some(NwTokenTree::Token(first_colon)),
        Some(NwTokenTree::Token(second_colon)),
        Some(NwTokenTree::Token(segment)),
    ) = (
        trees.get(position),
        trees.get(position + 1),
        trees.get(position + 2),
    ) {
        if first_colon.kind != TokenKind::Colon
            || second_colon.kind != TokenKind::Colon
            || segment.kind != TokenKind::Identifier
        {
            break;
        }
        segments.push(segment.text.clone());
        position += 3;
    }
    Ok((MacroPath::new(segments)?, position))
}

fn validate_macro_entry(
    source_name: &str,
    entry: &str,
    ndb: &crate::Ndb,
) -> Result<(), MacroExpansionError> {
    let Some(function) = ndb
        .functions
        .iter()
        .find(|function| function.label == entry)
    else {
        return Err(MacroExpansionError::without_span(format!(
            "procedural macro {source_name:?} has no entry function {entry:?}"
        )));
    };
    let tokenstream = crate::NdbType::EngineStructure(MACRO_TOKENSTREAM_INDEX);
    if function.return_type != tokenstream || function.args.as_slice() != [tokenstream] {
        return Err(MacroExpansionError::without_span(format!(
            "procedural macro entry {entry:?} must have signature `tokenstream \
             {entry}(tokenstream input)`"
        )));
    }
    Ok(())
}

fn lower_quote_expression(
    template: &NwTokenStream,
    invocation_span: Span,
) -> Result<NwTokenStream, MacroExpansionError> {
    let tokens = template.clone().into_tokens();
    let mut parts = Vec::new();
    let mut static_tokens = Vec::new();
    let mut position = 0;

    while let Some(token) = tokens.get(position) {
        if token.kind != TokenKind::Dollar {
            static_tokens.push(token.clone());
            position += 1;
            continue;
        }

        let Some(next) = tokens.get(position + 1) else {
            return Err(MacroExpansionError::new(
                token.span,
                "`$` at the end of `quote!` has no interpolation target",
            ));
        };
        match next.kind {
            TokenKind::Dollar => {
                static_tokens.push(token.clone());
                position += 2;
            }
            TokenKind::Identifier => {
                push_static_quote_part(&mut parts, &mut static_tokens, invocation_span);
                parts.push(NwTokenTree::Token(next.clone()));
                position += 2;
            }
            TokenKind::LeftParen | TokenKind::LeftSquareBracket | TokenKind::LeftBrace => {
                push_static_quote_part(&mut parts, &mut static_tokens, invocation_span);
                let end = flat_group_end(&tokens, position + 1).ok_or_else(|| {
                    MacroExpansionError::new(next.span, "unclosed compiler-time quote repetition")
                })?;
                let Some((separator, quantifier, suffix)) =
                    flat_repetition_suffix(&tokens, end.saturating_add(1))
                else {
                    return Err(MacroExpansionError::new(
                        next.span,
                        "compiler-time quote repetition requires `*`, `+`, or `?`",
                    ));
                };
                if quantifier == TokenKind::QuestionMark && separator.is_some() {
                    return Err(MacroExpansionError::new(
                        next.span,
                        "`?` compiler-time quote repetition cannot have a separator",
                    ));
                }
                let repeated_tokens = tokens.get(position + 2..end).ok_or_else(|| {
                    MacroExpansionError::new(next.span, "invalid quote repetition")
                })?;
                let names = repeated_quote_names(repeated_tokens);
                if names.is_empty() {
                    return Err(MacroExpansionError::new(
                        next.span,
                        "compiler-time quote repetition contains no tokenstream-list binding",
                    ));
                }
                let mut binding_expression =
                    function_call("__NWNRS_QuoteBindingsNew", Vec::new(), invocation_span);
                for name in names {
                    binding_expression = function_call(
                        "__NWNRS_QuoteBindingsInsert",
                        vec![
                            binding_expression,
                            punctuation(TokenKind::Comma, ",", invocation_span),
                            NwTokenTree::Token(Token::new(
                                TokenKind::String,
                                invocation_span,
                                name.clone(),
                            )),
                            punctuation(TokenKind::Comma, ",", invocation_span),
                            NwTokenTree::Token(Token::new(
                                TokenKind::Identifier,
                                invocation_span,
                                name,
                            )),
                        ],
                        invocation_span,
                    );
                }
                let quantifier_value = match quantifier {
                    TokenKind::Multiply => "0",
                    TokenKind::Plus => "1",
                    TokenKind::QuestionMark => "2",
                    _ => {
                        return Err(MacroExpansionError::new(
                            next.span,
                            "invalid compiler-time quote repetition quantifier",
                        ));
                    }
                };
                parts.push(function_call(
                    QUOTE_REPEAT_FUNCTION,
                    vec![
                        NwTokenTree::Token(Token::new(
                            TokenKind::String,
                            invocation_span,
                            render_flat_tokens(repeated_tokens),
                        )),
                        punctuation(TokenKind::Comma, ",", invocation_span),
                        binding_expression,
                        punctuation(TokenKind::Comma, ",", invocation_span),
                        NwTokenTree::Token(Token::new(
                            TokenKind::String,
                            invocation_span,
                            separator.map_or_else(String::new, |token| token.text),
                        )),
                        punctuation(TokenKind::Comma, ",", invocation_span),
                        NwTokenTree::Token(Token::new(
                            TokenKind::Integer,
                            invocation_span,
                            quantifier_value,
                        )),
                    ],
                    invocation_span,
                ));
                position = end.saturating_add(1).saturating_add(suffix);
            }
            _ => {
                return Err(MacroExpansionError::new(
                    next.span,
                    "`$` in `quote!` must be followed by a tokenstream variable or another `$`",
                ));
            }
        }
    }
    push_static_quote_part(&mut parts, &mut static_tokens, invocation_span);

    let mut parts = parts.into_iter();
    let Some(mut expression) = parts.next() else {
        return Ok(NwTokenStream::from_trees(vec![function_call(
            QUOTE_EMPTY_FUNCTION,
            Vec::new(),
            invocation_span,
        )]));
    };
    for part in parts {
        expression = function_call(
            QUOTE_CONCAT_FUNCTION,
            vec![
                expression,
                punctuation(TokenKind::Comma, ",", invocation_span),
                part,
            ],
            invocation_span,
        );
    }
    Ok(NwTokenStream::from_trees(vec![expression]))
}

fn flat_group_end(tokens: &[Token], open: usize) -> Option<usize> {
    let open_kind = &tokens.get(open)?.kind;
    let close_kind = match open_kind {
        TokenKind::LeftParen => TokenKind::RightParen,
        TokenKind::LeftSquareBracket => TokenKind::RightSquareBracket,
        TokenKind::LeftBrace => TokenKind::RightBrace,
        _ => return None,
    };
    let mut depth = 0_usize;
    for (offset, token) in tokens.get(open..)?.iter().enumerate() {
        if token.kind == *open_kind {
            depth = depth.saturating_add(1);
        } else if token.kind == close_kind {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(open + offset);
            }
        }
    }
    None
}

fn flat_repetition_suffix(
    tokens: &[Token],
    position: usize,
) -> Option<(Option<Token>, TokenKind, usize)> {
    let first = tokens.get(position)?;
    if matches!(
        first.kind,
        TokenKind::Multiply | TokenKind::Plus | TokenKind::QuestionMark
    ) {
        return Some((None, first.kind.clone(), 1));
    }
    let quantifier = tokens.get(position + 1)?;
    if !matches!(
        quantifier.kind,
        TokenKind::Multiply | TokenKind::Plus | TokenKind::QuestionMark
    ) {
        return None;
    }
    Some((Some(first.clone()), quantifier.kind.clone(), 2))
}

fn repeated_quote_names(tokens: &[Token]) -> Vec<String> {
    let mut names = std::collections::BTreeSet::new();
    let mut position = 0;
    while position < tokens.len() {
        if tokens
            .get(position)
            .is_some_and(|token| token.kind == TokenKind::Dollar)
            && let Some(next) = tokens.get(position + 1)
        {
            if next.kind == TokenKind::Identifier {
                names.insert(next.text.clone());
            }
            position += 2;
            continue;
        }
        position += 1;
    }
    names.into_iter().collect()
}

fn push_static_quote_part(
    parts: &mut Vec<NwTokenTree>,
    static_tokens: &mut Vec<Token>,
    span: Span,
) {
    if static_tokens.is_empty() {
        return;
    }
    let rendered = render_flat_tokens(static_tokens);
    static_tokens.clear();
    parts.push(function_call(
        QUOTE_STATIC_FUNCTION,
        vec![NwTokenTree::Token(Token::new(
            TokenKind::String,
            span,
            rendered,
        ))],
        span,
    ));
}

fn function_call(name: &str, arguments: Vec<NwTokenTree>, span: Span) -> NwTokenTree {
    NwTokenTree::Group(NwTokenGroup {
        delimiter:  NwDelimiter::Parenthesis,
        open_span:  span,
        close_span: span,
        stream:     NwTokenStream::from_trees(vec![
            NwTokenTree::Token(Token::new(TokenKind::Identifier, span, name)),
            NwTokenTree::Group(NwTokenGroup {
                delimiter:  NwDelimiter::Parenthesis,
                open_span:  span,
                close_span: span,
                stream:     NwTokenStream::from_trees(arguments),
            }),
        ]),
    })
}

fn punctuation(kind: TokenKind, text: &str, span: Span) -> NwTokenTree {
    NwTokenTree::Token(Token::new(kind, span, text))
}

pub(super) fn render_flat_tokens(tokens: &[Token]) -> String {
    render_flat_token_bytes(tokens)
        .into_iter()
        .map(char::from)
        .collect()
}

pub(super) fn render_flat_token_bytes(tokens: &[Token]) -> Vec<u8> {
    let mut rendered = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        let follows_attribute_hash = tokens
            .get(index.saturating_sub(1))
            .is_some_and(|previous| previous.kind == TokenKind::Hash);
        if index > 0 && !rendered.ends_with(b"\n") && !follows_attribute_hash {
            rendered.push(b' ');
        }
        match token.kind {
            TokenKind::String => render_string_literal(&mut rendered, &token.text),
            _ => rendered.extend(token_text_bytes(&token.text)),
        }
        if matches!(token.kind, TokenKind::Keyword(crate::Keyword::Include)) {
            // The included path is the next token. Its terminating newline is
            // emitted after that string below.
        } else if token.kind == TokenKind::String
            && index > 0
            && tokens.get(index - 1).is_some_and(|previous| {
                matches!(previous.kind, TokenKind::Keyword(crate::Keyword::Include))
            })
        {
            rendered.push(b'\n');
        }
    }
    rendered
}

fn render_string_literal(output: &mut Vec<u8>, value: &str) {
    output.push(b'"');
    for byte in token_text_bytes(value) {
        match byte {
            b'\n' => output.extend_from_slice(b"\\n"),
            b'\\' => output.extend_from_slice(b"\\\\"),
            b'"' => output.extend_from_slice(b"\\\""),
            byte if byte.is_ascii_control() || !byte.is_ascii() => {
                output.extend_from_slice(format!("\\x{byte:02x}").as_bytes());
            }
            byte => output.push(byte),
        }
    }
    output.push(b'"');
}

fn token_text_bytes(value: &str) -> impl Iterator<Item = u8> + '_ {
    value
        .chars()
        .map(|character| u8::try_from(u32::from(character)).unwrap_or(b'?'))
}
