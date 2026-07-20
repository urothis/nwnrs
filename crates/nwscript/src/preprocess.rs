use std::{
    collections::{HashMap, HashSet},
    error::Error,
    fmt,
};

use crate::{
    Keyword, LexerError, MacroExpansionError, MacroExpansionOptions, MacroRegistry, ScriptResolver,
    SourceError, SourceFile, SourceId, SourceLoadOptions, SourceMap, Token, TokenKind,
    expand_source_macros, lex_source,
};

/// One include relationship discovered while traversing source files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncludeEdge {
    /// Including file.
    pub from:         SourceId,
    /// Included file.
    pub to:           SourceId,
    /// Include string as it appeared in the source file.
    pub include_name: String,
}

/// One loaded root script plus all transitively discovered include files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceBundle {
    /// All loaded source files.
    pub source_map:    SourceMap,
    /// The root script file requested by the caller.
    pub root_id:       SourceId,
    /// Source ids in first-load order.
    pub source_order:  Vec<SourceId>,
    /// Include relationships observed during scanning.
    pub include_edges: Vec<IncludeEdge>,
}

/// One object-like `#define` captured during preprocessing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroDefinition {
    /// Macro name.
    pub name:        String,
    /// Replacement tokens captured from the define line.
    pub replacement: Vec<Token>,
    /// File where the macro was defined.
    pub source_id:   SourceId,
    /// One-based source line of the define.
    pub line:        usize,
}

/// One preprocessed token stream plus the macros captured while producing it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreprocessedSource {
    /// Tokens after include traversal and object-like macro expansion.
    pub tokens:  Vec<Token>,
    /// Macro definitions in encounter order, with later redefinitions included.
    pub defines: Vec<MacroDefinition>,
}

/// Errors returned while scanning include directives across multiple files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreprocessError {
    /// Source resolution or load failure.
    Source(SourceError),
    /// Lexing failure while scanning include directives.
    Lex(LexerError),
    /// Extended macro collection or expansion failed.
    Macro(MacroExpansionError),
}

impl fmt::Display for PreprocessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Source(error) => error.fmt(f),
            Self::Lex(error) => error.fmt(f),
            Self::Macro(error) => error.fmt(f),
        }
    }
}

impl Error for PreprocessError {}

impl From<SourceError> for PreprocessError {
    fn from(value: SourceError) -> Self {
        Self::Source(value)
    }
}

impl From<LexerError> for PreprocessError {
    fn from(value: LexerError) -> Self {
        Self::Lex(value)
    }
}

impl From<MacroExpansionError> for PreprocessError {
    fn from(value: MacroExpansionError) -> Self {
        Self::Macro(value)
    }
}

/// Loads a root script and recursively discovers its `#include` dependencies.
///
/// # Errors
///
/// Returns [`PreprocessError`] if any script cannot be resolved or loaded.
pub fn load_source_bundle<R: ScriptResolver + ?Sized>(
    resolver: &R,
    root_name: &str,
    options: SourceLoadOptions,
) -> Result<SourceBundle, PreprocessError> {
    let mut loader = SourceBundleLoader::new(resolver, options);
    let root_id = loader.load_script(root_name)?;
    Ok(SourceBundle {
        source_map: loader.source_map,
        root_id,
        source_order: loader.source_order,
        include_edges: loader.include_edges,
    })
}

/// Preprocesses one already-loaded source bundle into a token stream.
///
/// # Errors
///
/// Returns [`PreprocessError`] if macro expansion or include resolution fails.
pub fn preprocess_source_bundle(
    bundle: &SourceBundle,
) -> Result<PreprocessedSource, PreprocessError> {
    preprocess_source_bundle_with_macros(
        bundle,
        &mut MacroRegistry::new(),
        MacroExpansionOptions::default(),
    )
}

/// Preprocesses one source bundle with caller-provided built-in macros.
///
/// Object-like `#define` and include expansion runs first. Top-level
/// source-defined `macro_rules!` definitions are then added to `registry`
/// before all bang-macro invocations are recursively expanded.
///
/// # Errors
///
/// Returns [`PreprocessError`] if include loading, lexing, object-like macro
/// expansion, declarative macro collection, or bang-macro expansion fails.
pub fn preprocess_source_bundle_with_macros(
    bundle: &SourceBundle,
    registry: &mut MacroRegistry,
    options: MacroExpansionOptions,
) -> Result<PreprocessedSource, PreprocessError> {
    let mut preprocessor = BundlePreprocessor::new(bundle);
    preprocessor.expand_source(bundle.root_id)?;
    let mut preprocessed = preprocessor.finish(bundle.root_id)?;
    preprocessed.tokens = expand_source_macros(preprocessed.tokens, registry, options)?;
    Ok(preprocessed)
}

struct SourceBundleLoader<'a, R: ScriptResolver + ?Sized> {
    resolver:      &'a R,
    options:       SourceLoadOptions,
    source_map:    SourceMap,
    source_order:  Vec<SourceId>,
    include_edges: Vec<IncludeEdge>,
    active_stack:  Vec<String>,
}

impl<'a, R: ScriptResolver + ?Sized> SourceBundleLoader<'a, R> {
    fn new(resolver: &'a R, options: SourceLoadOptions) -> Self {
        Self {
            resolver,
            options,
            source_map: SourceMap::new(),
            source_order: Vec::new(),
            include_edges: Vec::new(),
            active_stack: Vec::new(),
        }
    }

    fn load_script(&mut self, script_name: &str) -> Result<SourceId, PreprocessError> {
        let normalized = script_name.to_ascii_lowercase();
        if self.active_stack.contains(&normalized) {
            return Err(SourceError::include_recursive(script_name).into());
        }
        if let Some(source) = self.source_map.get_by_name(script_name) {
            return Ok(source.id);
        }
        if self.source_map.len() >= crate::MAX_SOURCE_FILES {
            return Err(SourceError::too_many_source_files(script_name).into());
        }
        if self.active_stack.len() >= self.options.max_include_depth {
            return Err(SourceError::include_too_many_levels(
                script_name,
                self.options.max_include_depth,
            )
            .into());
        }

        let contents = self
            .resolver
            .resolve_script_bytes(script_name, self.options.res_type)?
            .filter(|bytes| !bytes.is_empty())
            .ok_or_else(|| SourceError::file_not_found(script_name))?;

        let source_id = self.source_map.next_id();
        let source_file = SourceFile::new(source_id, script_name, contents);
        let include_names = scan_include_names(&source_file)?;
        self.source_map.insert_file(source_file);
        self.source_order.push(source_id);
        self.active_stack.push(normalized);

        for include_name in include_names {
            let child_id = self.load_script(&include_name)?;
            self.include_edges.push(IncludeEdge {
                from: source_id,
                to: child_id,
                include_name,
            });
        }

        self.active_stack.pop();
        Ok(source_id)
    }
}

fn scan_include_names(source_file: &SourceFile) -> Result<Vec<String>, LexerError> {
    let tokens = lex_source(source_file)?;
    let mut includes = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        if tokens
            .get(index)
            .is_some_and(|token| token.kind == TokenKind::Keyword(Keyword::Include))
            && let Some(argument) = tokens.get(index + 1)
            && argument.kind == TokenKind::String
        {
            includes.push(argument.text.clone());
            index += 2;
            continue;
        }
        index += 1;
    }
    Ok(includes)
}

struct BundlePreprocessor<'a> {
    bundle:         &'a SourceBundle,
    defines:        HashMap<String, MacroDefinition>,
    define_order:   Vec<MacroDefinition>,
    expanded_files: HashSet<SourceId>,
    tokens:         Vec<Token>,
}

impl<'a> BundlePreprocessor<'a> {
    fn new(bundle: &'a SourceBundle) -> Self {
        Self {
            bundle,
            defines: HashMap::new(),
            define_order: Vec::new(),
            expanded_files: HashSet::new(),
            tokens: Vec::new(),
        }
    }

    fn finish(mut self, root_id: SourceId) -> Result<PreprocessedSource, PreprocessError> {
        let root = self
            .bundle
            .source_map
            .get(root_id)
            .ok_or_else(|| SourceError::file_not_found("root"))?;
        self.tokens.push(Token::new(
            TokenKind::Eof,
            crate::Span::new(root_id, root.len(), root.len()),
            "",
        ));
        Ok(PreprocessedSource {
            tokens:  self.tokens,
            defines: self.define_order,
        })
    }

    fn expand_source(&mut self, source_id: SourceId) -> Result<(), PreprocessError> {
        if !self.expanded_files.insert(source_id) {
            return Ok(());
        }

        let source = self
            .bundle
            .source_map
            .get(source_id)
            .ok_or_else(|| SourceError::file_not_found(source_id.to_string()))?;
        let tokens = lex_source(source)?;
        let mut index = 0;

        while index < tokens.len() {
            let Some(token) = tokens.get(index) else {
                break;
            };
            if token.kind == TokenKind::Eof {
                break;
            }

            let line = token_line(source, token).ok_or_else(|| LexerError {
                code:    crate::CompilerErrorCode::UnknownStateInCompiler,
                span:    crate::Span::new(source.id, token.span.start, token.span.end),
                message: "failed to resolve token line during preprocessing".to_string(),
            })?;
            let line_end = next_line_index(source, &tokens, index, line);

            if token.kind == TokenKind::Keyword(Keyword::Include)
                && let Some(argument) = tokens.get(index + 1)
                && argument.kind == TokenKind::String
                && token_line(source, argument) == Some(line)
                && let Some(include) = self.bundle.source_map.get_by_name(&argument.text)
            {
                self.expand_source(include.id)?;
                index = line_end;
                continue;
            }

            if token.kind == TokenKind::Keyword(Keyword::Define) {
                let line_tokens = tokens.get(index..line_end).unwrap_or(&[]);
                self.capture_define(source, line_tokens, line);
                index = line_end;
                continue;
            }

            for token in tokens.get(index..line_end).unwrap_or(&[]) {
                self.expand_token(token, &mut Vec::new());
            }
            index = line_end;
        }

        Ok(())
    }

    fn capture_define(&mut self, source: &SourceFile, line_tokens: &[Token], line: usize) {
        let Some(name_token) = line_tokens.get(1) else {
            return;
        };
        if name_token.kind != TokenKind::Identifier {
            return;
        }

        let replacement = line_tokens
            .iter()
            .skip(2)
            .filter(|token| token.kind != TokenKind::Eof)
            .cloned()
            .collect::<Vec<_>>();
        let definition = MacroDefinition {
            name: name_token.text.clone(),
            replacement,
            source_id: source.id,
            line,
        };
        self.defines
            .insert(definition.name.clone(), definition.clone());
        self.define_order.push(definition);
    }

    fn expand_token(&mut self, token: &Token, active: &mut Vec<String>) {
        if token.kind == TokenKind::Identifier
            && let Some(definition) = self.defines.get(&token.text).cloned()
            && !active.iter().any(|name| name == &definition.name)
        {
            active.push(definition.name.clone());
            for replacement in definition.replacement {
                // Mirror the upstream identifier rewrite path: once a define is
                // recognized, the replacement keeps its own typed token kind
                // but is attributed to the call site for diagnostics.
                let rewritten = Token::new(replacement.kind, token.span, replacement.text);
                self.expand_token(&rewritten, active);
            }
            active.pop();
            return;
        }

        self.tokens.push(token.clone());
    }
}

fn token_line(source: &SourceFile, token: &Token) -> Option<usize> {
    source
        .location(token.span.start)
        .map(|location| location.line)
}

fn next_line_index(source: &SourceFile, tokens: &[Token], start: usize, line: usize) -> usize {
    let mut index = start;
    while let Some(token) = tokens.get(index) {
        if token.kind == TokenKind::Eof {
            break;
        }
        if token_line(source, token) != Some(line) {
            break;
        }
        index += 1;
    }
    index
}

#[cfg(test)]
mod tests {
    use super::{load_source_bundle, preprocess_source_bundle};
    use crate::{CompilerErrorCode, InMemoryScriptResolver, Keyword, SourceLoadOptions, TokenKind};

    fn token_pairs(preprocessed: super::PreprocessedSource) -> Vec<(TokenKind, String)> {
        preprocessed
            .tokens
            .into_iter()
            .map(|token| (token.kind, token.text))
            .collect::<Vec<_>>()
    }

    #[test]
    fn ignores_duplicate_files_reached_through_transitive_includes()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut resolver = InMemoryScriptResolver::new();
        resolver.insert_source(
            "root",
            r#"#include "util"
#include "common"
#include "util"
void main() {}"#,
        );
        resolver.insert_source(
            "util",
            r#"#include "common"
int UTIL = 1;"#,
        );
        resolver.insert_source("common", "int COMMON = 2;");

        let bundle = load_source_bundle(&resolver, "root", SourceLoadOptions::default())?;
        let preprocessed = preprocess_source_bundle(&bundle)?;

        assert_eq!(bundle.source_map.len(), 3);
        assert_eq!(bundle.include_edges.len(), 4);
        assert_eq!(
            preprocessed
                .tokens
                .iter()
                .filter(|token| token.text == "COMMON")
                .count(),
            1
        );
        Ok(())
    }

    #[test]
    fn treats_empty_source_as_file_not_found() {
        let mut resolver = InMemoryScriptResolver::new();
        resolver.insert_source("root", r#"#include "empty""#);
        resolver.insert_source("empty", "");

        let error = load_source_bundle(&resolver, "root", SourceLoadOptions::default()).err();
        let code = error.and_then(|error| match error {
            super::PreprocessError::Source(source) => source.code(),
            super::PreprocessError::Lex(_) | super::PreprocessError::Macro(_) => None,
        });

        assert_eq!(code, Some(CompilerErrorCode::FileNotFound));
    }

    #[test]
    fn enforces_include_depth_limit() {
        let mut resolver = InMemoryScriptResolver::new();
        resolver.insert_source("root", r#"#include "a""#);
        resolver.insert_source("a", r#"#include "b""#);
        resolver.insert_source("b", r#"#include "c""#);
        resolver.insert_source("c", "void c() {}");

        let error = load_source_bundle(
            &resolver,
            "root",
            SourceLoadOptions {
                max_include_depth: 2,
                ..SourceLoadOptions::default()
            },
        )
        .err();
        let code = error.and_then(|error| match error {
            super::PreprocessError::Source(source) => source.code(),
            super::PreprocessError::Lex(_) | super::PreprocessError::Macro(_) => None,
        });

        assert_eq!(code, Some(CompilerErrorCode::IncludeTooManyLevels));
    }

    #[test]
    fn resolver_matches_include_names_case_insensitively() {
        let mut resolver = InMemoryScriptResolver::new();
        resolver.insert_source("ROOT", r#"#include "Util""#);
        resolver.insert_source("util", "void util() {}");

        let bundle = load_source_bundle(&resolver, "root", SourceLoadOptions::default());
        let count = bundle.ok().map(|bundle| bundle.source_map.len());

        assert_eq!(count, Some(2));
    }

    #[test]
    fn rejects_recursive_includes_before_reusing_the_loaded_source() {
        let mut resolver = InMemoryScriptResolver::new();
        resolver.insert_source("root", r#"#include "root""#);

        let code = load_source_bundle(&resolver, "root", SourceLoadOptions::default())
            .err()
            .and_then(|error| match error {
                super::PreprocessError::Source(source) => source.code(),
                super::PreprocessError::Lex(_) | super::PreprocessError::Macro(_) => None,
            });

        assert_eq!(code, Some(CompilerErrorCode::IncludeRecursive));
    }

    #[test]
    fn reuses_the_same_include_when_loaded_twice() -> Result<(), Box<dyn std::error::Error>> {
        let mut resolver = InMemoryScriptResolver::new();
        resolver.insert_source("root", "#include \"util\"\n#include \"UTIL\"");
        resolver.insert_source("util", "void helper() {}");

        let bundle = load_source_bundle(&resolver, "root", SourceLoadOptions::default())?;

        assert_eq!(bundle.source_map.len(), 2);
        assert_eq!(bundle.include_edges.len(), 2);
        assert_eq!(
            bundle.include_edges.first().map(|edge| edge.to),
            bundle.include_edges.last().map(|edge| edge.to)
        );
        Ok(())
    }

    #[test]
    fn preprocesses_object_like_defines_with_include_order()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut resolver = InMemoryScriptResolver::new();
        resolver.insert_source(
            "root",
            br#"#define VALUE 7
#include "util"
int x = VALUE;
"#,
        );
        resolver.insert_source(
            "util",
            br#"#define PLUS +
int y = VALUE PLUS 1;
"#,
        );

        let bundle = load_source_bundle(&resolver, "root", SourceLoadOptions::default())?;
        let pairs = token_pairs(preprocess_source_bundle(&bundle)?);

        assert_eq!(
            pairs,
            vec![
                (TokenKind::Keyword(Keyword::Int), "int".to_string()),
                (TokenKind::Identifier, "y".to_string()),
                (TokenKind::Assign, "=".to_string()),
                (TokenKind::Integer, "7".to_string()),
                (TokenKind::Plus, "+".to_string()),
                (TokenKind::Integer, "1".to_string()),
                (TokenKind::Semicolon, ";".to_string()),
                (TokenKind::Keyword(Keyword::Int), "int".to_string()),
                (TokenKind::Identifier, "x".to_string()),
                (TokenKind::Assign, "=".to_string()),
                (TokenKind::Integer, "7".to_string()),
                (TokenKind::Semicolon, ";".to_string()),
                (TokenKind::Eof, "".to_string()),
            ]
        );
        Ok(())
    }

    #[test]
    fn preprocess_define_redefinitions_use_latest_value() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut resolver = InMemoryScriptResolver::new();
        resolver.insert_source(
            "root",
            br#"#define VALUE 1
#define VALUE 2
int x = VALUE;
"#,
        );

        let bundle = load_source_bundle(&resolver, "root", SourceLoadOptions::default())?;
        let preprocessed = preprocess_source_bundle(&bundle)?;
        let integers = preprocessed
            .tokens
            .into_iter()
            .filter(|token| token.kind == TokenKind::Integer)
            .map(|token| token.text)
            .collect::<Vec<_>>();

        assert_eq!(integers, vec!["2".to_string()]);
        Ok(())
    }

    #[test]
    fn chained_define_expansion_preserves_upstream_literal_token_kinds()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut resolver = InMemoryScriptResolver::new();
        resolver.insert_source(
            "root",
            br#"#define BASE 4
#define VALUE BASE
int x = VALUE;
"#,
        );

        let bundle = load_source_bundle(&resolver, "root", SourceLoadOptions::default())?;
        let pairs = token_pairs(preprocess_source_bundle(&bundle)?);

        assert_eq!(
            pairs,
            vec![
                (TokenKind::Keyword(Keyword::Int), "int".to_string()),
                (TokenKind::Identifier, "x".to_string()),
                (TokenKind::Assign, "=".to_string()),
                (TokenKind::Integer, "4".to_string()),
                (TokenKind::Semicolon, ";".to_string()),
                (TokenKind::Eof, "".to_string()),
            ]
        );
        Ok(())
    }

    #[test]
    fn define_visibility_tracks_include_encounter_order() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut resolver = InMemoryScriptResolver::new();
        resolver.insert_source(
            "root",
            br#"#define VALUE 1
#include "util"
#define VALUE 2
int root_value = VALUE;
"#,
        );
        resolver.insert_source("util", br#"int util_value = VALUE;"#);

        let bundle = load_source_bundle(&resolver, "root", SourceLoadOptions::default())?;
        let pairs = token_pairs(preprocess_source_bundle(&bundle)?);

        assert_eq!(
            pairs,
            vec![
                (TokenKind::Keyword(Keyword::Int), "int".to_string()),
                (TokenKind::Identifier, "util_value".to_string()),
                (TokenKind::Assign, "=".to_string()),
                (TokenKind::Integer, "1".to_string()),
                (TokenKind::Semicolon, ";".to_string()),
                (TokenKind::Keyword(Keyword::Int), "int".to_string()),
                (TokenKind::Identifier, "root_value".to_string()),
                (TokenKind::Assign, "=".to_string()),
                (TokenKind::Integer, "2".to_string()),
                (TokenKind::Semicolon, ";".to_string()),
                (TokenKind::Eof, "".to_string()),
            ]
        );
        Ok(())
    }

    #[test]
    fn define_expansion_preserves_keyword_token_kinds() -> Result<(), Box<dyn std::error::Error>> {
        let mut resolver = InMemoryScriptResolver::new();
        resolver.insert_source(
            "root",
            br#"#define BAD_OBJECT OBJECT_INVALID
object value = BAD_OBJECT;
"#,
        );

        let bundle = load_source_bundle(&resolver, "root", SourceLoadOptions::default())?;
        let pairs = token_pairs(preprocess_source_bundle(&bundle)?);

        assert_eq!(
            pairs,
            vec![
                (TokenKind::Keyword(Keyword::Object), "object".to_string()),
                (TokenKind::Identifier, "value".to_string()),
                (TokenKind::Assign, "=".to_string()),
                (
                    TokenKind::Keyword(Keyword::ObjectInvalid),
                    "OBJECT_INVALID".to_string(),
                ),
                (TokenKind::Semicolon, ";".to_string()),
                (TokenKind::Eof, "".to_string()),
            ]
        );
        Ok(())
    }
}
