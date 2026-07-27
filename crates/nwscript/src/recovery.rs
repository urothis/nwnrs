use crate::{
    LangSpec, Lexer, Script, SemanticSymbolId, SemanticSymbolKind, SourceFile, Span, Token,
    TokenKind, TopLevelItem, parse_tokens,
};

/// One declaration recovered by the canonical compiler parser from an
/// incomplete source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveredDefinition {
    /// Stable compiler symbol identity.
    pub id:                SemanticSymbolId,
    /// Declared source name.
    pub name:              String,
    /// Compiler symbol category.
    pub kind:              SemanticSymbolKind,
    /// Exact identifier span.
    pub span:              Span,
    /// Full declaration span.
    pub declaration_span:  Span,
    /// Owning function, structure, or enum.
    pub container:         Option<SemanticSymbolId>,
    /// Whether this is an implementation rather than a declaration.
    pub is_implementation: bool,
}

/// One macro declaration recovered from the compiler token stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveredMacroDefinition {
    /// Final callable macro name.
    pub name:             String,
    /// Optional `proc_macro!` namespace.
    pub namespace:        Option<String>,
    /// Exact name span.
    pub span:             Span,
    /// Complete declaration or define-line span.
    pub declaration_span: Span,
}

/// One engine-structure type declared by `ENGINE_STRUCTURE_N`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveredEngineStructureDefinition {
    /// Engine structure type name.
    pub name:             String,
    /// Exact type-name span.
    pub span:             Span,
    /// Complete definition-line span.
    pub declaration_span: Span,
}

/// One recovered function parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveredFunctionParameter {
    /// Parameter name.
    pub name: String,
    /// Canonical source type label.
    pub ty:   String,
}

/// One callable signature recovered by the ordinary compiler parser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveredFunctionSignature {
    /// Function name.
    pub name:        String,
    /// Canonical return type label.
    pub return_type: String,
    /// Parameters in declaration order.
    pub parameters:  Vec<RecoveredFunctionParameter>,
    /// Full declaration or implementation span.
    pub span:        Span,
}

/// Recovers declarations from every independently parseable top-level region.
///
/// The recovering lexer supplies the canonical token vocabulary. Complete
/// sources are parsed once normally. If the full parse fails, this function
/// partitions only at balanced top-level declaration boundaries and sends
/// every partition back through the ordinary parser. It never reimplements
/// declaration grammar.
#[must_use]
pub fn recover_source_definitions(
    source: &SourceFile,
    langspec: Option<&LangSpec>,
) -> Vec<RecoveredDefinition> {
    let mut output = Vec::new();
    for (script, tokens) in recover_parsed_scripts(source, langspec) {
        output.extend(definitions_from_script(&script, &tokens));
    }
    output.sort_by_key(|definition| {
        (
            definition.span.source_id,
            definition.span.start,
            definition.span.end,
        )
    });
    output.dedup_by(|left, right| left.id == right.id && left.span == right.span);
    output
}

/// Recovers callable signatures from every independently parseable source
/// region.
#[must_use]
pub fn recover_function_signatures(
    source: &SourceFile,
    langspec: Option<&LangSpec>,
) -> Vec<RecoveredFunctionSignature> {
    let mut output = Vec::new();
    for (script, _) in recover_parsed_scripts(source, langspec) {
        output.extend(script.items.iter().filter_map(|item| {
            let TopLevelItem::Function(function) = item else {
                return None;
            };
            Some(RecoveredFunctionSignature {
                name:        function.name.clone(),
                return_type: recovered_type_label(&function.return_type),
                parameters:  function
                    .parameters
                    .iter()
                    .map(|parameter| RecoveredFunctionParameter {
                        name: parameter.name.clone(),
                        ty:   recovered_type_label(&parameter.ty),
                    })
                    .collect(),
                span:        function.span,
            })
        }));
    }
    output.sort_by_key(|function| {
        (
            function.span.source_id,
            function.span.start,
            function.span.end,
        )
    });
    output.dedup_by(|left, right| left.name == right.name && left.span == right.span);
    output
}

/// Recovers object-like defines and compiler macro declarations.
///
/// This uses the same compiler token vocabulary and path shape accepted by
/// macro collection. Malformed declarations are omitted and remain the
/// responsibility of normal compiler diagnostics.
#[must_use]
pub fn recover_macro_definitions(source: &SourceFile) -> Vec<RecoveredMacroDefinition> {
    let Ok(lexed) = Lexer::new(source.id, source.bytes()).lex_recovering() else {
        return Vec::new();
    };
    let tokens = lexed.tokens;
    let mut output = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        let (name, namespace, declaration_end) =
            if matches!(token.kind, TokenKind::Keyword(crate::Keyword::Define)) {
                let Some(name) = tokens.get(index.saturating_add(1)) else {
                    continue;
                };
                let end = source
                    .bytes()
                    .get(token.span.start..)
                    .and_then(|tail| tail.iter().position(|byte| *byte == b'\n'))
                    .map_or(source.len(), |length| {
                        token.span.start.saturating_add(length)
                    });
                (name, None, end)
            } else if token.kind == TokenKind::Identifier
                && token.text == "macro_rules"
                && tokens
                    .get(index.saturating_add(1))
                    .is_some_and(|next| next.kind == TokenKind::BooleanNot)
            {
                let Some(name) = tokens.get(index.saturating_add(2)) else {
                    continue;
                };
                (
                    name,
                    None,
                    declaration_group_end(&tokens, index.saturating_add(3))
                        .unwrap_or(name.span.end),
                )
            } else if token.kind == TokenKind::Identifier
                && token.text == "proc_macro"
                && tokens
                    .get(index.saturating_add(1))
                    .is_some_and(|next| next.kind == TokenKind::BooleanNot)
            {
                let mut segments = Vec::new();
                let mut cursor = index.saturating_add(2);
                while let Some(segment) = tokens
                    .get(cursor)
                    .filter(|segment| segment.kind == TokenKind::Identifier)
                {
                    segments.push(segment);
                    if matches!(
                        (tokens.get(cursor + 1), tokens.get(cursor + 2)),
                        (Some(first), Some(second))
                            if first.kind == TokenKind::Colon
                                && second.kind == TokenKind::Colon
                    ) {
                        cursor = cursor.saturating_add(3);
                    } else {
                        break;
                    }
                }
                let Some(name) = segments.last().copied() else {
                    continue;
                };
                let namespace = (segments.len() > 1).then(|| {
                    segments
                        .get(..segments.len().saturating_sub(1))
                        .unwrap_or_default()
                        .iter()
                        .map(|segment| segment.text.as_str())
                        .collect::<Vec<_>>()
                        .join("::")
                });
                (
                    name,
                    namespace,
                    declaration_group_end(&tokens, cursor.saturating_add(1))
                        .unwrap_or(name.span.end),
                )
            } else {
                continue;
            };
        if name.kind != TokenKind::Identifier {
            continue;
        }
        output.push(RecoveredMacroDefinition {
            name: name.text.clone(),
            namespace,
            span: name.span,
            declaration_span: Span::new(token.span.source_id, token.span.start, declaration_end),
        });
    }
    output
}

/// Recovers engine-structure type definitions from a language-spec source.
///
/// The compiler lexer owns recognition of `ENGINE_STRUCTURE_N`; consumers do
/// not need to parse preprocessor-shaped source text independently.
#[must_use]
pub fn recover_engine_structure_definitions(
    source: &SourceFile,
) -> Vec<RecoveredEngineStructureDefinition> {
    let Ok(lexed) = Lexer::new(source.id, source.bytes()).lex_recovering() else {
        return Vec::new();
    };
    lexed
        .tokens
        .iter()
        .enumerate()
        .filter_map(|(index, token)| {
            if token.kind != TokenKind::Keyword(crate::Keyword::EngineStructureDefinition) {
                return None;
            }
            let name = lexed
                .tokens
                .get(index.saturating_add(1))
                .filter(|name| name.kind == TokenKind::Identifier)?;
            let line_start = source
                .bytes()
                .get(..token.span.start)
                .and_then(|prefix| prefix.iter().rposition(|byte| *byte == b'\n'))
                .map_or(0, |offset| offset.saturating_add(1));
            let line_end = source
                .bytes()
                .get(name.span.end..)
                .and_then(|tail| tail.iter().position(|byte| *byte == b'\n'))
                .map_or(source.len(), |offset| name.span.end.saturating_add(offset));
            Some(RecoveredEngineStructureDefinition {
                name:             name.text.clone(),
                span:             name.span,
                declaration_span: Span::new(source.id, line_start, line_end),
            })
        })
        .collect()
}

fn declaration_group_end(tokens: &[Token], start: usize) -> Option<usize> {
    let left = tokens
        .iter()
        .enumerate()
        .skip(start)
        .find(|(_, token)| token.kind == TokenKind::LeftBrace)?
        .0;
    let mut depth = 0_usize;
    for token in tokens.iter().skip(left) {
        match token.kind {
            TokenKind::LeftBrace => depth = depth.saturating_add(1),
            TokenKind::RightBrace => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(token.span.end);
                }
            }
            TokenKind::Eof => break,
            _ => {}
        }
    }
    None
}

fn recover_parsed_scripts(
    source: &SourceFile,
    langspec: Option<&LangSpec>,
) -> Vec<(Script, Vec<Token>)> {
    let Ok(lexed) = Lexer::new(source.id, source.bytes()).lex_recovering() else {
        return Vec::new();
    };
    if let Ok(script) = parse_tokens(lexed.tokens.clone(), langspec) {
        return vec![(script, lexed.tokens)];
    }

    let tokens = lexed
        .tokens
        .iter()
        .filter(|token| token.kind != TokenKind::Eof)
        .cloned()
        .collect::<Vec<_>>();
    top_level_partitions(&tokens)
        .into_iter()
        .filter_map(|(start, end)| {
            let partition = tokens.get(start..end)?;
            let eof_offset = partition.last().map_or(0, |token| token.span.end);
            let mut candidate = partition.to_vec();
            candidate.push(Token::new(
                TokenKind::Eof,
                Span::new(source.id, eof_offset, eof_offset),
                "",
            ));
            parse_tokens(candidate.clone(), langspec)
                .ok()
                .map(|script| (script, candidate))
        })
        .collect()
}

fn recovered_type_label(ty: &crate::TypeSpec) -> String {
    let name = match &ty.kind {
        crate::TypeKind::Void => "void".to_string(),
        crate::TypeKind::Int => "int".to_string(),
        crate::TypeKind::Float => "float".to_string(),
        crate::TypeKind::String => "string".to_string(),
        crate::TypeKind::Object => "object".to_string(),
        crate::TypeKind::Vector => "vector".to_string(),
        crate::TypeKind::Struct(name) => format!("struct {name}"),
        crate::TypeKind::EngineStructure(name) | crate::TypeKind::Named(name) => name.clone(),
    };
    if ty.is_const {
        format!("const {name}")
    } else {
        name
    }
}

fn top_level_partitions(tokens: &[Token]) -> Vec<(usize, usize)> {
    let mut output = Vec::new();
    let mut start = 0_usize;
    let mut parens = 0_usize;
    let mut brackets = 0_usize;
    let mut braces = 0_usize;
    for (index, token) in tokens.iter().enumerate() {
        match token.kind {
            TokenKind::LeftParen => parens = parens.saturating_add(1),
            TokenKind::RightParen => parens = parens.saturating_sub(1),
            TokenKind::LeftSquareBracket => brackets = brackets.saturating_add(1),
            TokenKind::RightSquareBracket => brackets = brackets.saturating_sub(1),
            TokenKind::LeftBrace => braces = braces.saturating_add(1),
            TokenKind::RightBrace => braces = braces.saturating_sub(1),
            _ => {}
        }
        let balanced = parens == 0 && brackets == 0 && braces == 0;
        let boundary =
            balanced && matches!(token.kind, TokenKind::Semicolon | TokenKind::RightBrace);
        if boundary {
            let mut end = index.saturating_add(1);
            if token.kind == TokenKind::RightBrace
                && tokens
                    .get(end)
                    .is_some_and(|next| next.kind == TokenKind::Semicolon)
            {
                end = end.saturating_add(1);
            }
            output.push((start, end));
            start = end;
        }
    }
    if start < tokens.len() {
        output.push((start, tokens.len()));
    }
    output
}

fn definitions_from_script(script: &Script, tokens: &[Token]) -> Vec<RecoveredDefinition> {
    let mut output = Vec::new();
    for item in &script.items {
        match item {
            TopLevelItem::Function(function) => {
                let owner = SemanticSymbolId::Function(function.name.clone());
                let mut local_index = 0_u32;
                push_definition(
                    &mut output,
                    tokens,
                    owner.clone(),
                    &function.name,
                    SemanticSymbolKind::Function,
                    function.span,
                    None,
                    function.body.is_some(),
                );
                for parameter in &function.parameters {
                    let local = crate::HirLocalId(local_index);
                    local_index = local_index.saturating_add(1);
                    push_definition(
                        &mut output,
                        tokens,
                        SemanticSymbolId::Local {
                            function_source: function.span.source_id,
                            function_start: function.span.start,
                            local,
                        },
                        &parameter.name,
                        SemanticSymbolKind::Parameter,
                        parameter.span,
                        Some(owner.clone()),
                        true,
                    );
                }
                if let Some(body) = &function.body {
                    recover_local_definitions(
                        &mut output,
                        tokens,
                        body,
                        &owner,
                        function.span,
                        &mut local_index,
                    );
                }
            }
            TopLevelItem::Global(declaration) => {
                for declarator in &declaration.declarators {
                    push_definition(
                        &mut output,
                        tokens,
                        SemanticSymbolId::Global(declarator.name.clone()),
                        &declarator.name,
                        if declaration.ty.is_const {
                            SemanticSymbolKind::Constant
                        } else {
                            SemanticSymbolKind::Global
                        },
                        declaration.span,
                        None,
                        true,
                    );
                }
            }
            TopLevelItem::Struct(structure) => {
                let owner = SemanticSymbolId::Struct(structure.name.clone());
                push_definition(
                    &mut output,
                    tokens,
                    owner.clone(),
                    &structure.name,
                    SemanticSymbolKind::Struct,
                    structure.span,
                    None,
                    true,
                );
                for field in structure.fields.iter().flat_map(|field| &field.names) {
                    push_definition(
                        &mut output,
                        tokens,
                        SemanticSymbolId::Field {
                            owner: structure.name.clone(),
                            name:  field.name.clone(),
                        },
                        &field.name,
                        SemanticSymbolKind::Field,
                        field.span,
                        Some(owner.clone()),
                        true,
                    );
                }
            }
            TopLevelItem::Enum(enumeration) => {
                let owner = SemanticSymbolId::Enum(enumeration.name.clone());
                push_definition(
                    &mut output,
                    tokens,
                    owner.clone(),
                    &enumeration.name,
                    SemanticSymbolKind::Enum,
                    enumeration.span,
                    None,
                    true,
                );
                for variant in &enumeration.variants {
                    push_definition(
                        &mut output,
                        tokens,
                        SemanticSymbolId::EnumVariant {
                            owner: enumeration.name.clone(),
                            name:  variant.name.clone(),
                        },
                        &variant.name,
                        SemanticSymbolKind::EnumVariant,
                        variant.span,
                        Some(owner.clone()),
                        true,
                    );
                    for alias in &variant.aliases {
                        push_definition(
                            &mut output,
                            tokens,
                            SemanticSymbolId::Global(alias.name.clone()),
                            &alias.name,
                            SemanticSymbolKind::Constant,
                            alias.span,
                            Some(owner.clone()),
                            true,
                        );
                    }
                }
            }
            TopLevelItem::TypeAlias(alias) => push_definition(
                &mut output,
                tokens,
                SemanticSymbolId::TypeAlias(alias.name.clone()),
                &alias.name,
                SemanticSymbolKind::TypeAlias,
                alias.span,
                None,
                true,
            ),
            TopLevelItem::Include(_) | TopLevelItem::StaticAssert(_) => {}
        }
    }
    output
}

fn recover_local_definitions(
    output: &mut Vec<RecoveredDefinition>,
    tokens: &[Token],
    block: &crate::BlockStmt,
    owner: &SemanticSymbolId,
    function_span: Span,
    local_index: &mut u32,
) {
    for statement in &block.statements {
        recover_local_statement(output, tokens, statement, owner, function_span, local_index);
    }
}

fn recover_local_statement(
    output: &mut Vec<RecoveredDefinition>,
    tokens: &[Token],
    statement: &crate::Stmt,
    owner: &SemanticSymbolId,
    function_span: Span,
    local_index: &mut u32,
) {
    match statement {
        crate::Stmt::Declaration(declaration) => {
            for declarator in &declaration.declarators {
                let local = crate::HirLocalId(*local_index);
                *local_index = local_index.saturating_add(1);
                push_definition(
                    output,
                    tokens,
                    SemanticSymbolId::Local {
                        function_source: function_span.source_id,
                        function_start: function_span.start,
                        local,
                    },
                    &declarator.name,
                    SemanticSymbolKind::Local,
                    declaration.span,
                    Some(owner.clone()),
                    true,
                );
            }
        }
        crate::Stmt::Block(block) => {
            recover_local_definitions(output, tokens, block, owner, function_span, local_index);
        }
        crate::Stmt::If(statement) => {
            recover_local_statement(
                output,
                tokens,
                &statement.then_branch,
                owner,
                function_span,
                local_index,
            );
            if let Some(branch) = &statement.else_branch {
                recover_local_statement(output, tokens, branch, owner, function_span, local_index);
            }
        }
        crate::Stmt::Switch(statement) => recover_local_statement(
            output,
            tokens,
            &statement.body,
            owner,
            function_span,
            local_index,
        ),
        crate::Stmt::While(statement) => recover_local_statement(
            output,
            tokens,
            &statement.body,
            owner,
            function_span,
            local_index,
        ),
        crate::Stmt::DoWhile(statement) => recover_local_statement(
            output,
            tokens,
            &statement.body,
            owner,
            function_span,
            local_index,
        ),
        crate::Stmt::For(statement) => recover_local_statement(
            output,
            tokens,
            &statement.body,
            owner,
            function_span,
            local_index,
        ),
        crate::Stmt::Expression(_)
        | crate::Stmt::Return(_)
        | crate::Stmt::Case(_)
        | crate::Stmt::Default(_)
        | crate::Stmt::Break(_)
        | crate::Stmt::Continue(_)
        | crate::Stmt::Empty(_)
        | crate::Stmt::StaticAssert(_) => {}
    }
}

fn push_definition(
    output: &mut Vec<RecoveredDefinition>,
    tokens: &[Token],
    id: SemanticSymbolId,
    name: &str,
    kind: SemanticSymbolKind,
    declaration_span: Span,
    container: Option<SemanticSymbolId>,
    is_implementation: bool,
) {
    let span = tokens
        .iter()
        .find(|token| {
            token.kind == TokenKind::Identifier
                && token.text == name
                && token.span.source_id == declaration_span.source_id
                && token.span.start >= declaration_span.start
                && token.span.end <= declaration_span.end
        })
        .map_or(declaration_span, |token| token.span);
    output.push(RecoveredDefinition {
        id,
        name: name.to_string(),
        kind,
        span,
        declaration_span,
        container,
        is_implementation,
    });
}

#[cfg(test)]
mod tests {
    use super::{recover_engine_structure_definitions, recover_source_definitions};
    use crate::{SemanticSymbolKind, SourceFile, SourceId};

    #[test]
    fn recovers_complete_declarations_before_an_incomplete_tail() {
        let source = SourceFile::new(
            SourceId::new(0),
            "main.nss",
            b"void Complete() {}\nenum Mode { First, Second };\nvoid Broken(\n".to_vec(),
        );
        let definitions = recover_source_definitions(&source, None);
        assert!(definitions.iter().any(|definition| {
            definition.name == "Complete" && definition.kind == SemanticSymbolKind::Function
        }));
        assert!(definitions.iter().any(|definition| {
            definition.name == "Mode" && definition.kind == SemanticSymbolKind::Enum
        }));
        assert!(definitions.iter().any(|definition| {
            definition.name == "Second" && definition.kind == SemanticSymbolKind::EnumVariant
        }));
        assert!(
            !definitions
                .iter()
                .any(|definition| definition.name == "Broken")
        );
    }

    #[test]
    fn recovers_engine_structure_types_from_compiler_tokens() {
        let source = SourceFile::new(
            SourceId::new(0),
            "nwscript.nss",
            b"#define ENGINE_STRUCTURE_0 effect\n#define ENGINE_STRUCTURE_1 json\n".to_vec(),
        );
        let definitions = recover_engine_structure_definitions(&source);
        assert_eq!(
            definitions
                .iter()
                .map(|definition| definition.name.as_str())
                .collect::<Vec<_>>(),
            ["effect", "json"]
        );
        assert_eq!(
            source
                .span_bytes(
                    definitions
                        .first()
                        .expect("first engine structure")
                        .declaration_span,
                )
                .expect("definition bytes"),
            b"#define ENGINE_STRUCTURE_0 effect"
        );
    }
}
