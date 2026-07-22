use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    hash::{DefaultHasher, Hash, Hasher},
    path::{Path, PathBuf},
    sync::Arc,
};

use nwnrs_nwscript::{
    CompilerAnalysis, Keyword, Lexer, ScriptResolver, SemanticSymbolKind, SourceBundle,
    SourceError, SourceFile, SourceId, SourceMap, Span, Token, TokenKind, TopLevelItem,
};
use nwnrs_types::resman::ResType;

/// One NWScript symbol kind exposed to editor integrations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NwScriptSymbolKind {
    /// A function declaration or implementation.
    Function,
    /// A preprocessor or extended compiler macro.
    Macro,
    /// A strong `int`- or `string`-backed enum type.
    Enum,
    /// One named value inside a strong enum.
    EnumVariant,
    /// A transparent source type alias.
    TypeAlias,
    /// A generated compatibility constant declared with `#[alias(...)]`.
    Constant,
    /// A mutable project global or block-local variable.
    Variable,
    /// A function parameter.
    Parameter,
    /// A source-defined structure type.
    Struct,
    /// A named field inside a source-defined structure.
    Field,
    /// A builtin function declared by the compiler's implicit `nwscript.nss`.
    BuiltinFunction,
    /// A builtin constant declared by the compiler's implicit `nwscript.nss`.
    BuiltinConstant,
    /// An engine structure declared by the compiler's implicit `nwscript.nss`.
    EngineStructure,
}

/// One declaration category shown in an editor's document Outline.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NwScriptDocumentSymbolKind {
    /// A function declaration or implementation.
    Function,
    /// A top-level variable.
    Variable,
    /// A source-defined structure.
    Struct,
    /// A field nested under a structure.
    Field,
    /// A strong enum type.
    Enum,
    /// A value nested under a strong enum.
    EnumVariant,
    /// A transparent source type alias.
    TypeAlias,
    /// A generated compatibility alias or other named constant.
    Constant,
    /// A preprocessor, declarative, or procedural macro.
    Macro,
}

/// One one-based, end-exclusive source range used by document symbols.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NwScriptSourceRange {
    /// Start line.
    pub start_line:   usize,
    /// Start column.
    pub start_column: usize,
    /// End line.
    pub end_line:     usize,
    /// End column.
    pub end_column:   usize,
}

/// One hierarchical source-authored declaration shown in an editor Outline.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NwScriptDocumentSymbol {
    /// Displayed declaration name.
    pub name:            String,
    /// Semantic symbol category.
    pub kind:            NwScriptDocumentSymbolKind,
    /// Concise signature or type information displayed beside the name.
    pub detail:          Option<String>,
    /// Full declaration range.
    pub range:           NwScriptSourceRange,
    /// Identifier-only selection range.
    pub selection_range: NwScriptSourceRange,
    /// Nested declarations, such as enum variants and structure fields.
    pub children:        Vec<NwScriptDocumentSymbol>,
}

/// Compiler-resolved token category exposed to semantic-highlighting clients.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NwScriptSemanticTokenKind {
    /// A function declaration or call target.
    Function,
    /// A named function parameter.
    Parameter,
    /// A global or local variable.
    Variable,
    /// A structure field.
    Property,
    /// A structure or transparent type alias.
    Type,
    /// A strong enum type.
    Enum,
    /// A strong enum variant.
    EnumMember,
    /// A declarative, procedural, or object-like macro.
    Macro,
}

/// One semantic token in a source document.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NwScriptSemanticToken {
    /// Exact identifier source range.
    pub range:              NwScriptSourceRange,
    /// Compiler-resolved token category.
    pub kind:               NwScriptSemanticTokenKind,
    /// Whether this occurrence declares its symbol.
    pub is_declaration:     bool,
    /// Whether this occurrence represents a constant value.
    pub is_readonly:        bool,
    /// Whether this occurrence came from packed vanilla source.
    pub is_default_library: bool,
}

/// One restrained source hint computed from compiler syntax and semantics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NwScriptInlayHint {
    /// One-based insertion line.
    pub line:   usize,
    /// One-based insertion column.
    pub column: usize,
    /// Text displayed by the editor.
    pub label:  String,
    /// Stable hint category used by editor settings.
    pub kind:   &'static str,
}

/// Document symbols and non-fatal per-file warnings produced by one package
/// index pass.
pub type NwScriptProjectDocuments = (Vec<(PathBuf, Vec<NwScriptDocumentSymbol>)>, Vec<String>);

/// One compiler-filtered symbol occurrence used by references and rename.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NwScriptReference {
    /// Referenced symbol name.
    pub name:           String,
    /// Resolved symbol category.
    pub kind:           NwScriptSymbolKind,
    /// Physical path or logical packed-resource path.
    pub path:           PathBuf,
    /// Exact identifier range.
    pub range:          NwScriptSourceRange,
    /// Whether this occurrence is a declaration.
    pub is_declaration: bool,
    /// Enclosing function when the occurrence is inside a body.
    pub container:      Option<String>,
    /// Read-only URI for packed sources.
    pub virtual_uri:    Option<String>,
    /// Packed source resource name.
    pub resource_name:  Option<String>,
}

/// One resolved outgoing function call with every call-site range in the
/// selected caller.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NwScriptOutgoingCall {
    /// Resolved callee declaration.
    pub target: NwScriptSymbolDefinition,
    /// Call-site identifier ranges inside the caller.
    pub ranges: Vec<NwScriptSourceRange>,
}

/// One source definition returned to an editor integration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NwScriptSymbolDefinition {
    /// Symbol name as written in source.
    pub name:              String,
    /// Symbol category.
    pub kind:              NwScriptSymbolKind,
    /// Filesystem path containing the definition.
    pub path:              PathBuf,
    /// One-based selection start line.
    pub start_line:        usize,
    /// One-based selection start column.
    pub start_column:      usize,
    /// One-based selection end line.
    pub end_line:          usize,
    /// One-based selection end column.
    pub end_column:        usize,
    /// Displayable declaration signature.
    pub signature:         String,
    /// Documentation immediately preceding the declaration.
    pub documentation:     Option<String>,
    /// Whether a function has a body rather than only a declaration.
    pub is_implementation: bool,
    /// Read-only document URI when the source came from a packed game
    /// resource rather than a physical file.
    pub virtual_uri:       Option<String>,
    /// Logical NSS resource name used to reopen a packed game source.
    pub resource_name:     Option<String>,
}

/// Filesystem and project context for one definition lookup.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NwScriptDefinitionQuery {
    /// Source file containing the symbol reference.
    pub source_path:         PathBuf,
    /// Symbol name under the editor cursor.
    pub symbol:              String,
    /// Optional qualifier immediately preceding `symbol`, such as an enum or
    /// macro namespace path.
    pub qualifier:           Option<String>,
    /// Owning project root, when known by the editor.
    pub project_root:        Option<PathBuf>,
    /// Additional configured include search directories.
    pub include_directories: Vec<PathBuf>,
    /// Unsaved source contents keyed by filesystem path.
    pub source_overlays:     BTreeMap<PathBuf, Vec<u8>>,
    /// Optional explicit `nwscript.nss` language-specification path.
    pub langspec:            Option<PathBuf>,
    /// Maximum recursive include depth used by source resolution.
    pub max_include_depth:   usize,
    /// Optional Neverwinter Nights installation root override.
    pub root:                Option<PathBuf>,
    /// Optional Neverwinter Nights user-directory override.
    pub user:                Option<PathBuf>,
    /// Installation language used for resource lookup.
    pub language:            String,
    /// Include the installation override directory in resource lookup.
    pub load_ovr:            bool,
}

impl Default for NwScriptDefinitionQuery {
    fn default() -> Self {
        Self {
            source_path:         PathBuf::new(),
            symbol:              String::new(),
            qualifier:           None,
            project_root:        None,
            include_directories: Vec::new(),
            source_overlays:     BTreeMap::new(),
            langspec:            None,
            max_include_depth:   16,
            root:                None,
            user:                None,
            language:            "english".to_string(),
            load_ovr:            false,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ProjectAnalysisConfig {
    project_root:        Option<PathBuf>,
    include_directories: Vec<PathBuf>,
    langspec:            Option<PathBuf>,
    max_include_depth:   usize,
    root:                Option<PathBuf>,
    user:                Option<PathBuf>,
    language:            String,
    load_ovr:            bool,
}

impl From<&NwScriptDefinitionQuery> for ProjectAnalysisConfig {
    fn from(query: &NwScriptDefinitionQuery) -> Self {
        Self {
            project_root:        query.project_root.clone(),
            include_directories: query.include_directories.clone(),
            langspec:            query.langspec.clone(),
            max_include_depth:   query.max_include_depth,
            root:                query.root.clone(),
            user:                query.user.clone(),
            language:            query.language.clone(),
            load_ovr:            query.load_ovr,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SourceRevision {
    path: PathBuf,
    hash: u64,
}

#[derive(Clone)]
struct CachedCompilerAnalysis {
    config:    ProjectAnalysisConfig,
    revisions: Vec<SourceRevision>,
    analysis:  Arc<CompilerAnalysis>,
    weight:    usize,
    last_used: u64,
}

/// Persistent compiler analysis database for one package session.
///
/// Cached units are accepted only while every physical or overlaid source
/// revision used to build them still matches. Invalidating a dependency evicts
/// every root whose include graph consumed that source.
#[derive(Default)]
pub struct NwScriptProjectIndex {
    units:        BTreeMap<PathBuf, CachedCompilerAnalysis>,
    reverse_uses: BTreeMap<PathBuf, Vec<PathBuf>>,
    dirty_units:  BTreeSet<PathBuf>,
    generation:   u64,
    access_clock: u64,
    total_weight: usize,
}

impl NwScriptProjectIndex {
    /// Creates an empty project index.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Current snapshot generation. It changes whenever cached semantic state
    /// is replaced.
    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// Invalidates all compilation units that consumed `path`.
    pub fn invalidate_path(&mut self, path: &Path) {
        let path = canonical_source_path(path);
        let roots = self
            .reverse_uses
            .get(&path)
            .cloned()
            .unwrap_or_else(|| self.units.keys().cloned().collect());
        if roots.is_empty() {
            return;
        }
        for root in roots {
            self.dirty_units.insert(root);
        }
        self.generation = self.generation.wrapping_add(1);
    }

    /// Discards all indexed units and begins a new empty snapshot generation.
    pub fn clear(&mut self) {
        if self.units.is_empty() && self.reverse_uses.is_empty() {
            return;
        }
        self.units.clear();
        self.reverse_uses.clear();
        self.dirty_units.clear();
        self.total_weight = 0;
        self.generation = self.generation.wrapping_add(1);
    }

    /// Finds compiler-resolved references using reusable project analyses.
    ///
    /// # Errors
    ///
    /// Returns an error for source resolution, compiler analysis, or
    /// cancellation failures.
    pub fn find_references(
        &mut self,
        query: &NwScriptDefinitionQuery,
        line: usize,
        column: usize,
        cancellation: Option<&nwnrs_nwscript::CancellationToken>,
    ) -> Result<Vec<NwScriptReference>, String> {
        match find_compiler_index_references(self, query, line, column, cancellation)? {
            Some(references) => Ok(references),
            None => find_macro_references(self, query, line, column, cancellation),
        }
    }

    /// Finds definitions from compiler-owned symbol identities and source
    /// spans.
    ///
    /// Macro definitions and builtin language-spec declarations retain their
    /// dedicated source scanners because they are intentionally expanded or
    /// loaded outside typed HIR.
    ///
    /// # Errors
    ///
    /// Returns an error for source resolution or compiler analysis failures.
    pub fn definitions(
        &mut self,
        query: &NwScriptDefinitionQuery,
        cancellation: Option<&nwnrs_nwscript::CancellationToken>,
    ) -> Result<Vec<NwScriptSymbolDefinition>, String> {
        if query.symbol.is_empty() {
            return Ok(Vec::new());
        }
        let analysis = match self.analysis(query, cancellation) {
            Ok(analysis) => analysis,
            Err(_) => {
                check_cancellation(cancellation)?;
                return find_source_index_definitions(query, true);
            }
        };
        let resolver = build_symbol_resolver(query)?;
        let mut definitions = Vec::new();
        for definition in analysis
            .index
            .definitions
            .iter()
            .filter(|definition| definition.name == query.symbol)
        {
            check_cancellation(cancellation)?;
            let qualifier_matches = match (&definition.id, query.qualifier.as_deref()) {
                (
                    nwnrs_nwscript::SemanticSymbolId::Field {
                        owner, ..
                    }
                    | nwnrs_nwscript::SemanticSymbolId::EnumVariant {
                        owner, ..
                    },
                    Some(qualifier),
                ) => owner == qualifier,
                (
                    nwnrs_nwscript::SemanticSymbolId::Field {
                        ..
                    }
                    | nwnrs_nwscript::SemanticSymbolId::EnumVariant {
                        ..
                    },
                    None,
                ) => true,
                (_, None) => true,
                (_, Some(_)) => false,
            };
            if !qualifier_matches || definition.span.source_id == SourceId::new(u32::MAX) {
                continue;
            }
            let kind = nwscript_symbol_kind(definition.kind);
            let is_implementation = kind != NwScriptSymbolKind::Function
                || analysis.script.items.iter().any(|item| {
                    matches!(
                        item,
                        TopLevelItem::Function(function)
                            if function.name == definition.name
                                && function.span == definition.declaration_span
                                && function.body.is_some()
                    )
                });
            if let Some(mut resolved) = definition_from_compiler_span(
                &analysis.bundle,
                &resolver,
                definition.declaration_span,
                &definition.name,
                kind,
                is_implementation,
            ) {
                match &definition.id {
                    nwnrs_nwscript::SemanticSymbolId::Field {
                        owner, ..
                    } => {
                        resolved.signature = format!("{owner}.{}", resolved.signature);
                    }
                    nwnrs_nwscript::SemanticSymbolId::EnumVariant {
                        owner, ..
                    } => {
                        if let Some(name_start) = resolved.signature.find(&definition.name) {
                            resolved.signature = resolved.signature[name_start..].to_string();
                        }
                        resolved.signature = format!("{owner}::{}", resolved.signature);
                    }
                    nwnrs_nwscript::SemanticSymbolId::Global(name)
                        if matches!(
                            &definition.container,
                            Some(nwnrs_nwscript::SemanticSymbolId::Enum(_))
                        ) =>
                    {
                        if let Some((owner, variant)) = enum_alias_target(&analysis.script, name) {
                            resolved.signature = format!("{owner} {name} = {owner}::{variant}");
                        }
                    }
                    _ => {}
                }
                definitions.push(resolved);
            }
        }
        if definitions.is_empty() {
            return find_source_index_definitions(query, false);
        }
        definitions.sort_by(|left, right| {
            right
                .is_implementation
                .cmp(&left.is_implementation)
                .then_with(|| left.path.cmp(&right.path))
                .then_with(|| left.start_line.cmp(&right.start_line))
        });
        Ok(definitions)
    }

    /// Finds files that uniquely provide an unresolved symbol through the
    /// persistent project declaration index.
    ///
    /// # Errors
    ///
    /// Returns an error when package source enumeration or analysis fails.
    pub fn include_candidates(
        &mut self,
        query: &NwScriptDefinitionQuery,
        cancellation: Option<&nwnrs_nwscript::CancellationToken>,
    ) -> Result<Vec<NwScriptIncludeCandidate>, String> {
        if query.symbol.is_empty() {
            return Ok(Vec::new());
        }
        let files = list_nwscript_project_sources(query)?;
        let mut candidates = Vec::new();
        for file in files {
            check_cancellation(cancellation)?;
            if paths_refer_to_same_source(&file, &query.source_path) {
                continue;
            }
            let mut candidate_query = query.clone();
            candidate_query.source_path = file.clone();
            let Ok(definitions) = self.definitions(&candidate_query, cancellation) else {
                continue;
            };
            for definition in definitions.into_iter().filter(|definition| {
                paths_refer_to_same_source(&definition.path, &file)
                    && !matches!(
                        definition.kind,
                        NwScriptSymbolKind::Parameter | NwScriptSymbolKind::Field
                    )
            }) {
                let Some(include_name) = file.file_stem().and_then(|stem| stem.to_str()) else {
                    continue;
                };
                candidates.push(NwScriptIncludeCandidate {
                    include_name: include_name.to_string(),
                    definition,
                });
            }
        }
        candidates.sort_by(|left, right| {
            left.include_name
                .cmp(&right.include_name)
                .then_with(|| left.definition.path.cmp(&right.definition.path))
        });
        candidates.dedup_by(|left, right| {
            left.include_name == right.include_name && left.definition.path == right.definition.path
        });
        Ok(candidates)
    }

    /// Resolves outgoing calls from typed call targets in the cached compiler
    /// index.
    ///
    /// # Errors
    ///
    /// Returns an error when analysis fails or `line` is not inside a function.
    pub fn outgoing_calls(
        &mut self,
        query: &NwScriptDefinitionQuery,
        line: usize,
        cancellation: Option<&nwnrs_nwscript::CancellationToken>,
    ) -> Result<Vec<NwScriptOutgoingCall>, String> {
        let analysis = self.analysis(query, cancellation)?;
        let source = analysis
            .bundle
            .source_map
            .get(analysis.bundle.root_id)
            .ok_or_else(|| "call-hierarchy source is missing from its source map".to_string())?;
        let function = analysis
            .hir
            .functions
            .iter()
            .filter(|function| function.span.source_id == source.id)
            .find(|function| {
                source.location(function.span.start).is_some_and(|start| {
                    source
                        .location(function.span.end)
                        .is_some_and(|end| line >= start.line && line <= end.line)
                })
            })
            .ok_or_else(|| "call-hierarchy position is not inside a function".to_string())?;
        let mut calls =
            BTreeMap::<nwnrs_nwscript::SemanticSymbolId, Vec<NwScriptSourceRange>>::new();
        for reference in &analysis.index.references {
            if reference.span.source_id != source.id
                || reference.span.start < function.span.start
                || reference.span.end > function.span.end
                || !matches!(
                    reference.target,
                    nwnrs_nwscript::SemanticSymbolId::Function(_)
                        | nwnrs_nwscript::SemanticSymbolId::BuiltinFunction(_)
                )
            {
                continue;
            }
            if let Some(range) = source_range(source, reference.span) {
                calls
                    .entry(reference.target.clone())
                    .or_default()
                    .push(range);
            }
        }
        let mut outgoing = Vec::new();
        for (target, ranges) in calls {
            check_cancellation(cancellation)?;
            let name = match target {
                nwnrs_nwscript::SemanticSymbolId::Function(name)
                | nwnrs_nwscript::SemanticSymbolId::BuiltinFunction(name) => name,
                _ => continue,
            };
            let mut callee_query = query.clone();
            callee_query.symbol = name;
            callee_query.qualifier = None;
            if let Some(target) = self
                .definitions(&callee_query, cancellation)?
                .into_iter()
                .find(|definition| {
                    matches!(
                        definition.kind,
                        NwScriptSymbolKind::Function | NwScriptSymbolKind::BuiltinFunction
                    )
                })
            {
                outgoing.push(NwScriptOutgoingCall {
                    target,
                    ranges,
                });
            }
        }
        Ok(outgoing)
    }

    /// Returns compiler-backed symbols for a physical source while reusing its
    /// analyzed unit.
    ///
    /// # Errors
    ///
    /// Returns an error for source resolution, compiler analysis, or
    /// cancellation failures.
    pub fn document_symbols(
        &mut self,
        query: &NwScriptDefinitionQuery,
        cancellation: Option<&nwnrs_nwscript::CancellationToken>,
    ) -> Result<Vec<NwScriptDocumentSymbol>, String> {
        let analysis = match self.analysis(query, cancellation) {
            Ok(analysis) => analysis,
            Err(_) => {
                check_cancellation(cancellation)?;
                return list_nwscript_document_symbols(query, None);
            }
        };
        let source = analysis
            .bundle
            .source_map
            .get(analysis.bundle.root_id)
            .ok_or_else(|| "indexed source is missing from its source map".to_string())?;
        let tokens = lex_outline_tokens(source)?;
        let mut symbols = document_macro_symbols(source, &tokens);
        symbols.extend(document_symbols_from_ast(
            source,
            &tokens,
            analysis.script.items.clone(),
        ));
        symbols.sort_by_key(|symbol| {
            (
                symbol.range.start_line,
                symbol.range.start_column,
                symbol.range.end_line,
                symbol.range.end_column,
            )
        });
        symbols.dedup_by(|left, right| {
            left.name == right.name
                && left.kind == right.kind
                && left.selection_range == right.selection_range
        });
        Ok(symbols)
    }

    /// Returns semantic tokens and inlay hints from the cached typed compiler
    /// analysis.
    ///
    /// # Errors
    ///
    /// Returns an error for source resolution, compiler analysis, or
    /// cancellation failures.
    pub fn analyze_document(
        &mut self,
        query: &NwScriptDefinitionQuery,
        cancellation: Option<&nwnrs_nwscript::CancellationToken>,
    ) -> Result<(Vec<NwScriptSemanticToken>, Vec<NwScriptInlayHint>), String> {
        let analysis = self.analysis(query, cancellation)?;
        let source = analysis
            .bundle
            .source_map
            .get(analysis.bundle.root_id)
            .ok_or_else(|| "indexed source is missing from its source map".to_string())?;
        let tokens = lex_outline_tokens(source)?;
        semantic_document_from_analysis(source, &tokens, &analysis, false)
    }

    /// Builds the package Outline inventory while parsing every root at most
    /// once per revision.
    ///
    /// # Errors
    ///
    /// Returns an error when project source enumeration fails.
    pub fn project_documents(
        &mut self,
        query: &NwScriptDefinitionQuery,
        cancellation: Option<&nwnrs_nwscript::CancellationToken>,
    ) -> Result<NwScriptProjectDocuments, String> {
        let sources = list_nwscript_project_sources(query)?;
        let mut documents = Vec::with_capacity(sources.len());
        let mut warnings = Vec::new();
        for source in sources {
            if let Some(cancellation) = cancellation
                && let Err(error) = cancellation.check()
            {
                return Err(error.to_string());
            }
            let mut source_query = query.clone();
            source_query.source_path = source.clone();
            match self.document_symbols(&source_query, cancellation) {
                Ok(symbols) => documents.push((source, symbols)),
                Err(error) => warnings.push(format!("{}: {error}", source.display())),
            }
        }
        Ok((documents, warnings))
    }

    fn analysis(
        &mut self,
        query: &NwScriptDefinitionQuery,
        cancellation: Option<&nwnrs_nwscript::CancellationToken>,
    ) -> Result<Arc<CompilerAnalysis>, String> {
        if let Some(cancellation) = cancellation {
            cancellation.check().map_err(|error| error.to_string())?;
        }
        let root = canonical_source_path(&query.source_path);
        let config = ProjectAnalysisConfig::from(query);
        let reusable = self.units.get(&root).is_some_and(|cached| {
            !self.dirty_units.contains(&root)
                && cached.config == config
                && revisions_match(&cached.revisions, &query.source_overlays)
        });
        if reusable {
            self.access_clock = self.access_clock.wrapping_add(1);
            if let Some(cached) = self.units.get_mut(&root) {
                cached.last_used = self.access_clock;
                return Ok(Arc::clone(&cached.analysis));
            }
        }
        let last_good = self
            .units
            .get(&root)
            .filter(|cached| cached.config == config)
            .map(|cached| Arc::clone(&cached.analysis));
        let rebuilt = (|| {
            let resolver = build_symbol_resolver(query)?;
            let source_options = nwnrs_nwscript::SourceLoadOptions {
                max_include_depth: query.max_include_depth,
                ..nwnrs_nwscript::SourceLoadOptions::default()
            };
            let bundle = cancellation
                .map_or_else(
                    || {
                        nwnrs_nwscript::load_source_bundle(
                            &resolver,
                            &query.source_path.to_string_lossy(),
                            source_options,
                        )
                    },
                    |cancellation| {
                        nwnrs_nwscript::load_source_bundle_with_cancellation(
                            &resolver,
                            &query.source_path.to_string_lossy(),
                            source_options,
                            cancellation,
                        )
                    },
                )
                .map_err(|error| format!("failed to resolve compiler source graph: {error}"))?;
            check_cancellation(cancellation)?;
            let analysis = Arc::new(compiler_analysis_for_bundle(
                &resolver,
                query,
                &bundle,
                cancellation,
            )?);
            let revisions = analysis_revisions(&resolver, &analysis, &query.source_overlays);
            Ok::<_, String>((analysis, revisions))
        })();
        let (analysis, revisions) = match rebuilt {
            Ok(rebuilt) => rebuilt,
            Err(error) => {
                check_cancellation(cancellation)?;
                return last_good.ok_or(error);
            }
        };
        self.remove_unit(&root);
        self.access_clock = self.access_clock.wrapping_add(1);
        let weight = compiler_analysis_weight(&analysis);
        for revision in &revisions {
            let roots = self.reverse_uses.entry(revision.path.clone()).or_default();
            if !roots.contains(&root) {
                roots.push(root.clone());
            }
        }
        self.units.insert(
            root.clone(),
            CachedCompilerAnalysis {
                config,
                revisions,
                analysis: Arc::clone(&analysis),
                weight,
                last_used: self.access_clock,
            },
        );
        self.total_weight = self.total_weight.saturating_add(weight);
        self.evict_to_budget(&root);
        self.generation = self.generation.wrapping_add(1);
        Ok(analysis)
    }

    fn remove_unit(&mut self, root: &Path) {
        let root = canonical_source_path(root);
        let Some(cached) = self.units.remove(&root) else {
            self.dirty_units.remove(&root);
            return;
        };
        self.total_weight = self.total_weight.saturating_sub(cached.weight);
        self.dirty_units.remove(&root);
        for revision in cached.revisions {
            if let Some(roots) = self.reverse_uses.get_mut(&revision.path) {
                roots.retain(|candidate| candidate != &root);
                if roots.is_empty() {
                    self.reverse_uses.remove(&revision.path);
                }
            }
        }
    }

    fn evict_to_budget(&mut self, protected: &Path) {
        const MAX_SESSION_ANALYSIS_BYTES: usize = 256 * 1024 * 1024;
        while self.total_weight > MAX_SESSION_ANALYSIS_BYTES && self.units.len() > 1 {
            let candidate = self
                .units
                .iter()
                .filter(|(root, _)| root.as_path() != protected)
                .min_by_key(|(_, cached)| cached.last_used)
                .map(|(root, _)| root.clone());
            let Some(candidate) = candidate else {
                break;
            };
            self.remove_unit(&candidate);
        }
    }
}

fn compiler_analysis_weight(analysis: &CompilerAnalysis) -> usize {
    let source_bytes = analysis
        .bundle
        .source_map
        .iter()
        .map(SourceFile::len)
        .sum::<usize>();
    source_bytes
        .saturating_mul(8)
        .saturating_add(analysis.index.definitions.len().saturating_mul(256))
        .saturating_add(analysis.index.references.len().saturating_mul(128))
        .max(4096)
}

fn analysis_revisions(
    resolver: &SymbolResolver,
    analysis: &CompilerAnalysis,
    overlays: &BTreeMap<PathBuf, Vec<u8>>,
) -> Vec<SourceRevision> {
    let mut revisions = analysis
        .bundle
        .source_map
        .iter()
        .filter_map(|source| {
            let path = resolver.filesystem.resolve_script_path(&source.name)?;
            let path = canonical_source_path(&path);
            let bytes =
                overlay_contents(overlays, &path).unwrap_or_else(|| source.bytes().to_vec());
            Some(SourceRevision {
                path,
                hash: content_hash(&bytes),
            })
        })
        .collect::<Vec<_>>();
    revisions.sort_by(|left, right| left.path.cmp(&right.path));
    revisions.dedup_by(|left, right| left.path == right.path);
    revisions
}

fn revisions_match(revisions: &[SourceRevision], overlays: &BTreeMap<PathBuf, Vec<u8>>) -> bool {
    revisions.iter().all(|revision| {
        let bytes = overlay_contents(overlays, &revision.path)
            .or_else(|| fs::read(&revision.path).ok())
            .unwrap_or_default();
        content_hash(&bytes) == revision.hash
    })
}

fn overlay_contents(overlays: &BTreeMap<PathBuf, Vec<u8>>, path: &Path) -> Option<Vec<u8>> {
    overlays
        .iter()
        .find(|(candidate, _)| paths_refer_to_same_source(candidate, path))
        .map(|(_, contents)| contents.clone())
}

fn content_hash(bytes: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

fn canonical_source_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// Finds source definitions in the owning project, configured include roots,
/// and transitive local `nwpkg` include dependencies.
///
/// Function implementations are sorted before declarations so a normal editor
/// jump reaches executable source when both are present.
///
/// # Errors
///
/// Returns an error when local package dependencies cannot be resolved or an
/// explicitly selected search root cannot be traversed.
/// Finds project-, dependency-, include-, and game-aware compiler definitions.
///
/// # Errors
///
/// Returns an error when source or package resolution fails.
pub fn find_nwscript_definitions(
    query: &NwScriptDefinitionQuery,
) -> Result<Vec<NwScriptSymbolDefinition>, String> {
    NwScriptProjectIndex::new().definitions(query, None)
}

// Macros are erased before typed analysis and builtin declarations live in
// the external langspec. The optional ordinary-symbol scan is only tolerant
// recovery for a document that has never produced a valid semantic snapshot.
fn find_source_index_definitions(
    query: &NwScriptDefinitionQuery,
    recover_ordinary_symbols: bool,
) -> Result<Vec<NwScriptSymbolDefinition>, String> {
    if query.symbol.is_empty() {
        return Ok(Vec::new());
    }
    if !(1..=200).contains(&query.max_include_depth) {
        return Err("maximum include depth must be between 1 and 200".to_string());
    }
    let resolver = build_symbol_resolver(query)?;
    let bundle = nwnrs_nwscript::load_source_bundle(
        &resolver,
        &query.source_path.to_string_lossy(),
        nwnrs_nwscript::SourceLoadOptions {
            max_include_depth: query.max_include_depth,
            ..nwnrs_nwscript::SourceLoadOptions::default()
        },
    )
    .map_err(|error| format!("failed to resolve symbol source graph: {error}"))?;
    let mut files = bundle
        .source_order
        .iter()
        .filter_map(|source_id| bundle.source_map.get(*source_id))
        .filter_map(|source| resolver.filesystem.resolve_script_path(&source.name))
        .collect::<Vec<_>>();
    files.sort();
    files.dedup();

    let mut definitions = Vec::new();
    for path in files {
        let source = query
            .source_overlays
            .iter()
            .find_map(|(candidate, contents)| {
                paths_refer_to_same_source(candidate, &path).then_some(contents.clone())
            })
            .or_else(|| fs::read(&path).ok());
        let Some(source) = source else { continue };
        if recover_ordinary_symbols {
            definitions.extend(scan_source_definitions(
                &path,
                &source,
                &query.symbol,
                query.qualifier.as_deref(),
            ));
        } else {
            let source_id = SourceId::new(0);
            let Ok(tokens) = Lexer::new(source_id, &source).lex_all() else {
                continue;
            };
            let source_file = SourceFile::new(source_id, path.to_string_lossy(), source.clone());
            definitions.extend(scan_macro_definitions(
                &path,
                &source,
                &source_file,
                &tokens,
                &query.symbol,
                query.qualifier.as_deref(),
            ));
        }
    }
    for source_file in bundle
        .source_order
        .iter()
        .filter_map(|source_id| bundle.source_map.get(*source_id))
        .filter(|source| {
            resolver
                .filesystem
                .resolve_script_path(&source.name)
                .is_none()
        })
    {
        let path = virtual_source_path(&source_file.name);
        let source = source_file.bytes();
        let mut packed_definitions = if recover_ordinary_symbols {
            scan_source_definitions(&path, source, &query.symbol, query.qualifier.as_deref())
        } else {
            let Ok(tokens) = Lexer::new(source_file.id, source).lex_all() else {
                continue;
            };
            scan_macro_definitions(
                &path,
                source,
                source_file,
                &tokens,
                &query.symbol,
                query.qualifier.as_deref(),
            )
        };
        for definition in &mut packed_definitions {
            definition.documentation = definition.documentation.take().or_else(|| {
                line_start_offset(source, definition.start_line)
                    .and_then(|start| preceding_slash_documentation(source, start))
            });
            definition.virtual_uri = Some(virtual_source_uri(&source_file.name, source));
            definition.resource_name = Some(source_file.name.clone());
        }
        definitions.extend(packed_definitions);
    }
    if query.qualifier.is_none() {
        definitions.extend(scan_builtin_definitions(&resolver, query)?);
    }
    definitions.sort_by(|left, right| {
        right
            .is_implementation
            .cmp(&left.is_implementation)
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| left.start_line.cmp(&right.start_line))
    });
    Ok(definitions)
}

/// Finds semantically compatible occurrences of the symbol at a source
/// position across the effective include graph.
pub fn find_nwscript_references(
    query: &NwScriptDefinitionQuery,
    line: usize,
    column: usize,
) -> Result<Vec<NwScriptReference>, String> {
    let mut project_index = NwScriptProjectIndex::new();
    project_index.find_references(query, line, column, None)
}

fn find_macro_references(
    project_index: &mut NwScriptProjectIndex,
    query: &NwScriptDefinitionQuery,
    line: usize,
    column: usize,
    cancellation: Option<&nwnrs_nwscript::CancellationToken>,
) -> Result<Vec<NwScriptReference>, String> {
    let resolver = build_symbol_resolver(query)?;
    let analysis = project_index.analysis(query, cancellation)?;
    let root = analysis
        .bundle
        .source_map
        .get(analysis.bundle.root_id)
        .ok_or_else(|| "reference source is missing from its source map".to_string())?;
    let root_tokens = Lexer::new(root.id, root.bytes())
        .lex_all()
        .map_err(|error| format!("failed to lex reference source: {error}"))?;
    let offset = source_offset(root, line, column);
    let target_index = root_tokens
        .iter()
        .position(|token| {
            token.kind == TokenKind::Identifier
                && offset >= token.span.start
                && offset <= token.span.end
        })
        .ok_or_else(|| "reference position is not on an identifier".to_string())?;
    let target_token = root_tokens
        .get(target_index)
        .ok_or_else(|| "reference token index is invalid".to_string())?;
    if target_token.text != query.symbol {
        return Err("reference request symbol does not match the source position".to_string());
    }
    let target = macro_occurrence(&root_tokens, target_index)
        .ok_or_else(|| "compiler did not resolve a non-macro reference".to_string())?;
    let target_qualifier = query
        .qualifier
        .clone()
        .or_else(|| target.qualifier().map(str::to_string));
    if query.qualifier.is_some() && query.qualifier.as_deref() != target.qualifier() {
        return Err("reference request qualifier does not match the source position".to_string());
    }

    let mut source_files = Vec::new();
    source_files.extend(
        analysis
            .bundle
            .source_order
            .iter()
            .filter_map(|source_id| analysis.bundle.source_map.get(*source_id))
            .cloned(),
    );
    for source_path in list_nwscript_project_sources(query)? {
        check_cancellation(cancellation)?;
        let mut source_query = query.clone();
        source_query.source_path = source_path;
        let source_analysis = match project_index.analysis(&source_query, cancellation) {
            Ok(analysis) => analysis,
            Err(_) => {
                check_cancellation(cancellation)?;
                continue;
            }
        };
        source_files.extend(
            source_analysis
                .bundle
                .source_order
                .iter()
                .filter_map(|source_id| source_analysis.bundle.source_map.get(*source_id))
                .cloned(),
        );
    }
    source_files.sort_by(|left, right| left.name.cmp(&right.name));
    source_files.dedup_by(|left, right| left.name == right.name && left.bytes() == right.bytes());

    let mut references = Vec::new();
    for source_file in &source_files {
        check_cancellation(cancellation)?;
        let Ok(tokens) = Lexer::new(source_file.id, source_file.bytes()).lex_all() else {
            continue;
        };
        let physical_path = resolver.filesystem.resolve_script_path(&source_file.name);
        let path = physical_path
            .clone()
            .unwrap_or_else(|| virtual_source_path(&source_file.name));
        for (index, token) in tokens.iter().enumerate() {
            if token.kind != TokenKind::Identifier || token.text != query.symbol {
                continue;
            }
            let Some(occurrence) = macro_occurrence(&tokens, index) else {
                continue;
            };
            if occurrence.qualifier() != target_qualifier.as_deref() {
                continue;
            }
            let Some(range) = source_range(source_file, token.span) else {
                continue;
            };
            references.push(NwScriptReference {
                name: token.text.clone(),
                kind: NwScriptSymbolKind::Macro,
                path: path.clone(),
                range,
                is_declaration: occurrence.is_declaration(),
                container: None,
                virtual_uri: physical_path
                    .is_none()
                    .then(|| virtual_source_uri(&source_file.name, source_file.bytes())),
                resource_name: physical_path.is_none().then(|| source_file.name.clone()),
            });
        }
    }
    references.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.range.start_line.cmp(&right.range.start_line))
            .then_with(|| left.range.start_column.cmp(&right.range.start_column))
    });
    references.dedup_by(|left, right| {
        paths_refer_to_same_source(&left.path, &right.path) && left.range == right.range
    });
    Ok(references)
}

fn find_compiler_index_references(
    project_index: &mut NwScriptProjectIndex,
    query: &NwScriptDefinitionQuery,
    line: usize,
    column: usize,
    cancellation: Option<&nwnrs_nwscript::CancellationToken>,
) -> Result<Option<Vec<NwScriptReference>>, String> {
    let resolver = build_symbol_resolver(query)?;
    let bundle = nwnrs_nwscript::load_source_bundle(
        &resolver,
        &query.source_path.to_string_lossy(),
        nwnrs_nwscript::SourceLoadOptions {
            max_include_depth: query.max_include_depth,
            ..nwnrs_nwscript::SourceLoadOptions::default()
        },
    )
    .map_err(|error| format!("failed to resolve reference source graph: {error}"))?;
    let root = bundle
        .source_map
        .get(bundle.root_id)
        .ok_or_else(|| "reference source is missing from its source map".to_string())?;
    let root_tokens = Lexer::new(root.id, root.bytes())
        .lex_all()
        .map_err(|error| format!("failed to lex reference source: {error}"))?;
    let offset = source_offset(root, line, column);
    let token_index = root_tokens.iter().position(|token| {
        token.kind == TokenKind::Identifier
            && offset >= token.span.start
            && offset <= token.span.end
    });
    let Some(token_index) = token_index else {
        return Ok(Some(Vec::new()));
    };
    if root_tokens
        .get(token_index + 1)
        .is_some_and(|token| token.kind == TokenKind::BooleanNot)
    {
        return Ok(None);
    }

    let analysis = project_index.analysis(query, cancellation)?;
    let Some(target) = analysis.index.symbol_at(root.id, offset).cloned() else {
        return Ok(macro_occurrence(&root_tokens, token_index)
            .is_none()
            .then(Vec::new));
    };
    let local_target = matches!(target, nwnrs_nwscript::SemanticSymbolId::Local { .. });
    let source_roots = if local_target {
        vec![query.source_path.clone()]
    } else {
        list_nwscript_project_sources(query)?
    };
    let mut references = Vec::new();
    for source_path in source_roots {
        if let Some(cancellation) = cancellation {
            cancellation.check().map_err(|error| error.to_string())?;
        }
        let mut source_query = query.clone();
        source_query.source_path = source_path;
        let source_resolver = build_symbol_resolver(&source_query)?;
        let source_analysis = match project_index.analysis(&source_query, cancellation) {
            Ok(analysis) => analysis,
            Err(_) => continue,
        };
        let definition = source_analysis.index.definitions_for(&target).next();
        let name = definition
            .map(|definition| definition.name.clone())
            .unwrap_or_else(|| query.symbol.clone());
        let kind = definition
            .map(|definition| nwscript_symbol_kind(definition.kind))
            .unwrap_or(NwScriptSymbolKind::Variable);
        for definition in source_analysis.index.definitions_for(&target) {
            if let Some(reference) = indexed_reference(
                &source_resolver,
                &source_analysis.bundle,
                &name,
                kind,
                definition.span,
                true,
                definition.container.as_ref(),
                &source_analysis.index,
                &source_analysis.hir,
            ) {
                references.push(reference);
            }
        }
        for reference in source_analysis.index.references_for(&target) {
            if let Some(reference) = indexed_reference(
                &source_resolver,
                &source_analysis.bundle,
                &name,
                kind,
                reference.span,
                false,
                definition.and_then(|definition| definition.container.as_ref()),
                &source_analysis.index,
                &source_analysis.hir,
            ) {
                references.push(reference);
            }
        }
    }
    references.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.range.start_line.cmp(&right.range.start_line))
            .then_with(|| left.range.start_column.cmp(&right.range.start_column))
    });
    references.dedup_by(|left, right| {
        left.path == right.path
            && left.range == right.range
            && left.virtual_uri == right.virtual_uri
    });
    Ok(Some(references))
}

fn indexed_reference(
    resolver: &SymbolResolver,
    bundle: &SourceBundle,
    name: &str,
    kind: NwScriptSymbolKind,
    span: Span,
    is_declaration: bool,
    container: Option<&nwnrs_nwscript::SemanticSymbolId>,
    index: &nwnrs_nwscript::SemanticIndex,
    hir: &nwnrs_nwscript::HirModule,
) -> Option<NwScriptReference> {
    let source = bundle.source_map.get(span.source_id)?;
    let range = source_range(source, span)?;
    let physical_path = resolver
        .filesystem
        .resolve_script_path(&source.name)
        .map(|path| path.canonicalize().unwrap_or(path));
    let path = physical_path
        .clone()
        .unwrap_or_else(|| virtual_source_path(&source.name));
    let container = container
        .and_then(|container| {
            index
                .definitions_for(container)
                .next()
                .map(|definition| definition.name.clone())
        })
        .or_else(|| {
            (!is_declaration).then_some(()).and_then(|()| {
                hir.functions
                    .iter()
                    .filter(|function| {
                        function.span.source_id == span.source_id
                            && span.start >= function.span.start
                            && span.end <= function.span.end
                    })
                    .min_by_key(|function| function.span.end.saturating_sub(function.span.start))
                    .map(|function| function.name.clone())
            })
        });
    Some(NwScriptReference {
        name: name.to_string(),
        kind,
        path,
        range,
        is_declaration,
        container,
        virtual_uri: physical_path
            .is_none()
            .then(|| virtual_source_uri(&source.name, source.bytes())),
        resource_name: physical_path.is_none().then(|| source.name.clone()),
    })
}

const fn nwscript_symbol_kind(kind: SemanticSymbolKind) -> NwScriptSymbolKind {
    match kind {
        SemanticSymbolKind::Function => NwScriptSymbolKind::Function,
        SemanticSymbolKind::Global | SemanticSymbolKind::Local => NwScriptSymbolKind::Variable,
        SemanticSymbolKind::Constant => NwScriptSymbolKind::Constant,
        SemanticSymbolKind::Struct => NwScriptSymbolKind::Struct,
        SemanticSymbolKind::Field => NwScriptSymbolKind::Field,
        SemanticSymbolKind::Enum => NwScriptSymbolKind::Enum,
        SemanticSymbolKind::EnumVariant => NwScriptSymbolKind::EnumVariant,
        SemanticSymbolKind::TypeAlias => NwScriptSymbolKind::TypeAlias,
        SemanticSymbolKind::Parameter => NwScriptSymbolKind::Parameter,
        SemanticSymbolKind::BuiltinFunction => NwScriptSymbolKind::BuiltinFunction,
        SemanticSymbolKind::BuiltinConstant => NwScriptSymbolKind::BuiltinConstant,
        SemanticSymbolKind::EngineStructure => NwScriptSymbolKind::EngineStructure,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum MacroOccurrence {
    Declaration { qualifier: Option<String> },
    Invocation { qualifier: Option<String> },
}

impl MacroOccurrence {
    const fn is_declaration(&self) -> bool {
        matches!(self, Self::Declaration { .. })
    }

    fn qualifier(&self) -> Option<&str> {
        match self {
            Self::Declaration {
                qualifier,
            }
            | Self::Invocation {
                qualifier,
            } => qualifier.as_deref(),
        }
    }
}

fn macro_occurrence(tokens: &[Token], index: usize) -> Option<MacroOccurrence> {
    let token = tokens.get(index)?;
    if token.kind != TokenKind::Identifier {
        return None;
    }
    if tokens
        .get(index.wrapping_sub(1))
        .is_some_and(|previous| previous.kind == TokenKind::Keyword(Keyword::Define))
    {
        return Some(MacroOccurrence::Declaration {
            qualifier: None
        });
    }
    if index >= 2
        && tokens.get(index - 2).is_some_and(|previous| {
            previous.kind == TokenKind::Identifier && previous.text == "macro_rules"
        })
        && tokens
            .get(index - 1)
            .is_some_and(|previous| previous.kind == TokenKind::BooleanNot)
    {
        return Some(MacroOccurrence::Declaration {
            qualifier: None
        });
    }

    let path_start = macro_path_start(tokens, index);
    if path_start >= 2
        && tokens.get(path_start - 2).is_some_and(|previous| {
            previous.kind == TokenKind::Identifier && previous.text == "proc_macro"
        })
        && tokens
            .get(path_start - 1)
            .is_some_and(|previous| previous.kind == TokenKind::BooleanNot)
        && !tokens
            .get(index + 1)
            .is_some_and(|next| next.kind == TokenKind::Colon)
    {
        return Some(MacroOccurrence::Declaration {
            qualifier: macro_path_qualifier(tokens, path_start, index),
        });
    }
    if tokens
        .get(index + 1)
        .is_some_and(|next| next.kind == TokenKind::BooleanNot)
    {
        return Some(MacroOccurrence::Invocation {
            qualifier: macro_path_qualifier(tokens, path_start, index),
        });
    }
    None
}

fn macro_path_start(tokens: &[Token], index: usize) -> usize {
    let mut start = index;
    while start >= 3
        && tokens
            .get(start - 1)
            .is_some_and(|token| token.kind == TokenKind::Colon)
        && tokens
            .get(start - 2)
            .is_some_and(|token| token.kind == TokenKind::Colon)
        && tokens
            .get(start - 3)
            .is_some_and(|token| token.kind == TokenKind::Identifier)
    {
        start -= 3;
    }
    start
}

fn macro_path_qualifier(tokens: &[Token], start: usize, name: usize) -> Option<String> {
    (start < name).then(|| {
        (start..name)
            .step_by(3)
            .filter_map(|index| tokens.get(index))
            .map(|token| token.text.as_str())
            .collect::<Vec<_>>()
            .join("::")
    })
}

/// Resolves outgoing function calls from the function containing a source
/// position.
pub fn find_nwscript_outgoing_calls(
    query: &NwScriptDefinitionQuery,
    line: usize,
) -> Result<Vec<NwScriptOutgoingCall>, String> {
    NwScriptProjectIndex::new().outgoing_calls(query, line, None)
}
/// Enumerates hierarchical, source-authored declarations for one NSS
/// document. Included declarations and compiler-generated macro output are
/// excluded unless `resource_name` explicitly selects that included resource.
///
/// `resource_name` is used for read-only packed game documents. `None` indexes
/// `query.source_path`.
///
/// # Errors
///
/// Returns an error when the selected document cannot be resolved. Syntax or
/// macro-expansion errors fall back to a tolerant lexical outline so an
/// incomplete edit does not erase every discoverable declaration.
pub fn list_nwscript_document_symbols(
    query: &NwScriptDefinitionQuery,
    resource_name: Option<&str>,
) -> Result<Vec<NwScriptDocumentSymbol>, String> {
    if !(1..=200).contains(&query.max_include_depth) {
        return Err("maximum include depth must be between 1 and 200".to_string());
    }
    let resolver = build_symbol_resolver(query)?;
    let selected = resource_name.map_or_else(
        || query.source_path.to_string_lossy().into_owned(),
        str::to_string,
    );
    let bundle = match nwnrs_nwscript::load_source_bundle(
        &resolver,
        &selected,
        nwnrs_nwscript::SourceLoadOptions {
            max_include_depth: query.max_include_depth,
            ..nwnrs_nwscript::SourceLoadOptions::default()
        },
    ) {
        Ok(bundle) => bundle,
        Err(load_error) => {
            // Source-bundle discovery lexes include directives. While a user
            // is typing, a lexical error in the root must not erase the
            // Outline, so retain a root-only bundle for tolerant recovery.
            let Some(bytes) = resolver
                .resolve_script_bytes(&selected, nwnrs_nwscript::NW_SCRIPT_SOURCE_RES_TYPE)
                .map_err(|error| format!("failed to resolve Outline source: {error}"))?
            else {
                return Err(format!("failed to resolve Outline source: {load_error}"));
            };
            let mut source_map = SourceMap::new();
            let root_id = source_map.add_file(selected.clone(), bytes);
            SourceBundle {
                source_map,
                root_id,
                source_order: vec![root_id],
                include_edges: Vec::new(),
            }
        }
    };
    let source_file = bundle
        .source_map
        .get(bundle.root_id)
        .ok_or_else(|| "resolved Outline source is missing from its source map".to_string())?;
    let tokens = lex_outline_tokens(source_file)?;

    let mut symbols = document_macro_symbols(source_file, &tokens);
    let parsed = (|| {
        let mut registry = nwnrs_nwscript::MacroRegistry::new();
        nwnrs_nwscript::register_compiler_macros(&mut registry).ok()?;
        let preprocessed = nwnrs_nwscript::preprocess_source_bundle_with_macros(
            &bundle,
            &mut registry,
            nwnrs_nwscript::MacroExpansionOptions::default(),
        )
        .ok()?;
        nwnrs_nwscript::parse_tokens(preprocessed.tokens, None).ok()
    })();
    if let Some(script) = parsed {
        symbols.extend(document_symbols_from_ast(
            source_file,
            &tokens,
            script.items,
        ));
    } else {
        symbols.extend(fallback_function_symbols(source_file, &tokens));
    }
    symbols.sort_by_key(|symbol| {
        (
            symbol.range.start_line,
            symbol.range.start_column,
            symbol.range.end_line,
            symbol.range.end_column,
        )
    });
    symbols.dedup_by(|left, right| {
        left.name == right.name
            && left.kind == right.kind
            && left.selection_range == right.selection_range
    });
    Ok(symbols)
}

/// Returns semantic tokens and enum-value hints for one physical or virtual
/// NSS document. Included declarations participate in name classification but
/// only tokens authored in the selected document are emitted.
pub fn analyze_nwscript_document(
    query: &NwScriptDefinitionQuery,
    resource_name: Option<&str>,
) -> Result<(Vec<NwScriptSemanticToken>, Vec<NwScriptInlayHint>), String> {
    let resolver = build_symbol_resolver(query)?;
    let selected = resource_name.map_or_else(
        || query.source_path.to_string_lossy().into_owned(),
        str::to_string,
    );
    let bundle = nwnrs_nwscript::load_source_bundle(
        &resolver,
        &selected,
        nwnrs_nwscript::SourceLoadOptions {
            max_include_depth: query.max_include_depth,
            ..nwnrs_nwscript::SourceLoadOptions::default()
        },
    )
    .map_err(|error| format!("failed to resolve semantic source graph: {error}"))?;
    let source_file = bundle
        .source_map
        .get(bundle.root_id)
        .ok_or_else(|| "semantic source is missing from its source map".to_string())?;
    let tokens = lex_outline_tokens(source_file)?;
    let analysis = compiler_analysis_for_bundle(&resolver, query, &bundle, None)?;
    semantic_document_from_analysis(source_file, &tokens, &analysis, resource_name.is_some())
}

fn semantic_document_from_analysis(
    source_file: &SourceFile,
    tokens: &[Token],
    analysis: &CompilerAnalysis,
    default_library: bool,
) -> Result<(Vec<NwScriptSemanticToken>, Vec<NwScriptInlayHint>), String> {
    let mut hints = Vec::new();
    collect_parameter_hints(
        source_file,
        &analysis.script.items,
        Some(&analysis.langspec),
        &mut hints,
    );
    collect_enum_value_hints(source_file, tokens, &analysis.script.items, &mut hints);
    let mut semantic_tokens = Vec::new();
    for definition in analysis
        .index
        .definitions
        .iter()
        .filter(|definition| definition.span.source_id == source_file.id)
    {
        if let Some((kind, readonly)) = semantic_token_kind(definition.kind)
            && let Some(range) = source_range(source_file, definition.span)
        {
            semantic_tokens.push(NwScriptSemanticToken {
                range,
                kind,
                is_declaration: true,
                is_readonly: readonly,
                is_default_library: default_library,
            });
        }
    }
    for reference in analysis
        .index
        .references
        .iter()
        .filter(|reference| reference.span.source_id == source_file.id)
    {
        let kind = analysis
            .index
            .definitions_for(&reference.target)
            .next()
            .and_then(|definition| semantic_token_kind(definition.kind));
        if let Some((kind, readonly)) = kind
            && let Some(range) = source_range(source_file, reference.span)
        {
            semantic_tokens.push(NwScriptSemanticToken {
                range,
                kind,
                is_declaration: false,
                is_readonly: readonly,
                is_default_library: default_library,
            });
        }
    }
    for macro_symbol in document_macro_symbols(source_file, tokens) {
        semantic_tokens.push(NwScriptSemanticToken {
            range:              macro_symbol.selection_range,
            kind:               NwScriptSemanticTokenKind::Macro,
            is_declaration:     true,
            is_readonly:        true,
            is_default_library: default_library,
        });
    }
    semantic_tokens.sort_by_key(|token| {
        (
            token.range.start_line,
            token.range.start_column,
            token.range.end_line,
            token.range.end_column,
        )
    });
    semantic_tokens.dedup_by(|left, right| {
        left.range == right.range
            && left.kind == right.kind
            && left.is_declaration == right.is_declaration
    });
    Ok((semantic_tokens, hints))
}

fn compiler_analysis_for_bundle(
    resolver: &SymbolResolver,
    query: &NwScriptDefinitionQuery,
    bundle: &SourceBundle,
    cancellation: Option<&nwnrs_nwscript::CancellationToken>,
) -> Result<CompilerAnalysis, String> {
    check_cancellation(cancellation)?;
    let builtin = load_builtin_source(resolver, query)?
        .ok_or_else(|| "could not resolve nwscript.nss for compiler analysis".to_string())?;
    let langspec = nwnrs_nwscript::parse_langspec_bytes(&builtin.resource_name, &builtin.bytes)
        .map_err(|error| format!("failed to parse nwscript.nss: {error}"))?;
    check_cancellation(cancellation)?;
    let mut registry = nwnrs_nwscript::MacroRegistry::new();
    nwnrs_nwscript::register_compiler_macros(&mut registry)
        .map_err(|error| format!("failed to register compiler macros: {error}"))?;
    let script = match cancellation {
        Some(cancellation) => nwnrs_nwscript::parse_source_bundle_with_macros_and_cancellation(
            bundle,
            Some(&langspec),
            &mut registry,
            nwnrs_nwscript::MacroExpansionOptions::default(),
            cancellation,
        ),
        None => nwnrs_nwscript::parse_source_bundle_with_macros(
            bundle,
            Some(&langspec),
            &mut registry,
            nwnrs_nwscript::MacroExpansionOptions::default(),
        ),
    }
    .map_err(|error| format!("failed to parse semantic source: {error}"))?;
    check_cancellation(cancellation)?;
    let semantic = nwnrs_nwscript::analyze_script(&script, Some(&langspec))
        .map_err(|error| format!("failed to analyze semantic source: {error}"))?;
    check_cancellation(cancellation)?;
    let hir = nwnrs_nwscript::lower_to_hir(&script, &semantic, Some(&langspec))
        .map_err(|error| format!("failed to lower semantic source: {error}"))?;
    check_cancellation(cancellation)?;
    let index = nwnrs_nwscript::build_semantic_index(
        &script,
        &semantic,
        &hir,
        Some(&langspec),
        &bundle.source_map,
    );
    Ok(CompilerAnalysis {
        langspec,
        bundle: bundle.clone(),
        script,
        semantic,
        hir,
        index,
    })
}

fn check_cancellation(
    cancellation: Option<&nwnrs_nwscript::CancellationToken>,
) -> Result<(), String> {
    cancellation.map_or(Ok(()), |cancellation| {
        cancellation.check().map_err(|error| error.to_string())
    })
}

const fn semantic_token_kind(
    kind: SemanticSymbolKind,
) -> Option<(NwScriptSemanticTokenKind, bool)> {
    match kind {
        SemanticSymbolKind::Function | SemanticSymbolKind::BuiltinFunction => {
            Some((NwScriptSemanticTokenKind::Function, false))
        }
        SemanticSymbolKind::Parameter => Some((NwScriptSemanticTokenKind::Parameter, false)),
        SemanticSymbolKind::Global | SemanticSymbolKind::Local => {
            Some((NwScriptSemanticTokenKind::Variable, false))
        }
        SemanticSymbolKind::Constant | SemanticSymbolKind::BuiltinConstant => {
            Some((NwScriptSemanticTokenKind::Variable, true))
        }
        SemanticSymbolKind::Struct
        | SemanticSymbolKind::TypeAlias
        | SemanticSymbolKind::EngineStructure => Some((NwScriptSemanticTokenKind::Type, false)),
        SemanticSymbolKind::Field => Some((NwScriptSemanticTokenKind::Property, false)),
        SemanticSymbolKind::Enum => Some((NwScriptSemanticTokenKind::Enum, false)),
        SemanticSymbolKind::EnumVariant => Some((NwScriptSemanticTokenKind::EnumMember, true)),
    }
}

fn collect_enum_value_hints(
    source_file: &SourceFile,
    tokens: &[Token],
    items: &[TopLevelItem],
    hints: &mut Vec<NwScriptInlayHint>,
) {
    let macro_spans = macro_syntax_spans(tokens);
    for enumeration in items.iter().filter_map(|item| {
        let TopLevelItem::Enum(enumeration) = item else {
            return None;
        };
        Some(enumeration)
    }) {
        if enumeration.backing != nwnrs_nwscript::EnumBackingType::Int {
            continue;
        }
        let mut next_value = 0_i32;
        for variant in &enumeration.variants {
            let explicit = variant.value.as_ref().and_then(|value| {
                if let nwnrs_nwscript::ExprKind::Literal(nwnrs_nwscript::Literal::Integer(value)) =
                    value.kind
                {
                    Some(value)
                } else {
                    None
                }
            });
            if let Some(value) = explicit {
                next_value = value;
            } else if variant.span.source_id == source_file.id
                && let Some(selection) =
                    identifier_span(tokens, &macro_spans, variant.span, &variant.name)
                && let Some(location) = source_file.location(selection.end)
            {
                hints.push(NwScriptInlayHint {
                    line:   location.line,
                    column: location.column,
                    label:  format!(" = {next_value}"),
                    kind:   "enumValue",
                });
            }
            next_value = next_value.saturating_add(1);
        }
    }
}

fn source_offset(source: &SourceFile, line: usize, column: usize) -> usize {
    if line == 0 || column == 0 {
        return 0;
    }
    let mut current_line = 1_usize;
    let mut offset = 0_usize;
    while current_line < line {
        let Some(next) = source
            .bytes()
            .get(offset..)
            .and_then(|tail| tail.iter().position(|byte| *byte == b'\n'))
        else {
            return source.len();
        };
        offset = offset.saturating_add(next).saturating_add(1);
        current_line += 1;
    }
    offset.saturating_add(column - 1).min(source.len())
}

fn collect_parameter_hints(
    source_file: &SourceFile,
    items: &[TopLevelItem],
    langspec: Option<&nwnrs_nwscript::LangSpec>,
    hints: &mut Vec<NwScriptInlayHint>,
) {
    let mut signatures = items
        .iter()
        .filter_map(|item| {
            let TopLevelItem::Function(function) = item else {
                return None;
            };
            Some((
                function.name.clone(),
                function
                    .parameters
                    .iter()
                    .map(|parameter| parameter.name.clone())
                    .collect::<Vec<_>>(),
            ))
        })
        .collect::<BTreeMap<_, _>>();
    if let Some(langspec) = langspec {
        for function in &langspec.functions {
            signatures.entry(function.name.clone()).or_insert_with(|| {
                function
                    .parameters
                    .iter()
                    .map(|parameter| parameter.name.clone())
                    .collect()
            });
        }
    }
    for item in items {
        let TopLevelItem::Function(function) = item else {
            continue;
        };
        if function.span.source_id != source_file.id {
            continue;
        }
        if let Some(body) = &function.body {
            visit_statements_for_hints(source_file, &body.statements, &signatures, hints);
        }
    }
}

fn visit_statements_for_hints(
    source_file: &SourceFile,
    statements: &[nwnrs_nwscript::Stmt],
    signatures: &BTreeMap<String, Vec<String>>,
    hints: &mut Vec<NwScriptInlayHint>,
) {
    for statement in statements {
        match statement {
            nwnrs_nwscript::Stmt::Block(block) => {
                visit_statements_for_hints(source_file, &block.statements, signatures, hints);
            }
            nwnrs_nwscript::Stmt::Declaration(declaration) => {
                for initializer in declaration
                    .declarators
                    .iter()
                    .filter_map(|declarator| declarator.initializer.as_ref())
                {
                    visit_expression_for_hints(source_file, initializer, signatures, hints);
                }
            }
            nwnrs_nwscript::Stmt::Expression(statement) => {
                visit_expression_for_hints(source_file, &statement.expr, signatures, hints);
            }
            nwnrs_nwscript::Stmt::If(statement) => {
                visit_expression_for_hints(source_file, &statement.condition, signatures, hints);
                visit_statement_for_hints(source_file, &statement.then_branch, signatures, hints);
                if let Some(branch) = &statement.else_branch {
                    visit_statement_for_hints(source_file, branch, signatures, hints);
                }
            }
            nwnrs_nwscript::Stmt::Switch(statement) => {
                visit_expression_for_hints(source_file, &statement.condition, signatures, hints);
                visit_statement_for_hints(source_file, &statement.body, signatures, hints);
            }
            nwnrs_nwscript::Stmt::Return(statement) => {
                if let Some(value) = &statement.value {
                    visit_expression_for_hints(source_file, value, signatures, hints);
                }
            }
            nwnrs_nwscript::Stmt::While(statement) => {
                visit_expression_for_hints(source_file, &statement.condition, signatures, hints);
                visit_statement_for_hints(source_file, &statement.body, signatures, hints);
            }
            nwnrs_nwscript::Stmt::DoWhile(statement) => {
                visit_statement_for_hints(source_file, &statement.body, signatures, hints);
                visit_expression_for_hints(source_file, &statement.condition, signatures, hints);
            }
            nwnrs_nwscript::Stmt::For(statement) => {
                for expression in [
                    statement.initializer.as_ref(),
                    statement.condition.as_ref(),
                    statement.update.as_ref(),
                ]
                .into_iter()
                .flatten()
                {
                    visit_expression_for_hints(source_file, expression, signatures, hints);
                }
                visit_statement_for_hints(source_file, &statement.body, signatures, hints);
            }
            nwnrs_nwscript::Stmt::Case(statement) => {
                visit_expression_for_hints(source_file, &statement.value, signatures, hints);
            }
            nwnrs_nwscript::Stmt::StaticAssert(assertion) => {
                visit_expression_for_hints(source_file, &assertion.condition, signatures, hints);
            }
            _ => {}
        }
    }
}

fn visit_statement_for_hints(
    source_file: &SourceFile,
    statement: &nwnrs_nwscript::Stmt,
    signatures: &BTreeMap<String, Vec<String>>,
    hints: &mut Vec<NwScriptInlayHint>,
) {
    visit_statements_for_hints(
        source_file,
        std::slice::from_ref(statement),
        signatures,
        hints,
    );
}

fn visit_expression_for_hints(
    source_file: &SourceFile,
    expression: &nwnrs_nwscript::Expr,
    signatures: &BTreeMap<String, Vec<String>>,
    hints: &mut Vec<NwScriptInlayHint>,
) {
    match &expression.kind {
        nwnrs_nwscript::ExprKind::Call {
            callee,
            arguments,
        } => {
            if let nwnrs_nwscript::ExprKind::Identifier(name) = &callee.kind
                && let Some(parameters) = signatures.get(name)
            {
                for (argument, parameter) in arguments.iter().zip(parameters) {
                    if argument.span.source_id != source_file.id {
                        continue;
                    }
                    if matches!(
                        &argument.kind,
                        nwnrs_nwscript::ExprKind::Identifier(name) if name == parameter
                    ) {
                        continue;
                    }
                    if let Some(location) = source_file.location(argument.span.start) {
                        hints.push(NwScriptInlayHint {
                            line:   location.line,
                            column: location.column,
                            label:  format!("{parameter}:"),
                            kind:   if matches!(argument.kind, nwnrs_nwscript::ExprKind::Literal(_))
                            {
                                "parameterLiteral"
                            } else {
                                "parameter"
                            },
                        });
                    }
                }
            }
            visit_expression_for_hints(source_file, callee, signatures, hints);
            for argument in arguments {
                visit_expression_for_hints(source_file, argument, signatures, hints);
            }
        }
        nwnrs_nwscript::ExprKind::Match(expression) => {
            visit_expression_for_hints(source_file, &expression.value, signatures, hints);
            for arm in &expression.arms {
                if let Some(guard) = &arm.guard {
                    visit_expression_for_hints(source_file, guard, signatures, hints);
                }
                match &arm.body {
                    nwnrs_nwscript::MatchArmBody::Expr(expression) => {
                        visit_expression_for_hints(source_file, expression, signatures, hints);
                    }
                    nwnrs_nwscript::MatchArmBody::Block(block) => {
                        visit_statements_for_hints(
                            source_file,
                            &block.statements,
                            signatures,
                            hints,
                        );
                        if let Some(tail) = &block.tail {
                            visit_expression_for_hints(source_file, tail, signatures, hints);
                        }
                    }
                }
            }
        }
        nwnrs_nwscript::ExprKind::FieldAccess {
            base, ..
        }
        | nwnrs_nwscript::ExprKind::Unary {
            expr: base, ..
        } => {
            visit_expression_for_hints(source_file, base, signatures, hints);
        }
        nwnrs_nwscript::ExprKind::Binary {
            left,
            right,
            ..
        }
        | nwnrs_nwscript::ExprKind::Assignment {
            left,
            right,
            ..
        } => {
            visit_expression_for_hints(source_file, left, signatures, hints);
            visit_expression_for_hints(source_file, right, signatures, hints);
        }
        nwnrs_nwscript::ExprKind::Conditional {
            condition,
            when_true,
            when_false,
        } => {
            visit_expression_for_hints(source_file, condition, signatures, hints);
            visit_expression_for_hints(source_file, when_true, signatures, hints);
            visit_expression_for_hints(source_file, when_false, signatures, hints);
        }
        _ => {}
    }
}

fn lex_outline_tokens(source_file: &SourceFile) -> Result<Vec<Token>, String> {
    let mut recoverable = source_file.bytes().to_vec();
    const MAX_LEXICAL_RECOVERIES: usize = 64;
    for _ in 0..MAX_LEXICAL_RECOVERIES {
        match Lexer::new(source_file.id, &recoverable).lex_all() {
            Ok(tokens) => return Ok(tokens),
            Err(error) => {
                let start = error.span.start.min(recoverable.len());
                let end = error
                    .span
                    .end
                    .max(start.saturating_add(1))
                    .min(recoverable.len());
                if start >= end {
                    return Err(format!("failed to lex Outline source: {error}"));
                }
                for byte in recoverable.get_mut(start..end).unwrap_or_default() {
                    if !matches!(*byte, b'\n' | b'\r') {
                        *byte = b' ';
                    }
                }
            }
        }
    }
    Err(format!(
        "failed to lex Outline source after {MAX_LEXICAL_RECOVERIES} recoveries"
    ))
}

fn document_symbols_from_ast(
    source_file: &SourceFile,
    tokens: &[Token],
    items: Vec<TopLevelItem>,
) -> Vec<NwScriptDocumentSymbol> {
    let mut symbols = Vec::new();
    let macro_spans = macro_syntax_spans(tokens);
    for item in items {
        match item {
            TopLevelItem::Function(function) if function.span.source_id == source_file.id => {
                let Some(selection) =
                    authored_function_span(tokens, &macro_spans, function.span, &function.name)
                else {
                    continue;
                };
                let declaration_start =
                    preceding_attribute_start(source_file.bytes(), function.span.start);
                let detail_end = function
                    .body
                    .as_ref()
                    .map_or(function.span.end, |body| body.span.start);
                let detail = function_document_detail(
                    source_file.bytes(),
                    declaration_start,
                    function.span.start,
                    detail_end,
                );
                if let Some(symbol) = make_document_symbol(
                    source_file,
                    function.name,
                    NwScriptDocumentSymbolKind::Function,
                    Span::new(source_file.id, declaration_start, function.span.end),
                    selection,
                    Some(detail),
                    Vec::new(),
                ) {
                    symbols.push(symbol);
                }
            }
            TopLevelItem::Global(declaration) if declaration.span.source_id == source_file.id => {
                for declarator in declaration.declarators {
                    let Some(selection) =
                        identifier_span(tokens, &macro_spans, declarator.span, &declarator.name)
                    else {
                        continue;
                    };
                    if let Some(symbol) = make_document_symbol(
                        source_file,
                        declarator.name,
                        if declaration.ty.is_const {
                            NwScriptDocumentSymbolKind::Constant
                        } else {
                            NwScriptDocumentSymbolKind::Variable
                        },
                        declaration.span,
                        selection,
                        Some(source_detail(
                            source_file.bytes(),
                            declaration.span.start,
                            declaration.span.end,
                        )),
                        Vec::new(),
                    ) {
                        symbols.push(symbol);
                    }
                }
            }
            TopLevelItem::Struct(structure) if structure.span.source_id == source_file.id => {
                let Some(selection) =
                    identifier_span(tokens, &macro_spans, structure.span, &structure.name)
                else {
                    continue;
                };
                let mut children = Vec::new();
                for field in structure.fields {
                    for name in field.names {
                        if let Some(symbol) = make_document_symbol(
                            source_file,
                            name.name,
                            NwScriptDocumentSymbolKind::Field,
                            field.span,
                            name.span,
                            Some(source_detail(
                                source_file.bytes(),
                                field.span.start,
                                field.span.end,
                            )),
                            Vec::new(),
                        ) {
                            children.push(symbol);
                        }
                    }
                }
                if let Some(symbol) = make_document_symbol(
                    source_file,
                    structure.name,
                    NwScriptDocumentSymbolKind::Struct,
                    structure.span,
                    selection,
                    Some("struct".to_string()),
                    children,
                ) {
                    symbols.push(symbol);
                }
            }
            TopLevelItem::Enum(declaration) if declaration.span.source_id == source_file.id => {
                let Some(selection) =
                    identifier_span(tokens, &macro_spans, declaration.span, &declaration.name)
                else {
                    continue;
                };
                let detail_end = declaration
                    .variants
                    .first()
                    .map_or(declaration.span.end, |variant| variant.span.start);
                let mut children = Vec::new();
                for variant in declaration.variants {
                    let Some(variant_selection) =
                        identifier_span(tokens, &macro_spans, variant.span, &variant.name)
                    else {
                        continue;
                    };
                    let mut aliases = Vec::new();
                    for alias in variant.aliases {
                        if let Some(symbol) = make_document_symbol(
                            source_file,
                            alias.name,
                            NwScriptDocumentSymbolKind::Constant,
                            alias.span,
                            alias.span,
                            Some(format!("alias of {}::{}", declaration.name, variant.name)),
                            Vec::new(),
                        ) {
                            aliases.push(symbol);
                        }
                    }
                    if let Some(symbol) = make_document_symbol(
                        source_file,
                        variant.name,
                        NwScriptDocumentSymbolKind::EnumVariant,
                        variant.span,
                        variant_selection,
                        Some(source_detail(
                            source_file.bytes(),
                            variant.span.start,
                            variant.span.end,
                        )),
                        aliases,
                    ) {
                        children.push(symbol);
                    }
                }
                if let Some(symbol) = make_document_symbol(
                    source_file,
                    declaration.name,
                    NwScriptDocumentSymbolKind::Enum,
                    declaration.span,
                    selection,
                    Some(source_detail(
                        source_file.bytes(),
                        declaration.span.start,
                        detail_end,
                    )),
                    children,
                ) {
                    symbols.push(symbol);
                }
            }
            TopLevelItem::TypeAlias(alias) if alias.span.source_id == source_file.id => {
                let Some(selection) =
                    identifier_span(tokens, &macro_spans, alias.span, &alias.name)
                else {
                    continue;
                };
                if let Some(symbol) = make_document_symbol(
                    source_file,
                    alias.name,
                    NwScriptDocumentSymbolKind::TypeAlias,
                    alias.span,
                    selection,
                    Some(source_detail(
                        source_file.bytes(),
                        alias.span.start,
                        alias.span.end,
                    )),
                    Vec::new(),
                ) {
                    symbols.push(symbol);
                }
            }
            _ => {}
        }
    }
    symbols
}

fn document_macro_symbols(
    source_file: &SourceFile,
    tokens: &[Token],
) -> Vec<NwScriptDocumentSymbol> {
    let mut symbols = Vec::new();
    let mut brace_depth = 0_usize;
    let mut index = 0_usize;
    while index < tokens.len() {
        let Some(token) = tokens.get(index) else {
            break;
        };
        let mut selection = None;
        let mut declaration_end = None;
        if brace_depth == 0 && matches!(token.kind, TokenKind::Keyword(Keyword::Define)) {
            selection = tokens
                .get(index.saturating_add(1))
                .filter(|name| name.kind == TokenKind::Identifier);
            declaration_end = Some(line_end_offset(source_file.bytes(), token.span.start));
        } else if brace_depth == 0
            && token.kind == TokenKind::Identifier
            && matches!(token.text.as_str(), "macro_rules" | "proc_macro")
            && matches!(
                tokens.get(index.saturating_add(1)).map(|next| &next.kind),
                Some(TokenKind::BooleanNot)
            )
        {
            let mut cursor = index.saturating_add(2);
            while let Some(name) = tokens
                .get(cursor)
                .filter(|name| name.kind == TokenKind::Identifier)
            {
                selection = Some(name);
                if matches!(
                    (tokens.get(cursor + 1), tokens.get(cursor + 2)),
                    (Some(first), Some(second))
                        if first.kind == TokenKind::Colon && second.kind == TokenKind::Colon
                ) {
                    cursor += 3;
                } else {
                    break;
                }
            }
            if let Some(left_brace) = (cursor..tokens.len()).find(|candidate| {
                tokens
                    .get(*candidate)
                    .is_some_and(|candidate| candidate.kind == TokenKind::LeftBrace)
            }) {
                declaration_end = matching_delimiter(
                    tokens,
                    left_brace,
                    TokenKind::LeftBrace,
                    TokenKind::RightBrace,
                )
                .and_then(|right_brace| tokens.get(right_brace))
                .map(|right_brace| right_brace.span.end);
            }
        }
        if let (Some(name), Some(end)) = (selection, declaration_end)
            && let Some(symbol) = make_document_symbol(
                source_file,
                name.text.clone(),
                NwScriptDocumentSymbolKind::Macro,
                Span::new(source_file.id, token.span.start, end),
                name.span,
                Some(source_detail(source_file.bytes(), token.span.start, end)),
                Vec::new(),
            )
        {
            symbols.push(symbol);
        }
        match token.kind {
            TokenKind::LeftBrace => brace_depth = brace_depth.saturating_add(1),
            TokenKind::RightBrace => brace_depth = brace_depth.saturating_sub(1),
            _ => {}
        }
        index += 1;
    }
    symbols
}

fn fallback_function_symbols(
    source_file: &SourceFile,
    tokens: &[Token],
) -> Vec<NwScriptDocumentSymbol> {
    let mut symbols = Vec::new();
    let mut brace_depth = 0_usize;
    for (index, token) in tokens.iter().enumerate() {
        match token.kind {
            TokenKind::LeftBrace => brace_depth = brace_depth.saturating_add(1),
            TokenKind::RightBrace => brace_depth = brace_depth.saturating_sub(1),
            TokenKind::LeftParen if brace_depth == 0 => {
                let Some(name_index) = index.checked_sub(1) else {
                    continue;
                };
                let Some(name) = tokens.get(name_index) else {
                    continue;
                };
                let Some(start_index) = function_declaration_start(tokens, name_index) else {
                    continue;
                };
                let Some(right_paren) = matching_right_paren(tokens, index) else {
                    continue;
                };
                let Some(after) = tokens.get(right_paren.saturating_add(1)) else {
                    continue;
                };
                let end = if after.kind == TokenKind::LeftBrace {
                    matching_delimiter(
                        tokens,
                        right_paren.saturating_add(1),
                        TokenKind::LeftBrace,
                        TokenKind::RightBrace,
                    )
                    .and_then(|right_brace| tokens.get(right_brace))
                    .map_or(after.span.end, |right_brace| right_brace.span.end)
                } else if after.kind == TokenKind::Semicolon {
                    after.span.end
                } else {
                    continue;
                };
                let Some(start) = tokens.get(start_index).map(|start| start.span.start) else {
                    continue;
                };
                let declaration_start = preceding_attribute_start(source_file.bytes(), start);
                let detail_end = tokens
                    .get(right_paren)
                    .map_or(name.span.end, |end| end.span.end);
                if let Some(symbol) = make_document_symbol(
                    source_file,
                    name.text.clone(),
                    NwScriptDocumentSymbolKind::Function,
                    Span::new(source_file.id, declaration_start, end),
                    name.span,
                    Some(function_document_detail(
                        source_file.bytes(),
                        declaration_start,
                        start,
                        detail_end,
                    )),
                    Vec::new(),
                ) {
                    symbols.push(symbol);
                }
            }
            _ => {}
        }
    }
    symbols
}

fn make_document_symbol(
    source_file: &SourceFile,
    name: String,
    kind: NwScriptDocumentSymbolKind,
    span: Span,
    selection: Span,
    detail: Option<String>,
    children: Vec<NwScriptDocumentSymbol>,
) -> Option<NwScriptDocumentSymbol> {
    Some(NwScriptDocumentSymbol {
        name,
        kind,
        detail: detail.filter(|detail| !detail.is_empty()),
        range: source_range(source_file, span)?,
        selection_range: source_range(source_file, selection)?,
        children,
    })
}

fn source_range(source_file: &SourceFile, span: Span) -> Option<NwScriptSourceRange> {
    if span.source_id != source_file.id {
        return None;
    }
    let start = source_file.location(span.start)?;
    let end = source_file.location(span.end.min(source_file.bytes().len()))?;
    Some(NwScriptSourceRange {
        start_line:   start.line,
        start_column: start.column,
        end_line:     end.line,
        end_column:   end.column,
    })
}

fn identifier_span(tokens: &[Token], macro_spans: &[Span], span: Span, name: &str) -> Option<Span> {
    tokens
        .iter()
        .find(|token| {
            token.kind == TokenKind::Identifier
                && token.text == name
                && token.span.start >= span.start
                && token.span.end <= span.end
                && !macro_spans.iter().any(|macro_span| {
                    macro_span.source_id == token.span.source_id
                        && token.span.start >= macro_span.start
                        && token.span.end <= macro_span.end
                })
        })
        .map(|token| token.span)
}

fn authored_function_span(
    tokens: &[Token],
    macro_spans: &[Span],
    span: Span,
    name: &str,
) -> Option<Span> {
    tokens.iter().enumerate().find_map(|(index, token)| {
        if token.kind != TokenKind::LeftParen || index == 0 {
            return None;
        }
        let name_index = index - 1;
        let candidate = tokens.get(name_index)?;
        if candidate.kind != TokenKind::Identifier
            || candidate.text != name
            || candidate.span.start < span.start
            || candidate.span.end > span.end
            || macro_spans.iter().any(|macro_span| {
                macro_span.source_id == candidate.span.source_id
                    && candidate.span.start >= macro_span.start
                    && candidate.span.end <= macro_span.end
            })
            || function_declaration_start(tokens, name_index).is_none()
        {
            return None;
        }
        Some(candidate.span)
    })
}

fn macro_syntax_spans(tokens: &[Token]) -> Vec<Span> {
    let mut spans = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        if token.kind != TokenKind::Identifier
            || tokens.get(index + 1).map(|next| &next.kind) != Some(&TokenKind::BooleanNot)
        {
            continue;
        }
        let definition = matches!(token.text.as_str(), "macro_rules" | "proc_macro");
        let opener = if definition {
            (index + 2..tokens.len()).find(|candidate| {
                tokens
                    .get(*candidate)
                    .is_some_and(|candidate| candidate.kind == TokenKind::LeftBrace)
            })
        } else {
            Some(index + 2).filter(|candidate| {
                tokens.get(*candidate).is_some_and(|candidate| {
                    matches!(
                        candidate.kind,
                        TokenKind::LeftParen | TokenKind::LeftBrace | TokenKind::LeftSquareBracket
                    )
                })
            })
        };
        let Some(opener) = opener else {
            continue;
        };
        let Some(opener_token) = tokens.get(opener) else {
            continue;
        };
        let closing_kind = match opener_token.kind {
            TokenKind::LeftParen => TokenKind::RightParen,
            TokenKind::LeftBrace => TokenKind::RightBrace,
            TokenKind::LeftSquareBracket => TokenKind::RightSquareBracket,
            _ => continue,
        };
        if let Some(closing) =
            matching_delimiter(tokens, opener, opener_token.kind.clone(), closing_kind)
                .and_then(|closing| tokens.get(closing))
        {
            spans.push(Span::new(
                token.span.source_id,
                token.span.start,
                closing.span.end,
            ));
        }
    }
    spans
}

fn line_end_offset(source: &[u8], start: usize) -> usize {
    source
        .get(start..)
        .and_then(|tail| tail.iter().position(|byte| *byte == b'\n'))
        .map_or(source.len(), |offset| start.saturating_add(offset))
}

fn preceding_attribute_start(source: &[u8], declaration_start: usize) -> usize {
    let mut earliest = declaration_start.min(source.len());
    let mut cursor = earliest;
    loop {
        while cursor > 0 && source.get(cursor - 1).is_some_and(u8::is_ascii_whitespace) {
            cursor -= 1;
        }
        if cursor == 0 || source.get(cursor - 1) != Some(&b']') {
            break;
        }
        let mut depth = 0_usize;
        let mut opening = None;
        for index in (0..cursor).rev() {
            match source.get(index).copied() {
                Some(b']') => depth = depth.saturating_add(1),
                Some(b'[') => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        opening = Some(index);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(opening) = opening else {
            break;
        };
        let Some(attribute_start) = opening
            .checked_sub(1)
            .filter(|index| source.get(*index) == Some(&b'#'))
        else {
            break;
        };
        earliest = attribute_start;
        cursor = attribute_start;
    }
    earliest
}

fn function_document_detail(
    source: &[u8],
    attribute_start: usize,
    signature_start: usize,
    signature_end: usize,
) -> String {
    let signature = source_detail(source, signature_start, signature_end);
    let attributes = String::from_utf8_lossy(
        source
            .get(attribute_start..signature_start)
            .unwrap_or_default(),
    );
    const EVENT_MARKER: &str = "#[nwnrs::events(";
    let identity = attributes
        .rfind(EVENT_MARKER)
        .map(|start| &attributes[start + EVENT_MARKER.len()..])
        .and_then(|tail| tail.find(')').map(|end| tail[..end].trim()))
        .filter(|identity| !identity.is_empty());
    identity.map_or(signature.clone(), |identity| {
        format!("event: {identity} · {signature}")
    })
}

fn source_detail(source: &[u8], start: usize, end: usize) -> String {
    let text = String::from_utf8_lossy(source.get(start..end).unwrap_or_default())
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    const MAX_DETAIL_CHARS: usize = 240;
    if text.chars().count() <= MAX_DETAIL_CHARS {
        return text;
    }
    let mut truncated = text.chars().take(MAX_DETAIL_CHARS).collect::<String>();
    truncated.push('…');
    truncated
}

struct SymbolResolver {
    filesystem: nwnrs_nwscript::FileSystemScriptResolver,
    fallback:   Option<nwnrs_nwscript::SharedScriptResolver>,
}

fn build_symbol_resolver(query: &NwScriptDefinitionQuery) -> Result<SymbolResolver, String> {
    let mut roots = Vec::new();
    if let Some(root) = &query.project_root {
        push_unique_path(&mut roots, root.clone());
    }
    if let Some(parent) = query.source_path.parent() {
        push_unique_path(&mut roots, parent.to_path_buf());
    }
    for root in &query.include_directories {
        push_unique_path(&mut roots, root.clone());
    }
    let dependency_context = query.project_root.as_ref().unwrap_or(&query.source_path);
    for dependency in nwnrs_nwpkg::resolve_include_dependencies(dependency_context)? {
        push_unique_path(&mut roots, dependency.source_root);
    }
    if let Some(langspec) = &query.langspec
        && let Some(parent) = langspec.parent()
    {
        push_unique_path(&mut roots, parent.to_path_buf());
    }
    let mut filesystem = nwnrs_nwscript::FileSystemScriptResolver::new();
    for root in &roots {
        filesystem.add_root(root.clone());
    }
    for (path, contents) in &query.source_overlays {
        if roots.iter().any(|root| path_is_within(path, root)) {
            filesystem.add_overlay(path.clone(), contents.clone());
        }
    }
    Ok(SymbolResolver {
        filesystem,
        fallback: crate::compile::build_install_script_resolver(
            query.root.as_deref(),
            query.user.as_deref(),
            &query.language,
            query.load_ovr,
        )?,
    })
}

fn path_is_within(path: &Path, root: &Path) -> bool {
    let path = canonical_source_path(path);
    let root = canonical_source_path(root);
    path.starts_with(root)
}

impl ScriptResolver for SymbolResolver {
    fn resolve_script_bytes(
        &self,
        script_name: &str,
        res_type: ResType,
    ) -> Result<Option<Vec<u8>>, SourceError> {
        if let Some(source) = self
            .filesystem
            .resolve_script_bytes(script_name, res_type)?
        {
            return Ok(Some(source));
        }
        self.fallback.as_ref().map_or(Ok(None), |fallback| {
            fallback.resolve_script_bytes(script_name, res_type)
        })
    }
}

/// Immutable packed NSS source exposed to an editor from an installation
/// resource.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NwScriptVirtualSource {
    /// Stable read-only editor URI derived from the exact source contents.
    pub uri:      String,
    /// Source text presented by the editor document provider.
    pub contents: String,
}

/// One include or script resource resolved with normal compiler precedence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NwScriptResolvedSource {
    /// Editable filesystem path when a physical override exists.
    pub path:          PathBuf,
    /// Read-only URI when the source came from a packed game resource.
    pub virtual_uri:   Option<String>,
    /// Logical resource name needed to reopen packed source.
    pub resource_name: Option<String>,
}

/// Resolves an NSS include or script name exactly as the compiler would.
///
/// # Errors
///
/// Returns an error when project dependencies or game resource roots cannot
/// be resolved.
pub fn resolve_nwscript_source(
    query: &NwScriptDefinitionQuery,
    resource_name: &str,
) -> Result<Option<NwScriptResolvedSource>, String> {
    let resolver = build_symbol_resolver(query)?;
    if let Some(path) = resolver.filesystem.resolve_script_path(resource_name) {
        return Ok(Some(NwScriptResolvedSource {
            path,
            virtual_uri: None,
            resource_name: None,
        }));
    }
    let Some(bytes) = resolver
        .resolve_script_bytes(resource_name, nwnrs_nwscript::NW_SCRIPT_SOURCE_RES_TYPE)
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    Ok(Some(NwScriptResolvedSource {
        path:          virtual_source_path(resource_name),
        virtual_uri:   Some(virtual_source_uri(resource_name, &bytes)),
        resource_name: Some(resource_name.to_string()),
    }))
}

/// One source file that uniquely provides an unresolved symbol when included.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NwScriptIncludeCandidate {
    /// Include name without the `.nss` extension.
    pub include_name: String,
    /// Declaration provided by the candidate source.
    pub definition:   NwScriptSymbolDefinition,
}

/// Searches project and configured include roots for source files that define
/// a currently unresolved symbol but are not in the current include graph.
///
/// # Errors
///
/// Returns an error when project dependencies or candidate sources cannot be
/// resolved safely.
pub fn find_nwscript_include_candidates(
    query: &NwScriptDefinitionQuery,
) -> Result<Vec<NwScriptIncludeCandidate>, String> {
    NwScriptProjectIndex::new().include_candidates(query, None)
}
/// Enumerates every editable NSS source owned by a package, its configured
/// include roots, and its transitive local include dependencies.
///
/// # Errors
///
/// Returns an error when a project manifest or source root cannot be read, or
/// when the bounded editor index would exceed 20,000 source files.
pub fn list_nwscript_project_sources(
    query: &NwScriptDefinitionQuery,
) -> Result<Vec<PathBuf>, String> {
    let mut roots = Vec::new();
    if let Some(project_root) = &query.project_root {
        let source_root = nwnrs_nwpkg::read_project_manifest(project_root)?.map_or_else(
            || project_root.clone(),
            |manifest| project_root.join(manifest.source.path),
        );
        push_unique_path(&mut roots, source_root);
    }
    for root in &query.include_directories {
        push_unique_path(&mut roots, root.clone());
    }
    let dependency_context = query.project_root.as_ref().unwrap_or(&query.source_path);
    for dependency in nwnrs_nwpkg::resolve_include_dependencies(dependency_context)? {
        push_unique_path(&mut roots, dependency.source_root);
    }
    let mut files = Vec::new();
    for root in roots {
        collect_nss_files(&root, &mut files)?;
    }
    files = files
        .into_iter()
        .map(|file| file.canonicalize().unwrap_or(file))
        .collect();
    files.sort();
    files.dedup();
    const MAX_EDITOR_INDEX_FILES: usize = 20_000;
    if files.len() > MAX_EDITOR_INDEX_FILES {
        return Err(format!(
            "NWScript project index found {} NSS files; maximum is {MAX_EDITOR_INDEX_FILES}",
            files.len()
        ));
    }
    Ok(files)
}

fn collect_nss_files(root: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    if !root.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(root).map_err(|error| {
        format!(
            "failed to read include search root {}: {error}",
            root.display()
        )
    })? {
        let entry = entry.map_err(|error| {
            format!(
                "failed to read entry in include search root {}: {error}",
                root.display()
            )
        })?;
        let path = entry.path();
        if path.is_dir() {
            if !matches!(
                path.file_name().and_then(|name| name.to_str()),
                Some(".git" | "node_modules" | "target")
            ) {
                collect_nss_files(&path, files)?;
            }
        } else if path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("nss"))
        {
            files.push(path);
        }
    }
    Ok(())
}

struct LoadedBuiltinSource {
    bytes:         Vec<u8>,
    physical_path: Option<PathBuf>,
    virtual_uri:   String,
    resource_name: String,
}

/// Resolves the effective implicit `nwscript.nss` using normal compiler
/// precedence.
///
/// Workspace files and explicit compiler langspec paths return `None` because
/// VS Code can open them normally. Packed installation resources return a
/// read-only virtual document.
///
/// # Errors
///
/// Returns an error when source resolution fails.
pub fn load_nwscript_builtin_source(
    query: &NwScriptDefinitionQuery,
) -> Result<Option<NwScriptVirtualSource>, String> {
    let resolver = build_symbol_resolver(query)?;
    let Some(source) = load_builtin_source(&resolver, query)? else {
        return Ok(None);
    };
    if source.physical_path.is_some() {
        return Ok(None);
    }
    Ok(Some(NwScriptVirtualSource {
        uri:      source.virtual_uri,
        contents: String::from_utf8_lossy(&source.bytes).into_owned(),
    }))
}

/// Resolves one effective NSS resource and returns its immutable virtual
/// contents only when resolution fell through to a packed installation asset.
/// Workspace files and unsaved overlays take precedence and return `None`.
///
/// # Errors
///
/// Returns an error when project or installation resource resolution fails.
pub fn load_nwscript_virtual_source(
    query: &NwScriptDefinitionQuery,
    resource_name: &str,
) -> Result<Option<NwScriptVirtualSource>, String> {
    let resolver = build_symbol_resolver(query)?;
    if resolver
        .filesystem
        .resolve_script_path(resource_name)
        .is_some()
    {
        return Ok(None);
    }
    let Some(bytes) = resolver
        .resolve_script_bytes(resource_name, nwnrs_nwscript::NW_SCRIPT_SOURCE_RES_TYPE)
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    Ok(Some(NwScriptVirtualSource {
        uri:      virtual_source_uri(resource_name, &bytes),
        contents: String::from_utf8_lossy(&bytes).into_owned(),
    }))
}

fn load_builtin_source(
    resolver: &SymbolResolver,
    query: &NwScriptDefinitionQuery,
) -> Result<Option<LoadedBuiltinSource>, String> {
    let script_name = query.langspec.as_ref().map_or_else(
        || nwnrs_nwscript::DEFAULT_LANGSPEC_SCRIPT_NAME.to_string(),
        |path| path.to_string_lossy().into_owned(),
    );
    let physical_path = resolver.filesystem.resolve_script_path(&script_name);
    let Some(bytes) = resolver
        .resolve_script_bytes(&script_name, nwnrs_nwscript::NW_SCRIPT_SOURCE_RES_TYPE)
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    Ok(Some(LoadedBuiltinSource {
        virtual_uri: virtual_source_uri(&script_name, &bytes),
        bytes,
        physical_path,
        resource_name: script_name,
    }))
}

fn virtual_source_uri(resource_name: &str, source: &[u8]) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in resource_name.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash ^= 0xff;
    hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    for byte in source {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    let stem = virtual_source_path(resource_name)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("script")
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '_' | '-') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    format!("nwnrs-game:/{hash:016x}/{stem}.nss")
}

fn virtual_source_path(resource_name: &str) -> PathBuf {
    let stem = Path::new(resource_name)
        .file_stem()
        .unwrap_or_else(|| std::ffi::OsStr::new("script"));
    PathBuf::from(stem).with_extension("nss")
}

fn scan_builtin_definitions(
    resolver: &SymbolResolver,
    query: &NwScriptDefinitionQuery,
) -> Result<Vec<NwScriptSymbolDefinition>, String> {
    let Some(source) = load_builtin_source(resolver, query)? else {
        return Ok(Vec::new());
    };
    let path = source
        .physical_path
        .clone()
        .unwrap_or_else(|| PathBuf::from("nwscript.nss"));
    let source_id = SourceId::new(0);
    let source_file = SourceFile::new(source_id, path.to_string_lossy(), source.bytes.clone());
    let tokens = Lexer::new(source_id, &source.bytes)
        .lex_all()
        .map_err(|error| format!("failed to index implicit nwscript.nss: {error}"))?;
    let virtual_uri = source.physical_path.is_none().then_some(source.virtual_uri);
    let mut definitions = Vec::new();

    for mut definition in
        scan_function_definitions(&path, &source.bytes, &source_file, &tokens, &query.symbol)
    {
        definition.kind = NwScriptSymbolKind::BuiltinFunction;
        definition.documentation = line_start_offset(&source.bytes, definition.start_line)
            .and_then(|start| preceding_slash_documentation(&source.bytes, start));
        definition.virtual_uri.clone_from(&virtual_uri);
        definition.resource_name = virtual_uri.as_ref().map(|_| source.resource_name.clone());
        definitions.push(definition);
    }

    if let Some(mut definition) =
        scan_builtin_constant(&path, &source.bytes, &source_file, &tokens, &query.symbol)
    {
        definition.virtual_uri.clone_from(&virtual_uri);
        definition.resource_name = virtual_uri.as_ref().map(|_| source.resource_name.clone());
        definitions.push(definition);
    }

    if let Some(mut definition) =
        scan_engine_structure(&path, &source.bytes, &source_file, &tokens, &query.symbol)
    {
        definition.virtual_uri = virtual_uri;
        definition.resource_name = definition
            .virtual_uri
            .as_ref()
            .map(|_| source.resource_name);
        definitions.push(definition);
    }
    Ok(definitions)
}

fn scan_builtin_constant(
    path: &Path,
    source: &[u8],
    source_file: &SourceFile,
    tokens: &[Token],
    requested: &str,
) -> Option<NwScriptSymbolDefinition> {
    let (index, name) = tokens.iter().enumerate().find(|(index, token)| {
        token.kind == TokenKind::Identifier
            && token.text == requested
            && tokens
                .get(index + 1)
                .is_some_and(|next| next.kind == TokenKind::Assign)
    })?;
    let declaration_start = tokens.get(index.checked_sub(1)?)?.span.start;
    let declaration_end = tokens
        .get(index + 1..)?
        .iter()
        .find(|token| token.kind == TokenKind::Semicolon)?
        .span
        .end;
    let mut definition = make_definition(
        path,
        source,
        source_file,
        name,
        NwScriptSymbolKind::BuiltinConstant,
        declaration_start,
        declaration_end,
        false,
    )?;
    definition.documentation = None;
    Some(definition)
}

fn scan_engine_structure(
    path: &Path,
    source: &[u8],
    source_file: &SourceFile,
    tokens: &[Token],
    requested: &str,
) -> Option<NwScriptSymbolDefinition> {
    let name = tokens.iter().find(|token| {
        if token.kind != TokenKind::Identifier || token.text != requested {
            return false;
        }
        let start = line_start_offset_for_byte(source, token.span.start);
        let end = source
            .get(start..)
            .and_then(|tail| tail.iter().position(|byte| *byte == b'\n'))
            .map_or(source.len(), |offset| start.saturating_add(offset));
        String::from_utf8_lossy(source.get(start..end).unwrap_or_default())
            .trim_start()
            .starts_with("#define ENGINE_STRUCTURE_")
    })?;
    let declaration_start = line_start_offset_for_byte(source, name.span.start);
    let declaration_end = source
        .get(name.span.end..)
        .and_then(|tail| tail.iter().position(|byte| *byte == b'\n'))
        .map_or(source.len(), |offset| name.span.end.saturating_add(offset));
    let mut definition = make_definition(
        path,
        source,
        source_file,
        name,
        NwScriptSymbolKind::EngineStructure,
        declaration_start,
        declaration_end,
        false,
    )?;
    definition.documentation = None;
    Some(definition)
}

fn line_start_offset(source: &[u8], line: usize) -> Option<usize> {
    if line == 0 {
        return None;
    }
    let mut current = 1_usize;
    let mut offset = 0_usize;
    while current < line {
        let next = source
            .get(offset..)?
            .iter()
            .position(|byte| *byte == b'\n')?;
        offset = offset.saturating_add(next).saturating_add(1);
        current += 1;
    }
    Some(offset)
}

fn line_start_offset_for_byte(source: &[u8], offset: usize) -> usize {
    source
        .get(..offset.min(source.len()))
        .and_then(|prefix| prefix.iter().rposition(|byte| *byte == b'\n'))
        .map_or(0, |newline| newline.saturating_add(1))
}

fn preceding_slash_documentation(source: &[u8], declaration_start: usize) -> Option<String> {
    let prefix = String::from_utf8_lossy(source.get(..declaration_start)?);
    let mut lines = Vec::new();
    for line in prefix.lines().rev() {
        let trimmed = line.trim();
        let Some(documentation) = trimmed.strip_prefix("//") else {
            break;
        };
        let documentation = documentation.strip_prefix('/').unwrap_or(documentation);
        let documentation = documentation.strip_prefix(' ').unwrap_or(documentation);
        lines.push(documentation.trim_end().to_string());
    }
    lines.reverse();
    (!lines.is_empty()).then(|| lines.join("\n"))
}

fn definition_from_compiler_span(
    bundle: &SourceBundle,
    resolver: &SymbolResolver,
    span: Span,
    name: &str,
    kind: NwScriptSymbolKind,
    is_implementation: bool,
) -> Option<NwScriptSymbolDefinition> {
    let source_file = bundle.source_map.get(span.source_id)?;
    let physical_path = resolver.filesystem.resolve_script_path(&source_file.name);
    let path = physical_path
        .clone()
        .unwrap_or_else(|| virtual_source_path(&source_file.name));
    let tokens = Lexer::new(span.source_id, source_file.bytes())
        .lex_all()
        .ok()?;
    let (name_index, token) = tokens.iter().enumerate().find(|(_, token)| {
        token.kind == TokenKind::Identifier
            && token.text == name
            && token.span.start >= span.start
            && token.span.end <= span.end
    })?;
    let declaration_end = match kind {
        NwScriptSymbolKind::Function => tokens
            .get(name_index + 1..)
            .and_then(|tail| {
                tail.iter()
                    .position(|token| token.kind == TokenKind::LeftParen)
            })
            .map(|offset| name_index + 1 + offset)
            .and_then(|left_paren| matching_right_paren(&tokens, left_paren))
            .and_then(|right_paren| tokens.get(right_paren))
            .map_or(span.end, |token| token.span.end),
        NwScriptSymbolKind::Struct | NwScriptSymbolKind::Enum => tokens
            .get(name_index + 1..)
            .and_then(|tail| tail.iter().find(|token| token.kind == TokenKind::LeftBrace))
            .map_or(span.end, |token| token.span.start),
        _ => span.end,
    };
    let mut definition = make_definition(
        &path,
        source_file.bytes(),
        source_file,
        token,
        kind,
        span.start,
        declaration_end,
        is_implementation,
    )?;
    if physical_path.is_none() {
        definition.documentation = preceding_slash_documentation(source_file.bytes(), span.start)
            .or(definition.documentation);
        definition.virtual_uri = Some(virtual_source_uri(&source_file.name, source_file.bytes()));
        definition.resource_name = Some(source_file.name.clone());
    }
    Some(definition)
}

fn enum_alias_target(
    script: &nwnrs_nwscript::Script,
    alias_name: &str,
) -> Option<(String, String)> {
    script.items.iter().find_map(|item| {
        let TopLevelItem::Enum(enumeration) = item else {
            return None;
        };
        enumeration.variants.iter().find_map(|variant| {
            variant
                .aliases
                .iter()
                .any(|alias| alias.name == alias_name)
                .then(|| (enumeration.name.clone(), variant.name.clone()))
        })
    })
}

fn paths_refer_to_same_source(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left
            .to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy()),
    }
}

fn push_unique_path(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !path.as_os_str().is_empty() && !paths.contains(&path) {
        paths.push(path);
    }
}

fn scan_source_definitions(
    path: &Path,
    source: &[u8],
    requested: &str,
    qualifier: Option<&str>,
) -> Vec<NwScriptSymbolDefinition> {
    let source_id = SourceId::new(0);
    let Ok(tokens) = Lexer::new(source_id, source).lex_all() else {
        return Vec::new();
    };
    let source_file = SourceFile::new(source_id, path.to_string_lossy(), source);
    let mut definitions = if qualifier.is_none() {
        scan_function_definitions(path, source, &source_file, &tokens, requested)
    } else {
        Vec::new()
    };
    definitions.extend(scan_macro_definitions(
        path,
        source,
        &source_file,
        &tokens,
        requested,
        qualifier,
    ));
    definitions.extend(scan_extended_type_definitions(
        path,
        source,
        &source_file,
        &tokens,
        requested,
        qualifier,
    ));
    definitions
}

fn scan_function_definitions(
    path: &Path,
    source: &[u8],
    source_file: &SourceFile,
    tokens: &[Token],
    requested: &str,
) -> Vec<NwScriptSymbolDefinition> {
    let mut definitions = Vec::new();
    let mut brace_depth = 0_usize;
    for (index, token) in tokens.iter().enumerate() {
        match token.kind {
            TokenKind::LeftBrace => brace_depth = brace_depth.saturating_add(1),
            TokenKind::RightBrace => brace_depth = brace_depth.saturating_sub(1),
            TokenKind::LeftParen if brace_depth == 0 => {
                let Some(name_index) = index.checked_sub(1) else {
                    continue;
                };
                let Some(name) = tokens.get(name_index) else {
                    continue;
                };
                if name.kind != TokenKind::Identifier || name.text != requested {
                    continue;
                }
                let Some(declaration_start) = function_declaration_start(tokens, name_index) else {
                    continue;
                };
                let Some(right_paren) = matching_right_paren(tokens, index) else {
                    continue;
                };
                let Some(after) = tokens.get(right_paren.saturating_add(1)) else {
                    continue;
                };
                let is_implementation = after.kind == TokenKind::LeftBrace;
                if !is_implementation && after.kind != TokenKind::Semicolon {
                    continue;
                }
                if let Some(definition) = make_definition(
                    path,
                    source,
                    source_file,
                    name,
                    NwScriptSymbolKind::Function,
                    tokens
                        .get(declaration_start)
                        .map_or(name.span.start, |start| start.span.start),
                    tokens
                        .get(right_paren)
                        .map_or(name.span.end, |end| end.span.end),
                    is_implementation,
                ) {
                    definitions.push(definition);
                }
            }
            _ => {}
        }
    }
    definitions
}

fn function_declaration_start(tokens: &[Token], name_index: usize) -> Option<usize> {
    let type_index = name_index.checked_sub(1)?;
    let return_type = tokens.get(type_index)?;
    if is_simple_return_type(return_type) {
        return Some(type_index);
    }
    if return_type.kind == TokenKind::Identifier {
        if let Some(struct_index) = type_index.checked_sub(1)
            && matches!(
                tokens.get(struct_index).map(|token| &token.kind),
                Some(TokenKind::Keyword(Keyword::Struct))
            )
        {
            return Some(struct_index);
        }
        return Some(type_index);
    }
    None
}

fn is_simple_return_type(token: &Token) -> bool {
    matches!(
        token.kind,
        TokenKind::Keyword(
            Keyword::Void
                | Keyword::Int
                | Keyword::Float
                | Keyword::String
                | Keyword::Object
                | Keyword::Vector
                | Keyword::Action
        )
    )
}

fn matching_right_paren(tokens: &[Token], left_index: usize) -> Option<usize> {
    let mut depth = 0_usize;
    for (index, token) in tokens.iter().enumerate().skip(left_index) {
        match token.kind {
            TokenKind::LeftParen => depth = depth.saturating_add(1),
            TokenKind::RightParen => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            TokenKind::Eof => return None,
            _ => {}
        }
    }
    None
}

fn matching_delimiter(
    tokens: &[Token],
    left_index: usize,
    left: TokenKind,
    right: TokenKind,
) -> Option<usize> {
    let mut depth = 0_usize;
    for (index, token) in tokens.iter().enumerate().skip(left_index) {
        if token.kind == left {
            depth = depth.saturating_add(1);
        } else if token.kind == right {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(index);
            }
        } else if token.kind == TokenKind::Eof {
            return None;
        }
    }
    None
}

fn scan_extended_type_definitions(
    path: &Path,
    source: &[u8],
    source_file: &SourceFile,
    tokens: &[Token],
    requested: &str,
    qualifier: Option<&str>,
) -> Vec<NwScriptSymbolDefinition> {
    let mut definitions = Vec::new();
    let mut brace_depth = 0_usize;
    let mut index = 0_usize;
    while index < tokens.len() {
        let Some(token) = tokens.get(index) else {
            break;
        };
        if token.kind == TokenKind::Identifier && token.text == "enum" && brace_depth == 0 {
            let Some(name) = tokens.get(index + 1) else {
                break;
            };
            let Some(left_brace) = (index + 2..tokens.len()).find(|candidate| {
                tokens
                    .get(*candidate)
                    .is_some_and(|token| token.kind == TokenKind::LeftBrace)
            }) else {
                index += 1;
                continue;
            };
            let Some(right_brace) = matching_delimiter(
                tokens,
                left_brace,
                TokenKind::LeftBrace,
                TokenKind::RightBrace,
            ) else {
                index += 1;
                continue;
            };
            if qualifier.is_none()
                && name.kind == TokenKind::Identifier
                && name.text == requested
                && let Some(left_brace_token) = tokens.get(left_brace)
                && let Some(definition) = make_definition(
                    path,
                    source,
                    source_file,
                    name,
                    NwScriptSymbolKind::Enum,
                    token.span.start,
                    left_brace_token.span.start,
                    true,
                )
            {
                definitions.push(definition);
            }
            scan_enum_variants(
                path,
                source,
                source_file,
                tokens,
                left_brace + 1,
                right_brace,
                &name.text,
                requested,
                qualifier,
                &mut definitions,
            );
            index = right_brace + 1;
            continue;
        }
        if qualifier.is_none()
            && token.kind == TokenKind::Identifier
            && token.text == "type"
            && brace_depth == 0
            && let Some(name) = tokens.get(index + 1)
            && name.kind == TokenKind::Identifier
            && name.text == requested
        {
            let end = tokens
                .get(index + 2..)
                .unwrap_or_default()
                .iter()
                .find(|candidate| candidate.kind == TokenKind::Semicolon)
                .map_or(name.span.end, |semicolon| semicolon.span.start);
            if let Some(definition) = make_definition(
                path,
                source,
                source_file,
                name,
                NwScriptSymbolKind::TypeAlias,
                token.span.start,
                end,
                true,
            ) {
                definitions.push(definition);
            }
        }
        match token.kind {
            TokenKind::LeftBrace => brace_depth = brace_depth.saturating_add(1),
            TokenKind::RightBrace => brace_depth = brace_depth.saturating_sub(1),
            _ => {}
        }
        index += 1;
    }
    definitions
}

#[allow(clippy::too_many_arguments)]
fn scan_enum_variants(
    path: &Path,
    source: &[u8],
    source_file: &SourceFile,
    tokens: &[Token],
    start: usize,
    end: usize,
    enum_name: &str,
    requested: &str,
    qualifier: Option<&str>,
    definitions: &mut Vec<NwScriptSymbolDefinition>,
) {
    let mut segment_start = start;
    let mut index = start;
    let mut paren_depth = 0_usize;
    let mut square_depth = 0_usize;
    while index <= end {
        let at_end = index == end;
        let kind = (!at_end)
            .then(|| tokens.get(index).map(|token| &token.kind))
            .flatten();
        let at_separator = at_end
            || (matches!(kind, Some(TokenKind::Comma)) && paren_depth == 0 && square_depth == 0);
        if at_separator {
            scan_enum_variant_segment(
                path,
                source,
                source_file,
                tokens,
                segment_start,
                index,
                enum_name,
                requested,
                qualifier,
                definitions,
            );
            segment_start = index.saturating_add(1);
        } else {
            match kind {
                Some(TokenKind::LeftParen) => paren_depth = paren_depth.saturating_add(1),
                Some(TokenKind::RightParen) => paren_depth = paren_depth.saturating_sub(1),
                Some(TokenKind::LeftSquareBracket) => {
                    square_depth = square_depth.saturating_add(1);
                }
                Some(TokenKind::RightSquareBracket) => {
                    square_depth = square_depth.saturating_sub(1);
                }
                _ => {}
            }
        }
        index += 1;
    }
}

#[allow(clippy::too_many_arguments)]
fn scan_enum_variant_segment(
    path: &Path,
    source: &[u8],
    source_file: &SourceFile,
    tokens: &[Token],
    start: usize,
    end: usize,
    enum_name: &str,
    requested: &str,
    qualifier: Option<&str>,
    definitions: &mut Vec<NwScriptSymbolDefinition>,
) {
    if start >= end {
        return;
    }
    let mut cursor = start;
    let mut aliases = Vec::new();
    while cursor < end
        && tokens
            .get(cursor)
            .is_some_and(|token| token.kind == TokenKind::Hash)
    {
        let Some(left_square) = tokens
            .get(cursor + 1)
            .filter(|token| token.kind == TokenKind::LeftSquareBracket)
        else {
            return;
        };
        let Some(right_square) = matching_delimiter(
            tokens,
            cursor + 1,
            TokenKind::LeftSquareBracket,
            TokenKind::RightSquareBracket,
        ) else {
            return;
        };
        if tokens
            .get(cursor + 2)
            .is_some_and(|token| token.kind == TokenKind::Identifier && token.text == "alias")
            && let Some(alias) = tokens
                .get(cursor + 3..right_square)
                .unwrap_or_default()
                .iter()
                .find(|token| token.kind == TokenKind::Identifier)
        {
            aliases.push(alias);
        }
        let _ = left_square;
        cursor = right_square + 1;
    }
    let Some(variant) = tokens
        .get(cursor)
        .filter(|token| token.kind == TokenKind::Identifier)
    else {
        return;
    };
    let Some(declaration_start) = tokens.get(start).map(|token| token.span.start) else {
        return;
    };
    let Some(declaration_end) = tokens
        .get(end.saturating_sub(1))
        .map(|token| token.span.end)
    else {
        return;
    };
    if variant.text == requested
        && qualifier.is_none_or(|qualifier| qualifier == enum_name)
        && let Some(mut definition) = make_definition(
            path,
            source,
            source_file,
            variant,
            NwScriptSymbolKind::EnumVariant,
            declaration_start,
            declaration_end,
            true,
        )
    {
        let variant_signature = String::from_utf8_lossy(
            source
                .get(variant.span.start..declaration_end)
                .unwrap_or_default(),
        )
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
        definition.signature = format!("{enum_name}::{variant_signature}");
        definitions.push(definition);
    }
    for alias in aliases {
        if qualifier.is_some() {
            continue;
        }
        if alias.text != requested {
            continue;
        }
        if let Some(mut definition) = make_definition(
            path,
            source,
            source_file,
            alias,
            NwScriptSymbolKind::Constant,
            declaration_start,
            declaration_end,
            true,
        ) {
            definition.signature =
                format!("{enum_name} {} = {enum_name}::{}", alias.text, variant.text);
            definitions.push(definition);
        }
    }
}

fn scan_macro_definitions(
    path: &Path,
    source: &[u8],
    source_file: &SourceFile,
    tokens: &[Token],
    requested: &str,
    qualifier: Option<&str>,
) -> Vec<NwScriptSymbolDefinition> {
    let mut definitions = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        let (name, namespace) = if matches!(token.kind, TokenKind::Keyword(Keyword::Define)) {
            (tokens.get(index.saturating_add(1)), None)
        } else if token.kind == TokenKind::Identifier
            && token.text == "macro_rules"
            && matches!(
                tokens.get(index.saturating_add(1)).map(|next| &next.kind),
                Some(TokenKind::BooleanNot)
            )
        {
            (tokens.get(index.saturating_add(2)), None)
        } else if token.kind == TokenKind::Identifier
            && token.text == "proc_macro"
            && matches!(
                tokens.get(index.saturating_add(1)).map(|next| &next.kind),
                Some(TokenKind::BooleanNot)
            )
        {
            let mut path = Vec::new();
            let mut cursor = index.saturating_add(2);
            while let Some(segment) = tokens
                .get(cursor)
                .filter(|segment| segment.kind == TokenKind::Identifier)
            {
                path.push(segment);
                if matches!(
                    (tokens.get(cursor + 1), tokens.get(cursor + 2)),
                    (
                        Some(first),
                        Some(second)
                    ) if first.kind == TokenKind::Colon && second.kind == TokenKind::Colon
                ) {
                    cursor += 3;
                } else {
                    break;
                }
            }
            let name = path.last().copied();
            let namespace = (path.len() > 1).then(|| {
                path.get(..path.len().saturating_sub(1))
                    .unwrap_or_default()
                    .iter()
                    .map(|segment| segment.text.as_str())
                    .collect::<Vec<_>>()
                    .join("::")
            });
            (name, namespace)
        } else {
            (None, None)
        };
        let Some(name) = name else {
            continue;
        };
        if name.kind != TokenKind::Identifier
            || name.text != requested
            || qualifier.is_some_and(|qualifier| namespace.as_deref() != Some(qualifier))
            || (qualifier.is_some() && namespace.is_none())
        {
            continue;
        }
        let line_end = source
            .get(token.span.start..)
            .and_then(|tail| tail.iter().position(|byte| *byte == b'\n'))
            .map_or(source.len(), |length| {
                token.span.start.saturating_add(length)
            });
        if let Some(definition) = make_definition(
            path,
            source,
            source_file,
            name,
            NwScriptSymbolKind::Macro,
            token.span.start,
            line_end,
            true,
        ) {
            definitions.push(definition);
        }
    }
    definitions
}

#[allow(clippy::too_many_arguments)]
fn make_definition(
    path: &Path,
    source: &[u8],
    source_file: &SourceFile,
    name: &Token,
    kind: NwScriptSymbolKind,
    declaration_start: usize,
    declaration_end: usize,
    is_implementation: bool,
) -> Option<NwScriptSymbolDefinition> {
    let start = source_file.location(name.span.start)?;
    let end = source_file.location(name.span.end)?;
    let signature = String::from_utf8_lossy(source.get(declaration_start..declaration_end)?)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    Some(NwScriptSymbolDefinition {
        name: name.text.clone(),
        kind,
        path: path.to_path_buf(),
        start_line: start.line,
        start_column: start.column,
        end_line: end.line,
        end_column: end.column,
        signature,
        documentation: preceding_documentation(source, declaration_start),
        is_implementation,
        virtual_uri: None,
        resource_name: None,
    })
}

fn preceding_documentation(source: &[u8], declaration_start: usize) -> Option<String> {
    let prefix = String::from_utf8_lossy(source.get(..declaration_start)?);
    let mut lines = Vec::new();
    for line in prefix.lines().rev() {
        let trimmed = line.trim();
        if let Some(documentation) = trimmed.strip_prefix("///") {
            lines.push(documentation.trim().to_string());
        } else if trimmed.is_empty() && lines.is_empty() {
            continue;
        } else {
            break;
        }
    }
    lines.reverse();
    (!lines.is_empty()).then(|| lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{BTreeMap, HashSet},
        fs,
        path::PathBuf,
        sync::Arc,
    };

    use nwnrs_nwscript::{
        FileSystemScriptResolver, InMemoryScriptResolver, Lexer, SourceId, SourceMap, TokenKind,
        parse_langspec_bytes,
    };

    use super::{
        NwScriptDefinitionQuery, NwScriptDocumentSymbolKind, NwScriptProjectIndex,
        NwScriptSemanticTokenKind, NwScriptSymbolKind, SymbolResolver, analyze_nwscript_document,
        compiler_analysis_for_bundle, definition_from_compiler_span, find_nwscript_definitions,
        find_nwscript_outgoing_calls, find_nwscript_references, function_declaration_start,
        list_nwscript_document_symbols, macro_occurrence, preceding_slash_documentation,
        scan_builtin_definitions,
    };

    fn installed_vanilla_source() -> Option<Vec<u8>> {
        let resolver = crate::compile::build_install_script_resolver(None, None, "english", false)
            .ok()
            .flatten()?;
        resolver
            .resolve_script_bytes(
                nwnrs_nwscript::DEFAULT_LANGSPEC_SCRIPT_NAME,
                nwnrs_nwscript::NW_SCRIPT_SOURCE_RES_TYPE,
            )
            .ok()
            .flatten()
    }

    #[test]
    fn finds_function_implementations_declarations_docs_and_macros() {
        let root = std::env::temp_dir().join(format!("nwnrs-symbols-{}", std::process::id()));
        fs::create_dir_all(&root).expect("create symbol fixture");
        let source = root.join("symbols.nss");
        fs::write(
            &source,
            r#"/// Logs one message.
void NWNRS_Log(string message);

void NWNRS_Log(string message)
{
}

#define NWNRS_ENABLED 1
"#,
        )
        .expect("write symbol fixture");

        let definitions = find_nwscript_definitions(&NwScriptDefinitionQuery {
            source_path: source.clone(),
            symbol: "NWNRS_Log".to_string(),
            qualifier: None,
            project_root: Some(root.clone()),
            include_directories: Vec::new(),
            source_overlays: BTreeMap::new(),
            ..NwScriptDefinitionQuery::default()
        })
        .expect("find function definitions");
        assert_eq!(definitions.len(), 2);
        assert!(
            definitions
                .first()
                .is_some_and(|item| item.is_implementation)
        );
        assert_eq!(
            definitions
                .get(1)
                .and_then(|item| item.documentation.as_deref()),
            Some("Logs one message.")
        );
        assert!(
            definitions
                .iter()
                .all(|item| item.kind == NwScriptSymbolKind::Function)
        );

        let macros = find_nwscript_definitions(&NwScriptDefinitionQuery {
            source_path: source,
            symbol: "NWNRS_ENABLED".to_string(),
            qualifier: None,
            project_root: Some(root.clone()),
            include_directories: Vec::new(),
            source_overlays: BTreeMap::new(),
            ..NwScriptDefinitionQuery::default()
        })
        .expect("find macro definitions");
        let _ = fs::remove_dir_all(&root);
        assert_eq!(macros.len(), 1);
        assert_eq!(
            macros.first().map(|item| item.kind),
            Some(NwScriptSymbolKind::Macro)
        );
    }

    #[test]
    fn empty_symbol_has_no_definitions() {
        let definitions = find_nwscript_definitions(&NwScriptDefinitionQuery {
            source_path: PathBuf::from("missing.nss"),
            symbol: String::new(),
            qualifier: None,
            project_root: None,
            include_directories: Vec::new(),
            source_overlays: BTreeMap::new(),
            ..NwScriptDefinitionQuery::default()
        })
        .expect("empty query succeeds");
        assert!(definitions.is_empty());
    }

    #[test]
    fn finds_struct_fields_globals_parameters_and_locals() {
        let root =
            std::env::temp_dir().join(format!("nwnrs-complete-symbols-{}", std::process::id()));
        fs::create_dir_all(&root).expect("create complete symbol fixture");
        let source = root.join("complete.nss");
        fs::write(
            &source,
            "struct Stats { int score; };\nconst int Limit = 3;\nint Counter;\nvoid Handle(int \
             value) { int localValue = value; }\n",
        )
        .expect("write complete symbol fixture");

        let find = |name: &str| {
            find_nwscript_definitions(&NwScriptDefinitionQuery {
                source_path: source.clone(),
                symbol: name.to_string(),
                project_root: Some(root.clone()),
                ..NwScriptDefinitionQuery::default()
            })
            .expect("find complete symbol")
        };
        let find_kind = |name| {
            find(name)
                .into_iter()
                .next()
                .unwrap_or_else(|| panic!("missing {name} definition"))
                .kind
        };
        assert_eq!(find_kind("Stats"), NwScriptSymbolKind::Struct);
        assert_eq!(find_kind("score"), NwScriptSymbolKind::Field);
        assert_eq!(find_kind("Limit"), NwScriptSymbolKind::Constant);
        assert_eq!(find_kind("Counter"), NwScriptSymbolKind::Variable);
        assert_eq!(find_kind("value"), NwScriptSymbolKind::Parameter);
        assert_eq!(find_kind("localValue"), NwScriptSymbolKind::Variable);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn finds_enum_variants_compatibility_aliases_and_type_aliases() {
        let root =
            std::env::temp_dir().join(format!("nwnrs-extended-symbols-{}", std::process::id()));
        fs::create_dir_all(&root).expect("create extended symbol fixture");
        let source = root.join("extended.nss");
        fs::write(
            &source,
            r#"/// Controls structured logging verbosity.
enum LogLevel : int {
    /// Emits informational messages.
    #[default]
    #[alias(NWNRS_LOG_LEVEL_INFO)]
    Info = 2,
    Debug,
}

/// A shorter spelling for LogLevel.
type Level = LogLevel;
"#,
        )
        .expect("write extended symbol fixture");

        let find = |symbol: &str| {
            find_nwscript_definitions(&NwScriptDefinitionQuery {
                source_path: source.clone(),
                symbol: symbol.to_string(),
                qualifier: None,
                project_root: Some(root.clone()),
                include_directories: Vec::new(),
                source_overlays: BTreeMap::new(),
                ..NwScriptDefinitionQuery::default()
            })
            .unwrap_or_else(|error| panic!("find {symbol}: {error}"))
        };
        let enum_type = find("LogLevel");
        assert_eq!(
            enum_type.first().map(|item| item.kind),
            Some(NwScriptSymbolKind::Enum)
        );
        assert_eq!(
            enum_type
                .first()
                .and_then(|item| item.documentation.as_deref()),
            Some("Controls structured logging verbosity.")
        );
        let variant = find("Info");
        assert_eq!(
            variant.first().map(|item| item.kind),
            Some(NwScriptSymbolKind::EnumVariant)
        );
        assert_eq!(
            variant
                .first()
                .and_then(|item| item.documentation.as_deref()),
            Some("Emits informational messages.")
        );
        assert!(
            variant
                .first()
                .is_some_and(|item| item.signature.starts_with("LogLevel::Info = 2"))
        );
        let alias = find("NWNRS_LOG_LEVEL_INFO");
        assert_eq!(
            alias.first().map(|item| item.kind),
            Some(NwScriptSymbolKind::Constant)
        );
        assert_eq!(
            alias.first().map(|item| item.signature.as_str()),
            Some("LogLevel NWNRS_LOG_LEVEL_INFO = LogLevel::Info")
        );
        assert_eq!(
            find("Level").first().map(|item| item.kind),
            Some(NwScriptSymbolKind::TypeAlias)
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn searches_only_the_transitive_include_graph() {
        let root = std::env::temp_dir().join(format!("nwnrs-symbol-graph-{}", std::process::id()));
        fs::create_dir_all(&root).expect("create graph fixture");
        let source = root.join("main.nss");
        fs::write(
            &source,
            "#include \"reachable\"\nvoid main() { Shared(); }\n",
        )
        .expect("write root source");
        fs::write(root.join("reachable.nss"), "void Shared() {}\n")
            .expect("write reachable include");
        fs::write(root.join("unrelated.nss"), "void Shared() {}\n")
            .expect("write unrelated source");

        let definitions = find_nwscript_definitions(&NwScriptDefinitionQuery {
            source_path: source,
            symbol: "Shared".to_string(),
            qualifier: None,
            project_root: Some(root.clone()),
            include_directories: Vec::new(),
            source_overlays: BTreeMap::new(),
            ..NwScriptDefinitionQuery::default()
        })
        .expect("find reachable definition");

        assert_eq!(definitions.len(), 1);
        assert_eq!(
            definitions.first().and_then(|item| item.path.file_name()),
            Some(std::ffi::OsStr::new("reachable.nss"))
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn qualified_enum_variants_resolve_to_the_named_enum_only() {
        let root = std::env::temp_dir().join(format!("nwnrs-symbol-enum-{}", std::process::id()));
        fs::create_dir_all(&root).expect("create enum fixture");
        let source = root.join("main.nss");
        fs::write(
            &source,
            "enum First { Ready }\nenum Second { Ready }\nvoid main() {}\n",
        )
        .expect("write enum source");

        let definitions = find_nwscript_definitions(&NwScriptDefinitionQuery {
            source_path: source,
            symbol: "Ready".to_string(),
            qualifier: Some("Second".to_string()),
            project_root: Some(root.clone()),
            include_directories: Vec::new(),
            source_overlays: BTreeMap::new(),
            ..NwScriptDefinitionQuery::default()
        })
        .expect("find qualified variant");

        assert_eq!(definitions.len(), 1);
        assert!(
            definitions
                .first()
                .is_some_and(|definition| definition.signature.starts_with("Second::Ready"))
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn namespaced_procedural_macros_resolve_by_final_path_segment() {
        let root = std::env::temp_dir().join(format!("nwnrs-symbol-proc-{}", std::process::id()));
        fs::create_dir_all(&root).expect("create procedural macro fixture");
        let source = root.join("main.nss");
        fs::write(
            &source,
            "proc_macro! project::events::__build_event_dispatcher {\n".to_string()
                + "    tokenstream __build_event_dispatcher(tokenstream input) { return input; }\n"
                + "}\n",
        )
        .expect("write procedural macro source");

        let definitions = find_nwscript_definitions(&NwScriptDefinitionQuery {
            source_path: source,
            symbol: "__build_event_dispatcher".to_string(),
            qualifier: Some("project::events".to_string()),
            project_root: Some(root.clone()),
            include_directories: Vec::new(),
            source_overlays: BTreeMap::new(),
            ..NwScriptDefinitionQuery::default()
        })
        .expect("find procedural macro definition");

        assert_eq!(definitions.len(), 1);
        assert_eq!(
            definitions.first().map(|definition| definition.kind),
            Some(NwScriptSymbolKind::Macro)
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn unsaved_overlay_replaces_disk_contents_for_definition_lookup() {
        let root =
            std::env::temp_dir().join(format!("nwnrs-symbol-overlay-{}", std::process::id()));
        fs::create_dir_all(&root).expect("create overlay fixture");
        let source = root.join("main.nss");
        fs::write(&source, "void OldName() {}\n").expect("write disk source");
        let overlays = BTreeMap::from([(source.clone(), b"void NewName() {}\n".to_vec())]);

        let definitions = find_nwscript_definitions(&NwScriptDefinitionQuery {
            source_path: source.clone(),
            symbol: "NewName".to_string(),
            qualifier: None,
            project_root: Some(root.clone()),
            include_directories: Vec::new(),
            source_overlays: overlays,
            ..NwScriptDefinitionQuery::default()
        })
        .expect("find overlay definition");

        assert_eq!(definitions.len(), 1);
        assert_eq!(
            definitions.first().map(|definition| &definition.path),
            Some(&source)
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn resolves_vanilla_functions_constants_and_engine_structures() {
        if installed_vanilla_source().is_none() {
            return;
        }
        let root =
            std::env::temp_dir().join(format!("nwnrs-builtin-symbols-{}", std::process::id()));
        fs::create_dir_all(&root).expect("create builtin fixture root");
        let source = root.join("main.nss");
        fs::write(
            &source,
            "void main() { ActionMoveToLocation(GetLocation(OBJECT_SELF)); }\n",
        )
        .expect("write builtin caller");
        let find = |symbol: &str| {
            find_nwscript_definitions(&NwScriptDefinitionQuery {
                source_path: source.clone(),
                symbol: symbol.to_string(),
                project_root: Some(root.clone()),
                ..NwScriptDefinitionQuery::default()
            })
            .unwrap_or_else(|error| panic!("find builtin {symbol}: {error}"))
        };

        let function = find("ActionMoveToLocation");
        assert_eq!(function.len(), 1);
        let function = function.first().expect("builtin function definition");
        assert_eq!(function.kind, NwScriptSymbolKind::BuiltinFunction);
        assert_eq!(function.path, PathBuf::from("nwscript.nss"));
        assert!(function.virtual_uri.is_some());
        assert!(function.signature.starts_with("void ActionMoveToLocation("));
        let documentation = function
            .documentation
            .as_deref()
            .expect("vanilla function documentation");
        assert!(documentation.contains("The action subject will move to lDestination."));
        assert!(documentation.contains("- lDestination:"));
        assert!(documentation.contains("  invalid or a path cannot be found"));

        let constant = find("OBJECT_TYPE_CREATURE");
        assert_eq!(constant.len(), 1);
        let constant = constant.first().expect("builtin constant definition");
        assert_eq!(constant.kind, NwScriptSymbolKind::BuiltinConstant);
        assert!(constant.signature.starts_with("int OBJECT_TYPE_CREATURE ="));

        let structure = find("effect");
        assert_eq!(structure.len(), 1);
        let structure = structure.first().expect("engine structure definition");
        assert_eq!(structure.kind, NwScriptSymbolKind::EngineStructure);
        assert!(structure.signature.contains("ENGINE_STRUCTURE_0"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn workspace_nwscript_overrides_the_packed_game_resource() {
        let root =
            std::env::temp_dir().join(format!("nwnrs-workspace-langspec-{}", std::process::id()));
        fs::create_dir_all(&root).expect("create workspace langspec root");
        let source = root.join("main.nss");
        let langspec = root.join("nwscript.nss");
        fs::write(&source, "void main() { WorkspaceAction(1); }\n")
            .expect("write workspace caller");
        fs::write(
            &langspec,
            "// Documentation from the workspace override.\n// - nValue: Value supplied by the \
             workspace.\nvoid WorkspaceAction(int nValue);\n",
        )
        .expect("write workspace langspec");

        let definitions = find_nwscript_definitions(&NwScriptDefinitionQuery {
            source_path: source,
            symbol: "WorkspaceAction".to_string(),
            project_root: Some(root.clone()),
            ..NwScriptDefinitionQuery::default()
        })
        .expect("find workspace builtin override");
        let definition = definitions.first().expect("workspace builtin definition");
        assert_eq!(definition.path, langspec);
        assert_eq!(definition.virtual_uri, None);
        assert_eq!(definition.resource_name, None);
        assert!(
            definition
                .documentation
                .as_deref()
                .is_some_and(|docs| docs.contains("workspace override"))
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn every_installed_vanilla_function_has_adjacent_hover_documentation() {
        let Some(source) = installed_vanilla_source() else {
            return;
        };
        let langspec = parse_langspec_bytes("nwscript", &source).expect("parse vanilla langspec");
        let expected = langspec
            .functions
            .iter()
            .map(|function| function.name.as_str())
            .collect::<HashSet<_>>();
        let tokens = Lexer::new(SourceId::new(0), &source)
            .lex_all()
            .expect("lex vanilla langspec");
        let mut found = HashSet::new();
        let mut missing = Vec::new();
        let mut brace_depth = 0_usize;
        for (index, token) in tokens.iter().enumerate() {
            match token.kind {
                TokenKind::LeftBrace => brace_depth += 1,
                TokenKind::RightBrace => brace_depth = brace_depth.saturating_sub(1),
                TokenKind::LeftParen if brace_depth == 0 => {
                    let Some(name_index) = index.checked_sub(1) else {
                        continue;
                    };
                    let Some(name) = tokens.get(name_index) else {
                        continue;
                    };
                    if !expected.contains(name.text.as_str()) {
                        continue;
                    }
                    found.insert(name.text.as_str());
                    let declaration_start = function_declaration_start(&tokens, name_index)
                        .and_then(|start| tokens.get(start))
                        .map_or(name.span.start, |start| start.span.start);
                    if preceding_slash_documentation(&source, declaration_start)
                        .is_none_or(|documentation| documentation.trim().is_empty())
                    {
                        missing.push(name.text.clone());
                    }
                }
                _ => {}
            }
        }
        assert_eq!(
            found, expected,
            "not every parsed builtin function was indexed"
        );
        assert!(
            missing.is_empty(),
            "missing vanilla hover docs: {missing:?}"
        );
    }

    #[test]
    fn packed_vanilla_source_uses_a_stable_read_only_uri() {
        let Some(contents) = installed_vanilla_source() else {
            return;
        };
        let mut packed = InMemoryScriptResolver::new();
        packed.insert_source("nwscript", contents);
        let resolver = SymbolResolver {
            filesystem: FileSystemScriptResolver::new(),
            fallback:   Some(Arc::new(packed)),
        };
        let query = NwScriptDefinitionQuery {
            symbol: "ActionMoveToLocation".to_string(),
            ..NwScriptDefinitionQuery::default()
        };

        let definitions =
            scan_builtin_definitions(&resolver, &query).expect("index packed vanilla function");
        assert_eq!(definitions.len(), 1);
        let definition = definitions.first().expect("packed builtin definition");
        let uri = definition
            .virtual_uri
            .as_deref()
            .expect("packed source URI");
        assert!(uri.starts_with("nwnrs-game:/"));
        assert!(uri.ends_with("/nwscript.nss"));
        assert_eq!(definition.path, PathBuf::from("nwscript.nss"));
    }

    #[test]
    fn workspace_scripts_override_packed_vanilla_scripts() {
        let root =
            std::env::temp_dir().join(format!("nwnrs-vanilla-layering-{}", std::process::id()));
        fs::create_dir_all(&root).expect("create vanilla layering root");
        let main = root.join("main.nss");
        fs::write(
            &main,
            "#include \"vanilla_helper\"\nvoid main() { VanillaHelper(); }\n",
        )
        .expect("write vanilla include caller");

        let make_resolver = || {
            let mut filesystem = FileSystemScriptResolver::new();
            filesystem.add_root(&root);
            let mut packed = InMemoryScriptResolver::new();
            packed.insert_source("nwscript", "");
            packed.insert_source(
                "vanilla_helper",
                "// Documentation from the packed game script.\nvoid VanillaHelper() {}\n",
            );
            SymbolResolver {
                filesystem,
                fallback: Some(Arc::new(packed)),
            }
        };
        let find = |resolver: &SymbolResolver| {
            let bundle = nwnrs_nwscript::load_source_bundle(
                resolver,
                &main.to_string_lossy(),
                nwnrs_nwscript::SourceLoadOptions::default(),
            )
            .expect("load layered source graph");
            let analysis = compiler_analysis_for_bundle(
                resolver,
                &NwScriptDefinitionQuery {
                    source_path: main.clone(),
                    ..NwScriptDefinitionQuery::default()
                },
                &bundle,
                None,
            )
            .expect("analyze layered definitions");
            analysis
                .index
                .definitions
                .iter()
                .filter(|definition| {
                    definition.name == "VanillaHelper"
                        && definition.kind == nwnrs_nwscript::SemanticSymbolKind::Function
                })
                .filter_map(|definition| {
                    definition_from_compiler_span(
                        &bundle,
                        resolver,
                        definition.declaration_span,
                        &definition.name,
                        NwScriptSymbolKind::Function,
                        true,
                    )
                })
                .collect::<Vec<_>>()
        };

        let packed = find(&make_resolver());
        let packed = packed.first().expect("packed vanilla definition");
        assert_eq!(packed.path, PathBuf::from("vanilla_helper.nss"));
        assert_eq!(packed.resource_name.as_deref(), Some("vanilla_helper"));
        assert!(
            packed
                .virtual_uri
                .as_deref()
                .is_some_and(|uri| uri.starts_with("nwnrs-game:/"))
        );
        assert!(
            packed
                .documentation
                .as_deref()
                .is_some_and(|docs| docs.contains("packed game script"))
        );

        let workspace = root.join("vanilla_helper.nss");
        fs::write(
            &workspace,
            "/// Documentation from the workspace override.\nvoid VanillaHelper() {}\n",
        )
        .expect("write vanilla workspace override");
        let overridden = find(&make_resolver());
        let overridden = overridden.first().expect("workspace vanilla definition");
        assert_eq!(overridden.path, workspace);
        assert_eq!(overridden.virtual_uri, None);
        assert_eq!(overridden.resource_name, None);
        assert!(
            overridden
                .documentation
                .as_deref()
                .is_some_and(|docs| docs.contains("workspace override"))
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn document_outline_is_hierarchical_and_excludes_included_symbols() {
        let root = std::env::temp_dir().join(format!("nwnrs-outline-{}", std::process::id()));
        fs::create_dir_all(&root).expect("create Outline root");
        let source = root.join("outline.nss");
        fs::write(root.join("included.nss"), "void IncludedOnly() {}\n")
            .expect("write Outline include");
        fs::write(
            &source,
            r#"#define FEATURE_ENABLED 1
#include "included"
struct Stats { int score, assists; string label; };
enum LogLevel : int {
    #[default]
    #[alias(LOG_INFO)]
    Info = 0,
    Debug,
}
type Level = LogLevel;
const int GlobalCount = 1, OtherCount;
#[nwnrs::events(module_load)]
void HandleLoad(int nValue) {}
"#,
        )
        .expect("write Outline source");

        let symbols = list_nwscript_document_symbols(
            &NwScriptDefinitionQuery {
                source_path: source,
                project_root: Some(root.clone()),
                ..NwScriptDefinitionQuery::default()
            },
            None,
        )
        .expect("list Outline symbols");
        let names = symbols
            .iter()
            .map(|symbol| symbol.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            [
                "FEATURE_ENABLED",
                "Stats",
                "LogLevel",
                "Level",
                "GlobalCount",
                "OtherCount",
                "HandleLoad",
            ]
        );
        assert!(!names.contains(&"IncludedOnly"));

        let structure = symbols
            .iter()
            .find(|symbol| symbol.name == "Stats")
            .expect("structure symbol");
        assert_eq!(structure.kind, NwScriptDocumentSymbolKind::Struct);
        assert_eq!(
            structure
                .children
                .iter()
                .map(|child| child.name.as_str())
                .collect::<Vec<_>>(),
            ["score", "assists", "label"]
        );
        assert!(
            structure
                .children
                .iter()
                .all(|child| child.kind == NwScriptDocumentSymbolKind::Field)
        );

        let enumeration = symbols
            .iter()
            .find(|symbol| symbol.name == "LogLevel")
            .expect("enum symbol");
        assert_eq!(enumeration.kind, NwScriptDocumentSymbolKind::Enum);
        assert_eq!(
            enumeration
                .children
                .iter()
                .map(|child| child.name.as_str())
                .collect::<Vec<_>>(),
            ["Info", "Debug"]
        );
        let info = enumeration.children.first().expect("Info variant");
        assert_eq!(
            info.children.first().map(|child| child.name.as_str()),
            Some("LOG_INFO")
        );
        let function = symbols
            .iter()
            .find(|symbol| symbol.name == "HandleLoad")
            .expect("function symbol");
        assert_eq!(function.kind, NwScriptDocumentSymbolKind::Function);
        assert!(function.detail.as_deref().is_some_and(|detail| {
            detail == "event: module_load · void HandleLoad(int nValue)"
        }));
        assert_eq!(
            function.range.start_line + 1,
            function.selection_range.start_line
        );
        assert!(
            symbols
                .iter()
                .filter(|symbol| matches!(symbol.name.as_str(), "GlobalCount" | "OtherCount"))
                .all(|symbol| symbol.kind == NwScriptDocumentSymbolKind::Constant)
        );
        assert!(function.range.start_line <= function.selection_range.start_line);
        assert!(function.range.end_line >= function.selection_range.end_line);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn document_outline_uses_unsaved_overlays_and_survives_parse_errors() {
        let root =
            std::env::temp_dir().join(format!("nwnrs-outline-overlay-{}", std::process::id()));
        fs::create_dir_all(&root).expect("create overlay Outline root");
        let source = root.join("outline.nss");
        fs::write(&source, "void SavedName() {}\n").expect("write saved Outline source");
        let overlays = BTreeMap::from([(
            source.clone(),
            b"void UnsavedName() {}\nvoid Incomplete( {\n".to_vec(),
        )]);

        let symbols = list_nwscript_document_symbols(
            &NwScriptDefinitionQuery {
                source_path: source,
                project_root: Some(root.clone()),
                source_overlays: overlays,
                ..NwScriptDefinitionQuery::default()
            },
            None,
        )
        .expect("list dirty Outline symbols");
        assert!(symbols.iter().any(|symbol| symbol.name == "UnsavedName"));
        assert!(!symbols.iter().any(|symbol| symbol.name == "SavedName"));
        assert!(!symbols.iter().any(|symbol| symbol.name == "Incomplete"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn document_outline_excludes_synthetic_macro_output() {
        let root = std::env::temp_dir().join(format!("nwnrs-outline-macro-{}", std::process::id()));
        fs::create_dir_all(&root).expect("create macro Outline root");
        let source = root.join("outline.nss");
        fs::write(
            &source,
            "macro_rules! make { ($name:ident) => { void $name() {} }; }\nmake!(Generated)\nvoid \
             Authored() {}\n",
        )
        .expect("write macro Outline source");

        let symbols = list_nwscript_document_symbols(
            &NwScriptDefinitionQuery {
                source_path: source,
                project_root: Some(root.clone()),
                ..NwScriptDefinitionQuery::default()
            },
            None,
        )
        .expect("list macro Outline symbols");
        assert!(symbols.iter().any(|symbol| symbol.name == "make"));
        assert!(symbols.iter().any(|symbol| symbol.name == "Authored"));
        assert!(!symbols.iter().any(|symbol| symbol.name == "Generated"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn document_outline_recovers_symbols_after_a_lexical_error() {
        let root = std::env::temp_dir().join(format!("nwnrs-outline-lexer-{}", std::process::id()));
        fs::create_dir_all(&root).expect("create lexer Outline root");
        let source = root.join("outline.nss");
        fs::write(
            &source,
            "void Before() {}\nvoid Broken() { string value = \"unterminated\n}\nvoid After() {}\n",
        )
        .expect("write lexically invalid Outline source");

        let symbols = list_nwscript_document_symbols(
            &NwScriptDefinitionQuery {
                source_path: source,
                project_root: Some(root.clone()),
                ..NwScriptDefinitionQuery::default()
            },
            None,
        )
        .expect("recover lexical Outline symbols");
        assert!(symbols.iter().any(|symbol| symbol.name == "Before"));
        assert!(symbols.iter().any(|symbol| symbol.name == "After"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn semantic_document_classifies_declarations_uses_and_enum_hints() {
        let root = std::env::temp_dir().join(format!("nwnrs-semantic-{}", std::process::id()));
        fs::create_dir_all(&root).expect("create semantic root");
        let source = root.join("semantic.nss");
        fs::write(
            &source,
            "enum Mode : int { Ready, Running = 4, Done }\nconst int Limit = 3;\nvoid Use(int \
             value) { int localValue = value; Use(Limit); }\n",
        )
        .expect("write semantic source");
        let (tokens, hints) = analyze_nwscript_document(
            &NwScriptDefinitionQuery {
                source_path: source,
                project_root: Some(root.clone()),
                ..NwScriptDefinitionQuery::default()
            },
            None,
        )
        .expect("analyze semantic document");
        assert!(tokens.iter().any(|token| {
            token.kind == NwScriptSemanticTokenKind::Function && token.is_declaration
        }));
        assert!(tokens.iter().any(|token| {
            token.kind == NwScriptSemanticTokenKind::Parameter && token.is_declaration
        }));
        assert!(tokens.iter().any(|token| {
            token.kind == NwScriptSemanticTokenKind::Variable && token.is_readonly
        }));
        assert_eq!(
            hints
                .iter()
                .filter(|hint| hint.kind == "enumValue")
                .map(|hint| hint.label.as_str())
                .collect::<Vec<_>>(),
            [" = 0", " = 5"]
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn references_distinguish_calls_and_function_local_bindings() {
        let root = std::env::temp_dir().join(format!("nwnrs-references-{}", std::process::id()));
        fs::create_dir_all(&root).expect("create references root");
        let source = root.join("references.nss");
        fs::write(
            &source,
            "void Work() {}\nvoid Caller(int value) { int copy = value; Work(); value = copy; }\n",
        )
        .expect("write references source");
        let base = NwScriptDefinitionQuery {
            source_path: source,
            project_root: Some(root.clone()),
            ..NwScriptDefinitionQuery::default()
        };
        let calls = find_nwscript_references(
            &NwScriptDefinitionQuery {
                symbol: "Work".to_string(),
                ..base.clone()
            },
            2,
            44,
        )
        .expect("find function references");
        assert_eq!(calls.len(), 2);
        assert!(calls.iter().any(|reference| reference.is_declaration));
        assert!(calls.iter().any(|reference| {
            !reference.is_declaration && reference.container.as_deref() == Some("Caller")
        }));
        let outgoing = find_nwscript_outgoing_calls(
            &NwScriptDefinitionQuery {
                symbol: "Caller".to_string(),
                ..base.clone()
            },
            2,
        )
        .expect("find outgoing calls");
        assert_eq!(outgoing.len(), 1);
        let outgoing = outgoing.first().expect("Work outgoing call");
        assert_eq!(outgoing.target.name, "Work");
        assert_eq!(outgoing.ranges.len(), 1);

        let values = find_nwscript_references(
            &NwScriptDefinitionQuery {
                symbol: "value".to_string(),
                ..base
            },
            2,
            17,
        )
        .expect("find parameter references");
        assert_eq!(values.len(), 3);
        assert!(
            values
                .iter()
                .all(|reference| reference.container.as_deref() == Some("Caller"))
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn references_index_macro_declarations_and_invocations_without_semantic_fallbacks() {
        let root =
            std::env::temp_dir().join(format!("nwnrs-macro-references-{}", std::process::id()));
        fs::create_dir_all(&root).expect("create macro reference root");
        let source = root.join("macros.nss");
        fs::write(
            &source,
            "macro_rules! emit { ($body:tokens) => { $body }; }\nemit!(void Generated() {})\n",
        )
        .expect("write macro reference source");
        let references = find_nwscript_references(
            &NwScriptDefinitionQuery {
                source_path: source,
                project_root: Some(root.clone()),
                symbol: "emit".to_string(),
                ..NwScriptDefinitionQuery::default()
            },
            2,
            1,
        )
        .expect("find macro references");
        assert_eq!(references.len(), 2);
        assert_eq!(
            references
                .iter()
                .filter(|reference| reference.is_declaration)
                .count(),
            1
        );
        assert!(
            references
                .iter()
                .all(|reference| reference.kind == NwScriptSymbolKind::Macro)
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn macro_occurrences_preserve_the_complete_namespace() {
        let mut source_map = SourceMap::new();
        let source_id = source_map.add_file(
            "namespaced.nss",
            b"proc_macro! project::nested::emit { }\nproject::nested::emit!();\n".to_vec(),
        );
        let source = source_map.get(source_id).expect("namespaced source");
        let tokens = Lexer::new(source.id, source.bytes())
            .lex_all()
            .expect("lex namespaced macros");
        let occurrences = tokens
            .iter()
            .enumerate()
            .filter(|(_, token)| token.text == "emit")
            .filter_map(|(index, _)| macro_occurrence(&tokens, index))
            .collect::<Vec<_>>();
        assert_eq!(occurrences.len(), 2);
        assert!(
            occurrences
                .first()
                .expect("macro declaration")
                .is_declaration()
        );
        assert!(
            !occurrences
                .get(1)
                .expect("macro invocation")
                .is_declaration()
        );
        assert!(
            occurrences
                .iter()
                .all(|occurrence| occurrence.qualifier() == Some("project::nested"))
        );
    }

    #[test]
    fn references_cover_sibling_project_scripts() {
        let root =
            std::env::temp_dir().join(format!("nwnrs-project-references-{}", std::process::id()));
        fs::create_dir_all(&root).expect("create project reference root");
        let declaration = root.join("shared.nss");
        let first = root.join("first.nss");
        let second = root.join("second.nss");
        fs::write(&declaration, "void SharedWork() {}\n").expect("write shared source");
        fs::write(
            &first,
            "#include \"shared\"\nvoid First() { SharedWork(); }\n",
        )
        .expect("write first caller");
        fs::write(
            &second,
            "#include \"shared\"\nvoid Second() { SharedWork(); }\n",
        )
        .expect("write second caller");

        let references = find_nwscript_references(
            &NwScriptDefinitionQuery {
                source_path: first,
                project_root: Some(root.clone()),
                symbol: "SharedWork".to_string(),
                ..NwScriptDefinitionQuery::default()
            },
            2,
            16,
        )
        .expect("find package-wide references");
        assert_eq!(references.len(), 3);
        assert_eq!(
            references
                .iter()
                .filter(|reference| !reference.is_declaration)
                .count(),
            2
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn references_respect_nested_local_shadowing() {
        let root =
            std::env::temp_dir().join(format!("nwnrs-shadow-references-{}", std::process::id()));
        fs::create_dir_all(&root).expect("create shadow reference root");
        let source = root.join("shadow.nss");
        fs::write(
            &source,
            "void Test(int value) {\nvalue = 1;\n{\nint value = 2;\nvalue = 3;\n}\nvalue = 4;\n}\n",
        )
        .expect("write shadow source");
        let base = NwScriptDefinitionQuery {
            source_path: source,
            project_root: Some(root.clone()),
            symbol: "value".to_string(),
            ..NwScriptDefinitionQuery::default()
        };

        let outer = find_nwscript_references(&base, 2, 1).expect("find outer binding");
        assert_eq!(outer.len(), 3);
        assert!(
            outer
                .iter()
                .any(|reference| reference.range.start_line == 7)
        );
        assert!(
            !outer
                .iter()
                .any(|reference| reference.range.start_line == 5)
        );

        let inner = find_nwscript_references(&base, 5, 1).expect("find inner binding");
        assert_eq!(inner.len(), 2);
        assert!(
            inner
                .iter()
                .all(|reference| matches!(reference.range.start_line, 4 | 5))
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn references_keep_same_named_enum_variants_separate() {
        let root =
            std::env::temp_dir().join(format!("nwnrs-enum-references-{}", std::process::id()));
        fs::create_dir_all(&root).expect("create enum reference root");
        let source = root.join("enums.nss");
        fs::write(
            &source,
            "enum First : int { Ready }\nenum Second : int { Ready }\nvoid Use() { First value = \
             First::Ready; Second other = Second::Ready; }\n",
        )
        .expect("write enum source");
        let references = find_nwscript_references(
            &NwScriptDefinitionQuery {
                source_path: source,
                project_root: Some(root.clone()),
                symbol: "Ready".to_string(),
                qualifier: Some("First".to_string()),
                ..NwScriptDefinitionQuery::default()
            },
            3,
            36,
        )
        .expect("find qualified variant references");
        assert_eq!(references.len(), 2);
        assert!(
            references
                .iter()
                .any(|reference| reference.range.start_line == 1)
        );
        assert!(
            !references
                .iter()
                .any(|reference| reference.range.start_line == 2)
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn references_resolve_same_named_fields_from_receiver_types() {
        let root =
            std::env::temp_dir().join(format!("nwnrs-field-references-{}", std::process::id()));
        fs::create_dir_all(&root).expect("create field reference root");
        let source = root.join("fields.nss");
        fs::write(
            &source,
            "struct First { int value; };\nstruct Second { int value; };\nstruct Outer { struct \
             First inner; };\nvoid Use() { struct First first; struct Second second; struct Outer \
             outer; first.value = 1; second.value = 2; outer.inner.value = 3; }\n",
        )
        .expect("write field source");
        let base = NwScriptDefinitionQuery {
            source_path: source,
            project_root: Some(root.clone()),
            symbol: "value".to_string(),
            ..NwScriptDefinitionQuery::default()
        };
        let direct = find_nwscript_references(&base, 4, 84).expect("resolve direct field");
        assert_eq!(direct.len(), 3);
        assert!(
            direct
                .iter()
                .any(|reference| reference.range.start_line == 1)
        );
        assert!(
            !direct
                .iter()
                .any(|reference| reference.range.start_line == 2)
        );

        let second = find_nwscript_references(&base, 4, 102).expect("resolve second field");
        assert_eq!(second.len(), 2);
        assert!(
            second
                .iter()
                .any(|reference| reference.range.start_line == 2)
        );
        assert!(
            !second
                .iter()
                .any(|reference| reference.range.start_line == 1)
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn project_index_reuses_stable_units_and_revalidates_dependency_content() {
        let root = std::env::temp_dir().join(format!("nwnrs-project-index-{}", std::process::id()));
        fs::create_dir_all(&root).expect("create project index root");
        let langspec = root.join("nwscript.nss");
        let shared = root.join("shared.nss");
        let source = root.join("main.nss");
        fs::write(&langspec, "void PrintString(string value);\n").expect("write langspec");
        fs::write(&shared, "void Shared() {}\n").expect("write dependency");
        fs::write(&source, "#include \"shared\"\nvoid main() { Shared(); }\n")
            .expect("write indexed root");
        let query = NwScriptDefinitionQuery {
            source_path: source,
            project_root: Some(root.clone()),
            langspec: Some(langspec),
            ..NwScriptDefinitionQuery::default()
        };
        let mut index = NwScriptProjectIndex::new();
        index
            .document_symbols(&query, None)
            .expect("build initial index");
        let initial_generation = index.generation();
        index
            .document_symbols(&query, None)
            .expect("reuse unchanged index");
        assert_eq!(index.generation(), initial_generation);

        fs::write(&shared, "\nvoid Shared() {}\n").expect("update dependency contents");
        index
            .document_symbols(&query, None)
            .expect("rebuild after dependency edit");
        let changed_generation = index.generation();
        assert!(changed_generation > initial_generation);

        index.invalidate_path(&shared);
        assert!(index.generation() > changed_generation);
        index
            .document_symbols(&query, None)
            .expect("rebuild after explicit invalidation");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn project_index_never_turns_cancellation_into_a_last_good_result() {
        let root =
            std::env::temp_dir().join(format!("nwnrs-project-cancel-{}", std::process::id()));
        fs::create_dir_all(&root).expect("create cancellation root");
        let langspec = root.join("nwscript.nss");
        let source = root.join("main.nss");
        fs::write(&langspec, "").expect("write cancellation langspec");
        fs::write(&source, "void main() {}\n").expect("write cancellation source");
        let query = NwScriptDefinitionQuery {
            source_path: source,
            project_root: Some(root.clone()),
            langspec: Some(langspec),
            ..NwScriptDefinitionQuery::default()
        };
        let mut index = NwScriptProjectIndex::new();
        index
            .document_symbols(&query, None)
            .expect("build last-good analysis");
        let generation = index.generation();
        let cancellation = nwnrs_nwscript::CancellationToken::new();
        cancellation.cancel();
        let error = index
            .document_symbols(&query, Some(&cancellation))
            .expect_err("cancelled analysis must not return last-good data");
        assert_eq!(error, "operation cancelled");
        assert_eq!(index.generation(), generation);
        let _ = fs::remove_dir_all(root);
    }
}
