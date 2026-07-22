use std::{cell::RefCell, collections::BTreeMap, error::Error, fmt, io::Cursor, rc::Rc};

use crate::{SourceId, Span, Token, TokenKind};

/// Maximum nested macro expansions accepted by default.
pub const DEFAULT_MACRO_EXPANSION_DEPTH: usize = 64;
/// Maximum flattened tokens produced by one macro expansion by default.
pub const DEFAULT_MACRO_TOKEN_LIMIT: usize = 1_000_000;
/// Maximum VM instructions executed by one NWScript procedural invocation.
pub const DEFAULT_PROCEDURAL_MACRO_INSTRUCTION_LIMIT: usize = 1_000_000;
/// Maximum NWScript call depth during one procedural invocation.
pub const DEFAULT_PROCEDURAL_MACRO_RECURSION_LIMIT: usize = 64;
/// Maximum VM stack cells used by one procedural invocation.
pub const DEFAULT_PROCEDURAL_MACRO_STACK_LIMIT: usize = 1_000_000;
/// Compiler-runtime function used to construct a token stream from source.
pub const QUOTE_STATIC_FUNCTION: &str = "__NWNRS_QuoteStatic";
/// Compiler-runtime function used to concatenate two token streams.
pub const QUOTE_CONCAT_FUNCTION: &str = "__NWNRS_QuoteConcat";
/// Compiler-runtime function used to construct an empty token stream.
pub const QUOTE_EMPTY_FUNCTION: &str = "__NWNRS_QuoteEmpty";
/// Compiler-runtime function used to create an empty token-stream list.
pub const TOKENSTREAM_LIST_NEW_FUNCTION: &str = "__NWNRS_TokenStreamListNew";
/// Compiler-runtime function used to append one stream to a token-stream list.
pub const TOKENSTREAM_LIST_PUSH_FUNCTION: &str = "__NWNRS_TokenStreamListPush";
/// Compiler-runtime function used to append one nested list.
pub const TOKENSTREAM_LIST_PUSH_LIST_FUNCTION: &str = "__NWNRS_TokenStreamListPushList";
/// Compiler-runtime function used by lowered procedural quotation repetition.
pub const QUOTE_REPEAT_FUNCTION: &str = "__NWNRS_QuoteRepeat";
const MACRO_TOKENSTREAM_INDEX: u8 = 0;
const MACRO_TOKENSTREAM_LIST_INDEX: u8 = 1;
const MACRO_QUOTE_BINDINGS_INDEX: u8 = 2;
const MACRO_TOKEN_CURSOR_INDEX: u8 = 3;
const GENERATED_MACRO_SOURCE_ID: SourceId = SourceId::new(u32::MAX - 1);
const MACRO_LANGSPEC: &str = r#"
#define ENGINE_NUM_STRUCTURES 4
#define ENGINE_STRUCTURE_0 tokenstream
#define ENGINE_STRUCTURE_1 tokenstream_list
#define ENGINE_STRUCTURE_2 quote_bindings
#define ENGINE_STRUCTURE_3 token_cursor
int TRUE = 1;
int FALSE = 0;
tokenstream __NWNRS_QuoteStatic(string sSource);
tokenstream __NWNRS_QuoteConcat(tokenstream tsLeft, tokenstream tsRight);
tokenstream __NWNRS_QuoteEmpty();
int __NWNRS_TokenStreamLength(tokenstream tsInput);
tokenstream __NWNRS_TokenStreamGet(tokenstream tsInput, int nIndex);
int __NWNRS_TokenIsGroup(tokenstream tsInput);
string __NWNRS_TokenKind(tokenstream tsInput);
string __NWNRS_TokenText(tokenstream tsInput);
int __NWNRS_TokenDelimiter(tokenstream tsInput);
tokenstream __NWNRS_TokenParse(string sSource);
void __NWNRS_MacroError(string sMessage);
tokenstream_list __NWNRS_TokenStreamListNew();
tokenstream_list __NWNRS_TokenStreamListPush(tokenstream_list tslValues, tokenstream tsValue);
tokenstream_list __NWNRS_TokenStreamListPushList(tokenstream_list tslValues, tokenstream_list tslValue);
int __NWNRS_TokenStreamListLength(tokenstream_list tslValues);
tokenstream __NWNRS_TokenStreamListGet(tokenstream_list tslValues, int nIndex);
tokenstream_list __NWNRS_TokenStreamListGetList(tokenstream_list tslValues, int nIndex);
quote_bindings __NWNRS_QuoteBindingsNew();
quote_bindings __NWNRS_QuoteBindingsInsert(quote_bindings qbValues, string sName, tokenstream_list tslValues);
tokenstream __NWNRS_QuoteRepeat(string sTemplate, quote_bindings qbValues, string sSeparator, int nQuantifier);
token_cursor __NWNRS_TokenCursorNew(tokenstream tsInput);
int __NWNRS_TokenCursorPosition(token_cursor tcInput);
int __NWNRS_TokenCursorIsEnd(token_cursor tcInput);
tokenstream __NWNRS_TokenCursorPeek(token_cursor tcInput);
tokenstream __NWNRS_TokenCursorNext(token_cursor tcInput);
int __NWNRS_TokenCursorConsume(token_cursor tcInput, string sText);
tokenstream __NWNRS_TokenCursorExpect(token_cursor tcInput, string sText);
tokenstream __NWNRS_TokenCursorParseIdentifier(token_cursor tcInput);
tokenstream __NWNRS_TokenCursorParseLiteral(token_cursor tcInput);
tokenstream __NWNRS_TokenCursorParseTree(token_cursor tcInput);
tokenstream __NWNRS_TokenCursorParsePath(token_cursor tcInput);
tokenstream __NWNRS_TokenCursorParseExpression(token_cursor tcInput);
tokenstream __NWNRS_TokenCursorParseType(token_cursor tcInput);
tokenstream __NWNRS_TokenCursorParseStatement(token_cursor tcInput);
tokenstream __NWNRS_TokenCursorParseFunction(token_cursor tcInput);
tokenstream __NWNRS_TokenCursorParseStruct(token_cursor tcInput);
tokenstream_list __NWNRS_TokenCursorParseSeparated(token_cursor tcInput, string sKind, string sSeparator, int nMinimum, int nMaximum);
tokenstream __NWNRS_TokenCursorRemaining(token_cursor tcInput);
void __NWNRS_MacroErrorAt(tokenstream tsInput, string sMessage);
tokenstream_list __NWNRS_TokenStreamListSort(tokenstream_list tslValues);
tokenstream __NWNRS_TokenGroupContents(tokenstream tsGroup);
"#;

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
    trees: Vec<NwTokenTree>,
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
        let mut position = 0;
        parse_tree_level(tokens, &mut position, None)
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
    segments: Vec<String>,
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

/// Context supplied to one bang-macro implementation.
#[derive(Debug, Clone, Copy)]
pub struct MacroContext<'a> {
    expansion_stack: &'a [MacroPath],
}

impl<'a> MacroContext<'a> {
    /// Returns the active macro expansion stack, oldest first.
    #[must_use]
    pub fn expansion_stack(self) -> &'a [MacroPath] {
        self.expansion_stack
    }
}

/// One Rust-hosted bang macro in the extended `NWScript` compiler.
pub trait BangMacro: Send + Sync {
    /// Expands one invocation.
    ///
    /// # Errors
    ///
    /// Returns a diagnostic when the invocation arguments are invalid or
    /// expansion cannot be completed.
    fn expand(
        &self,
        invocation: &MacroInvocation,
        context: MacroContext<'_>,
    ) -> Result<MacroOutput, MacroExpansionError>;
}

/// Registered bang macros available to one expansion session.
#[derive(Default)]
pub struct MacroRegistry {
    macros: BTreeMap<MacroPath, Box<dyn BangMacro>>,
}

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
    values: BTreeMap<String, QuoteBinding>,
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
    render_flat_tokens(&stream.clone().into_tokens())
}

impl MacroRegistry {
    /// Creates an empty registry.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            macros: BTreeMap::new(),
        }
    }

    /// Registers or replaces one bang macro.
    pub fn register(&mut self, path: MacroPath, implementation: impl BangMacro + 'static) {
        self.macros.insert(path, Box::new(implementation));
    }

    /// Registers or replaces a bang macro by string path.
    ///
    /// # Errors
    ///
    /// Returns an error if `path` is invalid.
    pub fn register_path(
        &mut self,
        path: &str,
        implementation: impl BangMacro + 'static,
    ) -> Result<(), MacroExpansionError> {
        self.register(MacroPath::parse(path)?, implementation);
        Ok(())
    }

    /// Returns whether the registry contains `path`.
    #[must_use]
    pub fn contains(&self, path: &MacroPath) -> bool {
        self.macros.contains_key(path)
    }
}

/// Resource limits for one recursive expansion session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MacroExpansionOptions {
    /// Maximum active macro expansion depth.
    pub max_depth:   usize,
    /// Maximum flattened tokens in an intermediate or final stream.
    pub token_limit: usize,
}

/// One invocation observed while recursively expanding bang macros.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroExpansionTrace {
    /// Invoked macro path.
    pub path:   MacroPath,
    /// Source span occupied by the invocation.
    pub span:   Span,
    /// Active expansion depth, starting at one.
    pub depth:  usize,
    /// Tokens passed to the macro implementation.
    pub input:  NwTokenStream,
    /// Immediate output returned by the implementation, before recursive
    /// expansion.
    pub output: NwTokenStream,
}

impl Default for MacroExpansionOptions {
    fn default() -> Self {
        Self {
            max_depth:   DEFAULT_MACRO_EXPANSION_DEPTH,
            token_limit: DEFAULT_MACRO_TOKEN_LIMIT,
        }
    }
}

/// One macro-expansion or token-tree diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroExpansionError {
    /// Source span most directly associated with the failure, when known.
    pub span:            Option<Span>,
    /// Human-readable diagnostic.
    pub message:         String,
    /// Active macro expansion paths, oldest first.
    pub expansion_stack: Vec<MacroPath>,
}

impl MacroExpansionError {
    /// Creates an error associated with one source span.
    #[must_use]
    pub fn new(span: Span, message: impl Into<String>) -> Self {
        Self {
            span:            Some(span),
            message:         message.into(),
            expansion_stack: Vec::new(),
        }
    }

    /// Creates an error without source location information.
    #[must_use]
    pub fn without_span(message: impl Into<String>) -> Self {
        Self {
            span:            None,
            message:         message.into(),
            expansion_stack: Vec::new(),
        }
    }

    fn with_stack(mut self, stack: &[MacroPath]) -> Self {
        if self.expansion_stack.is_empty() {
            self.expansion_stack = stack.to_vec();
        }
        self
    }
}

impl fmt::Display for MacroExpansionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)?;
        if !self.expansion_stack.is_empty() {
            write!(f, " while expanding ")?;
            for (index, path) in self.expansion_stack.iter().enumerate() {
                if index > 0 {
                    f.write_str(" -> ")?;
                }
                path.fmt(f)?;
            }
        }
        Ok(())
    }
}

impl Error for MacroExpansionError {}

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
    let stream = NwTokenStream::from_tokens(&tokens)?;
    let mut expander = MacroExpander {
        registry,
        options,
        stack: Vec::new(),
        trace: false,
        traces: Vec::new(),
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
    let mut output = Vec::new();
    let mut position = 0;
    while position < stream.trees.len() {
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
    expand_source_macros_impl(tokens, registry, options, false).map(|(tokens, _traces)| tokens)
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
    expand_source_macros_impl(tokens, registry, options, true)
}

fn expand_source_macros_impl(
    tokens: Vec<Token>,
    registry: &mut MacroRegistry,
    options: MacroExpansionOptions,
    trace: bool,
) -> Result<(Vec<Token>, Vec<MacroExpansionTrace>), MacroExpansionError> {
    let eof = tokens
        .last()
        .filter(|token| token.kind == TokenKind::Eof)
        .cloned()
        .unwrap_or_else(|| Token::new(TokenKind::Eof, Span::new(SourceId::new(0), 0, 0), ""));
    let mut stream = NwTokenStream::from_tokens(&tokens)?;
    collect_nwscript_macros(&mut stream, registry)?;
    collect_declarative_macros(&mut stream, registry)?;
    let mut expander = MacroExpander {
        registry,
        options,
        stack: Vec::new(),
        trace,
        traces: Vec::new(),
    };
    let mut expanded = expander.expand_stream(stream)?;
    erase_nwnrs_macro_attributes(&mut expanded)?;
    let traces = expander.traces;
    let mut flattened = expanded.into_tokens();
    flattened.push(eof);
    Ok((flattened, traces))
}

/// Erases compiler-only nwnrs attribute syntax after NSS macros have expanded.
///
/// Event discovery and validation belong to the nwnrs project macro. This
/// frontend pass only prevents recognized compiler attributes from reaching
/// the ordinary NWScript parser.
fn erase_nwnrs_macro_attributes(stream: &mut NwTokenStream) -> Result<(), MacroExpansionError> {
    let mut output = Vec::new();
    let mut position = 0;
    while position < stream.trees.len() {
        let Some(NwTokenTree::Token(hash)) = stream.trees.get(position) else {
            if let Some(tree) = stream.trees.get(position).cloned() {
                output.push(tree);
            }
            position += 1;
            continue;
        };
        if hash.kind != TokenKind::Hash {
            output.push(NwTokenTree::Token(hash.clone()));
            position += 1;
            continue;
        }
        let Some(NwTokenTree::Group(attribute)) = stream.trees.get(position + 1) else {
            return Err(MacroExpansionError::new(
                hash.span,
                "`#` must be followed by a compiler attribute in brackets",
            ));
        };
        if attribute.delimiter != NwDelimiter::Bracket {
            return Err(MacroExpansionError::new(
                attribute.span(),
                "compiler attributes must use `#[...]`",
            ));
        }
        validate_nwnrs_attribute_route(attribute)?;
        position += 2;
    }
    stream.trees = output;
    Ok(())
}

fn validate_nwnrs_attribute_route(attribute: &NwTokenGroup) -> Result<(), MacroExpansionError> {
    let trees = attribute.stream.trees();
    let valid_path = matches!(
        trees,
        [
            NwTokenTree::Token(namespace),
            NwTokenTree::Token(first_colon),
            NwTokenTree::Token(second_colon),
            NwTokenTree::Token(name),
            NwTokenTree::Group(arguments),
        ] if namespace.text == "nwnrs"
            && first_colon.kind == TokenKind::Colon
            && second_colon.kind == TokenKind::Colon
            && name.text == "events"
            && arguments.delimiter == NwDelimiter::Parenthesis
    );
    if valid_path {
        Ok(())
    } else {
        Err(MacroExpansionError::new(
            attribute.span(),
            "only `#[nwnrs::events(...)]` compiler attributes are supported",
        ))
    }
}

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
        if !valid_identifier(entry) {
            return Err(MacroExpansionError::without_span(format!(
                "invalid procedural macro entry function {entry:?}"
            )));
        }
        let source_id = SourceId::new(0);
        let tokens = crate::lex_text(source_id, source).map_err(|error| {
            MacroExpansionError::without_span(format!(
                "failed to lex procedural macro {source_name:?}: {error}"
            ))
        })?;
        let mut registry = MacroRegistry::new();
        register_compiler_macros(&mut registry)?;
        let tokens = expand_source_macros(tokens, &mut registry, MacroExpansionOptions::default())?;
        let langspec = crate::parse_langspec("nwnrs_macro", MACRO_LANGSPEC).map_err(|error| {
            MacroExpansionError::without_span(format!(
                "failed to load the procedural macro language specification: {error}"
            ))
        })?;
        let script = crate::parse_tokens(tokens, Some(&langspec)).map_err(|error| {
            MacroExpansionError::without_span(format!(
                "failed to parse procedural macro {source_name:?}: {error}"
            ))
        })?;
        let mut source_map = crate::SourceMap::new();
        let root_id = source_map.add_file(source_name.to_string(), source.as_bytes().to_vec());
        let artifacts = crate::compile_script_with_source_map(
            &script,
            &source_map,
            root_id,
            Some(&langspec),
            crate::CompileOptions::default(),
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
        let ndb = crate::read_ndb(&mut Cursor::new(ndb_bytes)).map_err(|error| {
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
        _context: MacroContext<'_>,
    ) -> Result<MacroOutput, MacroExpansionError> {
        let arena = Rc::new(RefCell::new(TokenStreamArena::with_input(
            invocation.input.clone(),
        )));
        let vm = macro_vm(Rc::clone(&arena), GENERATED_MACRO_SOURCE_ID);
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
    let mut output = Vec::new();
    let mut position = 0;
    while position < stream.trees.len() {
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
        let source = render_nwscript_tokens(&body.stream);
        let implementation = NwScriptMacro::compile(&source_name, entry, &source)?;
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

#[derive(Debug, Clone)]
enum TokenStreamListValue {
    Stream(u32),
    List(u32),
}

#[derive(Debug, Clone)]
struct TokenCursorState {
    stream:   NwTokenStream,
    position: usize,
}

#[derive(Debug, Default)]
struct TokenStreamArena {
    streams:        Vec<Vec<Token>>,
    lists:          Vec<Vec<TokenStreamListValue>>,
    quote_bindings: Vec<QuoteBindings>,
    cursors:        Vec<TokenCursorState>,
}

impl TokenStreamArena {
    fn with_input(input: NwTokenStream) -> Self {
        Self {
            streams:        vec![input.clone().into_tokens()],
            lists:          vec![Vec::new()],
            quote_bindings: vec![QuoteBindings::new()],
            cursors:        vec![TokenCursorState {
                stream:   input,
                position: 0,
            }],
        }
    }

    fn insert(&mut self, stream: NwTokenStream) -> Result<u32, crate::VmError> {
        self.insert_tokens(stream.into_tokens())
    }

    fn insert_tokens(&mut self, tokens: Vec<Token>) -> Result<u32, crate::VmError> {
        let handle = u32::try_from(self.streams.len()).map_err(|error| crate::VmError::Setup {
            message: format!("compiler tokenstream arena exhausted: {error}"),
        })?;
        self.streams.push(tokens);
        Ok(handle)
    }

    fn get(&self, handle: u32) -> Option<&[Token]> {
        usize::try_from(handle)
            .ok()
            .and_then(|index| self.streams.get(index))
            .map(Vec::as_slice)
    }

    fn balanced(&self, handle: u32) -> Result<NwTokenStream, MacroExpansionError> {
        let tokens = self.get(handle).ok_or_else(|| {
            MacroExpansionError::without_span(format!(
                "unknown compiler tokenstream handle {handle}"
            ))
        })?;
        NwTokenStream::from_tokens(tokens)
    }

    fn insert_list(&mut self, values: Vec<TokenStreamListValue>) -> Result<u32, crate::VmError> {
        let handle = u32::try_from(self.lists.len()).map_err(|error| crate::VmError::Setup {
            message: format!("compiler tokenstream-list arena exhausted: {error}"),
        })?;
        self.lists.push(values);
        Ok(handle)
    }

    fn list(&self, handle: u32) -> Option<&[TokenStreamListValue]> {
        usize::try_from(handle)
            .ok()
            .and_then(|index| self.lists.get(index))
            .map(Vec::as_slice)
    }

    fn insert_quote_bindings(&mut self, bindings: QuoteBindings) -> Result<u32, crate::VmError> {
        let handle =
            u32::try_from(self.quote_bindings.len()).map_err(|error| crate::VmError::Setup {
                message: format!("compiler quote-binding arena exhausted: {error}"),
            })?;
        self.quote_bindings.push(bindings);
        Ok(handle)
    }

    fn bindings(&self, handle: u32) -> Option<&QuoteBindings> {
        usize::try_from(handle)
            .ok()
            .and_then(|index| self.quote_bindings.get(index))
    }

    fn binding_from_list(&self, handle: u32) -> Result<QuoteBinding, MacroExpansionError> {
        let values = self.list(handle).ok_or_else(|| {
            MacroExpansionError::without_span(format!(
                "unknown compiler tokenstream-list handle {handle}"
            ))
        })?;
        let mut bindings = Vec::with_capacity(values.len());
        for value in values {
            bindings.push(match value {
                TokenStreamListValue::Stream(handle) => {
                    QuoteBinding::Single(self.balanced(*handle)?)
                }
                TokenStreamListValue::List(handle) => self.binding_from_list(*handle)?,
            });
        }
        if bindings
            .iter()
            .all(|binding| matches!(binding, QuoteBinding::Single(_)))
        {
            Ok(QuoteBinding::Repeated(
                bindings
                    .into_iter()
                    .filter_map(|binding| match binding {
                        QuoteBinding::Single(stream) => Some(stream),
                        QuoteBinding::Repeated(_) | QuoteBinding::Nested(_) => None,
                    })
                    .collect(),
            ))
        } else {
            Ok(QuoteBinding::Nested(bindings))
        }
    }

    fn insert_cursor(&mut self, stream: NwTokenStream) -> Result<u32, crate::VmError> {
        let handle = u32::try_from(self.cursors.len()).map_err(|error| crate::VmError::Setup {
            message: format!("compiler token-cursor arena exhausted: {error}"),
        })?;
        self.cursors.push(TokenCursorState {
            stream,
            position: 0,
        });
        Ok(handle)
    }

    fn cursor(&self, handle: u32) -> Option<&TokenCursorState> {
        usize::try_from(handle)
            .ok()
            .and_then(|index| self.cursors.get(index))
    }

    fn cursor_mut(&mut self, handle: u32) -> Option<&mut TokenCursorState> {
        usize::try_from(handle)
            .ok()
            .and_then(|index| self.cursors.get_mut(index))
    }
}

fn macro_vm(arena: Rc<RefCell<TokenStreamArena>>, source_id: SourceId) -> crate::Vm {
    let mut vm = crate::Vm::new();
    vm.define_engine_structure_default(
        MACRO_TOKENSTREAM_INDEX,
        crate::VmEngineStructureValue::Word(0),
    );
    vm.define_engine_structure_default(
        MACRO_TOKENSTREAM_LIST_INDEX,
        crate::VmEngineStructureValue::Word(0),
    );
    vm.define_engine_structure_default(
        MACRO_QUOTE_BINDINGS_INDEX,
        crate::VmEngineStructureValue::Word(0),
    );
    vm.define_engine_structure_default(
        MACRO_TOKEN_CURSOR_INDEX,
        crate::VmEngineStructureValue::Word(0),
    );
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(0, move |script| {
            let source = script.pop_string()?;
            let tokens = crate::lex_bytes(source_id, source.as_bytes()).map_err(|error| {
                crate::VmError::Setup {
                    message: format!("could not lex quoted NWScript: {error}"),
                }
            })?;
            let tokens = tokens
                .into_iter()
                .filter(|token| token.kind != TokenKind::Eof)
                .collect();
            push_macro_tokenstream(script, arena.borrow_mut().insert_tokens(tokens)?);
            Ok(())
        });
    }
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(1, move |script| {
            let left = pop_macro_tokenstream_handle(script)?;
            let right = pop_macro_tokenstream_handle(script)?;
            let mut arena = arena.borrow_mut();
            let mut combined = arena
                .get(left)
                .map(<[Token]>::to_vec)
                .ok_or_else(|| unknown_tokenstream_handle(script, left))?;
            let right = arena
                .get(right)
                .map(<[Token]>::to_vec)
                .ok_or_else(|| unknown_tokenstream_handle(script, right))?;
            combined.extend(right);
            let handle = arena.insert_tokens(combined)?;
            push_macro_tokenstream(script, handle);
            Ok(())
        });
    }
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(2, move |script| {
            let handle = arena.borrow_mut().insert(NwTokenStream::new())?;
            push_macro_tokenstream(script, handle);
            Ok(())
        });
    }
    define_tokenstream_inspection_commands(&mut vm, Rc::clone(&arena), source_id);
    define_tokenstream_collection_commands(&mut vm, Rc::clone(&arena), source_id);
    define_token_cursor_commands(&mut vm, Rc::clone(&arena), source_id);
    define_token_project_commands(&mut vm, arena);
    vm
}

fn pop_macro_tokenstream_handle(script: &mut crate::VmScript) -> Result<u32, crate::VmError> {
    let value = script.pop_engine_structure_index(MACRO_TOKENSTREAM_INDEX)?;
    match value {
        crate::VmEngineStructureValue::Word(handle) => Ok(handle),
        crate::VmEngineStructureValue::Text(_) => Err(crate::VmError::TypeMismatch {
            offset:   script.ip(),
            message:  "expected handle-backed compiler tokenstream".to_string(),
            expected: Some("engine structure"),
            actual:   "engine structure",
        }),
    }
}

fn push_macro_tokenstream(script: &mut crate::VmScript, handle: u32) {
    script.push_engine_structure(
        MACRO_TOKENSTREAM_INDEX,
        crate::VmEngineStructureValue::Word(handle),
    );
}

fn pop_macro_handle(
    script: &mut crate::VmScript,
    index: u8,
    name: &'static str,
) -> Result<u32, crate::VmError> {
    match script.pop_engine_structure_index(index)? {
        crate::VmEngineStructureValue::Word(handle) => Ok(handle),
        crate::VmEngineStructureValue::Text(_) => Err(crate::VmError::TypeMismatch {
            offset:   script.ip(),
            message:  format!("expected handle-backed compiler {name}"),
            expected: Some("engine structure"),
            actual:   "engine structure",
        }),
    }
}

fn push_macro_handle(script: &mut crate::VmScript, index: u8, handle: u32) {
    script.push_engine_structure(index, crate::VmEngineStructureValue::Word(handle));
}

fn unknown_tokenstream_handle(script: &crate::VmScript, handle: u32) -> crate::VmError {
    crate::VmError::Setup {
        message: format!(
            "unknown compiler tokenstream handle {handle} at instruction {}",
            script.ip()
        ),
    }
}

fn macro_vm_error(error: MacroExpansionError) -> crate::VmError {
    crate::VmError::Setup {
        message: error.to_string(),
    }
}

fn define_tokenstream_inspection_commands(
    vm: &mut crate::Vm,
    arena: Rc<RefCell<TokenStreamArena>>,
    source_id: SourceId,
) {
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(3, move |script| {
            let handle = pop_macro_tokenstream_handle(script)?;
            let length = arena
                .borrow()
                .balanced(handle)
                .map_err(macro_vm_error)?
                .len();
            let length = i32::try_from(length).map_err(|error| crate::VmError::Setup {
                message: format!("tokenstream length exceeds NWScript integer range: {error}"),
            })?;
            script.push_int(length);
            Ok(())
        });
    }
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(4, move |script| {
            let stream_handle = pop_macro_tokenstream_handle(script)?;
            let index = macro_index(script.pop_int()?, script)?;
            let tree = arena
                .borrow()
                .balanced(stream_handle)
                .map_err(macro_vm_error)?
                .trees()
                .get(index)
                .cloned()
                .ok_or_else(|| crate::VmError::Setup {
                    message: format!("tokenstream index {index} is out of bounds"),
                })?;
            let handle = arena
                .borrow_mut()
                .insert(NwTokenStream::from_trees(vec![tree]))?;
            push_macro_tokenstream(script, handle);
            Ok(())
        });
    }
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(5, move |script| {
            let tree = pop_single_macro_tree(script, &arena)?;
            script.push_int(i32::from(matches!(tree, NwTokenTree::Group(_))));
            Ok(())
        });
    }
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(6, move |script| {
            let tree = pop_single_macro_tree(script, &arena)?;
            script.push_string(macro_tree_kind(&tree));
            Ok(())
        });
    }
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(7, move |script| {
            let tree = pop_single_macro_tree(script, &arena)?;
            let text = match tree {
                NwTokenTree::Token(token) => token.text,
                tree @ NwTokenTree::Group(_) => {
                    render_nwscript_tokens(&NwTokenStream::from_trees(vec![tree]))
                }
            };
            script.push_string(text);
            Ok(())
        });
    }
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(8, move |script| {
            let tree = pop_single_macro_tree(script, &arena)?;
            let delimiter = match tree {
                NwTokenTree::Token(_) => 0,
                NwTokenTree::Group(group) => match group.delimiter {
                    NwDelimiter::Parenthesis => 1,
                    NwDelimiter::Bracket => 2,
                    NwDelimiter::Brace => 3,
                },
            };
            script.push_int(delimiter);
            Ok(())
        });
    }
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(9, move |script| {
            let source = script.pop_string()?;
            let tokens = crate::lex_bytes(source_id, source.as_bytes()).map_err(|error| {
                crate::VmError::Setup {
                    message: format!("could not lex parsed tokenstream source: {error}"),
                }
            })?;
            let stream = NwTokenStream::from_tokens(&tokens).map_err(macro_vm_error)?;
            let handle = arena.borrow_mut().insert(stream)?;
            push_macro_tokenstream(script, handle);
            Ok(())
        });
    }
    vm.define_simple_command(10, |script| {
        let message = script.pop_string()?;
        Err(crate::VmError::Setup {
            message: format!("procedural macro reported: {}", message.to_string_lossy()),
        })
    });
}

fn define_tokenstream_collection_commands(
    vm: &mut crate::Vm,
    arena: Rc<RefCell<TokenStreamArena>>,
    source_id: SourceId,
) {
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(11, move |script| {
            let handle = arena.borrow_mut().insert_list(Vec::new())?;
            push_macro_handle(script, MACRO_TOKENSTREAM_LIST_INDEX, handle);
            Ok(())
        });
    }
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(12, move |script| {
            let list = pop_macro_handle(script, MACRO_TOKENSTREAM_LIST_INDEX, "tokenstream list")?;
            let stream = pop_macro_tokenstream_handle(script)?;
            let mut arena = arena.borrow_mut();
            let mut values = arena
                .list(list)
                .map(<[TokenStreamListValue]>::to_vec)
                .ok_or_else(|| crate::VmError::Setup {
                    message: format!("unknown compiler tokenstream-list handle {list}"),
                })?;
            values.push(TokenStreamListValue::Stream(stream));
            let handle = arena.insert_list(values)?;
            push_macro_handle(script, MACRO_TOKENSTREAM_LIST_INDEX, handle);
            Ok(())
        });
    }
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(13, move |script| {
            let list = pop_macro_handle(script, MACRO_TOKENSTREAM_LIST_INDEX, "tokenstream list")?;
            let nested =
                pop_macro_handle(script, MACRO_TOKENSTREAM_LIST_INDEX, "tokenstream list")?;
            let mut arena = arena.borrow_mut();
            if arena.list(nested).is_none() {
                return Err(crate::VmError::Setup {
                    message: format!("unknown compiler tokenstream-list handle {nested}"),
                });
            }
            let mut values = arena
                .list(list)
                .map(<[TokenStreamListValue]>::to_vec)
                .ok_or_else(|| crate::VmError::Setup {
                    message: format!("unknown compiler tokenstream-list handle {list}"),
                })?;
            values.push(TokenStreamListValue::List(nested));
            let handle = arena.insert_list(values)?;
            push_macro_handle(script, MACRO_TOKENSTREAM_LIST_INDEX, handle);
            Ok(())
        });
    }
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(14, move |script| {
            let list = pop_macro_handle(script, MACRO_TOKENSTREAM_LIST_INDEX, "tokenstream list")?;
            let length = arena
                .borrow()
                .list(list)
                .ok_or_else(|| crate::VmError::Setup {
                    message: format!("unknown compiler tokenstream-list handle {list}"),
                })?
                .len();
            script.push_int(
                i32::try_from(length).map_err(|error| crate::VmError::Setup {
                    message: format!(
                        "tokenstream-list length exceeds NWScript integer range: {error}"
                    ),
                })?,
            );
            Ok(())
        });
    }
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(15, move |script| {
            let list = pop_macro_handle(script, MACRO_TOKENSTREAM_LIST_INDEX, "tokenstream list")?;
            let index = macro_index(script.pop_int()?, script)?;
            let arena = arena.borrow();
            let value = arena
                .list(list)
                .and_then(|values| values.get(index))
                .ok_or_else(|| crate::VmError::Setup {
                    message: format!("tokenstream-list index {index} is out of bounds"),
                })?;
            let TokenStreamListValue::Stream(handle) = value else {
                return Err(crate::VmError::Setup {
                    message: format!("tokenstream-list entry {index} is a nested list"),
                });
            };
            push_macro_tokenstream(script, *handle);
            Ok(())
        });
    }
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(16, move |script| {
            let list = pop_macro_handle(script, MACRO_TOKENSTREAM_LIST_INDEX, "tokenstream list")?;
            let index = macro_index(script.pop_int()?, script)?;
            let arena = arena.borrow();
            let value = arena
                .list(list)
                .and_then(|values| values.get(index))
                .ok_or_else(|| crate::VmError::Setup {
                    message: format!("tokenstream-list index {index} is out of bounds"),
                })?;
            let TokenStreamListValue::List(handle) = value else {
                return Err(crate::VmError::Setup {
                    message: format!("tokenstream-list entry {index} is not a nested list"),
                });
            };
            push_macro_handle(script, MACRO_TOKENSTREAM_LIST_INDEX, *handle);
            Ok(())
        });
    }
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(17, move |script| {
            let handle = arena
                .borrow_mut()
                .insert_quote_bindings(QuoteBindings::new())?;
            push_macro_handle(script, MACRO_QUOTE_BINDINGS_INDEX, handle);
            Ok(())
        });
    }
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(18, move |script| {
            let bindings = pop_macro_handle(script, MACRO_QUOTE_BINDINGS_INDEX, "quote bindings")?;
            let name = script.pop_string()?.to_string_lossy().into_owned();
            let list = pop_macro_handle(script, MACRO_TOKENSTREAM_LIST_INDEX, "tokenstream list")?;
            let mut arena = arena.borrow_mut();
            let mut bindings =
                arena
                    .bindings(bindings)
                    .cloned()
                    .ok_or_else(|| crate::VmError::Setup {
                        message: format!("unknown compiler quote-binding handle {bindings}"),
                    })?;
            let binding = arena.binding_from_list(list).map_err(macro_vm_error)?;
            bindings.values.insert(name, binding);
            let handle = arena.insert_quote_bindings(bindings)?;
            push_macro_handle(script, MACRO_QUOTE_BINDINGS_INDEX, handle);
            Ok(())
        });
    }
    vm.define_simple_command(19, move |script| {
        let template = script.pop_string()?.to_string_lossy().into_owned();
        let bindings = pop_macro_handle(script, MACRO_QUOTE_BINDINGS_INDEX, "quote bindings")?;
        let separator = script.pop_string()?.to_string_lossy().into_owned();
        let quantifier = match script.pop_int()? {
            0 => "*",
            1 => "+",
            2 => "?",
            value => {
                return Err(crate::VmError::Setup {
                    message: format!("unknown quote repetition quantifier {value}"),
                });
            }
        };
        let source = format!("$({template}){separator}{quantifier}");
        let tokens = crate::lex_bytes(source_id, source.as_bytes()).map_err(|error| {
            crate::VmError::Setup {
                message: format!("could not lex quote repetition: {error}"),
            }
        })?;
        let quoted = {
            let arena = arena.borrow();
            let bindings = arena
                .bindings(bindings)
                .ok_or_else(|| crate::VmError::Setup {
                    message: format!("unknown compiler quote-binding handle {bindings}"),
                })?;
            let template = NwTokenStream::from_tokens(&tokens).map_err(macro_vm_error)?;
            quote_nwscript(&template, bindings).map_err(macro_vm_error)?
        };
        let handle = arena.borrow_mut().insert(quoted)?;
        push_macro_tokenstream(script, handle);
        Ok(())
    });
}

#[derive(Debug, Clone, Copy)]
enum CursorFragmentKind {
    Identifier,
    Literal,
    Tree,
    Path,
    Expression,
    Type,
    Statement,
    Function,
    Struct,
}

impl CursorFragmentKind {
    fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "ident" | "identifier" => Self::Identifier,
            "literal" => Self::Literal,
            "tt" | "tree" => Self::Tree,
            "path" => Self::Path,
            "expr" | "expression" => Self::Expression,
            "type" => Self::Type,
            "stmt" | "statement" => Self::Statement,
            "fn" | "function" => Self::Function,
            "struct" => Self::Struct,
            _ => return None,
        })
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Identifier => "identifier",
            Self::Literal => "literal",
            Self::Tree => "token tree",
            Self::Path => "path",
            Self::Expression => "expression",
            Self::Type => "type",
            Self::Statement => "statement",
            Self::Function => "function",
            Self::Struct => "struct",
        }
    }
}

fn define_token_cursor_commands(
    vm: &mut crate::Vm,
    arena: Rc<RefCell<TokenStreamArena>>,
    source_id: SourceId,
) {
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(20, move |script| {
            let stream = pop_macro_tokenstream_handle(script)?;
            let stream = arena.borrow().balanced(stream).map_err(macro_vm_error)?;
            let handle = arena.borrow_mut().insert_cursor(stream)?;
            push_macro_handle(script, MACRO_TOKEN_CURSOR_INDEX, handle);
            Ok(())
        });
    }
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(21, move |script| {
            let cursor = pop_macro_handle(script, MACRO_TOKEN_CURSOR_INDEX, "token cursor")?;
            let position = arena
                .borrow()
                .cursor(cursor)
                .ok_or_else(|| unknown_cursor_handle(cursor))?
                .position;
            script.push_int(
                i32::try_from(position).map_err(|error| crate::VmError::Setup {
                    message: format!(
                        "token-cursor position exceeds NWScript integer range: {error}"
                    ),
                })?,
            );
            Ok(())
        });
    }
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(22, move |script| {
            let cursor = pop_macro_handle(script, MACRO_TOKEN_CURSOR_INDEX, "token cursor")?;
            let arena = arena.borrow();
            let cursor = arena
                .cursor(cursor)
                .ok_or_else(|| unknown_cursor_handle(cursor))?;
            script.push_int(i32::from(cursor.position >= cursor.stream.len()));
            Ok(())
        });
    }
    define_cursor_tree_command(vm, 23, Rc::clone(&arena), false);
    define_cursor_tree_command(vm, 24, Rc::clone(&arena), true);
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(25, move |script| {
            let cursor = pop_macro_handle(script, MACRO_TOKEN_CURSOR_INDEX, "token cursor")?;
            let expected = script.pop_string()?.to_string_lossy().into_owned();
            let mut arena = arena.borrow_mut();
            let cursor = arena
                .cursor_mut(cursor)
                .ok_or_else(|| unknown_cursor_handle(cursor))?;
            let matched = cursor
                .stream
                .trees()
                .get(cursor.position)
                .is_some_and(|tree| macro_tree_text(tree) == expected);
            if matched {
                cursor.position = cursor.position.saturating_add(1);
            }
            script.push_int(i32::from(matched));
            Ok(())
        });
    }
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(26, move |script| {
            let cursor = pop_macro_handle(script, MACRO_TOKEN_CURSOR_INDEX, "token cursor")?;
            let expected = script.pop_string()?.to_string_lossy().into_owned();
            let tree = {
                let mut arena = arena.borrow_mut();
                let cursor = arena
                    .cursor_mut(cursor)
                    .ok_or_else(|| unknown_cursor_handle(cursor))?;
                let tree = cursor
                    .stream
                    .trees()
                    .get(cursor.position)
                    .cloned()
                    .ok_or_else(|| crate::VmError::Setup {
                        message: format!("expected {expected:?}, reached end of macro input"),
                    })?;
                if macro_tree_text(&tree) != expected {
                    return Err(crate::VmError::Setup {
                        message: format!(
                            "expected {expected:?}, found {:?}",
                            macro_tree_text(&tree)
                        ),
                    });
                }
                cursor.position = cursor.position.saturating_add(1);
                tree
            };
            let handle = arena
                .borrow_mut()
                .insert(NwTokenStream::from_trees(vec![tree]))?;
            push_macro_tokenstream(script, handle);
            Ok(())
        });
    }
    for (command, kind) in [
        (27, CursorFragmentKind::Identifier),
        (28, CursorFragmentKind::Literal),
        (29, CursorFragmentKind::Tree),
        (30, CursorFragmentKind::Path),
        (31, CursorFragmentKind::Expression),
        (32, CursorFragmentKind::Type),
        (33, CursorFragmentKind::Statement),
        (34, CursorFragmentKind::Function),
        (35, CursorFragmentKind::Struct),
    ] {
        define_cursor_parse_command(vm, command, Rc::clone(&arena), source_id, kind);
    }
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(36, move |script| {
            let cursor_handle = pop_macro_handle(script, MACRO_TOKEN_CURSOR_INDEX, "token cursor")?;
            let kind_text = script.pop_string()?.to_string_lossy().into_owned();
            let kind =
                CursorFragmentKind::parse(&kind_text).ok_or_else(|| crate::VmError::Setup {
                    message: format!("unknown token-cursor fragment kind {kind_text:?}"),
                })?;
            let separator = script.pop_string()?.to_string_lossy().into_owned();
            let minimum = macro_index(script.pop_int()?, script)?;
            let maximum = macro_index(script.pop_int()?, script)?;
            let mut parsed = Vec::new();
            loop {
                if maximum != 0 && parsed.len() >= maximum {
                    break;
                }
                let checkpoint = {
                    let arena = arena.borrow();
                    arena
                        .cursor(cursor_handle)
                        .ok_or_else(|| unknown_cursor_handle(cursor_handle))?
                        .position
                };
                let value = {
                    let mut arena = arena.borrow_mut();
                    parse_cursor_fragment(
                        arena
                            .cursor_mut(cursor_handle)
                            .ok_or_else(|| unknown_cursor_handle(cursor_handle))?,
                        kind,
                        source_id,
                    )
                };
                let Ok(value) = value else {
                    if let Some(cursor) = arena.borrow_mut().cursor_mut(cursor_handle) {
                        cursor.position = checkpoint;
                    }
                    break;
                };
                let stream = arena.borrow_mut().insert(value)?;
                parsed.push(TokenStreamListValue::Stream(stream));
                let has_separator = {
                    let mut arena = arena.borrow_mut();
                    let cursor = arena
                        .cursor_mut(cursor_handle)
                        .ok_or_else(|| unknown_cursor_handle(cursor_handle))?;
                    let matched = cursor
                        .stream
                        .trees()
                        .get(cursor.position)
                        .is_some_and(|tree| macro_tree_text(tree) == separator);
                    if matched {
                        cursor.position = cursor.position.saturating_add(1);
                    }
                    matched
                };
                if !has_separator {
                    break;
                }
            }
            if parsed.len() < minimum {
                return Err(crate::VmError::Setup {
                    message: format!(
                        "expected at least {minimum} {} value(s), parsed {}",
                        kind.name(),
                        parsed.len()
                    ),
                });
            }
            let handle = arena.borrow_mut().insert_list(parsed)?;
            push_macro_handle(script, MACRO_TOKENSTREAM_LIST_INDEX, handle);
            Ok(())
        });
    }
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(37, move |script| {
            let cursor_handle = pop_macro_handle(script, MACRO_TOKEN_CURSOR_INDEX, "token cursor")?;
            let remaining = {
                let arena = arena.borrow();
                let cursor = arena
                    .cursor(cursor_handle)
                    .ok_or_else(|| unknown_cursor_handle(cursor_handle))?;
                NwTokenStream::from_trees(
                    cursor
                        .stream
                        .trees()
                        .get(cursor.position..)
                        .unwrap_or_default()
                        .to_vec(),
                )
            };
            let handle = arena.borrow_mut().insert(remaining)?;
            push_macro_tokenstream(script, handle);
            Ok(())
        });
    }
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(38, move |script| {
            let input = pop_macro_tokenstream_handle(script)?;
            let message = script.pop_string()?.to_string_lossy().into_owned();
            let span = arena
                .borrow()
                .balanced(input)
                .ok()
                .and_then(|stream| stream.trees().first().map(NwTokenTree::span));
            match span {
                Some(span) => Err(crate::VmError::MacroDiagnostic {
                    span,
                    message,
                }),
                None => Err(crate::VmError::Setup {
                    message: format!(
                        "procedural macro reported for tokenstream {input}: {message}"
                    ),
                }),
            }
        });
    }
}

fn define_token_project_commands(vm: &mut crate::Vm, arena: Rc<RefCell<TokenStreamArena>>) {
    {
        let arena = Rc::clone(&arena);
        vm.define_simple_command(39, move |script| {
            let list = pop_macro_handle(script, MACRO_TOKENSTREAM_LIST_INDEX, "tokenstream list")?;
            let values = arena
                .borrow()
                .list(list)
                .map(<[TokenStreamListValue]>::to_vec)
                .ok_or_else(|| crate::VmError::Setup {
                    message: format!("unknown compiler tokenstream-list handle {list}"),
                })?;
            let mut keyed = {
                let arena = arena.borrow();
                values
                    .into_iter()
                    .map(|value| {
                        let TokenStreamListValue::Stream(handle) = value else {
                            return Err(crate::VmError::Setup {
                                message: "cannot text-sort a nested tokenstream list".to_string(),
                            });
                        };
                        let key = arena
                            .balanced(handle)
                            .map(|stream| render_nwscript_tokens(&stream))
                            .map_err(macro_vm_error)?;
                        Ok((key, TokenStreamListValue::Stream(handle)))
                    })
                    .collect::<Result<Vec<_>, crate::VmError>>()?
            };
            keyed.sort_by(|left, right| left.0.cmp(&right.0));
            let values = keyed.into_iter().map(|(_key, value)| value).collect();
            let handle = arena.borrow_mut().insert_list(values)?;
            push_macro_handle(script, MACRO_TOKENSTREAM_LIST_INDEX, handle);
            Ok(())
        });
    }
    vm.define_simple_command(40, move |script| {
        let tree = pop_single_macro_tree(script, &arena)?;
        let NwTokenTree::Group(group) = tree else {
            return Err(crate::VmError::Setup {
                message: "token group contents require exactly one delimited group".to_string(),
            });
        };
        let handle = arena.borrow_mut().insert(group.stream)?;
        push_macro_tokenstream(script, handle);
        Ok(())
    });
}

fn define_cursor_tree_command(
    vm: &mut crate::Vm,
    command: u16,
    arena: Rc<RefCell<TokenStreamArena>>,
    advance: bool,
) {
    vm.define_simple_command(command, move |script| {
        let cursor_handle = pop_macro_handle(script, MACRO_TOKEN_CURSOR_INDEX, "token cursor")?;
        let tree = {
            let mut arena = arena.borrow_mut();
            let cursor = arena
                .cursor_mut(cursor_handle)
                .ok_or_else(|| unknown_cursor_handle(cursor_handle))?;
            let tree = cursor
                .stream
                .trees()
                .get(cursor.position)
                .cloned()
                .ok_or_else(|| crate::VmError::Setup {
                    message: "token cursor reached end of input".to_string(),
                })?;
            if advance {
                cursor.position = cursor.position.saturating_add(1);
            }
            tree
        };
        let handle = arena
            .borrow_mut()
            .insert(NwTokenStream::from_trees(vec![tree]))?;
        push_macro_tokenstream(script, handle);
        Ok(())
    });
}

fn define_cursor_parse_command(
    vm: &mut crate::Vm,
    command: u16,
    arena: Rc<RefCell<TokenStreamArena>>,
    source_id: SourceId,
    kind: CursorFragmentKind,
) {
    vm.define_simple_command(command, move |script| {
        let cursor_handle = pop_macro_handle(script, MACRO_TOKEN_CURSOR_INDEX, "token cursor")?;
        let parsed = {
            let mut arena = arena.borrow_mut();
            parse_cursor_fragment(
                arena
                    .cursor_mut(cursor_handle)
                    .ok_or_else(|| unknown_cursor_handle(cursor_handle))?,
                kind,
                source_id,
            )
            .map_err(macro_vm_error)?
        };
        let handle = arena.borrow_mut().insert(parsed)?;
        push_macro_tokenstream(script, handle);
        Ok(())
    });
}

fn unknown_cursor_handle(handle: u32) -> crate::VmError {
    crate::VmError::Setup {
        message: format!("unknown compiler token-cursor handle {handle}"),
    }
}

fn macro_tree_text(tree: &NwTokenTree) -> String {
    match tree {
        NwTokenTree::Token(token) => token.text.clone(),
        NwTokenTree::Group(_) => {
            render_nwscript_tokens(&NwTokenStream::from_trees(vec![tree.clone()]))
        }
    }
}

fn parse_cursor_fragment(
    cursor: &mut TokenCursorState,
    kind: CursorFragmentKind,
    source_id: SourceId,
) -> Result<NwTokenStream, MacroExpansionError> {
    let trees = cursor.stream.trees();
    let start = cursor.position;
    let first = trees.get(start).ok_or_else(|| {
        MacroExpansionError::without_span(format!("expected {}, reached end of input", kind.name()))
    })?;
    let end = match kind {
        CursorFragmentKind::Tree => start.saturating_add(1),
        CursorFragmentKind::Identifier => match first {
            NwTokenTree::Token(token) if token.kind == TokenKind::Identifier => start + 1,
            _ => return Err(expected_cursor_fragment(first, kind)),
        },
        CursorFragmentKind::Literal => match first {
            NwTokenTree::Token(token) if literal_kind(&token.kind) => start + 1,
            _ => return Err(expected_cursor_fragment(first, kind)),
        },
        CursorFragmentKind::Path => cursor_path_end(trees, start)?,
        CursorFragmentKind::Type => cursor_type_end(trees, start)?,
        CursorFragmentKind::Expression => trees
            .iter()
            .enumerate()
            .skip(start)
            .find_map(|(index, tree)| match tree {
                NwTokenTree::Token(token)
                    if matches!(token.kind, TokenKind::Comma | TokenKind::Semicolon) =>
                {
                    Some(index)
                }
                _ => None,
            })
            .unwrap_or(trees.len()),
        CursorFragmentKind::Statement => cursor_statement_end(trees, start)?,
        CursorFragmentKind::Function => cursor_body_item_end(trees, start, false)?,
        CursorFragmentKind::Struct => cursor_body_item_end(trees, start, true)?,
    };
    if end <= start {
        return Err(expected_cursor_fragment(first, kind));
    }
    let stream = NwTokenStream::from_trees(
        trees
            .get(start..end)
            .ok_or_else(|| expected_cursor_fragment(first, kind))?
            .to_vec(),
    );
    validate_cursor_fragment(&stream, kind, source_id)?;
    cursor.position = end;
    Ok(stream)
}

fn expected_cursor_fragment(tree: &NwTokenTree, kind: CursorFragmentKind) -> MacroExpansionError {
    MacroExpansionError::new(
        tree.span(),
        format!(
            "expected {}, found {:?}",
            kind.name(),
            macro_tree_text(tree)
        ),
    )
}

fn cursor_path_end(trees: &[NwTokenTree], start: usize) -> Result<usize, MacroExpansionError> {
    let Some(NwTokenTree::Token(first)) = trees.get(start) else {
        return Err(MacroExpansionError::without_span(
            "expected identifier path",
        ));
    };
    if first.kind != TokenKind::Identifier {
        return Err(MacroExpansionError::new(
            first.span,
            "expected identifier path",
        ));
    }
    let mut end = start + 1;
    while let (
        Some(NwTokenTree::Token(left)),
        Some(NwTokenTree::Token(right)),
        Some(NwTokenTree::Token(segment)),
    ) = (trees.get(end), trees.get(end + 1), trees.get(end + 2))
    {
        if left.kind != TokenKind::Colon
            || right.kind != TokenKind::Colon
            || segment.kind != TokenKind::Identifier
        {
            break;
        }
        end += 3;
    }
    Ok(end)
}

fn cursor_type_end(trees: &[NwTokenTree], start: usize) -> Result<usize, MacroExpansionError> {
    let Some(NwTokenTree::Token(first)) = trees.get(start) else {
        return Err(MacroExpansionError::without_span("expected NWScript type"));
    };
    if first.text == "struct" {
        let Some(NwTokenTree::Token(name)) = trees.get(start + 1) else {
            return Err(MacroExpansionError::new(
                first.span,
                "struct type requires a name",
            ));
        };
        if name.kind != TokenKind::Identifier {
            return Err(MacroExpansionError::new(
                name.span,
                "struct type requires a name",
            ));
        }
        return Ok(start + 2);
    }
    if matches!(first.kind, TokenKind::Identifier | TokenKind::Keyword(_)) {
        Ok(start + 1)
    } else {
        Err(MacroExpansionError::new(
            first.span,
            "expected NWScript type",
        ))
    }
}

fn cursor_statement_end(trees: &[NwTokenTree], start: usize) -> Result<usize, MacroExpansionError> {
    if matches!(trees.get(start), Some(NwTokenTree::Group(group)) if group.delimiter == NwDelimiter::Brace)
    {
        return Ok(start + 1);
    }
    let first_text = trees.get(start).map(macro_tree_text).unwrap_or_default();
    if first_text == "if" {
        let then_start = cursor_control_body_start(trees, start)?;
        let then_end = cursor_statement_end(trees, then_start)?;
        if trees
            .get(then_end)
            .is_some_and(|tree| macro_tree_text(tree) == "else")
        {
            return cursor_statement_end(trees, then_end.saturating_add(1));
        }
        return Ok(then_end);
    }
    if matches!(first_text.as_str(), "while" | "for" | "switch") {
        let body_start = cursor_control_body_start(trees, start)?;
        return cursor_statement_end(trees, body_start);
    }
    if first_text == "do" {
        let body_end = cursor_statement_end(trees, start.saturating_add(1))?;
        let valid_tail = trees
            .get(body_end)
            .is_some_and(|tree| macro_tree_text(tree) == "while")
            && matches!(trees.get(body_end + 1), Some(NwTokenTree::Group(group)) if group.delimiter == NwDelimiter::Parenthesis)
            && matches!(trees.get(body_end + 2), Some(NwTokenTree::Token(token)) if token.kind == TokenKind::Semicolon);
        if valid_tail {
            return Ok(body_end + 3);
        }
        return Err(expected_cursor_fragment(
            trees.get(start).ok_or_else(|| {
                MacroExpansionError::without_span("expected NWScript do/while statement")
            })?,
            CursorFragmentKind::Statement,
        ));
    }
    for (index, tree) in trees.iter().enumerate().skip(start) {
        match tree {
            NwTokenTree::Token(token) if token.kind == TokenKind::Semicolon => {
                return Ok(index + 1);
            }
            _ => {}
        }
    }
    Err(expected_cursor_fragment(
        trees
            .get(start)
            .ok_or_else(|| MacroExpansionError::without_span("expected NWScript statement"))?,
        CursorFragmentKind::Statement,
    ))
}

fn cursor_control_body_start(
    trees: &[NwTokenTree],
    start: usize,
) -> Result<usize, MacroExpansionError> {
    if matches!(trees.get(start + 1), Some(NwTokenTree::Group(group)) if group.delimiter == NwDelimiter::Parenthesis)
    {
        Ok(start + 2)
    } else {
        Err(expected_cursor_fragment(
            trees.get(start).ok_or_else(|| {
                MacroExpansionError::without_span("expected NWScript control statement")
            })?,
            CursorFragmentKind::Statement,
        ))
    }
}

fn cursor_body_item_end(
    trees: &[NwTokenTree],
    start: usize,
    require_struct: bool,
) -> Result<usize, MacroExpansionError> {
    let first = trees
        .get(start)
        .ok_or_else(|| MacroExpansionError::without_span("expected body item"))?;
    if require_struct && macro_tree_text(first) != "struct" {
        return Err(expected_cursor_fragment(first, CursorFragmentKind::Struct));
    }
    let body_end = trees
        .iter()
        .enumerate()
        .skip(start)
        .find_map(|(index, tree)| {
            matches!(tree, NwTokenTree::Group(group) if group.delimiter == NwDelimiter::Brace)
                .then_some(index + 1)
        })
        .ok_or_else(|| {
            expected_cursor_fragment(
                first,
                if require_struct {
                    CursorFragmentKind::Struct
                } else {
                    CursorFragmentKind::Function
                },
            )
        })?;
    if !require_struct {
        return Ok(body_end);
    }
    if matches!(trees.get(body_end), Some(NwTokenTree::Token(token)) if token.kind == TokenKind::Semicolon)
    {
        Ok(body_end + 1)
    } else {
        Err(MacroExpansionError::new(
            first.span(),
            "NWScript struct declaration requires a trailing `;`",
        ))
    }
}

fn validate_cursor_fragment(
    stream: &NwTokenStream,
    kind: CursorFragmentKind,
    source_id: SourceId,
) -> Result<(), MacroExpansionError> {
    let source = render_nwscript_tokens(stream);
    let wrapped = match kind {
        CursorFragmentKind::Expression => format!("void main() {{ {source}; }}"),
        CursorFragmentKind::Statement => format!("void main() {{ {source} }}"),
        CursorFragmentKind::Function | CursorFragmentKind::Struct => source,
        CursorFragmentKind::Identifier
        | CursorFragmentKind::Literal
        | CursorFragmentKind::Tree
        | CursorFragmentKind::Path
        | CursorFragmentKind::Type => return Ok(()),
    };
    let tokens = crate::lex_text(source_id, &wrapped).map_err(|error| {
        MacroExpansionError::without_span(format!("failed to lex {}: {error}", kind.name()))
    })?;
    crate::parse_tokens(tokens, None).map_err(|error| {
        MacroExpansionError::new(
            stream
                .trees()
                .first()
                .map_or(Span::new(source_id, 0, 0), NwTokenTree::span),
            format!("invalid {}: {error}", kind.name()),
        )
    })?;
    Ok(())
}

fn macro_index(index: i32, script: &crate::VmScript) -> Result<usize, crate::VmError> {
    usize::try_from(index).map_err(|error| crate::VmError::Setup {
        message: format!(
            "negative tokenstream index {index} at instruction {}: {error}",
            script.ip()
        ),
    })
}

fn pop_single_macro_tree(
    script: &mut crate::VmScript,
    arena: &RefCell<TokenStreamArena>,
) -> Result<NwTokenTree, crate::VmError> {
    let handle = pop_macro_tokenstream_handle(script)?;
    let arena = arena.borrow();
    let stream = arena.balanced(handle).map_err(macro_vm_error)?;
    let [tree] = stream.trees() else {
        return Err(crate::VmError::Setup {
            message: format!(
                "token inspection requires exactly one token tree, received {}",
                stream.len()
            ),
        });
    };
    Ok(tree.clone())
}

fn macro_tree_kind(tree: &NwTokenTree) -> &'static str {
    match tree {
        NwTokenTree::Group(_) => "group",
        NwTokenTree::Token(token) => match token.kind {
            TokenKind::Eof => "eof",
            TokenKind::Identifier => "identifier",
            TokenKind::Integer => "integer",
            TokenKind::HexInteger => "hex_integer",
            TokenKind::BinaryInteger => "binary_integer",
            TokenKind::OctalInteger => "octal_integer",
            TokenKind::Float => "float",
            TokenKind::String => "string",
            TokenKind::HashedString => "hashed_string",
            TokenKind::Keyword(_) => "keyword",
            _ => "punctuation",
        },
    }
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
                    _ => unreachable!("validated quote repetition quantifier"),
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

fn render_flat_tokens(tokens: &[Token]) -> String {
    let mut rendered = String::new();
    for (index, token) in tokens.iter().enumerate() {
        let follows_attribute_hash = tokens
            .get(index.saturating_sub(1))
            .is_some_and(|previous| previous.kind == TokenKind::Hash);
        if index > 0 && !rendered.ends_with('\n') && !follows_attribute_hash {
            rendered.push(' ');
        }
        match token.kind {
            TokenKind::String => render_string_literal(&mut rendered, &token.text),
            _ => rendered.push_str(&token.text),
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
            rendered.push('\n');
        }
    }
    rendered
}

fn render_string_literal(output: &mut String, value: &str) {
    output.push('"');
    for character in value.chars() {
        match character {
            '\n' => output.push_str("\\n"),
            '\\' => output.push_str("\\\\"),
            '"' => output.push_str("\\\""),
            character if character.is_ascii_control() => {
                use fmt::Write as _;
                let _ = write!(output, "\\x{:02x}", u32::from(character));
            }
            character => output.push(character),
        }
    }
    output.push('"');
}

#[cfg(test)]
mod event_attribute_tests {
    use super::erase_nwnrs_macro_attributes;
    use crate::{
        MacroExpansionOptions, MacroRegistry, NwTokenStream, SourceId, expand_source_macros,
        lex_text, render_nwscript_tokens,
    };

    #[test]
    fn collects_and_erases_module_load_attributes() -> Result<(), Box<dyn std::error::Error>> {
        let tokens = lex_text(
            SourceId::new(0),
            "#[nwnrs::events(module_load)]\nvoid ProjectStart(json jEvent) {}",
        )?;
        let mut stream = NwTokenStream::from_tokens(&tokens)?;
        erase_nwnrs_macro_attributes(&mut stream)?;
        let rendered = render_nwscript_tokens(&stream);
        assert!(!rendered.contains("#["));
        assert!(rendered.contains("ProjectStart"));
        Ok(())
    }

    #[test]
    fn rejects_unknown_compiler_attribute_routes() -> Result<(), Box<dyn std::error::Error>> {
        let tokens = lex_text(
            SourceId::new(0),
            "#[other::attribute(value)]\nvoid ProjectStart() {}",
        )?;
        let mut stream = NwTokenStream::from_tokens(&tokens)?;
        let error = erase_nwnrs_macro_attributes(&mut stream)
            .expect_err("unknown attribute route should be rejected");
        assert!(error.message.contains("only `#[nwnrs::events(...)]`"));
        Ok(())
    }

    #[test]
    fn erases_event_attributes_emitted_by_bang_macros() -> Result<(), Box<dyn std::error::Error>> {
        let tokens = lex_text(
            SourceId::new(0),
            r#"
                macro_rules! handler {
                    () => {
                        #[nwnrs::events(module_load)]
                        void Generated(json event) {}
                    };
                }
                handler!()
            "#,
        )?;
        let expanded = expand_source_macros(
            tokens,
            &mut MacroRegistry::new(),
            MacroExpansionOptions::default(),
        )?;
        let stream = NwTokenStream::from_tokens(&expanded)?;
        let rendered = render_nwscript_tokens(&stream);
        assert!(!rendered.contains("#["));
        assert!(rendered.contains("Generated"));
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct DeclarativeMacro {
    path:  MacroPath,
    rules: Vec<DeclarativeRule>,
}

impl DeclarativeMacro {
    fn parse(path: &MacroPath, body: &NwTokenGroup) -> Result<Self, MacroExpansionError> {
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

fn rebase_source_id(stream: &mut NwTokenStream, source_id: SourceId, span: Span) {
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

fn literal_kind(kind: &TokenKind) -> bool {
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

fn quote_stream(
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

fn repetition_suffix(
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

struct MacroExpander<'a> {
    registry: &'a MacroRegistry,
    options:  MacroExpansionOptions,
    stack:    Vec<MacroPath>,
    trace:    bool,
    traces:   Vec<MacroExpansionTrace>,
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

fn parse_macro_invocation(
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

fn parse_tree_level(
    tokens: &[Token],
    position: &mut usize,
    expected: Option<(NwDelimiter, Span)>,
) -> Result<NwTokenStream, MacroExpansionError> {
    let mut trees = Vec::new();
    while let Some(token) = tokens.get(*position) {
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
            let stream = parse_tree_level(tokens, position, Some((delimiter, open_span)))?;
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

fn valid_identifier(value: &str) -> bool {
    let mut bytes = value.bytes();
    bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

#[cfg(test)]
mod tests {
    use super::{
        IdentityMacro, MacroExpansionOptions, MacroPath, MacroRegistry, NwDelimiter, NwTokenStream,
        QUOTE_CONCAT_FUNCTION, QUOTE_REPEAT_FUNCTION, QUOTE_STATIC_FUNCTION, QuoteBindings,
        expand_bang_macros, expand_source_macros, expand_source_macros_traced, quote_nwscript,
        register_compiler_macros, register_nwscript_macro, render_nwscript_tokens,
    };
    use crate::{SourceFile, SourceId, TokenKind, lex_text, parse_tokens};

    fn lex(input: &str) -> Vec<crate::Token> {
        match lex_text(SourceId::new(1), input) {
            Ok(tokens) => tokens,
            Err(error) => unreachable!("fixture should lex: {error}"),
        }
    }

    fn stream(input: &str) -> NwTokenStream {
        match NwTokenStream::from_tokens(&lex(input)) {
            Ok(stream) => stream,
            Err(error) => unreachable!("fixture should balance: {error}"),
        }
    }

    #[test]
    fn balances_and_flattens_nested_groups() {
        let tokens = lex("call(alpha[1], { beta(); });");
        let stream = match NwTokenStream::from_tokens(&tokens) {
            Ok(stream) => stream,
            Err(error) => unreachable!("fixture should balance: {error}"),
        };
        let flattened = stream.into_tokens();
        let expected = tokens
            .into_iter()
            .filter(|token| token.kind != TokenKind::Eof)
            .collect::<Vec<_>>();
        assert_eq!(flattened, expected);
    }

    #[test]
    fn rejects_mismatched_delimiters() {
        let error = NwTokenStream::from_tokens(&lex("call([1));"));
        assert!(error.is_err());
    }

    #[test]
    fn expands_namespaced_bang_macros_recursively() {
        let mut registry = MacroRegistry::new();
        assert!(
            registry
                .register_path("nwnrs::identity", IdentityMacro)
                .is_ok()
        );
        let expanded = match expand_bang_macros(
            lex("void main() { nwnrs::identity!(nwnrs::identity!(DoThing())); }"),
            &registry,
            MacroExpansionOptions::default(),
        ) {
            Ok(tokens) => tokens,
            Err(error) => unreachable!("fixture should expand: {error}"),
        };
        assert!(expanded.iter().any(|token| token.text == "DoThing"));
        assert!(!expanded.iter().any(|token| token.text == "identity"));
    }

    #[test]
    fn preserves_all_three_invocation_delimiters() {
        let mut registry = MacroRegistry::new();
        registry.register(
            match MacroPath::parse("identity") {
                Ok(path) => path,
                Err(error) => unreachable!("path should parse: {error}"),
            },
            IdentityMacro,
        );
        for (source, delimiter) in [
            ("identity!(1)", NwDelimiter::Parenthesis),
            ("identity![1]", NwDelimiter::Bracket),
            ("identity!{1}", NwDelimiter::Brace),
        ] {
            let trees = match NwTokenStream::from_tokens(&lex(source)) {
                Ok(stream) => stream,
                Err(error) => unreachable!("fixture should balance: {error}"),
            };
            let parsed = super::parse_macro_invocation(trees.trees(), 0);
            match parsed {
                Ok(Some((invocation, _))) => assert_eq!(invocation.delimiter, delimiter),
                Ok(None) => unreachable!("fixture should be a macro invocation"),
                Err(error) => unreachable!("fixture should parse: {error}"),
            }
        }
    }

    #[test]
    fn reports_unknown_macro_with_its_path() {
        let error = expand_bang_macros(
            lex("void main() { missing!(); }"),
            &MacroRegistry::new(),
            MacroExpansionOptions::default(),
        );
        match error {
            Ok(_) => unreachable!("unknown macro should fail"),
            Err(error) => assert!(error.to_string().contains("unknown macro `missing`")),
        }
    }

    #[test]
    fn quote_interpolates_single_and_repeated_bindings() {
        let mut bindings = QuoteBindings::new();
        bindings.insert("name", stream("Generated"));
        bindings.insert_repeated(
            "arguments",
            vec![stream("first"), stream("second"), stream("third")],
        );
        let quoted = match quote_nwscript(
            &stream("void $name($($arguments),*) { $$ignored; }"),
            &bindings,
        ) {
            Ok(tokens) => tokens,
            Err(error) => unreachable!("fixture should quote: {error}"),
        };
        let texts = quoted
            .into_tokens()
            .into_iter()
            .map(|token| token.text)
            .collect::<Vec<_>>();
        assert_eq!(
            texts,
            vec![
                "void",
                "Generated",
                "(",
                "first",
                ",",
                "second",
                ",",
                "third",
                ")",
                "{",
                "$",
                "ignored",
                ";",
                "}"
            ]
        );
    }

    #[test]
    fn quote_rejects_mismatched_zipped_repetition_lengths() {
        let mut bindings = QuoteBindings::new();
        bindings.insert_repeated("left", vec![stream("A"), stream("B")]);
        bindings.insert_repeated("right", vec![stream("1")]);
        let error = quote_nwscript(&stream("$($left = $right;)*"), &bindings)
            .expect_err("zipped bindings with different lengths should fail");
        assert!(error.to_string().contains("different lengths"));
    }

    #[test]
    fn source_defined_macro_rules_expand_into_parseable_nwscript() {
        let source = r#"
            macro_rules! make_handler {
                ($name:ident, $body:tokens) => {
                    void $name() { $body }
                };
            }

            make_handler!(main, int value = 7;)
        "#;
        let expanded = match expand_source_macros(
            lex(source),
            &mut MacroRegistry::new(),
            MacroExpansionOptions::default(),
        ) {
            Ok(tokens) => tokens,
            Err(error) => unreachable!("fixture should expand: {error}"),
        };
        let script = match parse_tokens(expanded, None) {
            Ok(script) => script,
            Err(error) => unreachable!("expanded fixture should parse: {error}"),
        };
        assert_eq!(script.items.len(), 1);
    }

    #[test]
    fn declarative_macro_static_tokens_use_the_invocation_span() {
        let source =
            "macro_rules! make { ($name:ident) => { void $name() {} }; }\nmake!(Generated)";
        let original = lex(source);
        let invocation = original
            .iter()
            .rev()
            .find(|token| token.text == "make")
            .expect("invocation token")
            .span;
        let captured = original
            .iter()
            .find(|token| token.text == "Generated")
            .expect("captured token")
            .span;
        let expanded = expand_source_macros(
            original,
            &mut MacroRegistry::new(),
            MacroExpansionOptions::default(),
        )
        .expect("expand declarative macro");

        let static_span = expanded
            .iter()
            .find(|token| token.text == "void")
            .expect("static output token")
            .span;
        assert_eq!(static_span.source_id, invocation.source_id);
        assert_eq!(static_span.start, invocation.start);
        assert!(static_span.end > invocation.end);
        assert_eq!(
            expanded
                .iter()
                .find(|token| token.text == "Generated")
                .expect("captured output token")
                .span,
            captured
        );
    }

    #[test]
    fn source_defined_macros_can_expand_other_macros() {
        let source = r#"
            macro_rules! inner {
                ($value:literal) => { $value };
            }
            macro_rules! outer {
                ($value:literal) => { inner!($value) };
            }
            void main() { int value = outer!(11); }
        "#;
        let expanded = match expand_source_macros(
            lex(source),
            &mut MacroRegistry::new(),
            MacroExpansionOptions::default(),
        ) {
            Ok(tokens) => tokens,
            Err(error) => unreachable!("fixture should expand recursively: {error}"),
        };
        assert!(expanded.iter().any(|token| token.text == "11"));
        assert!(!expanded.iter().any(|token| token.text == "inner"));
        assert!(!expanded.iter().any(|token| token.text == "outer"));
    }

    #[test]
    fn traced_expansion_records_immediate_nested_outputs() {
        let source = r#"
            macro_rules! inner { () => { 7 }; }
            macro_rules! outer { () => { inner!() }; }
            void main() { int value = outer!(); }
        "#;
        let (_expanded, trace) = expand_source_macros_traced(
            lex(source),
            &mut MacroRegistry::new(),
            MacroExpansionOptions::default(),
        )
        .expect("traced macros should expand");
        let [outer, inner] = trace.as_slice() else {
            panic!("expected exactly two trace records");
        };
        assert_eq!(outer.path.to_string(), "outer");
        assert_eq!(outer.depth, 1);
        assert!(render_nwscript_tokens(&outer.output).contains("inner"));
        assert_eq!(inner.path.to_string(), "inner");
        assert_eq!(inner.depth, 2);
    }

    #[test]
    fn declarative_matcher_repetition_supports_separators_and_quantifiers() {
        let source = r#"
            macro_rules! call_all {
                ($($name:ident),+) => {
                    void main() { $($name();)* }
                };
            }
            call_all!(First, Second, Third)
        "#;
        let expanded = expand_source_macros(
            lex(source),
            &mut MacroRegistry::new(),
            MacroExpansionOptions::default(),
        )
        .expect("repeated declarative matcher should expand");
        let names = expanded
            .iter()
            .filter(|token| matches!(token.text.as_str(), "First" | "Second" | "Third"))
            .map(|token| token.text.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, ["First", "Second", "Third"]);
        assert!(parse_tokens(expanded, None).is_ok());
    }

    #[test]
    fn declarative_matcher_repetition_handles_empty_optional_and_required_inputs() {
        let source = r#"
            macro_rules! optional_constant {
                ($($value:literal)?) => {
                    $(const int OPTIONAL = $value;)?
                    void main() {}
                };
            }
            optional_constant!()
        "#;
        let expanded = expand_source_macros(
            lex(source),
            &mut MacroRegistry::new(),
            MacroExpansionOptions::default(),
        )
        .expect("empty optional repetition should expand");
        assert!(!expanded.iter().any(|token| token.text == "OPTIONAL"));
        assert!(parse_tokens(expanded, None).is_ok());

        let present = source.replace("optional_constant!()", "optional_constant!(9)");
        let expanded = expand_source_macros(
            lex(&present),
            &mut MacroRegistry::new(),
            MacroExpansionOptions::default(),
        )
        .expect("present optional repetition should expand");
        assert!(expanded.iter().any(|token| token.text == "OPTIONAL"));
        assert!(parse_tokens(expanded, None).is_ok());

        let required = r#"
            macro_rules! at_least_one {
                ($($value:literal),+) => { void main() {} };
            }
            at_least_one!()
        "#;
        assert!(
            expand_source_macros(
                lex(required),
                &mut MacroRegistry::new(),
                MacroExpansionOptions::default(),
            )
            .is_err()
        );
    }

    #[test]
    fn declarative_repetition_supports_nested_and_zipped_bindings() {
        let source = r#"
            macro_rules! constants {
                ($(($($name:ident = $value:literal),*));*) => {
                    $($(const int $name = $value;)*)*
                    void main() {}
                };
            }
            constants!((FIRST = 1, SECOND = 2); (THIRD = 3))
        "#;
        let expanded = expand_source_macros(
            lex(source),
            &mut MacroRegistry::new(),
            MacroExpansionOptions::default(),
        )
        .expect("nested declarative matcher should expand");
        for expected in ["FIRST", "1", "SECOND", "2", "THIRD", "3"] {
            assert!(expanded.iter().any(|token| token.text == expected));
        }
        assert!(parse_tokens(expanded, None).is_ok());
    }

    #[test]
    fn compiler_quote_lowers_interpolation_to_tokenstream_calls() {
        let mut registry = MacroRegistry::new();
        assert!(register_compiler_macros(&mut registry).is_ok());
        let expanded = match expand_bang_macros(
            lex(
                "tokenstream Build(tokenstream name, tokenstream body) { return quote!{ void \
                 $name() { $body } }; }",
            ),
            &registry,
            MacroExpansionOptions::default(),
        ) {
            Ok(tokens) => tokens,
            Err(error) => unreachable!("fixture should lower quote: {error}"),
        };
        assert!(
            expanded
                .iter()
                .any(|token| token.text == QUOTE_STATIC_FUNCTION)
        );
        assert!(
            expanded
                .iter()
                .any(|token| token.text == QUOTE_CONCAT_FUNCTION)
        );
        assert!(!expanded.iter().any(|token| token.text == "quote"));
        assert!(!expanded.iter().any(|token| token.kind == TokenKind::Dollar));
    }

    #[test]
    fn renderer_round_trips_strings_and_extended_attributes() {
        let original =
            stream(r#"#[nwnrs::event(module_load)] void handler(string value = "a\n\"\\");"#);
        let rendered = render_nwscript_tokens(&original);
        let reparsed = stream(&rendered);
        assert_eq!(
            original
                .into_tokens()
                .into_iter()
                .map(|token| (token.kind, token.text))
                .collect::<Vec<_>>(),
            reparsed
                .into_tokens()
                .into_iter()
                .map(|token| (token.kind, token.text))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn compiler_quote_lowers_repetition_to_collection_runtime() {
        let mut registry = MacroRegistry::new();
        assert!(register_compiler_macros(&mut registry).is_ok());
        let expanded = expand_bang_macros(
            lex("tokenstream Build(tokenstream_list values) { return quote!($($values),*); }"),
            &registry,
            MacroExpansionOptions::default(),
        )
        .expect("compiler quote repetition should lower");
        assert!(
            expanded
                .iter()
                .any(|token| token.text == QUOTE_REPEAT_FUNCTION)
        );
        assert!(!expanded.iter().any(|token| token.kind == TokenKind::Dollar));
    }

    #[test]
    fn nwscript_procedural_quote_repeats_and_zips_tokenstream_lists() {
        let source = r#"
            proc_macro! make_constants {
                tokenstream make_constants(tokenstream input) {
                    tokenstream_list names = __NWNRS_TokenStreamListNew();
                    tokenstream_list values = __NWNRS_TokenStreamListNew();
                    names = __NWNRS_TokenStreamListPush(
                        names, __NWNRS_TokenStreamGet(input, 0));
                    names = __NWNRS_TokenStreamListPush(
                        names, __NWNRS_TokenStreamGet(input, 2));
                    values = __NWNRS_TokenStreamListPush(
                        values, __NWNRS_TokenStreamGet(input, 1));
                    values = __NWNRS_TokenStreamListPush(
                        values, __NWNRS_TokenStreamGet(input, 3));
                    return quote! { $(const int $names = $values;)* };
                }
            }

            make_constants!(FIRST 1 SECOND 2)
            void main() {}
        "#;
        let expanded = expand_source_macros(
            lex(source),
            &mut MacroRegistry::new(),
            MacroExpansionOptions::default(),
        )
        .expect("procedural quote repetition should execute");
        for expected in ["FIRST", "1", "SECOND", "2"] {
            assert!(expanded.iter().any(|token| token.text == expected));
        }
        assert!(parse_tokens(expanded, None).is_ok());
    }

    #[test]
    fn nwscript_procedural_quote_supports_nested_tokenstream_lists() {
        let source = r#"
            proc_macro! make_constants {
                tokenstream make_constants(tokenstream input) {
                    tokenstream_list first = __NWNRS_TokenStreamListNew();
                    first = __NWNRS_TokenStreamListPush(
                        first, __NWNRS_TokenStreamGet(input, 0));
                    first = __NWNRS_TokenStreamListPush(
                        first, __NWNRS_TokenStreamGet(input, 1));

                    tokenstream_list second = __NWNRS_TokenStreamListNew();
                    second = __NWNRS_TokenStreamListPush(
                        second, __NWNRS_TokenStreamGet(input, 2));

                    tokenstream_list rows = __NWNRS_TokenStreamListNew();
                    rows = __NWNRS_TokenStreamListPushList(rows, first);
                    rows = __NWNRS_TokenStreamListPushList(rows, second);
                    return quote! { $($(const int $rows = 1;)*)* void main() {} };
                }
            }

            make_constants!(FIRST SECOND THIRD)
        "#;
        let expanded = expand_source_macros(
            lex(source),
            &mut MacroRegistry::new(),
            MacroExpansionOptions::default(),
        )
        .expect("nested procedural quote repetition should execute");
        for expected in ["FIRST", "SECOND", "THIRD"] {
            assert!(expanded.iter().any(|token| token.text == expected));
        }
        assert!(parse_tokens(expanded, None).is_ok());
    }

    #[test]
    fn nwscript_procedural_macro_can_parse_with_a_token_cursor() {
        let source = r#"
            proc_macro! rebuild_function {
                tokenstream rebuild_function(tokenstream input) {
                    token_cursor cursor = __NWNRS_TokenCursorNew(input);
                    tokenstream function = __NWNRS_TokenCursorParseFunction(cursor);
                    if (!__NWNRS_TokenCursorIsEnd(cursor)) {
                        __NWNRS_MacroErrorAt(
                            __NWNRS_TokenCursorRemaining(cursor),
                            "expected exactly one function");
                    }
                    return quote! { $function };
                }
            }

            rebuild_function!(void Generated(int value) { int copy = value; })
        "#;
        let expanded = expand_source_macros(
            lex(source),
            &mut MacroRegistry::new(),
            MacroExpansionOptions::default(),
        )
        .expect("token cursor should parse a function");
        assert!(expanded.iter().any(|token| token.text == "Generated"));
        assert!(parse_tokens(expanded, None).is_ok());
    }

    #[test]
    fn nwscript_token_cursor_parses_separated_expressions_for_quote_repetition() {
        let source = r#"
            proc_macro! make_body {
                tokenstream make_body(tokenstream input) {
                    token_cursor cursor = __NWNRS_TokenCursorNew(input);
                    tokenstream_list expressions = __NWNRS_TokenCursorParseSeparated(
                        cursor, "expr", ",", 1, 0);
                    if (!__NWNRS_TokenCursorIsEnd(cursor)) {
                        __NWNRS_MacroErrorAt(
                            __NWNRS_TokenCursorRemaining(cursor),
                            "unexpected input");
                    }
                    return quote! { void main() { $($expressions;)* } };
                }
            }

            make_body!(First(), Second(2), 3 + 4)
        "#;
        let expanded = expand_source_macros(
            lex(source),
            &mut MacroRegistry::new(),
            MacroExpansionOptions::default(),
        )
        .expect("token cursor should parse separated expressions");
        for expected in ["First", "Second", "3", "4"] {
            assert!(expanded.iter().any(|token| token.text == expected));
        }
        assert!(parse_tokens(expanded, None).is_ok());
    }

    #[test]
    fn nwscript_token_cursor_keeps_if_else_as_one_statement() {
        let source = r#"
            proc_macro! parse_statements {
                tokenstream parse_statements(tokenstream input) {
                    token_cursor cursor = __NWNRS_TokenCursorNew(input);
                    tokenstream first = __NWNRS_TokenCursorParseStatement(cursor);
                    tokenstream second = __NWNRS_TokenCursorParseStatement(cursor);
                    if (!__NWNRS_TokenCursorIsEnd(cursor)) {
                        __NWNRS_MacroError("expected exactly two statements");
                    }
                    return quote! { void main() { $first $second } };
                }
            }

            parse_statements!(
                if (TRUE) { First(); } else if (FALSE) { Second(); } else { Third(); }
                int value = 7;
            )
        "#;
        let expanded = expand_source_macros(
            lex(source),
            &mut MacroRegistry::new(),
            MacroExpansionOptions::default(),
        )
        .expect("token cursor should consume a complete if/else statement");
        for expected in ["First", "Second", "Third", "value"] {
            assert!(expanded.iter().any(|token| token.text == expected));
        }
        assert!(parse_tokens(expanded, None).is_ok());
    }

    #[test]
    fn nwscript_token_cursor_exposes_named_fragment_parsers() {
        let source = r#"
            proc_macro! parse_fragments {
                tokenstream parse_fragments(tokenstream input) {
                    token_cursor cursor = __NWNRS_TokenCursorNew(input);
                    tokenstream name = __NWNRS_TokenCursorParseIdentifier(cursor);
                    __NWNRS_TokenCursorExpect(cursor, ",");
                    tokenstream value = __NWNRS_TokenCursorParseLiteral(cursor);
                    __NWNRS_TokenCursorExpect(cursor, ",");
                    tokenstream path = __NWNRS_TokenCursorParsePath(cursor);
                    __NWNRS_TokenCursorExpect(cursor, ",");
                    tokenstream type = __NWNRS_TokenCursorParseType(cursor);
                    __NWNRS_TokenCursorExpect(cursor, ",");
                    tokenstream tree = __NWNRS_TokenCursorParseTree(cursor);
                    __NWNRS_TokenCursorExpect(cursor, ",");
                    tokenstream structure = __NWNRS_TokenCursorParseStruct(cursor);
                    if (!__NWNRS_TokenCursorIsEnd(cursor)) {
                        __NWNRS_MacroError("unexpected fragment input");
                    }
                    return quote! {
                        $structure
                        const int $name = $value;
                        void main() {}
                    };
                }
            }

            parse_fragments!(
                Generated, 17, project::value, struct Example, (one + two),
                struct GeneratedStruct { int member; };
            )
        "#;
        let expanded = expand_source_macros(
            lex(source),
            &mut MacroRegistry::new(),
            MacroExpansionOptions::default(),
        )
        .expect("named cursor fragment parsers should execute");
        for expected in ["Generated", "17", "GeneratedStruct", "member"] {
            assert!(expanded.iter().any(|token| token.text == expected));
        }
        assert!(parse_tokens(expanded, None).is_ok());
    }

    #[test]
    fn nwscript_project_macro_can_open_groups_and_sort_collections() {
        let source = r#"
            proc_macro! sorted_calls {
                tokenstream sorted_calls(tokenstream input) {
                    token_cursor outer = __NWNRS_TokenCursorNew(input);
                    tokenstream group = __NWNRS_TokenCursorParseTree(outer);
                    token_cursor inner = __NWNRS_TokenCursorNew(
                        __NWNRS_TokenGroupContents(group));
                    tokenstream_list names = __NWNRS_TokenCursorParseSeparated(
                        inner, "ident", ",", 1, 0);
                    names = __NWNRS_TokenStreamListSort(names);
                    return quote! { void main() { $($names();)* } };
                }
            }
            sorted_calls!({Zulu, Alpha, Middle})
        "#;
        let expanded = expand_source_macros(
            lex(source),
            &mut MacroRegistry::new(),
            MacroExpansionOptions::default(),
        )
        .expect("project token utilities should execute");
        let rendered = render_nwscript_tokens(
            &NwTokenStream::from_tokens(&expanded).expect("expanded stream should balance"),
        );
        let alpha = rendered.find("Alpha").expect("Alpha call");
        let middle = rendered.find("Middle").expect("Middle call");
        let zulu = rendered.find("Zulu").expect("Zulu call");
        assert!(alpha < middle && middle < zulu);
        assert!(parse_tokens(expanded, None).is_ok());
    }

    #[test]
    fn nwscript_macro_error_at_reports_the_input_location() {
        let source = r#"
            proc_macro! fail_at_input {
                tokenstream fail_at_input(tokenstream input) {
                    __NWNRS_MacroErrorAt(input, "deliberate failure");
                    return input;
                }
            }
            fail_at_input!(Problem)
        "#;
        let error = expand_source_macros(
            lex(source),
            &mut MacroRegistry::new(),
            MacroExpansionOptions::default(),
        )
        .expect_err("macro should report a located error");
        assert!(error.to_string().contains("deliberate failure"));
        let file = SourceFile::new(SourceId::new(1), "macro-input.nss", source);
        assert_eq!(
            file.span_text(
                error
                    .span
                    .expect("macro error should retain its input span")
            ),
            Some("Problem")
        );
    }

    #[test]
    fn nwscript_procedural_macro_compiles_executes_and_returns_tokens() {
        let implementation = r#"
            tokenstream make_function(tokenstream input) {
                return quote! {
                    void generated() { $input }
                };
            }
        "#;
        let mut registry = MacroRegistry::new();
        assert!(
            register_nwscript_macro(
                &mut registry,
                "make_function",
                "make_function.nss",
                "make_function",
                implementation,
            )
            .is_ok()
        );
        let expanded = match expand_bang_macros(
            lex("make_function!(int value = 7;)"),
            &registry,
            MacroExpansionOptions::default(),
        ) {
            Ok(tokens) => tokens,
            Err(error) => unreachable!("procedural macro should execute: {error}"),
        };
        let script = match parse_tokens(expanded, None) {
            Ok(script) => script,
            Err(error) => unreachable!("generated function should parse: {error}"),
        };
        assert_eq!(script.items.len(), 1);
    }

    #[test]
    fn source_defined_nwscript_procedural_macro_is_collected_and_removed() {
        let source = r#"
            proc_macro! project::make_constant {
                tokenstream make_constant(tokenstream input) {
                    return quote! { const int GENERATED = $input; };
                }
            }

            project::make_constant!(19)
            void main() { int value = GENERATED; }
        "#;
        let expanded = match expand_source_macros(
            lex(source),
            &mut MacroRegistry::new(),
            MacroExpansionOptions::default(),
        ) {
            Ok(tokens) => tokens,
            Err(error) => unreachable!("source procedural macro should expand: {error}"),
        };
        assert!(!expanded.iter().any(|token| token.text == "proc_macro"));
        assert!(!expanded.iter().any(|token| token.text == "make_constant"));
        let script = match parse_tokens(expanded, None) {
            Ok(script) => script,
            Err(error) => unreachable!("procedural output should parse: {error}"),
        };
        assert_eq!(script.items.len(), 2);
    }

    #[test]
    fn procedural_macro_static_tokens_use_the_invocation_span() {
        let source = r#"
            proc_macro! make_constant {
                tokenstream make_constant(tokenstream input) {
                    return quote! { const int GENERATED = $input; };
                }
            }
            make_constant!(19)
        "#;
        let original = lex(source);
        let invocation = original
            .iter()
            .rev()
            .find(|token| token.text == "make_constant")
            .expect("invocation token")
            .span;
        let captured = original
            .iter()
            .find(|token| token.text == "19")
            .expect("captured token")
            .span;
        let expanded = expand_source_macros(
            original,
            &mut MacroRegistry::new(),
            MacroExpansionOptions::default(),
        )
        .expect("expand procedural macro");

        let static_span = expanded
            .iter()
            .find(|token| token.text == "const")
            .expect("static output token")
            .span;
        assert_eq!(static_span.source_id, invocation.source_id);
        assert_eq!(static_span.start, invocation.start);
        assert!(static_span.end > invocation.end);
        assert_eq!(
            expanded
                .iter()
                .find(|token| token.text == "19")
                .expect("captured output token")
                .span,
            captured
        );
    }

    #[test]
    fn nwscript_procedural_macro_can_inspect_input_token_trees() {
        let source = r#"
            proc_macro! make_named_function {
                tokenstream make_named_function(tokenstream input) {
                    if (__NWNRS_TokenStreamLength(input) != 1) {
                        __NWNRS_MacroError("expected one function name");
                    }
                    tokenstream name = __NWNRS_TokenStreamGet(input, 0);
                    if (__NWNRS_TokenKind(name) != "identifier") {
                        __NWNRS_MacroError("function name must be an identifier");
                    }
                    return quote! { void $name() {} };
                }
            }

            make_named_function!(Generated)
        "#;
        let expanded = match expand_source_macros(
            lex(source),
            &mut MacroRegistry::new(),
            MacroExpansionOptions::default(),
        ) {
            Ok(tokens) => tokens,
            Err(error) => unreachable!("inspection macro should expand: {error}"),
        };
        let script = match parse_tokens(expanded, None) {
            Ok(script) => script,
            Err(error) => unreachable!("inspected output should parse: {error}"),
        };
        assert_eq!(script.items.len(), 1);
    }
}
