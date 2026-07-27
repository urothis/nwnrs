use std::collections::BTreeMap;

use nwnrs_nwscript::{
    BuiltinFunction, BuiltinType, CompilerAnalysis, FunctionDecl, LangSpec, Lexer,
    SemanticSymbolDefinition, SemanticSymbolId, SemanticSymbolKind, SemanticType, SourceBundle,
    SourceFile, Span, Token, TokenKind, TopLevelItem, TypeKind, TypeSpec,
};

use crate::NwScriptSourceRange;

/// Editor completion category derived from a compiler symbol.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum NwScriptCompletionKind {
    /// A callable function.
    Function,
    /// A local or global variable.
    Variable,
    /// A named constant.
    Constant,
    /// A function parameter.
    Parameter,
    /// A structure field.
    Field,
    /// A structure, alias, or engine-defined type.
    Type,
    /// An enum type.
    Enum,
    /// An enum variant.
    EnumMember,
    /// A compiler macro.
    Macro,
}

/// One ready-to-present NWScript completion.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NwScriptCompletion {
    /// User-facing candidate name.
    pub label:         String,
    /// Compact declaration detail.
    pub detail:        String,
    /// Source documentation when available.
    pub documentation: Option<String>,
    /// Text inserted when the candidate is accepted.
    pub insert_text:   String,
    /// Stable editor sort key.
    pub sort_text:     String,
    /// Source range replaced by the insertion.
    pub replacement:   NwScriptSourceRange,
    /// Candidate category.
    pub kind:          NwScriptCompletionKind,
}

/// One function parameter displayed by signature help.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NwScriptSignatureParameter {
    /// Parameter declaration label.
    pub label:         String,
    /// Parameter documentation when available.
    pub documentation: Option<String>,
}

/// One callable signature displayed by signature help.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NwScriptSignature {
    /// Complete callable declaration.
    pub label:         String,
    /// Function documentation when available.
    pub documentation: Option<String>,
    /// Parameters in declaration order.
    pub parameters:    Vec<NwScriptSignatureParameter>,
}

/// Compiler-owned signature-help response.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NwScriptSignatureHelp {
    /// Matching callable overloads.
    pub signatures:       Vec<NwScriptSignature>,
    /// Initially selected signature.
    pub active_signature: usize,
    /// Argument position at the cursor.
    pub active_parameter: usize,
}

pub(crate) fn completions_from_analysis(
    analysis: &CompilerAnalysis,
    line: usize,
    column: usize,
) -> Result<Vec<NwScriptCompletion>, String> {
    let source = root_source(analysis)?;
    let offset = source_offset(source, line, column);
    let tokens = Lexer::new(source.id, source.bytes())
        .lex_recovering()
        .map_err(|error| error.to_string())?
        .tokens;
    let (replacement_span, prefix) = completion_prefix(source, offset);
    let member = member_context(&tokens, replacement_span.start, &analysis.index);
    let current_function = analysis.script.items.iter().find_map(|item| {
        let TopLevelItem::Function(function) = item else {
            return None;
        };
        (function.span.source_id == source.id
            && offset >= function.span.start
            && offset <= function.span.end)
            .then_some(function)
    });

    let mut candidates = BTreeMap::<(String, NwScriptCompletionKind), NwScriptCompletion>::new();
    for definition in &analysis.index.definitions {
        if !definition.name.starts_with(&prefix)
            || !definition_visible(
                definition,
                current_function,
                source.id,
                offset,
                member.as_ref(),
            )
        {
            continue;
        }
        let kind = completion_kind(definition.kind);
        let detail = definition_detail(analysis, definition);
        let priority = completion_priority(kind);
        candidates
            .entry((definition.name.clone(), kind))
            .or_insert_with(|| NwScriptCompletion {
                label: definition.name.clone(),
                detail,
                documentation: definition_documentation(analysis, definition),
                insert_text: definition.name.clone(),
                sort_text: format!("{priority:02}-{}", definition.name),
                replacement: source_range(source, replacement_span),
                kind,
            });
    }

    // Macros are erased before typed HIR. Their declarations still come from
    // the compiler's canonical recovering macro index.
    if member.is_none() {
        for file in analysis.bundle.source_map.iter() {
            for definition in nwnrs_nwscript::recover_macro_definitions(file) {
                if !definition.name.starts_with(&prefix) {
                    continue;
                }
                let kind = NwScriptCompletionKind::Macro;
                let label = definition.namespace.as_ref().map_or_else(
                    || definition.name.clone(),
                    |namespace| format!("{namespace}::{}", definition.name),
                );
                candidates
                    .entry((label.clone(), kind))
                    .or_insert_with(|| NwScriptCompletion {
                        label: label.clone(),
                        detail: "compiler macro".to_string(),
                        documentation: None,
                        insert_text: label.clone(),
                        sort_text: format!("{:02}-{label}", completion_priority(kind)),
                        replacement: source_range(source, replacement_span),
                        kind,
                    });
            }
        }
    }

    Ok(candidates.into_values().collect())
}

pub(crate) fn signature_help_from_analysis(
    analysis: &CompilerAnalysis,
    line: usize,
    column: usize,
) -> Result<Option<NwScriptSignatureHelp>, String> {
    let source = root_source(analysis)?;
    let offset = source_offset(source, line, column);
    let tokens = Lexer::new(source.id, source.bytes())
        .lex_recovering()
        .map_err(|error| error.to_string())?
        .tokens;
    let Some((callee, active_parameter)) = active_call(&tokens, offset) else {
        return Ok(None);
    };
    let mut signatures = Vec::new();
    for item in &analysis.script.items {
        let TopLevelItem::Function(function) = item else {
            continue;
        };
        if function.name == callee {
            signatures.push(signature_from_function(
                function,
                definition_documentation_for_span(analysis, function.span, &function.name),
            ));
        }
    }
    if let Some(function) = analysis
        .langspec
        .functions
        .iter()
        .find(|function| function.name == callee)
    {
        signatures.push(signature_from_builtin(function));
    }
    signatures.sort_by(|left, right| left.label.cmp(&right.label));
    signatures.dedup_by(|left, right| left.label == right.label);
    if signatures.is_empty() {
        return Ok(None);
    }
    Ok(Some(NwScriptSignatureHelp {
        signatures,
        active_signature: 0,
        active_parameter,
    }))
}

pub(crate) fn completions_from_recovery(
    bundle: &SourceBundle,
    langspec: &LangSpec,
    line: usize,
    column: usize,
) -> Result<Vec<NwScriptCompletion>, String> {
    let source = bundle
        .source_map
        .get(bundle.root_id)
        .ok_or_else(|| "compiler recovery is missing its root source".to_string())?;
    let offset = source_offset(source, line, column);
    let (replacement_span, prefix) = completion_prefix(source, offset);
    let tokens = Lexer::new(source.id, source.bytes())
        .lex_recovering()
        .map_err(|error| error.to_string())?
        .tokens;
    let enum_owner = recovery_enum_owner(&tokens, replacement_span.start);
    let recovered = bundle
        .source_map
        .iter()
        .flat_map(|file| nwnrs_nwscript::recover_source_definitions(file, Some(langspec)))
        .collect::<Vec<_>>();
    let current_function = recovered
        .iter()
        .find(|definition| {
            definition.kind == SemanticSymbolKind::Function
                && definition.declaration_span.source_id == source.id
                && offset >= definition.declaration_span.start
                && offset <= definition.declaration_span.end
        })
        .map(|definition| definition.id.clone());
    let mut candidates = BTreeMap::<(String, NwScriptCompletionKind), NwScriptCompletion>::new();
    for definition in recovered {
        if !definition.name.starts_with(&prefix)
            || !recovered_definition_visible(
                &definition,
                current_function.as_ref(),
                source.id,
                offset,
                enum_owner.as_deref(),
            )
        {
            continue;
        }
        let Some(definition_source) = bundle.source_map.get(definition.span.source_id) else {
            continue;
        };
        let kind = completion_kind(definition.kind);
        let detail = definition_source
            .span_bytes(definition.declaration_span)
            .map(|bytes| {
                nwnrs_types::encoding::NwnText::decode(
                    bytes,
                    nwnrs_types::encoding::LegacyEncoding::Windows1252,
                )
                .display_text()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
            })
            .unwrap_or_else(|| format!("{:?}", definition.kind));
        let documentation = crate::symbols::preceding_slash_documentation(
            definition_source.bytes(),
            definition.declaration_span.start,
        );
        let priority = completion_priority(kind);
        candidates
            .entry((definition.name.clone(), kind))
            .or_insert_with(|| NwScriptCompletion {
                label: definition.name.clone(),
                detail,
                documentation,
                insert_text: definition.name.clone(),
                sort_text: format!("{priority:02}-{}", definition.name),
                replacement: source_range(source, replacement_span),
                kind,
            });
    }
    if enum_owner.is_none() {
        append_recovered_macro_completions(
            &mut candidates,
            bundle,
            source,
            replacement_span,
            &prefix,
        );
        append_builtin_completions(&mut candidates, langspec, source, replacement_span, &prefix);
    }
    Ok(candidates.into_values().collect())
}

pub(crate) fn signature_help_from_recovery(
    bundle: &SourceBundle,
    langspec: &LangSpec,
    line: usize,
    column: usize,
) -> Result<Option<NwScriptSignatureHelp>, String> {
    let source = bundle
        .source_map
        .get(bundle.root_id)
        .ok_or_else(|| "compiler recovery is missing its root source".to_string())?;
    let offset = source_offset(source, line, column);
    let tokens = Lexer::new(source.id, source.bytes())
        .lex_recovering()
        .map_err(|error| error.to_string())?
        .tokens;
    let Some((callee, active_parameter)) = active_call(&tokens, offset) else {
        return Ok(None);
    };
    let mut signatures = bundle
        .source_map
        .iter()
        .flat_map(|file| {
            nwnrs_nwscript::recover_function_signatures(file, Some(langspec))
                .into_iter()
                .filter(|function| function.name == callee)
                .map(move |function| {
                    let parameters = function
                        .parameters
                        .into_iter()
                        .map(|parameter| NwScriptSignatureParameter {
                            label:         format!("{} {}", parameter.ty, parameter.name),
                            documentation: None,
                        })
                        .collect::<Vec<_>>();
                    let joined = parameters
                        .iter()
                        .map(|parameter| parameter.label.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    NwScriptSignature {
                        label: format!("{} {}({joined})", function.return_type, function.name),
                        documentation: crate::symbols::preceding_slash_documentation(
                            file.bytes(),
                            function.span.start,
                        ),
                        parameters,
                    }
                })
        })
        .collect::<Vec<_>>();
    signatures.extend(
        langspec
            .functions
            .iter()
            .filter(|function| function.name == callee)
            .map(signature_from_builtin),
    );
    signatures.sort_by(|left, right| left.label.cmp(&right.label));
    signatures.dedup_by(|left, right| left.label == right.label);
    if signatures.is_empty() {
        return Ok(None);
    }
    Ok(Some(NwScriptSignatureHelp {
        signatures,
        active_signature: 0,
        active_parameter,
    }))
}

fn recovered_definition_visible(
    definition: &nwnrs_nwscript::RecoveredDefinition,
    current_function: Option<&SemanticSymbolId>,
    source_id: nwnrs_nwscript::SourceId,
    offset: usize,
    enum_owner: Option<&str>,
) -> bool {
    match &definition.id {
        SemanticSymbolId::Field {
            ..
        } => false,
        SemanticSymbolId::EnumVariant {
            owner, ..
        } => enum_owner.is_some_and(|expected| expected == owner),
        SemanticSymbolId::Local {
            ..
        } => {
            definition.container.as_ref() == current_function
                && (definition.span.source_id != source_id || definition.span.start <= offset)
        }
        _ => enum_owner.is_none(),
    }
}

fn recovery_enum_owner(tokens: &[Token], prefix_start: usize) -> Option<String> {
    let preceding = tokens
        .iter()
        .enumerate()
        .take_while(|(_, token)| token.span.end <= prefix_start)
        .map(|(index, _)| index)
        .last()?;
    if tokens.get(preceding)?.kind != TokenKind::Colon
        || tokens.get(preceding.checked_sub(1)?)?.kind != TokenKind::Colon
    {
        return None;
    }
    tokens
        .get(preceding.checked_sub(2)?)
        .filter(|token| token.kind == TokenKind::Identifier)
        .map(|token| token.text.clone())
}

fn append_recovered_macro_completions(
    candidates: &mut BTreeMap<(String, NwScriptCompletionKind), NwScriptCompletion>,
    bundle: &SourceBundle,
    source: &SourceFile,
    replacement_span: Span,
    prefix: &str,
) {
    for file in bundle.source_map.iter() {
        for definition in nwnrs_nwscript::recover_macro_definitions(file) {
            let label = definition.namespace.as_ref().map_or_else(
                || definition.name.clone(),
                |namespace| format!("{namespace}::{}", definition.name),
            );
            if !label.starts_with(prefix) {
                continue;
            }
            let kind = NwScriptCompletionKind::Macro;
            candidates
                .entry((label.clone(), kind))
                .or_insert_with(|| NwScriptCompletion {
                    label: label.clone(),
                    detail: "compiler macro".to_string(),
                    documentation: crate::symbols::preceding_slash_documentation(
                        file.bytes(),
                        definition.declaration_span.start,
                    ),
                    insert_text: label.clone(),
                    sort_text: format!("{:02}-{label}", completion_priority(kind)),
                    replacement: source_range(source, replacement_span),
                    kind,
                });
        }
    }
}

fn append_builtin_completions(
    candidates: &mut BTreeMap<(String, NwScriptCompletionKind), NwScriptCompletion>,
    langspec: &LangSpec,
    source: &SourceFile,
    replacement_span: Span,
    prefix: &str,
) {
    for function in &langspec.functions {
        if !function.name.starts_with(prefix) {
            continue;
        }
        let signature = signature_from_builtin(function);
        let kind = NwScriptCompletionKind::Function;
        candidates
            .entry((function.name.clone(), kind))
            .or_insert_with(|| NwScriptCompletion {
                label: function.name.clone(),
                detail: signature.label,
                documentation: signature.documentation,
                insert_text: function.name.clone(),
                sort_text: format!("{:02}-{}", completion_priority(kind), function.name),
                replacement: source_range(source, replacement_span),
                kind,
            });
    }
    for constant in &langspec.constants {
        if !constant.name.starts_with(prefix) {
            continue;
        }
        let kind = NwScriptCompletionKind::Constant;
        candidates
            .entry((constant.name.clone(), kind))
            .or_insert_with(|| NwScriptCompletion {
                label: constant.name.clone(),
                detail: format!("{} {}", builtin_type_label(&constant.ty), constant.name),
                documentation: None,
                insert_text: constant.name.clone(),
                sort_text: format!("{:02}-{}", completion_priority(kind), constant.name),
                replacement: source_range(source, replacement_span),
                kind,
            });
    }
    for name in &langspec.engine_structures {
        if !name.starts_with(prefix) {
            continue;
        }
        let kind = NwScriptCompletionKind::Type;
        candidates
            .entry((name.clone(), kind))
            .or_insert_with(|| NwScriptCompletion {
                label: name.clone(),
                detail: "engine structure".to_string(),
                documentation: None,
                insert_text: name.clone(),
                sort_text: format!("{:02}-{name}", completion_priority(kind)),
                replacement: source_range(source, replacement_span),
                kind,
            });
    }
}

fn root_source(analysis: &CompilerAnalysis) -> Result<&SourceFile, String> {
    analysis
        .bundle
        .source_map
        .get(analysis.bundle.root_id)
        .ok_or_else(|| "compiler analysis is missing its root source".to_string())
}

fn completion_prefix(source: &SourceFile, offset: usize) -> (Span, String) {
    let mut start = offset.min(source.len());
    while start > 0
        && source
            .bytes()
            .get(start - 1)
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
    {
        start -= 1;
    }
    let prefix = source
        .bytes()
        .get(start..offset.min(source.len()))
        .unwrap_or_default()
        .iter()
        .copied()
        .map(char::from)
        .collect();
    (
        Span::new(source.id, start, offset.min(source.len())),
        prefix,
    )
}

#[derive(Clone)]
enum MemberContext {
    Struct(String),
    Enum(String),
}

fn member_context(
    tokens: &[Token],
    prefix_start: usize,
    index: &nwnrs_nwscript::SemanticIndex,
) -> Option<MemberContext> {
    let preceding = tokens
        .iter()
        .enumerate()
        .take_while(|(_, token)| token.span.end <= prefix_start)
        .map(|(index, _)| index)
        .last()?;
    let operator = tokens.get(preceding)?;
    let (qualifier_index, enum_operator) = if operator.kind == TokenKind::StructurePartSpecify {
        (preceding.checked_sub(1)?, false)
    } else if operator.kind == TokenKind::Colon
        && tokens
            .get(preceding.checked_sub(1)?)
            .is_some_and(|token| token.kind == TokenKind::Colon)
    {
        (preceding.checked_sub(2)?, true)
    } else {
        return None;
    };
    let qualifier = tokens.get(qualifier_index)?;
    let target = index.symbol_at(qualifier.span.source_id, qualifier.span.start)?;
    if enum_operator {
        return match target {
            SemanticSymbolId::Enum(name) => Some(MemberContext::Enum(name.clone())),
            _ => None,
        };
    }
    index
        .definitions_for(target)
        .find_map(|definition| match &definition.ty {
            Some(SemanticType::Struct(name)) => Some(MemberContext::Struct(name.clone())),
            Some(SemanticType::Enum {
                name, ..
            }) => Some(MemberContext::Enum(name.clone())),
            _ => None,
        })
}

fn definition_visible(
    definition: &SemanticSymbolDefinition,
    current_function: Option<&FunctionDecl>,
    source_id: nwnrs_nwscript::SourceId,
    offset: usize,
    member: Option<&MemberContext>,
) -> bool {
    if let Some(member) = member {
        return matches!(
            (member, &definition.id),
            (
                MemberContext::Struct(expected),
                SemanticSymbolId::Field { owner, .. }
            ) if expected == owner
        ) || matches!(
            (member, &definition.id),
            (
                MemberContext::Enum(expected),
                SemanticSymbolId::EnumVariant { owner, .. }
            ) if expected == owner
        );
    }
    match &definition.id {
        SemanticSymbolId::Field {
            ..
        }
        | SemanticSymbolId::EnumVariant {
            ..
        } => false,
        SemanticSymbolId::Local {
            function_source,
            function_start,
            ..
        } => current_function.is_some_and(|function| {
            *function_source == function.span.source_id
                && *function_start == function.span.start
                && (definition.span.source_id != source_id || definition.span.start <= offset)
        }),
        _ => true,
    }
}

fn completion_kind(kind: SemanticSymbolKind) -> NwScriptCompletionKind {
    match kind {
        SemanticSymbolKind::Function | SemanticSymbolKind::BuiltinFunction => {
            NwScriptCompletionKind::Function
        }
        SemanticSymbolKind::Global | SemanticSymbolKind::Local => NwScriptCompletionKind::Variable,
        SemanticSymbolKind::Constant | SemanticSymbolKind::BuiltinConstant => {
            NwScriptCompletionKind::Constant
        }
        SemanticSymbolKind::Parameter => NwScriptCompletionKind::Parameter,
        SemanticSymbolKind::Field => NwScriptCompletionKind::Field,
        SemanticSymbolKind::Struct
        | SemanticSymbolKind::TypeAlias
        | SemanticSymbolKind::EngineStructure => NwScriptCompletionKind::Type,
        SemanticSymbolKind::Enum => NwScriptCompletionKind::Enum,
        SemanticSymbolKind::EnumVariant => NwScriptCompletionKind::EnumMember,
    }
}

const fn completion_priority(kind: NwScriptCompletionKind) -> usize {
    match kind {
        NwScriptCompletionKind::Parameter => 0,
        NwScriptCompletionKind::Variable => 1,
        NwScriptCompletionKind::Constant => 2,
        NwScriptCompletionKind::Function => 3,
        NwScriptCompletionKind::Field | NwScriptCompletionKind::EnumMember => 4,
        NwScriptCompletionKind::Type | NwScriptCompletionKind::Enum => 5,
        NwScriptCompletionKind::Macro => 6,
    }
}

fn definition_detail(analysis: &CompilerAnalysis, definition: &SemanticSymbolDefinition) -> String {
    if definition.span.source_id == nwnrs_nwscript::SourceId::new(u32::MAX) {
        return format!("{:?}", definition.kind);
    }
    analysis
        .bundle
        .source_map
        .get(definition.declaration_span.source_id)
        .and_then(|source| source.span_bytes(definition.declaration_span))
        .map(|bytes| {
            nwnrs_types::encoding::NwnText::decode(
                bytes,
                nwnrs_types::encoding::LegacyEncoding::Windows1252,
            )
            .display_text()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
        })
        .unwrap_or_else(|| format!("{:?}", definition.kind))
}

fn definition_documentation(
    analysis: &CompilerAnalysis,
    definition: &SemanticSymbolDefinition,
) -> Option<String> {
    definition_documentation_for_span(analysis, definition.declaration_span, &definition.name)
}

fn definition_documentation_for_span(
    analysis: &CompilerAnalysis,
    span: Span,
    _name: &str,
) -> Option<String> {
    let source = analysis.bundle.source_map.get(span.source_id)?;
    crate::symbols::preceding_slash_documentation(source.bytes(), span.start)
}

fn source_range(source: &SourceFile, span: Span) -> NwScriptSourceRange {
    let start = source
        .location(span.start)
        .unwrap_or(nwnrs_nwscript::SourceLocation {
            offset: 0,
            line:   1,
            column: 1,
        });
    let end = source.location(span.end).unwrap_or(start);
    NwScriptSourceRange {
        start_line:   start.line,
        start_column: start.column,
        end_line:     end.line,
        end_column:   end.column,
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

fn active_call(tokens: &[Token], offset: usize) -> Option<(String, usize)> {
    let mut stack = Vec::<(usize, usize)>::new();
    for (index, token) in tokens
        .iter()
        .enumerate()
        .take_while(|(_, token)| token.span.start < offset)
    {
        match token.kind {
            TokenKind::LeftParen => stack.push((index, 0)),
            TokenKind::Comma => {
                if let Some((_, arguments)) = stack.last_mut() {
                    *arguments = arguments.saturating_add(1);
                }
            }
            TokenKind::RightParen => {
                stack.pop();
            }
            _ => {}
        }
    }
    let (left, active) = stack.last().copied()?;
    let callee = tokens.get(left.checked_sub(1)?)?;
    (callee.kind == TokenKind::Identifier).then(|| (callee.text.clone(), active))
}

fn signature_from_function(
    function: &FunctionDecl,
    documentation: Option<String>,
) -> NwScriptSignature {
    let parameters = function
        .parameters
        .iter()
        .map(|parameter| NwScriptSignatureParameter {
            label:         format!("{} {}", type_label(&parameter.ty), parameter.name),
            documentation: None,
        })
        .collect::<Vec<_>>();
    let joined = parameters
        .iter()
        .map(|parameter| parameter.label.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    NwScriptSignature {
        label: format!(
            "{} {}({joined})",
            type_label(&function.return_type),
            function.name
        ),
        documentation,
        parameters,
    }
}

fn signature_from_builtin(function: &BuiltinFunction) -> NwScriptSignature {
    let parameters = function
        .parameters
        .iter()
        .map(|parameter| NwScriptSignatureParameter {
            label:         format!("{} {}", builtin_type_label(&parameter.ty), parameter.name),
            documentation: None,
        })
        .collect::<Vec<_>>();
    let parameter_text = parameters
        .iter()
        .map(|parameter| parameter.label.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    NwScriptSignature {
        label: format!(
            "{} {}({parameter_text})",
            builtin_type_label(&function.return_type),
            function.name
        ),
        documentation: None,
        parameters,
    }
}

fn type_label(ty: &TypeSpec) -> String {
    let value = match &ty.kind {
        TypeKind::Void => "void".to_string(),
        TypeKind::Int => "int".to_string(),
        TypeKind::Float => "float".to_string(),
        TypeKind::String => "string".to_string(),
        TypeKind::Object => "object".to_string(),
        TypeKind::Vector => "vector".to_string(),
        TypeKind::Struct(name) => format!("struct {name}"),
        TypeKind::EngineStructure(name) | TypeKind::Named(name) => name.clone(),
    };
    if ty.is_const {
        format!("const {value}")
    } else {
        value
    }
}

fn builtin_type_label(ty: &BuiltinType) -> &str {
    match ty {
        BuiltinType::Int => "int",
        BuiltinType::Float => "float",
        BuiltinType::String => "string",
        BuiltinType::Object => "object",
        BuiltinType::Void => "void",
        BuiltinType::Action => "action",
        BuiltinType::Vector => "vector",
        BuiltinType::EngineStructure(name) => name,
    }
}

#[cfg(test)]
mod tests {
    use nwnrs_nwscript::{CompilerResourceLimits, SourceBundle, SourceMap, parse_langspec_bytes};

    use super::{completions_from_recovery, signature_help_from_recovery};

    fn recovery_fixture() -> (SourceBundle, nwnrs_nwscript::LangSpec) {
        let mut source_map = SourceMap::new();
        let root_id = source_map.add_file(
            "main.nss",
            br#"enum Mode : int { Ready, Waiting };
void Helper(int first, string second) {}
void main() {
    int localValue;
    MissingSymbol();
    PrintString("incomplete");
    Mode value = Mode::Ready;
    Helper(localValue, "ok");
}
"#,
        );
        let macro_id = source_map.add_file("macros.nss", b"#define LOCAL_MACRO 1\n".to_vec());
        let langspec = parse_langspec_bytes(
            "nwscript.nss",
            br#"#define ENGINE_NUM_STRUCTURES 0
int TRUE = 1;
void PrintString(string sMessage);
"#,
        )
        .expect("parse test langspec");
        (
            SourceBundle {
                source_map,
                root_id,
                source_order: vec![root_id, macro_id],
                include_edges: Vec::new(),
                resource_limits: CompilerResourceLimits::default(),
            },
            langspec,
        )
    }

    #[test]
    fn recovery_completions_include_builtins_locals_enums_and_macros() {
        let (bundle, langspec) = recovery_fixture();

        let builtins =
            completions_from_recovery(&bundle, &langspec, 6, 10).expect("builtin completions");
        assert!(builtins.iter().any(|item| item.label == "PrintString"));

        let locals =
            completions_from_recovery(&bundle, &langspec, 8, 17).expect("local completions");
        assert!(locals.iter().any(|item| item.label == "localValue"));

        let variants =
            completions_from_recovery(&bundle, &langspec, 7, 27).expect("enum completions");
        assert!(variants.iter().any(|item| item.label == "Ready"));

        let macros =
            completions_from_recovery(&bundle, &langspec, 1, 1).expect("macro completions");
        assert!(macros.iter().any(|item| item.label == "LOCAL_MACRO"));
    }

    #[test]
    fn recovery_signature_help_retains_user_and_builtin_callables() {
        let (bundle, langspec) = recovery_fixture();
        let helper = signature_help_from_recovery(&bundle, &langspec, 8, 23)
            .expect("signature recovery")
            .expect("helper signature");
        assert_eq!(helper.active_parameter, 1);
        assert_eq!(
            helper.signatures.first().expect("helper signature").label,
            "void Helper(int first, string second)"
        );

        let builtin = signature_help_from_recovery(&bundle, &langspec, 6, 17)
            .expect("builtin signature recovery")
            .expect("builtin signature");
        assert_eq!(
            builtin.signatures.first().expect("builtin signature").label,
            "void PrintString(string sMessage)"
        );
    }
}
