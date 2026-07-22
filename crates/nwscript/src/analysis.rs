use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use crate::{
    Expr, ExprKind, HirBlock, HirCallTarget, HirExpr, HirExprKind, HirFunction, HirLocalId,
    HirModule, HirStmt, HirValueRef, LangSpec, Lexer, Script, SemanticModel, SemanticType,
    SourceId, SourceMap, Span, Stmt, Token, TokenKind, TopLevelItem, TypeKind, TypeSpec,
};

/// A stable semantic identity within one immutable compiler analysis snapshot.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SemanticSymbolId {
    /// A user function, including all compatible declarations and its
    /// implementation.
    Function(String),
    /// A global variable or constant.
    Global(String),
    /// A user structure.
    Struct(String),
    /// A field owned by a user structure.
    Field {
        /// Owning structure name.
        owner: String,
        /// Field name.
        name:  String,
    },
    /// A strong enum type.
    Enum(String),
    /// A variant owned by a strong enum.
    EnumVariant {
        /// Owning enum name.
        owner: String,
        /// Variant name.
        name:  String,
    },
    /// A transparent type alias.
    TypeAlias(String),
    /// A function-local slot. Local ids are stable within the containing
    /// function analysis.
    Local {
        /// Source containing the function declaration.
        function_source: SourceId,
        /// Byte offset where the containing function begins.
        function_start:  usize,
        /// Compiler-assigned local slot.
        local:           HirLocalId,
    },
    /// A function supplied by the active language specification.
    BuiltinFunction(String),
    /// A constant supplied by the active language specification.
    BuiltinConstant(String),
    /// An engine structure supplied by the active language specification.
    EngineStructure(String),
}

/// The compiler-owned category of a semantic symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SemanticSymbolKind {
    /// User function.
    Function,
    /// Mutable global.
    Global,
    /// Constant global.
    Constant,
    /// User structure.
    Struct,
    /// Structure field.
    Field,
    /// Strong enum type.
    Enum,
    /// Strong enum variant.
    EnumVariant,
    /// Transparent type alias.
    TypeAlias,
    /// Function parameter.
    Parameter,
    /// Block-local variable.
    Local,
    /// Language-spec function.
    BuiltinFunction,
    /// Language-spec constant.
    BuiltinConstant,
    /// Language-spec engine structure.
    EngineStructure,
}

/// One source-authored declaration resolved by the compiler.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticSymbolDefinition {
    /// Stable semantic identity.
    pub id:               SemanticSymbolId,
    /// Source name.
    pub name:             String,
    /// Symbol category.
    pub kind:             SemanticSymbolKind,
    /// Exact identifier span when the declaration is source-authored.
    pub span:             Span,
    /// Full declaration span when source syntax provides one.
    pub declaration_span: Span,
    /// Resolved declared type when applicable.
    pub ty:               Option<SemanticType>,
    /// Owning function, structure, or enum.
    pub container:        Option<SemanticSymbolId>,
}

/// One source occurrence whose target was resolved by the compiler.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticSymbolReference {
    /// Resolved declaration identity.
    pub target: SemanticSymbolId,
    /// Exact source occurrence.
    pub span:   Span,
    /// Resolved expression type when applicable.
    pub ty:     Option<SemanticType>,
    /// Whether this occurrence mutates its target.
    pub write:  bool,
}

/// Source-addressable semantic facts for one fully analyzed compilation unit.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticIndex {
    /// Source and builtin declarations.
    pub definitions: Vec<SemanticSymbolDefinition>,
    /// Resolved source occurrences.
    pub references:  Vec<SemanticSymbolReference>,
}

impl SemanticIndex {
    /// Returns every declaration for `id` in source order.
    pub fn definitions_for(
        &self,
        id: &SemanticSymbolId,
    ) -> impl Iterator<Item = &SemanticSymbolDefinition> {
        self.definitions
            .iter()
            .filter(move |definition| &definition.id == id)
    }

    /// Returns every resolved use of `id` in source order.
    pub fn references_for(
        &self,
        id: &SemanticSymbolId,
    ) -> impl Iterator<Item = &SemanticSymbolReference> {
        self.references
            .iter()
            .filter(move |reference| &reference.target == id)
    }

    /// Finds the semantic target at a byte position, preferring a reference
    /// over its declaration when generated spans overlap.
    #[must_use]
    pub fn symbol_at(&self, source_id: SourceId, offset: usize) -> Option<&SemanticSymbolId> {
        self.references
            .iter()
            .filter(|reference| span_contains(reference.span, source_id, offset))
            .min_by_key(|reference| reference.span.end.saturating_sub(reference.span.start))
            .map(|reference| &reference.target)
            .or_else(|| {
                self.definitions
                    .iter()
                    .filter(|definition| span_contains(definition.span, source_id, offset))
                    .min_by_key(|definition| {
                        definition.span.end.saturating_sub(definition.span.start)
                    })
                    .map(|definition| &definition.id)
            })
    }
}

/// Builds the editor/query index from the same typed HIR consumed by code
/// generation.
#[must_use]
pub fn build_semantic_index(
    script: &Script,
    semantic: &SemanticModel,
    hir: &HirModule,
    langspec: Option<&LangSpec>,
    sources: &SourceMap,
) -> SemanticIndex {
    SemanticIndexBuilder::new(script, semantic, hir, langspec, sources).build()
}

struct SemanticIndexBuilder<'a> {
    script:   &'a Script,
    semantic: &'a SemanticModel,
    hir:      &'a HirModule,
    langspec: Option<&'a LangSpec>,
    tokens:   HashMap<SourceId, Vec<Token>>,
    index:    SemanticIndex,
}

impl<'a> SemanticIndexBuilder<'a> {
    fn new(
        script: &'a Script,
        semantic: &'a SemanticModel,
        hir: &'a HirModule,
        langspec: Option<&'a LangSpec>,
        sources: &'a SourceMap,
    ) -> Self {
        let tokens = sources
            .iter()
            .filter_map(|source| {
                Lexer::new(source.id, source.bytes())
                    .lex_all()
                    .ok()
                    .map(|tokens| (source.id, tokens))
            })
            .collect();
        Self {
            script,
            semantic,
            hir,
            langspec,
            tokens,
            index: SemanticIndex::default(),
        }
    }

    fn build(mut self) -> SemanticIndex {
        self.record_top_level_definitions();
        self.record_builtin_definitions();
        self.record_type_references();
        for function in &self.hir.functions {
            self.record_function(function);
        }
        for global in &self.hir.globals {
            if let Some(initializer) = &global.initializer {
                self.record_expr(initializer, None);
            }
        }
        self.index.definitions.sort_by_key(|definition| {
            (
                definition.span.source_id,
                definition.span.start,
                definition.span.end,
            )
        });
        self.index.references.sort_by_key(|reference| {
            (
                reference.span.source_id,
                reference.span.start,
                reference.span.end,
            )
        });
        self.index
    }

    fn record_top_level_definitions(&mut self) {
        for item in &self.script.items {
            match item {
                TopLevelItem::Function(function) => {
                    let id = SemanticSymbolId::Function(function.name.clone());
                    self.definition(
                        id,
                        &function.name,
                        SemanticSymbolKind::Function,
                        function.span,
                        self.semantic
                            .functions
                            .get(&function.name)
                            .map(|function| function.return_type.clone()),
                        None,
                    );
                }
                TopLevelItem::Global(declaration) => {
                    for declarator in &declaration.declarators {
                        let global = self.semantic.globals.get(&declarator.name);
                        self.definition(
                            SemanticSymbolId::Global(declarator.name.clone()),
                            &declarator.name,
                            if global.is_some_and(|global| global.is_const) {
                                SemanticSymbolKind::Constant
                            } else {
                                SemanticSymbolKind::Global
                            },
                            declarator.span,
                            global.map(|global| global.ty.clone()),
                            None,
                        );
                    }
                }
                TopLevelItem::Struct(structure) => {
                    let owner = SemanticSymbolId::Struct(structure.name.clone());
                    self.definition(
                        owner.clone(),
                        &structure.name,
                        SemanticSymbolKind::Struct,
                        structure.span,
                        Some(SemanticType::Struct(structure.name.clone())),
                        None,
                    );
                    let resolved_fields = self
                        .semantic
                        .structs
                        .get(&structure.name)
                        .map(|resolved| {
                            resolved
                                .fields
                                .iter()
                                .map(|field| (field.name.clone(), field.ty.clone()))
                                .collect::<BTreeMap<_, _>>()
                        })
                        .unwrap_or_default();
                    for field in structure.fields.iter().flat_map(|field| &field.names) {
                        self.definition(
                            SemanticSymbolId::Field {
                                owner: structure.name.clone(),
                                name:  field.name.clone(),
                            },
                            &field.name,
                            SemanticSymbolKind::Field,
                            field.span,
                            resolved_fields.get(&field.name).cloned(),
                            Some(owner.clone()),
                        );
                    }
                }
                TopLevelItem::Enum(enumeration) => {
                    let owner = SemanticSymbolId::Enum(enumeration.name.clone());
                    self.definition(
                        owner.clone(),
                        &enumeration.name,
                        SemanticSymbolKind::Enum,
                        enumeration.span,
                        self.semantic.enums.get(&enumeration.name).map(|resolved| {
                            SemanticType::Enum {
                                name:    enumeration.name.clone(),
                                backing: resolved.backing,
                            }
                        }),
                        None,
                    );
                    for variant in &enumeration.variants {
                        self.definition(
                            SemanticSymbolId::EnumVariant {
                                owner: enumeration.name.clone(),
                                name:  variant.name.clone(),
                            },
                            &variant.name,
                            SemanticSymbolKind::EnumVariant,
                            variant.span,
                            self.semantic.enums.get(&enumeration.name).map(|resolved| {
                                SemanticType::Enum {
                                    name:    enumeration.name.clone(),
                                    backing: resolved.backing,
                                }
                            }),
                            Some(owner.clone()),
                        );
                        for alias in &variant.aliases {
                            self.definition(
                                SemanticSymbolId::Global(alias.name.clone()),
                                &alias.name,
                                SemanticSymbolKind::Constant,
                                alias.span,
                                self.semantic.enums.get(&enumeration.name).map(|resolved| {
                                    SemanticType::Enum {
                                        name:    enumeration.name.clone(),
                                        backing: resolved.backing,
                                    }
                                }),
                                Some(owner.clone()),
                            );
                        }
                    }
                }
                TopLevelItem::TypeAlias(alias) => self.definition(
                    SemanticSymbolId::TypeAlias(alias.name.clone()),
                    &alias.name,
                    SemanticSymbolKind::TypeAlias,
                    alias.span,
                    self.semantic.aliases.get(&alias.name).cloned(),
                    None,
                ),
                TopLevelItem::Include(_) | TopLevelItem::StaticAssert(_) => {}
            }
        }
    }

    fn record_builtin_definitions(&mut self) {
        let Some(langspec) = self.langspec else {
            return
        };
        for function in &langspec.functions {
            self.index.definitions.push(SemanticSymbolDefinition {
                id:               SemanticSymbolId::BuiltinFunction(function.name.clone()),
                name:             function.name.clone(),
                kind:             SemanticSymbolKind::BuiltinFunction,
                span:             Span::new(SourceId::new(u32::MAX), 0, 0),
                declaration_span: Span::new(SourceId::new(u32::MAX), 0, 0),
                ty:               self
                    .semantic
                    .functions
                    .get(&function.name)
                    .map(|function| function.return_type.clone()),
                container:        None,
            });
        }
        for constant in &langspec.constants {
            self.index.definitions.push(SemanticSymbolDefinition {
                id:               SemanticSymbolId::BuiltinConstant(constant.name.clone()),
                name:             constant.name.clone(),
                kind:             SemanticSymbolKind::BuiltinConstant,
                span:             Span::new(SourceId::new(u32::MAX), 0, 0),
                declaration_span: Span::new(SourceId::new(u32::MAX), 0, 0),
                ty:               None,
                container:        None,
            });
        }
        for structure in &langspec.engine_structures {
            self.index.definitions.push(SemanticSymbolDefinition {
                id:               SemanticSymbolId::EngineStructure(structure.clone()),
                name:             structure.clone(),
                kind:             SemanticSymbolKind::EngineStructure,
                span:             Span::new(SourceId::new(u32::MAX), 0, 0),
                declaration_span: Span::new(SourceId::new(u32::MAX), 0, 0),
                ty:               Some(SemanticType::EngineStructure(structure.clone())),
                container:        None,
            });
        }
    }

    fn record_function(&mut self, function: &HirFunction) {
        let function_id = SemanticSymbolId::Function(function.name.clone());
        for parameter in &function.parameters {
            self.definition(
                SemanticSymbolId::Local {
                    function_source: function.span.source_id,
                    function_start:  function.span.start,
                    local:           parameter.local,
                },
                &parameter.name,
                SemanticSymbolKind::Parameter,
                parameter.span,
                Some(parameter.ty.clone()),
                Some(function_id.clone()),
            );
            if let Some(default) = &parameter.default {
                self.record_expr(default, Some(function));
            }
        }
        for local in function
            .locals
            .iter()
            .filter(|local| matches!(local.kind, crate::HirLocalKind::Local))
        {
            self.definition(
                SemanticSymbolId::Local {
                    function_source: function.span.source_id,
                    function_start:  function.span.start,
                    local:           local.id,
                },
                &local.name,
                SemanticSymbolKind::Local,
                local.declaration_span,
                Some(local.ty.clone()),
                Some(function_id.clone()),
            );
        }
        if let Some(body) = &function.body {
            self.record_block(body, function);
        }
    }

    fn record_block(&mut self, block: &HirBlock, function: &HirFunction) {
        for statement in &block.statements {
            self.record_stmt(statement, function);
        }
    }

    fn record_stmt(&mut self, statement: &HirStmt, function: &HirFunction) {
        match statement {
            HirStmt::Block(block) => self.record_block(block, function),
            HirStmt::Declare(declaration) => {
                for declarator in &declaration.declarators {
                    if let Some(initializer) = &declarator.initializer {
                        self.record_expr(initializer, Some(function));
                    }
                }
            }
            HirStmt::Expr(expression) | HirStmt::Case(expression) => {
                self.record_expr(expression, Some(function));
            }
            HirStmt::If(statement) => {
                self.record_expr(&statement.condition, Some(function));
                self.record_stmt(&statement.then_branch, function);
                if let Some(branch) = &statement.else_branch {
                    self.record_stmt(branch, function);
                }
            }
            HirStmt::Switch(statement) => {
                self.record_expr(&statement.condition, Some(function));
                self.record_stmt(&statement.body, function);
            }
            HirStmt::Return(statement) => {
                if let Some(value) = &statement.value {
                    self.record_expr(value, Some(function));
                }
            }
            HirStmt::While(statement) => {
                self.record_expr(&statement.condition, Some(function));
                self.record_stmt(&statement.body, function);
            }
            HirStmt::DoWhile(statement) => {
                self.record_stmt(&statement.body, function);
                self.record_expr(&statement.condition, Some(function));
            }
            HirStmt::For(statement) => {
                for expression in [
                    statement.initializer.as_ref(),
                    statement.condition.as_ref(),
                    statement.update.as_ref(),
                ]
                .into_iter()
                .flatten()
                {
                    self.record_expr(expression, Some(function));
                }
                self.record_stmt(&statement.body, function);
            }
            HirStmt::Default(_) | HirStmt::Break(_) | HirStmt::Continue(_) | HirStmt::Empty(_) => {}
        }
    }

    fn record_expr(&mut self, expression: &HirExpr, function: Option<&HirFunction>) {
        match &expression.kind {
            HirExprKind::Value(value) => {
                let target = match value {
                    HirValueRef::Local(local) => function.map(|function| SemanticSymbolId::Local {
                        function_source: function.span.source_id,
                        function_start:  function.span.start,
                        local:           *local,
                    }),
                    HirValueRef::Global(name) | HirValueRef::ConstGlobal(name) => {
                        Some(SemanticSymbolId::Global(name.clone()))
                    }
                    HirValueRef::BuiltinConstant(name) => {
                        Some(SemanticSymbolId::BuiltinConstant(name.clone()))
                    }
                };
                if let Some(target) = target {
                    self.reference(
                        target,
                        expression.span,
                        None,
                        Some(expression.ty.clone()),
                        false,
                    );
                }
            }
            HirExprKind::Call {
                target,
                arguments,
            } => {
                let (target, name) = match target {
                    HirCallTarget::Function(name) => {
                        (SemanticSymbolId::Function(name.clone()), name.as_str())
                    }
                    HirCallTarget::Builtin(name) => (
                        SemanticSymbolId::BuiltinFunction(name.clone()),
                        name.as_str(),
                    ),
                };
                self.reference(
                    target,
                    expression.span,
                    Some(name),
                    Some(expression.ty.clone()),
                    false,
                );
                for argument in arguments {
                    self.record_expr(argument, function);
                }
            }
            HirExprKind::FieldAccess {
                base,
                field,
            } => {
                self.record_expr(base, function);
                if let SemanticType::Struct(owner) = &base.ty {
                    self.reference(
                        SemanticSymbolId::Field {
                            owner: owner.clone(),
                            name:  field.clone(),
                        },
                        expression.span,
                        Some(field),
                        Some(expression.ty.clone()),
                        false,
                    );
                }
            }
            HirExprKind::Match {
                value,
                arms,
            } => {
                self.record_expr(value, function);
                for arm in arms {
                    if let Some(guard) = &arm.guard {
                        self.record_expr(guard, function);
                    }
                    match &arm.body {
                        crate::HirMatchArmBody::Expr(expression) => {
                            self.record_expr(expression, function);
                        }
                        crate::HirMatchArmBody::Block {
                            block,
                            tail,
                            ..
                        } => {
                            if let Some(function) = function {
                                self.record_block(block, function);
                            }
                            if let Some(tail) = tail {
                                self.record_expr(tail, function);
                            }
                        }
                    }
                }
            }
            HirExprKind::CheckedEnumConversion {
                value,
                fallback,
                ..
            }
            | HirExprKind::Binary {
                left: value,
                right: fallback,
                ..
            }
            | HirExprKind::Assignment {
                left: value,
                right: fallback,
                ..
            } => {
                self.record_expr(value, function);
                self.record_expr(fallback, function);
            }
            HirExprKind::Unary {
                expr, ..
            } => self.record_expr(expr, function),
            HirExprKind::Conditional {
                condition,
                when_true,
                when_false,
            } => {
                self.record_expr(condition, function);
                self.record_expr(when_true, function);
                self.record_expr(when_false, function);
            }
            HirExprKind::Literal(_) => {}
        }
    }

    fn record_type_references(&mut self) {
        for item in &self.script.items {
            match item {
                TopLevelItem::Function(function) => {
                    self.type_reference(&function.return_type);
                    for parameter in &function.parameters {
                        self.type_reference(&parameter.ty);
                        if let Some(default) = &parameter.default {
                            self.record_scoped_identifiers(default);
                        }
                    }
                    if let Some(body) = &function.body {
                        for statement in &body.statements {
                            self.record_stmt_types(statement);
                        }
                    }
                }
                TopLevelItem::Global(declaration) => {
                    self.type_reference(&declaration.ty);
                    for declarator in &declaration.declarators {
                        if let Some(initializer) = &declarator.initializer {
                            self.record_scoped_identifiers(initializer);
                        }
                    }
                }
                TopLevelItem::Struct(structure) => {
                    for field in &structure.fields {
                        self.type_reference(&field.ty);
                    }
                }
                TopLevelItem::Enum(enumeration) => {
                    for variant in &enumeration.variants {
                        if let Some(value) = &variant.value {
                            self.record_scoped_identifiers(value);
                        }
                    }
                }
                TopLevelItem::TypeAlias(alias) => self.type_reference(&alias.target),
                TopLevelItem::StaticAssert(assertion) => {
                    self.record_scoped_identifiers(&assertion.condition);
                }
                TopLevelItem::Include(_) => {}
            }
        }
    }

    fn record_stmt_types(&mut self, statement: &Stmt) {
        match statement {
            Stmt::Block(block) => {
                for statement in &block.statements {
                    self.record_stmt_types(statement);
                }
            }
            Stmt::Declaration(declaration) => {
                self.type_reference(&declaration.ty);
                for declarator in &declaration.declarators {
                    if let Some(initializer) = &declarator.initializer {
                        self.record_scoped_identifiers(initializer);
                    }
                }
            }
            Stmt::Expression(statement) => self.record_scoped_identifiers(&statement.expr),
            Stmt::If(statement) => {
                self.record_scoped_identifiers(&statement.condition);
                self.record_stmt_types(&statement.then_branch);
                if let Some(branch) = &statement.else_branch {
                    self.record_stmt_types(branch);
                }
            }
            Stmt::Switch(statement) => {
                self.record_scoped_identifiers(&statement.condition);
                self.record_stmt_types(&statement.body);
            }
            Stmt::Return(statement) => {
                if let Some(value) = &statement.value {
                    self.record_scoped_identifiers(value);
                }
            }
            Stmt::While(statement) => {
                self.record_scoped_identifiers(&statement.condition);
                self.record_stmt_types(&statement.body);
            }
            Stmt::DoWhile(statement) => {
                self.record_stmt_types(&statement.body);
                self.record_scoped_identifiers(&statement.condition);
            }
            Stmt::For(statement) => {
                for expression in [
                    statement.initializer.as_ref(),
                    statement.condition.as_ref(),
                    statement.update.as_ref(),
                ]
                .into_iter()
                .flatten()
                {
                    self.record_scoped_identifiers(expression);
                }
                self.record_stmt_types(&statement.body);
            }
            Stmt::Case(statement) => self.record_scoped_identifiers(&statement.value),
            Stmt::StaticAssert(assertion) => self.record_scoped_identifiers(&assertion.condition),
            Stmt::Default(_) | Stmt::Break(_) | Stmt::Continue(_) | Stmt::Empty(_) => {}
        }
    }

    fn record_scoped_identifiers(&mut self, expression: &Expr) {
        match &expression.kind {
            ExprKind::ScopedIdentifier {
                scope,
                name,
            } => self.reference(
                SemanticSymbolId::EnumVariant {
                    owner: scope.clone(),
                    name:  name.clone(),
                },
                expression.span,
                Some(name),
                self.semantic
                    .enums
                    .get(scope)
                    .map(|resolved| SemanticType::Enum {
                        name:    scope.clone(),
                        backing: resolved.backing,
                    }),
                false,
            ),
            ExprKind::Match(expression) => {
                self.record_scoped_identifiers(&expression.value);
                for arm in &expression.arms {
                    for pattern in &arm.patterns {
                        if let crate::MatchPattern::Variant {
                            span,
                            scope,
                            name,
                        } = pattern
                        {
                            self.reference(
                                SemanticSymbolId::EnumVariant {
                                    owner: scope.clone(),
                                    name:  name.clone(),
                                },
                                *span,
                                Some(name),
                                self.semantic
                                    .enums
                                    .get(scope)
                                    .map(|resolved| SemanticType::Enum {
                                        name:    scope.clone(),
                                        backing: resolved.backing,
                                    }),
                                false,
                            );
                        }
                    }
                    if let Some(guard) = &arm.guard {
                        self.record_scoped_identifiers(guard);
                    }
                    match &arm.body {
                        crate::MatchArmBody::Expr(expression) => {
                            self.record_scoped_identifiers(expression);
                        }
                        crate::MatchArmBody::Block(block) => {
                            for statement in &block.statements {
                                self.record_stmt_types(statement);
                            }
                            if let Some(tail) = &block.tail {
                                self.record_scoped_identifiers(tail);
                            }
                        }
                    }
                }
            }
            ExprKind::Call {
                callee,
                arguments,
            } => {
                self.record_scoped_identifiers(callee);
                for argument in arguments {
                    self.record_scoped_identifiers(argument);
                }
            }
            ExprKind::FieldAccess {
                base, ..
            }
            | ExprKind::Unary {
                expr: base, ..
            } => {
                self.record_scoped_identifiers(base);
            }
            ExprKind::Binary {
                left,
                right,
                ..
            }
            | ExprKind::Assignment {
                left,
                right,
                ..
            } => {
                self.record_scoped_identifiers(left);
                self.record_scoped_identifiers(right);
            }
            ExprKind::Conditional {
                condition,
                when_true,
                when_false,
            } => {
                self.record_scoped_identifiers(condition);
                self.record_scoped_identifiers(when_true);
                self.record_scoped_identifiers(when_false);
            }
            ExprKind::Literal(_) | ExprKind::Identifier(_) => {}
        }
    }

    fn type_reference(&mut self, ty: &TypeSpec) {
        let (target, name) = match &ty.kind {
            TypeKind::Struct(name) => (SemanticSymbolId::Struct(name.clone()), name.as_str()),
            TypeKind::EngineStructure(name) => (
                SemanticSymbolId::EngineStructure(name.clone()),
                name.as_str(),
            ),
            TypeKind::Named(name) if self.semantic.enums.contains_key(name) => {
                (SemanticSymbolId::Enum(name.clone()), name.as_str())
            }
            TypeKind::Named(name) => (SemanticSymbolId::TypeAlias(name.clone()), name.as_str()),
            _ => return,
        };
        self.reference(target, ty.span, Some(name), None, false);
    }

    fn definition(
        &mut self,
        id: SemanticSymbolId,
        name: &str,
        kind: SemanticSymbolKind,
        approximate_span: Span,
        ty: Option<SemanticType>,
        container: Option<SemanticSymbolId>,
    ) {
        let span = self
            .identifier_span(approximate_span, name, false)
            .unwrap_or(approximate_span);
        self.index.definitions.push(SemanticSymbolDefinition {
            id,
            name: name.to_string(),
            kind,
            span,
            declaration_span: approximate_span,
            ty,
            container,
        });
    }

    fn reference(
        &mut self,
        target: SemanticSymbolId,
        approximate_span: Span,
        name: Option<&str>,
        ty: Option<SemanticType>,
        write: bool,
    ) {
        let span = name
            .and_then(|name| self.identifier_span(approximate_span, name, true))
            .unwrap_or(approximate_span);
        self.index.references.push(SemanticSymbolReference {
            target,
            span,
            ty,
            write,
        });
    }

    fn identifier_span(&self, span: Span, name: &str, from_end: bool) -> Option<Span> {
        let tokens = self.tokens.get(&span.source_id)?;
        let mut matching = tokens.iter().filter(|token| {
            token.kind == TokenKind::Identifier
                && token.text == name
                && token.span.start >= span.start
                && token.span.end <= span.end
        });
        if from_end {
            matching.next_back().map(|token| token.span)
        } else {
            matching.next().map(|token| token.span)
        }
    }
}

fn span_contains(span: Span, source_id: SourceId, offset: usize) -> bool {
    span.source_id == source_id && offset >= span.start && offset <= span.end
}

#[cfg(test)]
mod tests {
    use super::{SemanticSymbolId, build_semantic_index};
    use crate::{SourceFile, SourceId, SourceMap, analyze_script, lower_to_hir, parse_source};

    #[test]
    fn typed_index_resolves_shadowed_locals_and_struct_fields()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = br#"
struct Pair { int left; int right; };
struct Pair MakePair();
void main() {
    struct Pair pair = MakePair();
    int value = pair.left;
    { int value = pair.right; value = value + 1; }
    value = value + 1;
}
"#;
        let source_id = SourceId::new(0);
        let file = SourceFile::new(source_id, "main", source.to_vec());
        let script = parse_source(&file, None)?;
        let semantic = analyze_script(&script, None)?;
        let hir = lower_to_hir(&script, &semantic, None)?;
        let mut source_map = SourceMap::new();
        source_map.insert_file(file);
        let index = build_semantic_index(&script, &semantic, &hir, None, &source_map);

        let left = SemanticSymbolId::Field {
            owner: "Pair".to_string(),
            name:  "left".to_string(),
        };
        let right = SemanticSymbolId::Field {
            owner: "Pair".to_string(),
            name:  "right".to_string(),
        };
        assert_eq!(index.references_for(&left).count(), 1);
        assert_eq!(index.references_for(&right).count(), 1);

        let value_definitions = index
            .definitions
            .iter()
            .filter(|definition| definition.name == "value")
            .collect::<Vec<_>>();
        assert_eq!(value_definitions.len(), 2);
        let outer = value_definitions.first().expect("outer value definition");
        let inner = value_definitions.get(1).expect("inner value definition");
        assert_ne!(outer.id, inner.id);
        assert_eq!(index.references_for(&outer.id).count(), 2);
        assert_eq!(index.references_for(&inner.id).count(), 2);
        Ok(())
    }
}
