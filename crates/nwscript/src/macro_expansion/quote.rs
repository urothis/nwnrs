use super::*;

/// One value available for `$name` interpolation in a quoted token template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuoteBinding {
    /// One token stream inserted at each reference.
    Single(NwTokenStream),
    /// A sequence consumed by `$($name)*`-style repetition.
    Repeated(Vec<NwTokenStream>),
    /// Recursively nested repetition values.
    Nested(Vec<QuoteBinding>),
}

/// Named values used while quoting an extended `NWScript` token template.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QuoteBindings {
    pub(super) values: BTreeMap<String, QuoteBinding>,
}

impl QuoteBindings {
    /// Creates an empty binding set.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            values: BTreeMap::new(),
        }
    }

    /// Adds or replaces one non-repeated binding.
    pub fn insert(&mut self, name: impl Into<String>, tokens: NwTokenStream) {
        self.values
            .insert(name.into(), QuoteBinding::Single(tokens));
    }

    /// Adds or replaces one repeated binding.
    pub fn insert_repeated(&mut self, name: impl Into<String>, values: Vec<NwTokenStream>) {
        self.values
            .insert(name.into(), QuoteBinding::Repeated(values));
    }

    /// Adds or replaces one recursively nested repeated binding.
    pub fn insert_nested(&mut self, name: impl Into<String>, values: Vec<QuoteBinding>) {
        self.values
            .insert(name.into(), QuoteBinding::Nested(values));
    }

    /// Adds or replaces a fully constructed binding.
    pub fn insert_binding(&mut self, name: impl Into<String>, binding: QuoteBinding) {
        self.values.insert(name.into(), binding);
    }

    /// Returns a named binding.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&QuoteBinding> {
        self.values.get(name)
    }
}

/// Quotes a token template using `$name` interpolation and repetition.
///
/// Supported repetition forms are `$($values)*`, `$($values),*`, their `+`
/// equivalents, and `?` for an optional zero-or-one expansion. A literal `$`
/// is written as `$$`. Unlike Rust's `quote!`, `#` remains ordinary NWScript
/// syntax so `#include` and `#[...]` are not ambiguous.
///
/// # Errors
///
/// Returns an error for missing bindings, repeated bindings outside a
/// repetition, incompatible repetition lengths, or malformed `$` syntax.
pub fn quote_nwscript(
    template: &NwTokenStream,
    bindings: &QuoteBindings,
) -> Result<NwTokenStream, MacroExpansionError> {
    quote_stream(template, bindings, &[])
}

/// Renders a token stream as canonical generated `NWScript` source.
///
/// This renderer prioritizes lexical round trips over human formatting. String
/// payloads are escaped explicitly, and tokens are separated so adjacent
/// identifiers or operators cannot merge accidentally.
#[must_use]
pub fn render_nwscript_tokens(stream: &NwTokenStream) -> String {
    render_nwscript_tokens_bytes(stream)
        .into_iter()
        .map(char::from)
        .collect()
}

/// Renders a token stream to exact canonical `NWScript` source bytes.
///
/// Token strings use the lexer's byte-shadow representation, where
/// `U+0000..=U+00FF` maps directly to `0x00..=0xFF`. Non-ASCII string bytes
/// are emitted as `\xNN`, so the generated source is ASCII without ever
/// re-encoding a payload as UTF-8.
#[must_use]
pub fn render_nwscript_tokens_bytes(stream: &NwTokenStream) -> Vec<u8> {
    render_flat_token_bytes(&stream.clone().into_tokens())
}

pub(super) fn quote_stream(
    template: &NwTokenStream,
    bindings: &QuoteBindings,
    repetition_path: &[usize],
) -> Result<NwTokenStream, MacroExpansionError> {
    let trees = template.trees();
    let mut output = NwTokenStream::new();
    let mut position = 0;
    while position < trees.len() {
        let Some(tree) = trees.get(position) else {
            break;
        };
        let NwTokenTree::Token(dollar) = tree else {
            if let NwTokenTree::Group(group) = tree {
                let mut group = group.clone();
                group.stream = quote_stream(&group.stream, bindings, repetition_path)?;
                output.push(NwTokenTree::Group(group));
            } else {
                output.push(tree.clone());
            }
            position += 1;
            continue;
        };
        if dollar.kind != TokenKind::Dollar {
            output.push(tree.clone());
            position += 1;
            continue;
        }

        let Some(next) = trees.get(position + 1) else {
            return Err(MacroExpansionError::new(
                dollar.span,
                "quoted `$` requires a binding, repetition, or second `$`",
            ));
        };
        if let NwTokenTree::Token(next_token) = next {
            if next_token.kind == TokenKind::Dollar {
                output.push(NwTokenTree::Token(next_token.clone()));
                position += 2;
                continue;
            }
            if next_token.kind != TokenKind::Identifier {
                return Err(MacroExpansionError::new(
                    next_token.span,
                    "quoted `$` must be followed by a binding name",
                ));
            }
            let Some(binding) = bindings.get(&next_token.text) else {
                return Err(MacroExpansionError::new(
                    next_token.span,
                    format!("unknown quote binding `${}`", next_token.text),
                ));
            };
            let tokens = resolve_quote_binding(binding, repetition_path).ok_or_else(|| {
                MacroExpansionError::new(
                    next_token.span,
                    format!(
                        "quote binding `${}` is unavailable at repetition depth {}",
                        next_token.text,
                        repetition_path.len()
                    ),
                )
            })?;
            output.extend(tokens.clone());
            position += 2;
            continue;
        }

        let NwTokenTree::Group(repeated) = next else {
            return Err(MacroExpansionError::new(
                next.span(),
                "quoted repetition requires a delimited template",
            ));
        };
        let Some((separator, quantifier, consumed)) = repetition_suffix(trees, position + 2) else {
            return Err(MacroExpansionError::new(
                repeated.span(),
                "quoted repetition requires `*`, `+`, or `?`",
            ));
        };
        let count = repetition_count(&repeated.stream, bindings, repetition_path)?;
        if quantifier == TokenKind::Plus && count == 0 {
            return Err(MacroExpansionError::new(
                repeated.span(),
                "`+` quote repetition requires at least one value",
            ));
        }
        if quantifier == TokenKind::QuestionMark && count > 1 {
            return Err(MacroExpansionError::new(
                repeated.span(),
                "`?` quote repetition accepts at most one value",
            ));
        }
        for index in 0..count {
            if index > 0
                && let Some(separator) = &separator
            {
                output.push(separator.clone());
            }
            let mut nested_path = repetition_path.to_vec();
            nested_path.push(index);
            output.extend(quote_stream(&repeated.stream, bindings, &nested_path)?);
        }
        position += 2 + consumed;
    }
    Ok(output)
}

pub(super) fn repetition_suffix(
    trees: &[NwTokenTree],
    position: usize,
) -> Option<(Option<NwTokenTree>, TokenKind, usize)> {
    let first = trees.get(position)?;
    if let NwTokenTree::Token(token) = first
        && matches!(
            token.kind,
            TokenKind::Multiply | TokenKind::Plus | TokenKind::QuestionMark
        )
    {
        return Some((None, token.kind.clone(), 1));
    }
    let NwTokenTree::Token(quantifier) = trees.get(position + 1)? else {
        return None;
    };
    if !matches!(
        quantifier.kind,
        TokenKind::Multiply | TokenKind::Plus | TokenKind::QuestionMark
    ) {
        return None;
    }
    Some((Some(first.clone()), quantifier.kind.clone(), 2))
}

fn repetition_count(
    template: &NwTokenStream,
    bindings: &QuoteBindings,
    repetition_path: &[usize],
) -> Result<usize, MacroExpansionError> {
    let mut lengths = Vec::new();
    collect_repeated_lengths(template, bindings, repetition_path, &mut lengths)?;
    let Some(first) = lengths.first().copied() else {
        return Err(MacroExpansionError::without_span(
            "quote repetition contains no repeated binding",
        ));
    };
    if lengths.iter().any(|length| *length != first) {
        return Err(MacroExpansionError::without_span(
            "quote repetition bindings have different lengths",
        ));
    }
    Ok(first)
}

fn collect_repeated_lengths(
    stream: &NwTokenStream,
    bindings: &QuoteBindings,
    repetition_path: &[usize],
    output: &mut Vec<usize>,
) -> Result<(), MacroExpansionError> {
    let trees = stream.trees();
    let mut position = 0;
    while position < trees.len() {
        if let Some(NwTokenTree::Token(dollar)) = trees.get(position)
            && dollar.kind == TokenKind::Dollar
            && let Some(NwTokenTree::Token(name)) = trees.get(position + 1)
            && name.kind == TokenKind::Identifier
        {
            if let Some(binding) = bindings.get(&name.text)
                && let Some(length) = quote_binding_repetition_length(binding, repetition_path)
            {
                output.push(length);
            }
            position += 2;
            continue;
        }
        if let Some(NwTokenTree::Group(group)) = trees.get(position) {
            collect_repeated_lengths(&group.stream, bindings, repetition_path, output)?;
        }
        position += 1;
    }
    Ok(())
}

fn resolve_quote_binding<'a>(
    binding: &'a QuoteBinding,
    repetition_path: &[usize],
) -> Option<&'a NwTokenStream> {
    match binding {
        QuoteBinding::Single(stream) => Some(stream),
        QuoteBinding::Repeated(values) => {
            let [index] = repetition_path else {
                return None;
            };
            values.get(*index)
        }
        QuoteBinding::Nested(values) => {
            let (index, remaining) = repetition_path.split_first()?;
            resolve_quote_binding(values.get(*index)?, remaining)
        }
    }
}

fn quote_binding_repetition_length(
    binding: &QuoteBinding,
    repetition_path: &[usize],
) -> Option<usize> {
    if repetition_path.is_empty() {
        return match binding {
            QuoteBinding::Single(_) => None,
            QuoteBinding::Repeated(values) => Some(values.len()),
            QuoteBinding::Nested(values) => Some(values.len()),
        };
    }
    match binding {
        QuoteBinding::Single(_) | QuoteBinding::Repeated(_) => None,
        QuoteBinding::Nested(values) => {
            let (index, remaining) = repetition_path.split_first()?;
            quote_binding_repetition_length(values.get(*index)?, remaining)
        }
    }
}
