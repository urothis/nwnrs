use super::*;

pub(super) struct DeclarativeMacro {
    path:  MacroPath,
    rules: Vec<DeclarativeRule>,
}

impl DeclarativeMacro {
    pub(super) fn parse(
        path: &MacroPath,
        body: &NwTokenGroup,
    ) -> Result<Self, MacroExpansionError> {
        let trees = body.stream.trees();
        let mut rules = Vec::new();
        let mut position = 0;
        while position < trees.len() {
            let Some(NwTokenTree::Group(pattern)) = trees.get(position) else {
                return Err(MacroExpansionError::new(
                    body.span(),
                    format!("macro `{path}` expected a delimited matcher"),
                ));
            };
            let Some(NwTokenTree::Token(assign)) = trees.get(position + 1) else {
                return Err(MacroExpansionError::new(
                    pattern.span(),
                    "macro rule matcher must be followed by `=>`",
                ));
            };
            let Some(NwTokenTree::Token(greater)) = trees.get(position + 2) else {
                return Err(MacroExpansionError::new(
                    pattern.span(),
                    "macro rule matcher must be followed by `=>`",
                ));
            };
            let Some(NwTokenTree::Group(template)) = trees.get(position + 3) else {
                return Err(MacroExpansionError::new(
                    greater.span,
                    "macro rule requires a delimited expansion template",
                ));
            };
            if assign.kind != TokenKind::Assign || greater.kind != TokenKind::GreaterThan {
                return Err(MacroExpansionError::new(
                    assign.span,
                    "macro rule matcher must be followed by `=>`",
                ));
            }
            let mut names = BTreeMap::new();
            let matcher = parse_matchers(&pattern.stream, &mut names)?;
            rules.push(DeclarativeRule {
                delimiter: pattern.delimiter,
                matcher,
                template: template.stream.clone(),
            });
            position += 4;
            if let Some(NwTokenTree::Token(semicolon)) = trees.get(position)
                && semicolon.kind == TokenKind::Semicolon
            {
                position += 1;
            }
        }
        if rules.is_empty() {
            return Err(MacroExpansionError::new(
                body.span(),
                format!("macro `{path}` requires at least one rule"),
            ));
        }
        Ok(Self {
            path: path.clone(),
            rules,
        })
    }
}

impl BangMacro for DeclarativeMacro {
    fn expand(
        &self,
        invocation: &MacroInvocation,
        _context: MacroContext<'_>,
    ) -> Result<MacroOutput, MacroExpansionError> {
        for rule in &self.rules {
            if rule.delimiter != invocation.delimiter {
                continue;
            }
            let mut bindings = QuoteBindings::new();
            if let Some((matched_bindings, consumed)) =
                match_matchers(&rule.matcher, invocation.input.trees(), 0, 0, &mut bindings)
                && consumed == invocation.input.len()
            {
                let mut template = rule.template.clone();
                rebase_all_spans(&mut template, invocation.span);
                return quote_nwscript(&template, &matched_bindings).map(MacroOutput::expanded);
            }
        }
        Err(MacroExpansionError::new(
            invocation.span,
            format!("no rules matched invocation of `{}`", self.path),
        ))
    }
}

fn rebase_all_spans(stream: &mut NwTokenStream, span: Span) {
    for tree in &mut stream.trees {
        match tree {
            NwTokenTree::Token(token) => token.span = span,
            NwTokenTree::Group(group) => {
                group.open_span = span;
                group.close_span = span;
                rebase_all_spans(&mut group.stream, span);
            }
        }
    }
}

pub(super) fn rebase_source_id(stream: &mut NwTokenStream, source_id: SourceId, span: Span) {
    for tree in &mut stream.trees {
        match tree {
            NwTokenTree::Token(token) if token.span.source_id == source_id => token.span = span,
            NwTokenTree::Token(_) => {}
            NwTokenTree::Group(group) => {
                if group.open_span.source_id == source_id {
                    group.open_span = span;
                }
                if group.close_span.source_id == source_id {
                    group.close_span = span;
                }
                rebase_source_id(&mut group.stream, source_id, span);
            }
        }
    }
}

#[derive(Debug, Clone)]
struct DeclarativeRule {
    delimiter: NwDelimiter,
    matcher:   Vec<Matcher>,
    template:  NwTokenStream,
}

#[derive(Debug, Clone)]
enum Matcher {
    Literal(NwTokenTree),
    Group {
        delimiter: NwDelimiter,
        matcher:   Vec<Self>,
    },
    Capture {
        name:     String,
        fragment: Fragment,
    },
    Repeat {
        matcher:    Vec<Self>,
        separator:  Option<NwTokenTree>,
        quantifier: TokenKind,
        captures:   Vec<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fragment {
    TokenTree,
    Identifier,
    Literal,
    Expression,
    Tokens,
}

fn parse_matchers(
    stream: &NwTokenStream,
    names: &mut BTreeMap<String, Span>,
) -> Result<Vec<Matcher>, MacroExpansionError> {
    let trees = stream.trees();
    let mut matchers = Vec::new();
    let mut position = 0;
    while position < trees.len() {
        let Some(NwTokenTree::Token(dollar)) = trees.get(position) else {
            if let Some(NwTokenTree::Group(group)) = trees.get(position) {
                matchers.push(Matcher::Group {
                    delimiter: group.delimiter,
                    matcher:   parse_matchers(&group.stream, names)?,
                });
            } else if let Some(tree) = trees.get(position) {
                matchers.push(Matcher::Literal(tree.clone()));
            }
            position += 1;
            continue;
        };
        if dollar.kind != TokenKind::Dollar {
            matchers.push(Matcher::Literal(NwTokenTree::Token(dollar.clone())));
            position += 1;
            continue;
        }
        if let Some(NwTokenTree::Group(repeated)) = trees.get(position + 1) {
            let Some((separator, quantifier, consumed)) = repetition_suffix(trees, position + 2)
            else {
                return Err(MacroExpansionError::new(
                    repeated.span(),
                    "macro matcher repetition requires `*`, `+`, or `?`",
                ));
            };
            if quantifier == TokenKind::QuestionMark && separator.is_some() {
                return Err(MacroExpansionError::new(
                    repeated.span(),
                    "`?` macro matcher repetition cannot have a separator",
                ));
            }
            let before = names
                .keys()
                .cloned()
                .collect::<std::collections::BTreeSet<_>>();
            let matcher = parse_matchers(&repeated.stream, names)?;
            let captures = names
                .keys()
                .filter(|name| !before.contains(*name))
                .cloned()
                .collect();
            matchers.push(Matcher::Repeat {
                matcher,
                separator,
                quantifier,
                captures,
            });
            position += 2 + consumed;
            continue;
        }
        let Some(NwTokenTree::Token(name)) = trees.get(position + 1) else {
            return Err(MacroExpansionError::new(
                dollar.span,
                "macro matcher `$` must be followed by a capture name",
            ));
        };
        let Some(NwTokenTree::Token(colon)) = trees.get(position + 2) else {
            return Err(MacroExpansionError::new(
                name.span,
                "macro capture name must be followed by `:fragment`",
            ));
        };
        let Some(NwTokenTree::Token(fragment)) = trees.get(position + 3) else {
            return Err(MacroExpansionError::new(
                colon.span,
                "macro capture requires a fragment kind",
            ));
        };
        if name.kind != TokenKind::Identifier
            || colon.kind != TokenKind::Colon
            || fragment.kind != TokenKind::Identifier
        {
            return Err(MacroExpansionError::new(
                dollar.span,
                "expected macro capture `$name:fragment`",
            ));
        }
        if let Some(previous) = names.insert(name.text.clone(), name.span) {
            return Err(MacroExpansionError::new(
                name.span,
                format!(
                    "duplicate macro capture `${}`; first declared at byte {}",
                    name.text, previous.start
                ),
            ));
        }
        let fragment_kind = match fragment.text.as_str() {
            "tt" => Fragment::TokenTree,
            "ident" => Fragment::Identifier,
            "literal" => Fragment::Literal,
            "expr" => Fragment::Expression,
            "tokens" => Fragment::Tokens,
            _ => {
                return Err(MacroExpansionError::new(
                    fragment.span,
                    format!("unknown macro fragment `{}`", fragment.text),
                ));
            }
        };
        matchers.push(Matcher::Capture {
            name:     name.text.clone(),
            fragment: fragment_kind,
        });
        position += 4;
    }
    Ok(matchers)
}

fn match_matchers(
    matchers: &[Matcher],
    input: &[NwTokenTree],
    matcher_position: usize,
    input_position: usize,
    bindings: &mut QuoteBindings,
) -> Option<(QuoteBindings, usize)> {
    if matcher_position >= matchers.len() {
        return Some((bindings.clone(), input_position));
    }
    let matcher = matchers.get(matcher_position)?;
    match matcher {
        Matcher::Literal(expected) => {
            let actual = input.get(input_position)?;
            if !same_tree_shape(expected, actual) {
                return None;
            }
            match_matchers(
                matchers,
                input,
                matcher_position + 1,
                input_position + 1,
                bindings,
            )
        }
        Matcher::Group {
            delimiter,
            matcher,
        } => {
            let NwTokenTree::Group(group) = input.get(input_position)? else {
                return None;
            };
            if group.delimiter != *delimiter {
                return None;
            }
            let mut nested_bindings = bindings.clone();
            let (matched_bindings, consumed) =
                match_matchers(matcher, group.stream.trees(), 0, 0, &mut nested_bindings)?;
            if consumed != group.stream.len() {
                return None;
            }
            *bindings = matched_bindings;
            match_matchers(
                matchers,
                input,
                matcher_position + 1,
                input_position + 1,
                bindings,
            )
        }
        Matcher::Capture {
            name,
            fragment,
        } => match fragment {
            Fragment::TokenTree => {
                let tree = input.get(input_position)?.clone();
                bindings.insert(name, NwTokenStream::from_trees(vec![tree]));
                match_matchers(
                    matchers,
                    input,
                    matcher_position + 1,
                    input_position + 1,
                    bindings,
                )
            }
            Fragment::Identifier => {
                let NwTokenTree::Token(token) = input.get(input_position)? else {
                    return None;
                };
                if token.kind != TokenKind::Identifier {
                    return None;
                }
                bindings.insert(
                    name,
                    NwTokenStream::from_trees(vec![NwTokenTree::Token(token.clone())]),
                );
                match_matchers(
                    matchers,
                    input,
                    matcher_position + 1,
                    input_position + 1,
                    bindings,
                )
            }
            Fragment::Literal => {
                let NwTokenTree::Token(token) = input.get(input_position)? else {
                    return None;
                };
                if !literal_kind(&token.kind) {
                    return None;
                }
                bindings.insert(
                    name,
                    NwTokenStream::from_trees(vec![NwTokenTree::Token(token.clone())]),
                );
                match_matchers(
                    matchers,
                    input,
                    matcher_position + 1,
                    input_position + 1,
                    bindings,
                )
            }
            Fragment::Expression | Fragment::Tokens => {
                let minimum_end = input_position.checked_add(1)?;
                if matcher_position + 1 >= matchers.len() {
                    bindings.insert(
                        name,
                        NwTokenStream::from_trees(input.get(input_position..)?.to_vec()),
                    );
                    return Some((bindings.clone(), input.len()));
                }
                for end in minimum_end..=input.len() {
                    let mut candidate = bindings.clone();
                    candidate.insert(
                        name,
                        NwTokenStream::from_trees(input.get(input_position..end)?.to_vec()),
                    );
                    if let Some(result) =
                        match_matchers(matchers, input, matcher_position + 1, end, &mut candidate)
                    {
                        return Some(result);
                    }
                }
                None
            }
        },
        Matcher::Repeat {
            matcher,
            separator,
            quantifier,
            captures,
        } => {
            let mut states = Vec::new();
            collect_repetition_states(
                matcher,
                separator.as_ref(),
                quantifier,
                input,
                input_position,
                Vec::new(),
                true,
                &mut states,
            );
            states.sort_by(|left, right| {
                right
                    .iterations
                    .len()
                    .cmp(&left.iterations.len())
                    .then_with(|| right.position.cmp(&left.position))
            });
            for state in states {
                let mut candidate = bindings.clone();
                if !bind_repetition_captures(&mut candidate, captures, &state.iterations) {
                    continue;
                }
                if let Some(result) = match_matchers(
                    matchers,
                    input,
                    matcher_position + 1,
                    state.position,
                    &mut candidate,
                ) {
                    return Some(result);
                }
            }
            None
        }
    }
}

#[derive(Clone)]
struct RepetitionMatchState {
    position:   usize,
    iterations: Vec<QuoteBindings>,
}

fn collect_repetition_states(
    matcher: &[Matcher],
    separator: Option<&NwTokenTree>,
    quantifier: &TokenKind,
    input: &[NwTokenTree],
    position: usize,
    iterations: Vec<QuoteBindings>,
    record_current: bool,
    output: &mut Vec<RepetitionMatchState>,
) {
    let minimum = usize::from(quantifier == &TokenKind::Plus);
    let maximum = (quantifier == &TokenKind::QuestionMark).then_some(1);
    if record_current && iterations.len() >= minimum {
        output.push(RepetitionMatchState {
            position,
            iterations: iterations.clone(),
        });
    }
    if maximum.is_some_and(|maximum| iterations.len() >= maximum) || position >= input.len() {
        return;
    }

    for end in position.saturating_add(1)..=input.len() {
        let Some(slice) = input.get(position..end) else {
            continue;
        };
        let mut iteration = QuoteBindings::new();
        let Some((iteration, consumed)) = match_matchers(matcher, slice, 0, 0, &mut iteration)
        else {
            continue;
        };
        if consumed != slice.len() {
            continue;
        }
        let mut next_iterations = iterations.clone();
        next_iterations.push(iteration);
        if separator.is_none() {
            collect_repetition_states(
                matcher,
                None,
                quantifier,
                input,
                end,
                next_iterations.clone(),
                true,
                output,
            );
        }
        if let Some(separator) = separator {
            output.push(RepetitionMatchState {
                position:   end,
                iterations: next_iterations.clone(),
            });
            if input
                .get(end)
                .is_some_and(|actual| same_tree_shape(separator, actual))
            {
                collect_repetition_states(
                    matcher,
                    Some(separator),
                    quantifier,
                    input,
                    end.saturating_add(1),
                    next_iterations,
                    false,
                    output,
                );
            }
        }
    }
}

fn bind_repetition_captures(
    bindings: &mut QuoteBindings,
    captures: &[String],
    iterations: &[QuoteBindings],
) -> bool {
    for name in captures {
        let mut values = Vec::with_capacity(iterations.len());
        for iteration in iterations {
            let Some(value) = iteration.values.get(name).cloned() else {
                return false;
            };
            values.push(value);
        }
        let binding = if values
            .iter()
            .all(|value| matches!(value, QuoteBinding::Single(_)))
        {
            QuoteBinding::Repeated(
                values
                    .into_iter()
                    .filter_map(|value| match value {
                        QuoteBinding::Single(stream) => Some(stream),
                        QuoteBinding::Repeated(_) | QuoteBinding::Nested(_) => None,
                    })
                    .collect(),
            )
        } else {
            QuoteBinding::Nested(values)
        };
        bindings.values.insert(name.clone(), binding);
    }
    true
}

fn same_tree_shape(left: &NwTokenTree, right: &NwTokenTree) -> bool {
    match (left, right) {
        (NwTokenTree::Token(left), NwTokenTree::Token(right)) => {
            left.kind == right.kind && left.text == right.text
        }
        (NwTokenTree::Group(left), NwTokenTree::Group(right)) => {
            left.delimiter == right.delimiter
                && left.stream.len() == right.stream.len()
                && left
                    .stream
                    .trees()
                    .iter()
                    .zip(right.stream.trees())
                    .all(|(left, right)| same_tree_shape(left, right))
        }
        _ => false,
    }
}

pub(super) fn literal_kind(kind: &TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Integer
            | TokenKind::HexInteger
            | TokenKind::BinaryInteger
            | TokenKind::OctalInteger
            | TokenKind::Float
            | TokenKind::String
            | TokenKind::HashedString
    )
}
