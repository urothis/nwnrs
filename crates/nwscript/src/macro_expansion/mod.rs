use std::{cell::RefCell, collections::BTreeMap, error::Error, fmt, io::Cursor, rc::Rc};

use crate::{SourceId, Span, Token, TokenKind};

mod attribute;
mod declarative;
mod expander;
mod procedural;
mod quote;
mod runtime;
mod token_tree;

pub use attribute::*;
use declarative::*;
#[cfg(test)]
use expander::parse_macro_invocation;
pub use expander::*;
pub use procedural::*;
pub use quote::*;
use runtime::{TokenStreamArena, macro_vm};
pub use token_tree::*;

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
/// Maximum aggregate token copies retained by one procedural macro arena.
pub const DEFAULT_PROCEDURAL_MACRO_ARENA_TOKEN_LIMIT: usize = 4_000_000;
/// Maximum estimated token payload bytes retained by one procedural macro
/// arena.
pub const DEFAULT_PROCEDURAL_MACRO_ARENA_BYTE_LIMIT: usize = 64 * 1024 * 1024;
/// Maximum aggregate handles retained by one procedural macro arena.
pub const DEFAULT_PROCEDURAL_MACRO_ARENA_HANDLE_LIMIT: usize = 100_000;
/// Maximum aggregate list entries retained by one procedural macro arena.
pub const DEFAULT_PROCEDURAL_MACRO_ARENA_LIST_ITEM_LIMIT: usize = 1_000_000;
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
const MACRO_ORDERING_GRAPH_INDEX: u8 = 4;
const GENERATED_MACRO_SOURCE_ID: SourceId = SourceId::new(u32::MAX - 1);
const MACRO_LANGSPEC: &str = r#"
#define ENGINE_NUM_STRUCTURES 5
#define ENGINE_STRUCTURE_0 tokenstream
#define ENGINE_STRUCTURE_1 tokenstream_list
#define ENGINE_STRUCTURE_2 quote_bindings
#define ENGINE_STRUCTURE_3 token_cursor
#define ENGINE_STRUCTURE_4 ordering_graph
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
ordering_graph __NWNRS_OrderingGraphNew();
void __NWNRS_OrderingGraphAddNode(ordering_graph ogGraph, string sKey, tokenstream tsPayload, tokenstream tsDiagnostic, int nOrder);
int __NWNRS_OrderingGraphContains(ordering_graph ogGraph, string sKey);
int __NWNRS_OrderingGraphAddEdge(ordering_graph ogGraph, string sBefore, string sAfter);
tokenstream_list __NWNRS_OrderingGraphSort(ordering_graph ogGraph);
tokenstream_list __NWNRS_OrderingGraphCycle(ordering_graph ogGraph);
int __NWNRS_TokenSpanEquals(tokenstream tsLeft, tokenstream tsRight);
"#;

/// Context supplied to one bang-macro implementation.
#[derive(Debug, Clone, Copy)]
pub struct MacroContext<'a> {
    expansion_stack: &'a [MacroPath],
    options:         MacroExpansionOptions,
    cancellation:    Option<&'a crate::CancellationToken>,
}

impl<'a> MacroContext<'a> {
    /// Returns the active macro expansion stack, oldest first.
    #[must_use]
    pub fn expansion_stack(self) -> &'a [MacroPath] {
        self.expansion_stack
    }

    /// Returns the resource limits for this expansion session.
    #[must_use]
    pub const fn options(self) -> MacroExpansionOptions {
        self.options
    }

    /// Returns the cooperative cancellation token for this expansion.
    #[must_use]
    pub const fn cancellation(self) -> Option<&'a crate::CancellationToken> {
        self.cancellation
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
    pub max_depth:             usize,
    /// Maximum flattened tokens in an intermediate or final stream.
    pub token_limit:           usize,
    /// Maximum aggregate token copies retained by a procedural macro arena.
    pub arena_token_limit:     usize,
    /// Maximum estimated token payload bytes retained by an arena.
    pub arena_byte_limit:      usize,
    /// Maximum aggregate stream/list/binding/cursor handles in an arena.
    pub arena_handle_limit:    usize,
    /// Maximum aggregate entries retained across token-stream lists.
    pub arena_list_item_limit: usize,
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
            max_depth:             DEFAULT_MACRO_EXPANSION_DEPTH,
            token_limit:           DEFAULT_MACRO_TOKEN_LIMIT,
            arena_token_limit:     DEFAULT_PROCEDURAL_MACRO_ARENA_TOKEN_LIMIT,
            arena_byte_limit:      DEFAULT_PROCEDURAL_MACRO_ARENA_BYTE_LIMIT,
            arena_handle_limit:    DEFAULT_PROCEDURAL_MACRO_ARENA_HANDLE_LIMIT,
            arena_list_item_limit: DEFAULT_PROCEDURAL_MACRO_ARENA_LIST_ITEM_LIMIT,
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

#[cfg(test)]
mod tests;
