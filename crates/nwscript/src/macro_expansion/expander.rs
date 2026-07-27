use super::*;

/// Expands registered bang macros in an already lexed token stream.
///
/// # Errors
///
/// Returns an error for invalid token-tree delimiters, unknown macros, macro
/// implementation failures, or resource-limit violations.
pub fn expand_bang_macros(
    tokens: Vec<Token>,
    registry: &MacroRegistry,
    options: MacroExpansionOptions,
) -> Result<Vec<Token>, MacroExpansionError> {
    let eof = tokens
        .last()
        .filter(|token| token.kind == TokenKind::Eof)
        .cloned()
        .unwrap_or_else(|| Token::new(TokenKind::Eof, Span::new(SourceId::new(0), 0, 0), ""));
    let limits = crate::CompilerResourceLimits {
        max_tokens: options.token_limit,
        max_macro_output_tokens: options.token_limit,
        ..crate::CompilerResourceLimits::default()
    };
    let stream = NwTokenStream::from_tokens_with_limits_and_cancellation(&tokens, limits, None)?;
    let mut expander = MacroExpander {
        registry,
        options,
        stack: Vec::new(),
        trace: false,
        traces: Vec::new(),
        cancellation: None,
    };
    let expanded = expander.expand_stream(stream)?;
    let mut flattened = expanded.into_tokens();
    flattened.push(eof);
    Ok(flattened)
}

/// Expands one already-balanced invocation through a registered macro while
/// preserving every input token's original source span.
///
/// This is used by project-wide compiler passes whose input is assembled from
/// multiple source files and therefore must not be rendered and re-lexed.
///
/// # Errors
///
/// Returns an error when the path is invalid or unregistered, the macro fails,
/// a nested expansion fails, or an expansion resource limit is exceeded.
pub fn expand_registered_macro(
    registry: &MacroRegistry,
    path: &str,
    input: NwTokenStream,
    invocation_span: Span,
    options: MacroExpansionOptions,
) -> Result<NwTokenStream, MacroExpansionError> {
    expand_registered_macro_impl(registry, path, input, invocation_span, options, None)
}

/// Expands one registered macro with cooperative cancellation.
///
/// # Errors
///
/// Returns a macro diagnostic or cancellation diagnostic.
pub fn expand_registered_macro_with_cancellation(
    registry: &MacroRegistry,
    path: &str,
    input: NwTokenStream,
    invocation_span: Span,
    options: MacroExpansionOptions,
    cancellation: &crate::CancellationToken,
) -> Result<NwTokenStream, MacroExpansionError> {
    expand_registered_macro_impl(
        registry,
        path,
        input,
        invocation_span,
        options,
        Some(cancellation),
    )
}

fn expand_registered_macro_impl(
    registry: &MacroRegistry,
    path: &str,
    input: NwTokenStream,
    invocation_span: Span,
    options: MacroExpansionOptions,
    cancellation: Option<&crate::CancellationToken>,
) -> Result<NwTokenStream, MacroExpansionError> {
    if cancellation.is_some_and(crate::CancellationToken::is_cancelled) {
        return Err(MacroExpansionError::new(
            invocation_span,
            "operation cancelled",
        ));
    }
    let path = MacroPath::parse(path)?;
    if options.max_depth == 0 {
        return Err(MacroExpansionError::new(
            invocation_span,
            "macro expansion maximum depth must be greater than zero",
        ));
    }
    let Some(implementation) = registry.macros.get(&path) else {
        return Err(MacroExpansionError::new(
            invocation_span,
            format!("unknown macro `{path}`"),
        ));
    };
    let stack = vec![path.clone()];
    let output = implementation
        .expand(
            &MacroInvocation {
                path: path.clone(),
                delimiter: NwDelimiter::Brace,
                input,
                span: invocation_span,
            },
            MacroContext {
                expansion_stack: &stack,
                options,
                cancellation,
            },
        )
        .map_err(|error| error.with_stack(&stack))?;
    let output = if output.recursively_expand {
        MacroExpander {
            registry,
            options,
            stack,
            trace: false,
            traces: Vec::new(),
            cancellation,
        }
        .expand_stream(output.tokens)?
    } else {
        output.tokens
    };
    if output.flattened_len() > options.token_limit {
        return Err(MacroExpansionError::new(
            invocation_span,
            format!(
                "macro expansion exceeded token limit of {}",
                options.token_limit
            ),
        ));
    }
    Ok(output)
}

/// Collects top-level `macro_rules!` definitions, removes them from `stream`,
/// and registers their bang macros.
///
/// The matcher language supports fixed tokens; identifier, literal, `tt`,
/// `expr`, and `tokens` fragments; and nested Rust-style repetition with
/// separators and the `*`, `+`, and `?` quantifiers. Expansion templates use
/// the same `$` quotation syntax as [`quote_nwscript`].
///
/// # Errors
///
/// Returns an error for malformed definitions, duplicate macro names, invalid
/// matcher fragments, or duplicate capture names.
pub fn collect_declarative_macros(
    stream: &mut NwTokenStream,
    registry: &mut MacroRegistry,
) -> Result<(), MacroExpansionError> {
    collect_declarative_macros_impl(stream, registry, None)
}

pub(crate) fn collect_declarative_macros_with_cancellation(
    stream: &mut NwTokenStream,
    registry: &mut MacroRegistry,
    cancellation: &crate::CancellationToken,
) -> Result<(), MacroExpansionError> {
    collect_declarative_macros_impl(stream, registry, Some(cancellation))
}

fn collect_declarative_macros_impl(
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
        if keyword.kind != TokenKind::Identifier || keyword.text != "macro_rules" {
            output.push(NwTokenTree::Token(keyword.clone()));
            position += 1;
            continue;
        }

        let Some(NwTokenTree::Token(bang)) = stream.trees.get(position + 1) else {
            return Err(MacroExpansionError::new(
                keyword.span,
                "`macro_rules` must be followed by `!`, a name, and a rule group",
            ));
        };
        let Some(NwTokenTree::Token(name)) = stream.trees.get(position + 2) else {
            return Err(MacroExpansionError::new(
                keyword.span,
                "`macro_rules!` requires a macro name",
            ));
        };
        let Some(NwTokenTree::Group(body)) = stream.trees.get(position + 3) else {
            return Err(MacroExpansionError::new(
                keyword.span,
                "`macro_rules!` requires a braced rule group",
            ));
        };
        if bang.kind != TokenKind::BooleanNot
            || name.kind != TokenKind::Identifier
            || body.delimiter != NwDelimiter::Brace
        {
            return Err(MacroExpansionError::new(
                keyword.span,
                "expected `macro_rules! name { ... }`",
            ));
        }
        let path = MacroPath::new([name.text.clone()])?;
        if registry.contains(&path) {
            return Err(MacroExpansionError::new(
                name.span,
                format!("macro `{path}` is already defined"),
            ));
        }
        let implementation = DeclarativeMacro::parse(&path, body)?;
        registry.register(path, implementation);
        position += 4;
        if let Some(NwTokenTree::Token(semicolon)) = stream.trees.get(position)
            && semicolon.kind == TokenKind::Semicolon
        {
            position += 1;
        }
    }
    stream.trees = output;
    Ok(())
}

/// Collects source-defined declarative macros and expands all remaining bang
/// macro invocations.
///
/// # Errors
///
/// Returns any token-tree, definition, invocation, implementation, or resource
/// limit diagnostic encountered during collection or expansion.
pub fn expand_source_macros(
    tokens: Vec<Token>,
    registry: &mut MacroRegistry,
    options: MacroExpansionOptions,
) -> Result<Vec<Token>, MacroExpansionError> {
    expand_source_macros_impl(tokens, registry, options, false, None)
        .map(|(tokens, _traces)| tokens)
}

pub(crate) fn expand_source_macros_with_cancellation(
    tokens: Vec<Token>,
    registry: &mut MacroRegistry,
    options: MacroExpansionOptions,
    cancellation: &crate::CancellationToken,
) -> Result<Vec<Token>, MacroExpansionError> {
    expand_source_macros_impl(tokens, registry, options, false, Some(cancellation))
        .map(|(tokens, _traces)| tokens)
}

/// Collects and expands source-defined macros while recording each invocation.
///
/// # Errors
///
/// Returns the same diagnostics as [`expand_source_macros`].
pub fn expand_source_macros_traced(
    tokens: Vec<Token>,
    registry: &mut MacroRegistry,
    options: MacroExpansionOptions,
) -> Result<(Vec<Token>, Vec<MacroExpansionTrace>), MacroExpansionError> {
    expand_source_macros_impl(tokens, registry, options, true, None)
}

fn expand_source_macros_impl(
    tokens: Vec<Token>,
    registry: &mut MacroRegistry,
    options: MacroExpansionOptions,
    trace: bool,
    cancellation: Option<&crate::CancellationToken>,
) -> Result<(Vec<Token>, Vec<MacroExpansionTrace>), MacroExpansionError> {
    let eof = tokens
        .last()
        .filter(|token| token.kind == TokenKind::Eof)
        .cloned()
        .unwrap_or_else(|| Token::new(TokenKind::Eof, Span::new(SourceId::new(0), 0, 0), ""));
    let limits = crate::CompilerResourceLimits {
        max_tokens: options.token_limit,
        max_macro_output_tokens: options.token_limit,
        ..crate::CompilerResourceLimits::default()
    };
    let mut stream =
        NwTokenStream::from_tokens_with_limits_and_cancellation(&tokens, limits, cancellation)?;
    match cancellation {
        Some(cancellation) => {
            collect_nwscript_macros_with_cancellation(&mut stream, registry, cancellation)?;
            collect_declarative_macros_with_cancellation(&mut stream, registry, cancellation)?;
        }
        None => {
            collect_nwscript_macros(&mut stream, registry)?;
            collect_declarative_macros(&mut stream, registry)?;
        }
    }
    let mut expander = MacroExpander {
        registry,
        options,
        stack: Vec::new(),
        trace,
        traces: Vec::new(),
        cancellation,
    };
    let mut expanded = expander.expand_stream(stream)?;
    erase_nwnrs_macro_attributes(&mut expanded)?;
    let traces = expander.traces;
    let mut flattened = expanded.into_tokens();
    flattened.push(eof);
    Ok((flattened, traces))
}

#[derive(Clone)]
struct MacroExpander<'a> {
    registry:     &'a MacroRegistry,
    options:      MacroExpansionOptions,
    stack:        Vec<MacroPath>,
    trace:        bool,
    traces:       Vec<MacroExpansionTrace>,
    cancellation: Option<&'a crate::CancellationToken>,
}

impl MacroExpander<'_> {
    fn expand_stream(
        &mut self,
        stream: NwTokenStream,
    ) -> Result<NwTokenStream, MacroExpansionError> {
        let mut output = NwTokenStream::new();
        let trees = stream.trees;
        let mut position = 0;

        while position < trees.len() {
            if self
                .cancellation
                .is_some_and(crate::CancellationToken::is_cancelled)
            {
                return Err(MacroExpansionError::without_span("operation cancelled"));
            }
            if let Some((invocation, consumed)) = parse_macro_invocation(&trees, position)? {
                if self.stack.len() >= self.options.max_depth {
                    return Err(MacroExpansionError::new(
                        invocation.span,
                        format!(
                            "macro expansion exceeded maximum depth of {}",
                            self.options.max_depth
                        ),
                    )
                    .with_stack(&self.stack));
                }
                let Some(implementation) = self.registry.macros.get(&invocation.path) else {
                    return Err(MacroExpansionError::new(
                        invocation.span,
                        format!("unknown macro `{}`", invocation.path),
                    )
                    .with_stack(&self.stack));
                };

                self.stack.push(invocation.path.clone());
                let result = implementation.expand(
                    &invocation,
                    MacroContext {
                        expansion_stack: &self.stack,
                        options:         self.options,
                        cancellation:    self.cancellation,
                    },
                );
                let expanded = match result {
                    Ok(result) => result,
                    Err(error) => {
                        let error = error.with_stack(&self.stack);
                        self.stack.pop();
                        return Err(error);
                    }
                };
                if self.trace {
                    self.traces.push(MacroExpansionTrace {
                        path:   invocation.path.clone(),
                        span:   invocation.span,
                        depth:  self.stack.len(),
                        input:  invocation.input.clone(),
                        output: expanded.tokens.clone(),
                    });
                }
                let replacement = if expanded.recursively_expand {
                    match self.expand_stream(expanded.tokens) {
                        Ok(tokens) => tokens,
                        Err(error) => {
                            self.stack.pop();
                            return Err(error);
                        }
                    }
                } else {
                    expanded.tokens
                };
                self.stack.pop();
                output.extend(replacement);
                position += consumed;
            } else {
                let Some(tree) = trees.get(position).cloned() else {
                    break;
                };
                let tree = match tree {
                    NwTokenTree::Group(mut group) => {
                        group.stream = self.expand_stream(group.stream)?;
                        NwTokenTree::Group(group)
                    }
                    token => token,
                };
                output.push(tree);
                position += 1;
            }

            if output.flattened_len() > self.options.token_limit {
                return Err(MacroExpansionError::without_span(format!(
                    "macro expansion exceeded token limit of {}",
                    self.options.token_limit
                ))
                .with_stack(&self.stack));
            }
        }

        Ok(output)
    }
}

pub(super) fn parse_macro_invocation(
    trees: &[NwTokenTree],
    start: usize,
) -> Result<Option<(MacroInvocation, usize)>, MacroExpansionError> {
    let Some(NwTokenTree::Token(first)) = trees.get(start) else {
        return Ok(None);
    };
    if first.kind != TokenKind::Identifier {
        return Ok(None);
    }

    let mut segments = vec![first.text.clone()];
    let mut position = start + 1;
    while let Some(segment) = macro_path_segment(trees, position) {
        segments.push(segment.text.clone());
        position += 3;
    }

    let Some(NwTokenTree::Token(bang)) = trees.get(position) else {
        return Ok(None);
    };
    if bang.kind != TokenKind::BooleanNot {
        return Ok(None);
    }
    let Some(NwTokenTree::Group(arguments)) = trees.get(position + 1) else {
        return Ok(None);
    };
    let path = MacroPath::new(segments)?;
    let span = Span::new(
        first.span.source_id,
        first.span.start,
        arguments.close_span.end,
    );
    Ok(Some((
        MacroInvocation {
            path,
            delimiter: arguments.delimiter,
            input: arguments.stream.clone(),
            span,
        },
        position + 2 - start,
    )))
}

fn macro_path_segment(trees: &[NwTokenTree], position: usize) -> Option<&Token> {
    let NwTokenTree::Token(first_colon) = trees.get(position)? else {
        return None;
    };
    let NwTokenTree::Token(second_colon) = trees.get(position + 1)? else {
        return None;
    };
    let NwTokenTree::Token(segment) = trees.get(position + 2)? else {
        return None;
    };
    (first_colon.kind == TokenKind::Colon
        && second_colon.kind == TokenKind::Colon
        && segment.kind == TokenKind::Identifier)
        .then_some(segment)
}
