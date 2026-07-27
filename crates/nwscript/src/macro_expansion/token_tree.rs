use super::*;

/// One balanced delimiter surrounding an [`NwTokenStream`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NwDelimiter {
    /// `( ... )`
    Parenthesis,
    /// `[ ... ]`
    Bracket,
    /// `{ ... }`
    Brace,
}

impl NwDelimiter {
    const fn from_open(kind: &TokenKind) -> Option<Self> {
        match kind {
            TokenKind::LeftParen => Some(Self::Parenthesis),
            TokenKind::LeftSquareBracket => Some(Self::Bracket),
            TokenKind::LeftBrace => Some(Self::Brace),
            _ => None,
        }
    }

    const fn from_close(kind: &TokenKind) -> Option<Self> {
        match kind {
            TokenKind::RightParen => Some(Self::Parenthesis),
            TokenKind::RightSquareBracket => Some(Self::Bracket),
            TokenKind::RightBrace => Some(Self::Brace),
            _ => None,
        }
    }
}

/// One balanced group in an extended `NWScript` token stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NwTokenGroup {
    /// Delimiter kind.
    pub delimiter:  NwDelimiter,
    /// Span of the opening delimiter.
    pub open_span:  Span,
    /// Span of the closing delimiter.
    pub close_span: Span,
    /// Tokens contained by the delimiters.
    pub stream:     NwTokenStream,
}

impl NwTokenGroup {
    /// Returns the span covering both delimiters and their contents.
    #[must_use]
    pub const fn span(&self) -> Span {
        Span::new(
            self.open_span.source_id,
            self.open_span.start,
            self.close_span.end,
        )
    }
}

/// One leaf token or balanced group in extended `NWScript` syntax.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NwTokenTree {
    /// One ordinary lexer token.
    Token(Token),
    /// One balanced delimiter group.
    Group(NwTokenGroup),
}

impl NwTokenTree {
    /// Returns the source span represented by this tree.
    #[must_use]
    pub const fn span(&self) -> Span {
        match self {
            Self::Token(token) => token.span,
            Self::Group(group) => group.span(),
        }
    }
}

/// A balanced extended-`NWScript` token stream.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NwTokenStream {
    pub(super) trees: Vec<NwTokenTree>,
}

impl NwTokenStream {
    /// Creates an empty stream.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            trees: Vec::new()
        }
    }

    /// Creates a stream from already balanced trees.
    #[must_use]
    pub fn from_trees(trees: Vec<NwTokenTree>) -> Self {
        Self {
            trees,
        }
    }

    /// Parses lexer tokens into balanced token trees.
    ///
    /// The optional final EOF token is ignored. Callers restoring a flat token
    /// stream should append the compilation unit's EOF token afterwards.
    ///
    /// # Errors
    ///
    /// Returns an error for mismatched, unexpected, or unclosed delimiters.
    pub fn from_tokens(tokens: &[Token]) -> Result<Self, MacroExpansionError> {
        Self::from_tokens_with_limits_and_cancellation(
            tokens,
            crate::CompilerResourceLimits::default(),
            None,
        )
    }

    /// Parses lexer tokens with explicit resource bounds and cancellation.
    pub fn from_tokens_with_limits_and_cancellation(
        tokens: &[Token],
        limits: crate::CompilerResourceLimits,
        cancellation: Option<&crate::CancellationToken>,
    ) -> Result<Self, MacroExpansionError> {
        limits
            .validate()
            .map_err(MacroExpansionError::without_span)?;
        if tokens.len() > limits.max_tokens {
            return Err(MacroExpansionError::without_span(format!(
                "token tree input contains {} tokens, exceeding configured maximum {}",
                tokens.len(),
                limits.max_tokens
            )));
        }
        let mut position = 0;
        parse_tree_level(tokens, &mut position, None, 0, limits, cancellation)
    }

    /// Returns the trees in this stream.
    #[must_use]
    pub fn trees(&self) -> &[NwTokenTree] {
        &self.trees
    }

    /// Adds one tree to the end of this stream.
    pub fn push(&mut self, tree: NwTokenTree) {
        self.trees.push(tree);
    }

    /// Adds all trees from another stream.
    pub fn extend(&mut self, stream: Self) {
        self.trees.extend(stream.trees);
    }

    /// Returns whether this stream contains no trees.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.trees.is_empty()
    }

    /// Returns the number of top-level trees.
    #[must_use]
    pub fn len(&self) -> usize {
        self.trees.len()
    }

    /// Returns the flattened token count, including group delimiters.
    #[must_use]
    pub fn flattened_len(&self) -> usize {
        self.trees.iter().map(flattened_tree_len).sum()
    }

    /// Flattens the stream back into ordinary lexer tokens.
    #[must_use]
    pub fn into_tokens(self) -> Vec<Token> {
        let mut tokens = Vec::with_capacity(self.flattened_len());
        flatten_trees(self.trees, &mut tokens);
        tokens
    }
}

/// A namespaced bang-macro path such as `nwnrs::event`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MacroPath {
    pub(super) segments: Vec<String>,
}

impl MacroPath {
    /// Creates a path from one or more identifier segments.
    ///
    /// # Errors
    ///
    /// Returns an error when the path is empty or contains an invalid
    /// `NWScript` identifier.
    pub fn new(
        segments: impl IntoIterator<Item = impl Into<String>>,
    ) -> Result<Self, MacroExpansionError> {
        let segments = segments.into_iter().map(Into::into).collect::<Vec<_>>();
        if segments.is_empty() {
            return Err(MacroExpansionError::without_span(
                "macro path requires at least one segment",
            ));
        }
        if let Some(segment) = segments.iter().find(|segment| !valid_identifier(segment)) {
            return Err(MacroExpansionError::without_span(format!(
                "invalid macro path segment {segment:?}"
            )));
        }
        Ok(Self {
            segments,
        })
    }

    /// Parses a `::`-separated macro path.
    ///
    /// # Errors
    ///
    /// Returns an error when any segment is absent or invalid.
    pub fn parse(path: &str) -> Result<Self, MacroExpansionError> {
        Self::new(path.split("::"))
    }

    /// Returns the path segments.
    #[must_use]
    pub fn segments(&self) -> &[String] {
        &self.segments
    }
}

impl fmt::Display for MacroPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.segments.join("::"))
    }
}

/// One parsed `path!(...)`, `path![...]`, or `path!{...}` invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroInvocation {
    /// Macro path.
    pub path:      MacroPath,
    /// Invocation delimiter.
    pub delimiter: NwDelimiter,
    /// Unexpanded input tokens inside the invocation delimiter.
    pub input:     NwTokenStream,
    /// Span covering the path, bang, and argument group.
    pub span:      Span,
}

/// Output from one bang-macro implementation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroOutput {
    /// Replacement tokens.
    pub tokens:             NwTokenStream,
    /// Whether macro calls in the replacement should be recursively expanded.
    pub recursively_expand: bool,
}

impl MacroOutput {
    /// Creates recursively expanded replacement output.
    #[must_use]
    pub fn expanded(tokens: NwTokenStream) -> Self {
        Self {
            tokens,
            recursively_expand: true,
        }
    }

    /// Creates opaque replacement output whose nested macro invocations remain
    /// untouched.
    #[must_use]
    pub fn opaque(tokens: NwTokenStream) -> Self {
        Self {
            tokens,
            recursively_expand: false,
        }
    }
}

fn parse_tree_level(
    tokens: &[Token],
    position: &mut usize,
    expected: Option<(NwDelimiter, Span)>,
    depth: usize,
    limits: crate::CompilerResourceLimits,
    cancellation: Option<&crate::CancellationToken>,
) -> Result<NwTokenStream, MacroExpansionError> {
    if depth > limits.max_syntax_nesting {
        return Err(MacroExpansionError::without_span(format!(
            "token-tree nesting exceeded configured maximum of {}",
            limits.max_syntax_nesting
        )));
    }
    let mut trees = Vec::new();
    while let Some(token) = tokens.get(*position) {
        if *position & 0x0fff == 0 {
            check_macro_cancellation(cancellation)?;
        }
        if token.kind == TokenKind::Eof {
            break;
        }
        if let Some(delimiter) = NwDelimiter::from_close(&token.kind) {
            let Some((expected_delimiter, _open_span)) = expected else {
                return Err(MacroExpansionError::new(
                    token.span,
                    "unexpected closing delimiter",
                ));
            };
            if delimiter != expected_delimiter {
                return Err(MacroExpansionError::new(
                    token.span,
                    "mismatched closing delimiter",
                ));
            }
            *position += 1;
            return Ok(NwTokenStream::from_trees(trees));
        }
        if let Some(delimiter) = NwDelimiter::from_open(&token.kind) {
            let open_span = token.span;
            *position += 1;
            let stream = parse_tree_level(
                tokens,
                position,
                Some((delimiter, open_span)),
                depth + 1,
                limits,
                cancellation,
            )?;
            let Some(close) = tokens.get(position.saturating_sub(1)) else {
                return Err(MacroExpansionError::new(
                    open_span,
                    "unclosed token delimiter",
                ));
            };
            trees.push(NwTokenTree::Group(NwTokenGroup {
                delimiter,
                open_span,
                close_span: close.span,
                stream,
            }));
            continue;
        }
        trees.push(NwTokenTree::Token(token.clone()));
        *position += 1;
    }

    if let Some((_delimiter, open_span)) = expected {
        return Err(MacroExpansionError::new(
            open_span,
            "unclosed token delimiter",
        ));
    }
    Ok(NwTokenStream::from_trees(trees))
}

fn flatten_trees(trees: Vec<NwTokenTree>, output: &mut Vec<Token>) {
    for tree in trees {
        match tree {
            NwTokenTree::Token(token) => output.push(token),
            NwTokenTree::Group(group) => {
                output.push(delimiter_token(group.delimiter, true, group.open_span));
                flatten_trees(group.stream.trees, output);
                output.push(delimiter_token(group.delimiter, false, group.close_span));
            }
        }
    }
}

fn flattened_tree_len(tree: &NwTokenTree) -> usize {
    match tree {
        NwTokenTree::Token(_) => 1,
        NwTokenTree::Group(group) => 2 + group.stream.flattened_len(),
    }
}

fn delimiter_token(delimiter: NwDelimiter, opening: bool, span: Span) -> Token {
    let (kind, text) = match (delimiter, opening) {
        (NwDelimiter::Parenthesis, true) => (TokenKind::LeftParen, "("),
        (NwDelimiter::Parenthesis, false) => (TokenKind::RightParen, ")"),
        (NwDelimiter::Bracket, true) => (TokenKind::LeftSquareBracket, "["),
        (NwDelimiter::Bracket, false) => (TokenKind::RightSquareBracket, "]"),
        (NwDelimiter::Brace, true) => (TokenKind::LeftBrace, "{"),
        (NwDelimiter::Brace, false) => (TokenKind::RightBrace, "}"),
    };
    Token::new(kind, span, text)
}

pub(super) fn valid_identifier(value: &str) -> bool {
    let mut bytes = value.bytes();
    bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}
