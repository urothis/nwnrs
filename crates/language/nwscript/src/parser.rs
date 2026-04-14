use std::{collections::HashSet, error::Error, fmt};

use crate::{
    AssignmentOp, BinaryOp, BlockStmt, CaseStmt, CompilerErrorCode, Declaration, DefaultStmt,
    DoWhileStmt, Expr, ExprKind, ExpressionStmt, ForStmt, FunctionDecl, IfStmt, IncludeDirective,
    Keyword, LangSpec, Literal, MagicLiteral, NamedItem, Parameter, ReturnStmt, Script, SimpleStmt,
    Span, Stmt, StructDecl, StructFieldDecl, SwitchStmt, Token, TokenKind, TopLevelItem, TypeKind,
    TypeSpec, UnaryOp, VarDeclarator, WhileStmt,
    int_literal::{parse_wrapping_decimal_i32, parse_wrapping_prefixed_i32},
    lexer::{LexerError, lex_source},
    preprocess::{PreprocessError, preprocess_source_bundle},
    source::{SourceFile, SourceId},
};

/// One parser error aligned to the upstream compiler's diagnostic space.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParserError {
    /// Stable upstream-aligned compiler error code.
    pub code:    CompilerErrorCode,
    /// Source span where parsing failed.
    pub span:    Span,
    /// Human-readable error message.
    pub message: String,
}

impl ParserError {
    fn new(code: CompilerErrorCode, span: Span, message: impl Into<String>) -> Self {
        Self {
            code,
            span,
            message: message.into(),
        }
    }
}

impl fmt::Display for ParserError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.message, self.code.code())
    }
}

impl Error for ParserError {}

/// One parser failure.
#[derive(Debug)]
pub enum ParseError {
    /// Lexing failed before parsing could begin.
    Lex(LexerError),
    /// Syntactic parsing failed.
    Parse(ParserError),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Lex(error) => error.fmt(f),
            Self::Parse(error) => error.fmt(f),
        }
    }
}

impl Error for ParseError {}

impl From<LexerError> for ParseError {
    fn from(value: LexerError) -> Self {
        Self::Lex(value)
    }
}

impl From<ParserError> for ParseError {
    fn from(value: ParserError) -> Self {
        Self::Parse(value)
    }
}

/// One parser failure after source resolution and preprocessing.
#[derive(Debug)]
pub enum ResolvedParseError {
    /// Source loading or preprocessing failed.
    Preprocess(PreprocessError),
    /// Parsing failed after preprocessing.
    Parse(ParserError),
}

impl fmt::Display for ResolvedParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Preprocess(error) => error.fmt(f),
            Self::Parse(error) => error.fmt(f),
        }
    }
}

impl Error for ResolvedParseError {}

impl From<PreprocessError> for ResolvedParseError {
    fn from(value: PreprocessError) -> Self {
        Self::Preprocess(value)
    }
}

impl From<ParserError> for ResolvedParseError {
    fn from(value: ParserError) -> Self {
        Self::Parse(value)
    }
}

/// Parses one already-tokenized `NWScript` translation unit.
pub fn parse_tokens(
    tokens: Vec<Token>,
    langspec: Option<&LangSpec>,
) -> Result<Script, ParserError> {
    Parser::new(tokens, langspec).parse_script()
}

/// Lexes and parses one source file.
pub fn parse_source(
    source: &SourceFile,
    langspec: Option<&LangSpec>,
) -> Result<Script, ParseError> {
    let tokens = lex_source(source)?;
    parse_tokens(tokens, langspec).map_err(ParseError::from)
}

/// Lexes and parses a byte buffer associated with `source_id`.
pub fn parse_bytes(
    source_id: SourceId,
    input: &[u8],
    langspec: Option<&LangSpec>,
) -> Result<Script, ParseError> {
    let tokens = crate::lex_bytes(source_id, input)?;
    parse_tokens(tokens, langspec).map_err(ParseError::from)
}

/// Lexes and parses a text buffer associated with `source_id`.
pub fn parse_text(
    source_id: SourceId,
    input: &str,
    langspec: Option<&LangSpec>,
) -> Result<Script, ParseError> {
    parse_bytes(source_id, input.as_bytes(), langspec)
}

/// Parses one already-loaded source bundle after include traversal and macro
/// expansion.
pub fn parse_source_bundle(
    bundle: &crate::SourceBundle,
    langspec: Option<&LangSpec>,
) -> Result<Script, ResolvedParseError> {
    let preprocessed = preprocess_source_bundle(bundle)?;
    parse_tokens(preprocessed.tokens, langspec).map_err(ResolvedParseError::from)
}

/// Resolves, preprocesses, and parses one named root script.
pub fn parse_resolved_script<R: crate::ScriptResolver + ?Sized>(
    resolver: &R,
    root_name: &str,
    options: crate::SourceLoadOptions,
    langspec: Option<&LangSpec>,
) -> Result<Script, ResolvedParseError> {
    let bundle = crate::load_source_bundle(resolver, root_name, options)?;
    parse_source_bundle(&bundle, langspec)
}

struct Parser<'a> {
    tokens:            Vec<Token>,
    position:          usize,
    engine_structures: HashSet<&'a str>,
}

impl<'a> Parser<'a> {
    fn new(tokens: Vec<Token>, langspec: Option<&'a LangSpec>) -> Self {
        let engine_structures = langspec
            .map(|spec| {
                spec.engine_structures
                    .iter()
                    .map(String::as_str)
                    .collect::<HashSet<_>>()
            })
            .unwrap_or_default();
        Self {
            tokens,
            position: 0,
            engine_structures,
        }
    }

    fn parse_script(mut self) -> Result<Script, ParserError> {
        let mut items = Vec::new();
        while !self.at_eof() {
            if self.matches_keyword(Keyword::Include) {
                items.push(TopLevelItem::Include(self.parse_include_directive()?));
                continue;
            }
            items.push(self.parse_top_level_item()?);
        }
        Ok(Script {
            items,
        })
    }

    fn parse_include_directive(&mut self) -> Result<IncludeDirective, ParserError> {
        let include = self.consume_keyword(
            Keyword::Include,
            CompilerErrorCode::UnknownStateInCompiler,
            "expected #include",
        )?;
        let path = self.consume_string(
            CompilerErrorCode::UnknownStateInCompiler,
            "expected string literal after #include",
        )?;
        Ok(IncludeDirective {
            span: join_spans(include.span, path.span),
            path: path.text,
        })
    }

    fn parse_top_level_item(&mut self) -> Result<TopLevelItem, ParserError> {
        let ty = self.parse_any_type_specifier()?;

        if matches!(ty.kind, TypeKind::Struct(_)) && self.matches_kind(&TokenKind::LeftBrace) {
            return self.parse_struct_definition(&ty).map(TopLevelItem::Struct);
        }

        let name = self.consume_identifier(
            CompilerErrorCode::FunctionDefinitionMissingName,
            "expected identifier after type specifier",
        )?;

        if self.matches_kind(&TokenKind::LeftParen) {
            return self
                .parse_function_declaration_or_definition(ty, name)
                .map(TopLevelItem::Function);
        }

        let declaration = self.parse_declaration_after_name(ty, name)?;
        Ok(TopLevelItem::Global(declaration))
    }

    fn parse_struct_definition(&mut self, ty: &TypeSpec) -> Result<StructDecl, ParserError> {
        let name = match &ty.kind {
            TypeKind::Struct(name) => name.clone(),
            _ => {
                return Err(ParserError::new(
                    CompilerErrorCode::BadTypeSpecifier,
                    ty.span,
                    "struct definition requires a named struct type specifier",
                ));
            }
        };

        self.consume_kind(
            TokenKind::LeftBrace,
            CompilerErrorCode::BadTypeSpecifier,
            "expected { after struct name",
        )?;

        let mut fields = Vec::new();
        while !self.matches_kind(&TokenKind::RightBrace) && !self.at_eof() {
            let field_type = self.parse_non_void_type_specifier()?;
            let names = self.parse_struct_field_names()?;
            let semicolon = self.consume_kind(
                TokenKind::Semicolon,
                CompilerErrorCode::NoSemicolonAfterExpression,
                "expected ; after struct field declaration",
            )?;
            fields.push(StructFieldDecl {
                span: join_spans(field_type.span, semicolon.span),
                ty: field_type,
                names,
            });
        }

        self.consume_kind(
            TokenKind::RightBrace,
            CompilerErrorCode::BadTypeSpecifier,
            "expected } after struct body",
        )?;
        let semicolon = self.consume_kind(
            TokenKind::Semicolon,
            CompilerErrorCode::NoSemicolonAfterStructure,
            "expected ; after struct declaration",
        )?;

        Ok(StructDecl {
            span: join_spans(ty.span, semicolon.span),
            name,
            fields,
        })
    }

    fn parse_struct_field_names(&mut self) -> Result<Vec<NamedItem>, ParserError> {
        let mut names = Vec::new();
        loop {
            let name = self.consume_identifier(
                CompilerErrorCode::ParsingVariableList,
                "expected field name",
            )?;
            names.push(NamedItem {
                span: name.span,
                name: name.text,
            });
            if !self.matches_kind(&TokenKind::Comma) {
                break;
            }
            self.advance();
        }
        Ok(names)
    }

    fn parse_function_declaration_or_definition(
        &mut self,
        return_type: TypeSpec,
        name: Token,
    ) -> Result<FunctionDecl, ParserError> {
        self.consume_kind(
            TokenKind::LeftParen,
            CompilerErrorCode::FunctionDefinitionMissingParameterList,
            "expected ( after function name",
        )?;
        let parameters = self.parse_parameter_list()?;
        self.consume_kind(
            TokenKind::RightParen,
            CompilerErrorCode::MalformedParameterList,
            "expected ) after parameter list",
        )?;

        if self.matches_kind(&TokenKind::Semicolon) {
            let semicolon = self.advance_required(
                CompilerErrorCode::NoSemicolonAfterExpression,
                "expected ; after parameter list",
            )?;
            return Ok(FunctionDecl {
                span: join_spans(return_type.span, semicolon.span),
                return_type,
                name: name.text,
                parameters,
                body: None,
            });
        }

        if !self.matches_kind(&TokenKind::LeftBrace) {
            return Err(self.error_here(
                CompilerErrorCode::UnknownStateInCompiler,
                "expected ; or function body after parameter list",
            ));
        }

        let body = self.parse_block_statement()?;
        Ok(FunctionDecl {
            span: join_spans(return_type.span, body.span),
            return_type,
            name: name.text,
            parameters,
            body: Some(body),
        })
    }

    fn parse_parameter_list(&mut self) -> Result<Vec<Parameter>, ParserError> {
        let mut parameters = Vec::new();
        if self.matches_kind(&TokenKind::RightParen) {
            return Ok(parameters);
        }

        loop {
            let ty = self.parse_non_void_type_specifier()?;
            let name = self.consume_identifier(
                CompilerErrorCode::BadVariableName,
                "expected parameter name",
            )?;
            let default = if self.matches_kind(&TokenKind::Assign) {
                self.advance();
                Some(self.parse_parameter_default_value()?)
            } else {
                None
            };
            let end_span = default.as_ref().map_or(name.span, |expr| expr.span);
            parameters.push(Parameter {
                span: join_spans(ty.span, end_span),
                ty,
                name: name.text,
                default,
            });

            if !self.matches_kind(&TokenKind::Comma) {
                break;
            }
            self.advance();
        }

        Ok(parameters)
    }

    fn parse_parameter_default_value(&mut self) -> Result<Expr, ParserError> {
        self.parse_expression()
    }

    fn parse_any_type_specifier(&mut self) -> Result<TypeSpec, ParserError> {
        if self.matches_keyword(Keyword::Void) {
            let token = self.advance_required(
                CompilerErrorCode::InvalidDeclarationType,
                "expected void token",
            )?;
            return Ok(TypeSpec {
                span:     token.span,
                is_const: false,
                kind:     TypeKind::Void,
            });
        }
        self.parse_non_void_type_specifier()
    }

    fn parse_non_void_type_specifier(&mut self) -> Result<TypeSpec, ParserError> {
        let const_token = if self.matches_keyword(Keyword::Const) {
            self.advance()
        } else {
            None
        };
        let is_const = const_token.is_some();

        let token = self.peek().cloned().ok_or_else(|| {
            self.error_here(
                CompilerErrorCode::InvalidDeclarationType,
                "unexpected EOF in type specifier",
            )
        })?;

        let kind = match token.kind {
            TokenKind::Keyword(Keyword::Int) => {
                self.advance();
                TypeKind::Int
            }
            TokenKind::Keyword(Keyword::Float) => {
                self.advance();
                TypeKind::Float
            }
            TokenKind::Keyword(Keyword::String) => {
                self.advance();
                TypeKind::String
            }
            TokenKind::Keyword(Keyword::Object) => {
                if is_const {
                    return Err(ParserError::new(
                        CompilerErrorCode::InvalidTypeForConstKeyword,
                        token.span,
                        "const is only valid for int, float, and string declarations",
                    ));
                }
                self.advance();
                TypeKind::Object
            }
            TokenKind::Keyword(Keyword::Vector) => {
                if is_const {
                    return Err(ParserError::new(
                        CompilerErrorCode::InvalidTypeForConstKeyword,
                        token.span,
                        "const is only valid for int, float, and string declarations",
                    ));
                }
                self.advance();
                TypeKind::Vector
            }
            TokenKind::Keyword(Keyword::Struct) => {
                if is_const {
                    return Err(ParserError::new(
                        CompilerErrorCode::InvalidTypeForConstKeyword,
                        token.span,
                        "const is not valid on struct declarations",
                    ));
                }
                self.advance();
                let name = self.consume_identifier(
                    CompilerErrorCode::InvalidDeclarationType,
                    "expected struct name after struct",
                )?;
                return Ok(TypeSpec {
                    span: join_spans(
                        const_token.map_or(token.span, |token| token.span),
                        name.span,
                    ),
                    is_const,
                    kind: TypeKind::Struct(name.text),
                });
            }
            TokenKind::Identifier if self.is_engine_structure_name(&token) => {
                if is_const {
                    return Err(ParserError::new(
                        CompilerErrorCode::InvalidTypeForConstKeyword,
                        token.span,
                        "const is not valid on engine structure declarations",
                    ));
                }
                self.advance();
                TypeKind::EngineStructure(token.text)
            }
            _ => {
                return Err(ParserError::new(
                    if is_const {
                        CompilerErrorCode::InvalidTypeForConstKeyword
                    } else {
                        CompilerErrorCode::InvalidDeclarationType
                    },
                    token.span,
                    "expected a non-void type specifier",
                ));
            }
        };

        let start_span = const_token.map_or(token.span, |token| token.span);
        Ok(TypeSpec {
            span: join_spans(start_span, token.span),
            is_const,
            kind,
        })
    }

    fn parse_declaration_after_name(
        &mut self,
        ty: TypeSpec,
        first_name: Token,
    ) -> Result<Declaration, ParserError> {
        let mut declarators = vec![self.parse_declarator_after_name(first_name)?];
        while self.matches_kind(&TokenKind::Comma) {
            self.advance();
            let name = self.consume_identifier(
                CompilerErrorCode::ParsingVariableList,
                "expected variable name after comma",
            )?;
            declarators.push(self.parse_declarator_after_name(name)?);
        }
        let semicolon = self.consume_kind(
            TokenKind::Semicolon,
            CompilerErrorCode::NoSemicolonAfterExpression,
            "expected ; after declaration",
        )?;
        Ok(Declaration {
            span: join_spans(ty.span, semicolon.span),
            ty,
            declarators,
        })
    }

    fn parse_declarator_after_name(&mut self, name: Token) -> Result<VarDeclarator, ParserError> {
        let initializer = if self.matches_kind(&TokenKind::Assign) {
            self.advance();
            Some(self.parse_expression()?)
        } else {
            None
        };
        let end_span = initializer.as_ref().map_or(name.span, |expr| expr.span);
        Ok(VarDeclarator {
            span: join_spans(name.span, end_span),
            name: name.text,
            initializer,
        })
    }

    fn parse_block_statement(&mut self) -> Result<BlockStmt, ParserError> {
        let left_brace = self.consume_kind(
            TokenKind::LeftBrace,
            CompilerErrorCode::ProgramCompoundStatementAtStart,
            "expected {",
        )?;
        let mut statements = Vec::new();
        while !self.matches_kind(&TokenKind::RightBrace) && !self.at_eof() {
            statements.push(self.parse_statement()?);
        }
        let right_brace = self.consume_kind(
            TokenKind::RightBrace,
            CompilerErrorCode::UnexpectedEndCompoundStatement,
            "expected } at end of compound statement",
        )?;
        Ok(BlockStmt {
            span: join_spans(left_brace.span, right_brace.span),
            statements,
        })
    }

    fn parse_statement(&mut self) -> Result<Stmt, ParserError> {
        if self.matches_kind(&TokenKind::LeftBrace) {
            return self.parse_block_statement().map(Stmt::Block);
        }
        if self.matches_keyword(Keyword::If) {
            return self.parse_if_statement().map(Stmt::If);
        }
        if self.matches_keyword(Keyword::Else) {
            return Err(self.error_here(
                CompilerErrorCode::ElseWithoutCorrespondingIf,
                "else without corresponding if",
            ));
        }
        if self.matches_keyword(Keyword::Switch) {
            return self.parse_switch_statement().map(Stmt::Switch);
        }
        if self.matches_keyword(Keyword::Return) {
            return self.parse_return_statement().map(Stmt::Return);
        }
        if self.matches_keyword(Keyword::While) {
            return self.parse_while_statement().map(Stmt::While);
        }
        if self.matches_keyword(Keyword::Do) {
            return self.parse_do_while_statement().map(Stmt::DoWhile);
        }
        if self.matches_keyword(Keyword::For) {
            return self.parse_for_statement().map(Stmt::For);
        }
        if self.matches_keyword(Keyword::Case) {
            return self.parse_case_statement().map(Stmt::Case);
        }
        if self.matches_keyword(Keyword::Default) {
            return self.parse_default_statement().map(Stmt::Default);
        }
        if self.matches_keyword(Keyword::Break) {
            let keyword = self.advance_required(
                CompilerErrorCode::NoSemicolonAfterStatement,
                "expected break token",
            )?;
            let semicolon = self.consume_kind(
                TokenKind::Semicolon,
                CompilerErrorCode::NoSemicolonAfterStatement,
                "expected ; after break",
            )?;
            return Ok(Stmt::Break(SimpleStmt {
                span: join_spans(keyword.span, semicolon.span),
            }));
        }
        if self.matches_keyword(Keyword::Continue) {
            let keyword = self.advance_required(
                CompilerErrorCode::NoSemicolonAfterStatement,
                "expected continue token",
            )?;
            let semicolon = self.consume_kind(
                TokenKind::Semicolon,
                CompilerErrorCode::NoSemicolonAfterStatement,
                "expected ; after continue",
            )?;
            return Ok(Stmt::Continue(SimpleStmt {
                span: join_spans(keyword.span, semicolon.span),
            }));
        }
        if self.matches_kind(&TokenKind::Semicolon) {
            let semicolon =
                self.advance_required(CompilerErrorCode::NoSemicolonAfterStatement, "expected ;")?;
            return Ok(Stmt::Empty(SimpleStmt {
                span: semicolon.span,
            }));
        }
        if self.starts_non_void_type_specifier() {
            return self.parse_statement_declaration().map(Stmt::Declaration);
        }

        let expr = self.parse_expression()?;
        let semicolon = self.consume_kind(
            TokenKind::Semicolon,
            CompilerErrorCode::NoSemicolonAfterExpression,
            "expected ; after expression",
        )?;
        Ok(Stmt::Expression(ExpressionStmt {
            span: join_spans(expr.span, semicolon.span),
            expr,
        }))
    }

    fn parse_statement_declaration(&mut self) -> Result<Declaration, ParserError> {
        let ty = self.parse_non_void_type_specifier()?;
        let name = self.consume_identifier(
            CompilerErrorCode::ParsingVariableList,
            "expected variable name in declaration",
        )?;
        self.parse_declaration_after_name(ty, name)
    }

    fn parse_if_statement(&mut self) -> Result<IfStmt, ParserError> {
        let if_token = self.consume_keyword(
            Keyword::If,
            CompilerErrorCode::UnknownStateInCompiler,
            "expected if",
        )?;
        self.consume_kind(
            TokenKind::LeftParen,
            CompilerErrorCode::NoLeftBracketOnExpression,
            "expected ( after if",
        )?;
        let condition = self.parse_expression()?;
        self.consume_kind(
            TokenKind::RightParen,
            CompilerErrorCode::NoRightBracketOnExpression,
            "expected ) after if condition",
        )?;
        let then_branch = self.parse_statement()?;
        if matches!(then_branch, Stmt::Empty(_)) {
            return Err(ParserError::new(
                CompilerErrorCode::IfConditionCannotBeFollowedByANullStatement,
                if_token.span,
                "if condition cannot be followed by an empty statement",
            ));
        }
        let else_branch = if self.matches_keyword(Keyword::Else) {
            self.advance();
            let branch = self.parse_statement()?;
            if matches!(branch, Stmt::Empty(_)) {
                return Err(ParserError::new(
                    CompilerErrorCode::ElseCannotBeFollowedByANullStatement,
                    if_token.span,
                    "else cannot be followed by an empty statement",
                ));
            }
            Some(Box::new(branch))
        } else {
            None
        };
        let end_span = else_branch
            .as_ref()
            .map_or_else(|| then_branch.span(), |stmt| stmt.span());
        Ok(IfStmt {
            span: join_spans(if_token.span, end_span),
            condition,
            then_branch: Box::new(then_branch),
            else_branch,
        })
    }

    fn parse_switch_statement(&mut self) -> Result<SwitchStmt, ParserError> {
        let switch_token = self.consume_keyword(
            Keyword::Switch,
            CompilerErrorCode::UnknownStateInCompiler,
            "expected switch",
        )?;
        self.consume_kind(
            TokenKind::LeftParen,
            CompilerErrorCode::NoLeftBracketOnExpression,
            "expected ( after switch",
        )?;
        let condition = self.parse_expression()?;
        self.consume_kind(
            TokenKind::RightParen,
            CompilerErrorCode::NoRightBracketOnExpression,
            "expected ) after switch condition",
        )?;
        let body = self.parse_statement()?;
        if matches!(body, Stmt::Empty(_)) {
            return Err(ParserError::new(
                CompilerErrorCode::SwitchConditionCannotBeFollowedByANullStatement,
                switch_token.span,
                "switch condition cannot be followed by an empty statement",
            ));
        }
        Ok(SwitchStmt {
            span: join_spans(switch_token.span, body.span()),
            condition,
            body: Box::new(body),
        })
    }

    fn parse_return_statement(&mut self) -> Result<ReturnStmt, ParserError> {
        let return_token = self.consume_keyword(
            Keyword::Return,
            CompilerErrorCode::UnknownStateInCompiler,
            "expected return",
        )?;
        if self.matches_kind(&TokenKind::Semicolon) {
            let semicolon = self.advance_required(
                CompilerErrorCode::ParsingReturnStatement,
                "expected ; after return",
            )?;
            return Ok(ReturnStmt {
                span:  join_spans(return_token.span, semicolon.span),
                value: None,
            });
        }
        let value = self.parse_expression()?;
        let semicolon = self.consume_kind(
            TokenKind::Semicolon,
            CompilerErrorCode::ParsingReturnStatement,
            "expected ; after return value",
        )?;
        Ok(ReturnStmt {
            span:  join_spans(return_token.span, semicolon.span),
            value: Some(value),
        })
    }

    fn parse_while_statement(&mut self) -> Result<WhileStmt, ParserError> {
        let while_token = self.consume_keyword(
            Keyword::While,
            CompilerErrorCode::UnknownStateInCompiler,
            "expected while",
        )?;
        self.consume_kind(
            TokenKind::LeftParen,
            CompilerErrorCode::NoLeftBracketOnExpression,
            "expected ( after while",
        )?;
        let condition = self.parse_expression()?;
        self.consume_kind(
            TokenKind::RightParen,
            CompilerErrorCode::NoRightBracketOnExpression,
            "expected ) after while condition",
        )?;
        let body = self.parse_statement()?;
        if matches!(body, Stmt::Empty(_)) {
            return Err(ParserError::new(
                CompilerErrorCode::WhileConditionCannotBeFollowedByANullStatement,
                while_token.span,
                "while condition cannot be followed by an empty statement",
            ));
        }
        Ok(WhileStmt {
            span: join_spans(while_token.span, body.span()),
            condition,
            body: Box::new(body),
        })
    }

    fn parse_do_while_statement(&mut self) -> Result<DoWhileStmt, ParserError> {
        let do_token = self.consume_keyword(
            Keyword::Do,
            CompilerErrorCode::UnknownStateInCompiler,
            "expected do",
        )?;
        let body = self.parse_statement()?;
        self.consume_keyword(
            Keyword::While,
            CompilerErrorCode::NoWhileAfterDoKeyword,
            "expected while after do body",
        )?;
        self.consume_kind(
            TokenKind::LeftParen,
            CompilerErrorCode::NoLeftBracketOnExpression,
            "expected ( after while in do-while statement",
        )?;
        let condition = self.parse_expression()?;
        self.consume_kind(
            TokenKind::RightParen,
            CompilerErrorCode::NoRightBracketOnExpression,
            "expected ) after do-while condition",
        )?;
        let semicolon = self.consume_kind(
            TokenKind::Semicolon,
            CompilerErrorCode::NoSemicolonAfterExpression,
            "expected ; after do-while statement",
        )?;
        Ok(DoWhileStmt {
            span: join_spans(do_token.span, semicolon.span),
            body: Box::new(body),
            condition,
        })
    }

    fn parse_for_statement(&mut self) -> Result<ForStmt, ParserError> {
        let for_token = self.consume_keyword(
            Keyword::For,
            CompilerErrorCode::UnknownStateInCompiler,
            "expected for",
        )?;
        self.consume_kind(
            TokenKind::LeftParen,
            CompilerErrorCode::NoLeftBracketOnExpression,
            "expected ( after for",
        )?;
        let initializer = if self.matches_kind(&TokenKind::Semicolon) {
            None
        } else {
            Some(self.parse_expression()?)
        };
        self.consume_kind(
            TokenKind::Semicolon,
            CompilerErrorCode::NoSemicolonAfterExpression,
            "expected ; after for initializer",
        )?;
        let condition = if self.matches_kind(&TokenKind::Semicolon) {
            None
        } else {
            Some(self.parse_expression()?)
        };
        self.consume_kind(
            TokenKind::Semicolon,
            CompilerErrorCode::NoSemicolonAfterExpression,
            "expected ; after for condition",
        )?;
        let update = if self.matches_kind(&TokenKind::RightParen) {
            None
        } else {
            Some(self.parse_expression()?)
        };
        self.consume_kind(
            TokenKind::RightParen,
            CompilerErrorCode::NoRightBracketOnExpression,
            "expected ) after for update expression",
        )?;
        let body = self.parse_statement()?;
        if matches!(body, Stmt::Empty(_)) {
            return Err(ParserError::new(
                CompilerErrorCode::ForStatementCannotBeFollowedByANullStatement,
                for_token.span,
                "for statement cannot be followed by an empty statement",
            ));
        }
        Ok(ForStmt {
            span: join_spans(for_token.span, body.span()),
            initializer,
            condition,
            update,
            body: Box::new(body),
        })
    }

    fn parse_case_statement(&mut self) -> Result<CaseStmt, ParserError> {
        let case_token = self.consume_keyword(
            Keyword::Case,
            CompilerErrorCode::UnknownStateInCompiler,
            "expected case",
        )?;
        let value = self.parse_conditional_expression()?;
        let colon = self.consume_kind(
            TokenKind::Colon,
            CompilerErrorCode::NoColonAfterCaseLabel,
            "expected : after case expression",
        )?;
        Ok(CaseStmt {
            span: join_spans(case_token.span, colon.span),
            value,
        })
    }

    fn parse_default_statement(&mut self) -> Result<DefaultStmt, ParserError> {
        let token = self.consume_keyword(
            Keyword::Default,
            CompilerErrorCode::UnknownStateInCompiler,
            "expected default",
        )?;
        let colon = self.consume_kind(
            TokenKind::Colon,
            CompilerErrorCode::NoColonAfterDefaultLabel,
            "expected : after default",
        )?;
        Ok(DefaultStmt {
            span: join_spans(token.span, colon.span),
        })
    }

    fn parse_expression(&mut self) -> Result<Expr, ParserError> {
        self.parse_assignment_expression()
    }

    fn parse_assignment_expression(&mut self) -> Result<Expr, ParserError> {
        let left = self.parse_conditional_expression()?;
        let Some(op) = self.current_assignment_op() else {
            return Ok(left);
        };
        self.advance();
        let right = self.parse_conditional_expression()?;
        Ok(Expr {
            span: join_spans(left.span, right.span),
            kind: ExprKind::Assignment {
                op,
                left: Box::new(left),
                right: Box::new(right),
            },
        })
    }

    fn parse_conditional_expression(&mut self) -> Result<Expr, ParserError> {
        let condition = self.parse_logical_or_expression()?;
        if !self.matches_kind(&TokenKind::QuestionMark) {
            return Ok(condition);
        }
        self.advance();
        let when_true = self.parse_conditional_expression()?;
        self.consume_kind(
            TokenKind::Colon,
            CompilerErrorCode::ConditionalRequiresSecondExpression,
            "expected : in conditional expression",
        )?;
        let when_false = self.parse_conditional_expression()?;
        Ok(Expr {
            span: join_spans(condition.span, when_false.span),
            kind: ExprKind::Conditional {
                condition:  Box::new(condition),
                when_true:  Box::new(when_true),
                when_false: Box::new(when_false),
            },
        })
    }

    fn parse_logical_or_expression(&mut self) -> Result<Expr, ParserError> {
        self.parse_left_associative_binary(
            Self::parse_logical_and_expression,
            &[(TokenKind::LogicalOr, BinaryOp::LogicalOr)],
        )
    }

    fn parse_logical_and_expression(&mut self) -> Result<Expr, ParserError> {
        self.parse_left_associative_binary(
            Self::parse_inclusive_or_expression,
            &[(TokenKind::LogicalAnd, BinaryOp::LogicalAnd)],
        )
    }

    fn parse_inclusive_or_expression(&mut self) -> Result<Expr, ParserError> {
        self.parse_left_associative_binary(
            Self::parse_exclusive_or_expression,
            &[(TokenKind::InclusiveOr, BinaryOp::InclusiveOr)],
        )
    }

    fn parse_exclusive_or_expression(&mut self) -> Result<Expr, ParserError> {
        self.parse_left_associative_binary(
            Self::parse_boolean_and_expression,
            &[(TokenKind::ExclusiveOr, BinaryOp::ExclusiveOr)],
        )
    }

    fn parse_boolean_and_expression(&mut self) -> Result<Expr, ParserError> {
        self.parse_left_associative_binary(
            Self::parse_equality_expression,
            &[(TokenKind::BooleanAnd, BinaryOp::BooleanAnd)],
        )
    }

    fn parse_equality_expression(&mut self) -> Result<Expr, ParserError> {
        self.parse_left_associative_binary(
            Self::parse_relational_expression,
            &[
                (TokenKind::NotEqual, BinaryOp::NotEqual),
                (TokenKind::EqualEqual, BinaryOp::EqualEqual),
            ],
        )
    }

    fn parse_relational_expression(&mut self) -> Result<Expr, ParserError> {
        self.parse_left_associative_binary(
            Self::parse_shift_expression,
            &[
                (TokenKind::GreaterEqual, BinaryOp::GreaterEqual),
                (TokenKind::LessEqual, BinaryOp::LessEqual),
                (TokenKind::LessThan, BinaryOp::LessThan),
                (TokenKind::GreaterThan, BinaryOp::GreaterThan),
            ],
        )
    }

    fn parse_shift_expression(&mut self) -> Result<Expr, ParserError> {
        self.parse_left_associative_binary(
            Self::parse_additive_expression,
            &[
                (TokenKind::ShiftLeft, BinaryOp::ShiftLeft),
                (TokenKind::ShiftRight, BinaryOp::ShiftRight),
                (TokenKind::UnsignedShiftRight, BinaryOp::UnsignedShiftRight),
            ],
        )
    }

    fn parse_additive_expression(&mut self) -> Result<Expr, ParserError> {
        self.parse_left_associative_binary(
            Self::parse_multiplicative_expression,
            &[
                (TokenKind::Plus, BinaryOp::Add),
                (TokenKind::Minus, BinaryOp::Subtract),
            ],
        )
    }

    fn parse_multiplicative_expression(&mut self) -> Result<Expr, ParserError> {
        self.parse_left_associative_binary(
            Self::parse_unary_expression,
            &[
                (TokenKind::Multiply, BinaryOp::Multiply),
                (TokenKind::Divide, BinaryOp::Divide),
                (TokenKind::Modulus, BinaryOp::Modulus),
            ],
        )
    }

    fn parse_unary_expression(&mut self) -> Result<Expr, ParserError> {
        let Some(token) = self.peek().cloned() else {
            return Err(self.error_here(
                CompilerErrorCode::UnknownStateInCompiler,
                "unexpected EOF in expression",
            ));
        };

        let op = match token.kind {
            TokenKind::Minus => Some(UnaryOp::Negate),
            TokenKind::Tilde => Some(UnaryOp::OnesComplement),
            TokenKind::BooleanNot => Some(UnaryOp::BooleanNot),
            TokenKind::Increment => Some(UnaryOp::PreIncrement),
            TokenKind::Decrement => Some(UnaryOp::PreDecrement),
            TokenKind::Plus => None,
            _ => return self.parse_postfix_expression(),
        };

        self.advance();
        let expr = self.parse_postfix_expression()?;
        if matches!(token.kind, TokenKind::Plus) {
            return Ok(expr);
        }
        Ok(Expr {
            span: join_spans(token.span, expr.span),
            kind: ExprKind::Unary {
                op:   op.ok_or_else(|| {
                    self.error_here(
                        CompilerErrorCode::UnknownStateInCompiler,
                        "missing unary operator",
                    )
                })?,
                expr: Box::new(expr),
            },
        })
    }

    fn parse_postfix_expression(&mut self) -> Result<Expr, ParserError> {
        let mut expr = self.parse_primary_expression()?;
        loop {
            if self.matches_kind(&TokenKind::StructurePartSpecify) {
                self.advance();
                let field = self.consume_identifier(
                    CompilerErrorCode::UndefinedFieldInStructure,
                    "expected field name after .",
                )?;
                expr = Expr {
                    span: join_spans(expr.span, field.span),
                    kind: ExprKind::FieldAccess {
                        base:  Box::new(expr),
                        field: field.text,
                    },
                };
                continue;
            }
            if self.matches_kind(&TokenKind::Increment) {
                let token = self.advance_required(
                    CompilerErrorCode::UnknownStateInCompiler,
                    "expected increment token",
                )?;
                expr = Expr {
                    span: join_spans(expr.span, token.span),
                    kind: ExprKind::Unary {
                        op:   UnaryOp::PostIncrement,
                        expr: Box::new(expr),
                    },
                };
                continue;
            }
            if self.matches_kind(&TokenKind::Decrement) {
                let token = self.advance_required(
                    CompilerErrorCode::UnknownStateInCompiler,
                    "expected decrement token",
                )?;
                expr = Expr {
                    span: join_spans(expr.span, token.span),
                    kind: ExprKind::Unary {
                        op:   UnaryOp::PostDecrement,
                        expr: Box::new(expr),
                    },
                };
                continue;
            }
            break;
        }
        Ok(expr)
    }

    fn parse_primary_expression(&mut self) -> Result<Expr, ParserError> {
        let token = self.peek().cloned().ok_or_else(|| {
            self.error_here(
                CompilerErrorCode::UnknownStateInCompiler,
                "unexpected EOF in expression",
            )
        })?;

        match token.kind {
            TokenKind::Integer
            | TokenKind::HexInteger
            | TokenKind::BinaryInteger
            | TokenKind::OctalInteger
            | TokenKind::Float
            | TokenKind::String
            | TokenKind::LeftSquareBracket
            | TokenKind::Keyword(
                Keyword::ObjectSelf
                | Keyword::ObjectInvalid
                | Keyword::LocationInvalid
                | Keyword::JsonNull
                | Keyword::JsonFalse
                | Keyword::JsonTrue
                | Keyword::JsonObject
                | Keyword::JsonArray
                | Keyword::JsonString
                | Keyword::FunctionMacro
                | Keyword::FileMacro
                | Keyword::LineMacro
                | Keyword::DateMacro
                | Keyword::TimeMacro,
            ) => self.parse_literal_expression(),
            TokenKind::LeftParen => {
                let left = self
                    .advance_required(CompilerErrorCode::NoLeftBracketOnExpression, "expected (")?;
                let expr = self.parse_expression()?;
                let right = self.consume_kind(
                    TokenKind::RightParen,
                    CompilerErrorCode::NoRightBracketOnExpression,
                    "expected ) after parenthesized expression",
                )?;
                Ok(Expr {
                    span: join_spans(left.span, right.span),
                    kind: expr.kind,
                })
            }
            TokenKind::Identifier => {
                let identifier = self
                    .advance_required(CompilerErrorCode::BadVariableName, "expected identifier")?;
                let mut expr = Expr {
                    span: identifier.span,
                    kind: ExprKind::Identifier(identifier.text),
                };
                if self.matches_kind(&TokenKind::LeftParen) {
                    let (arguments, end_span) = self.parse_argument_list()?;
                    expr = Expr {
                        span: join_spans(expr.span, end_span),
                        kind: ExprKind::Call {
                            callee: Box::new(expr),
                            arguments,
                        },
                    };
                }
                Ok(expr)
            }
            _ => Err(ParserError::new(
                CompilerErrorCode::UnknownStateInCompiler,
                token.span,
                "unexpected token in expression",
            )),
        }
    }

    fn parse_argument_list(&mut self) -> Result<(Vec<Expr>, Span), ParserError> {
        let left_paren = self.consume_kind(
            TokenKind::LeftParen,
            CompilerErrorCode::NoLeftBracketOnArgList,
            "expected ( before argument list",
        )?;
        let mut arguments = Vec::new();
        if self.matches_kind(&TokenKind::RightParen) {
            let right =
                self.advance_required(CompilerErrorCode::MalformedParameterList, "expected )")?;
            return Ok((arguments, join_spans(left_paren.span, right.span)));
        }

        loop {
            arguments.push(self.parse_expression()?);
            if self.matches_kind(&TokenKind::RightParen) {
                let right =
                    self.advance_required(CompilerErrorCode::MalformedParameterList, "expected )")?;
                return Ok((arguments, join_spans(left_paren.span, right.span)));
            }
            self.consume_kind(
                TokenKind::Comma,
                CompilerErrorCode::UnknownStateInCompiler,
                "expected , or ) in argument list",
            )?;
        }
    }

    fn parse_literal_expression(&mut self) -> Result<Expr, ParserError> {
        let token = self.peek().cloned().ok_or_else(|| {
            self.error_here(
                CompilerErrorCode::BadConstantType,
                "unexpected EOF in literal",
            )
        })?;

        let (span, literal) = match token.kind {
            TokenKind::Integer => {
                self.advance();
                (token.span, Literal::Integer(parse_decimal_integer(&token)?))
            }
            TokenKind::HexInteger => {
                self.advance();
                (
                    token.span,
                    Literal::Integer(parse_prefixed_integer(&token, 16)?),
                )
            }
            TokenKind::BinaryInteger => {
                self.advance();
                (
                    token.span,
                    Literal::Integer(parse_prefixed_integer(&token, 2)?),
                )
            }
            TokenKind::OctalInteger => {
                self.advance();
                (
                    token.span,
                    Literal::Integer(parse_prefixed_integer(&token, 8)?),
                )
            }
            TokenKind::Float => {
                self.advance();
                let value = token.text.parse::<f32>().map_err(|_error| {
                    ParserError::new(
                        CompilerErrorCode::BadConstantType,
                        token.span,
                        format!("invalid floating-point literal {:?}", token.text),
                    )
                })?;
                (token.span, Literal::Float(value))
            }
            TokenKind::String => {
                self.advance();
                (token.span, Literal::String(token.text))
            }
            TokenKind::LeftSquareBracket => self.parse_vector_literal()?,
            TokenKind::Keyword(Keyword::ObjectSelf) => {
                self.advance();
                (token.span, Literal::ObjectSelf)
            }
            TokenKind::Keyword(Keyword::ObjectInvalid) => {
                self.advance();
                (token.span, Literal::ObjectInvalid)
            }
            TokenKind::Keyword(Keyword::LocationInvalid) => {
                self.advance();
                (token.span, Literal::LocationInvalid)
            }
            TokenKind::Keyword(Keyword::JsonNull) => {
                self.advance();
                (token.span, Literal::Json("null".to_string()))
            }
            TokenKind::Keyword(Keyword::JsonFalse) => {
                self.advance();
                (token.span, Literal::Json("false".to_string()))
            }
            TokenKind::Keyword(Keyword::JsonTrue) => {
                self.advance();
                (token.span, Literal::Json("true".to_string()))
            }
            TokenKind::Keyword(Keyword::JsonObject) => {
                self.advance();
                (token.span, Literal::Json("{}".to_string()))
            }
            TokenKind::Keyword(Keyword::JsonArray) => {
                self.advance();
                (token.span, Literal::Json("[]".to_string()))
            }
            TokenKind::Keyword(Keyword::JsonString) => {
                self.advance();
                (token.span, Literal::Json("\"\"".to_string()))
            }
            TokenKind::Keyword(Keyword::FunctionMacro) => {
                self.advance();
                (token.span, Literal::Magic(MagicLiteral::Function))
            }
            TokenKind::Keyword(Keyword::FileMacro) => {
                self.advance();
                (token.span, Literal::Magic(MagicLiteral::File))
            }
            TokenKind::Keyword(Keyword::LineMacro) => {
                self.advance();
                (token.span, Literal::Magic(MagicLiteral::Line))
            }
            TokenKind::Keyword(Keyword::DateMacro) => {
                self.advance();
                (token.span, Literal::Magic(MagicLiteral::Date))
            }
            TokenKind::Keyword(Keyword::TimeMacro) => {
                self.advance();
                (token.span, Literal::Magic(MagicLiteral::Time))
            }
            _ => {
                return Err(ParserError::new(
                    CompilerErrorCode::BadConstantType,
                    token.span,
                    "unexpected token in literal expression",
                ));
            }
        };
        Ok(Expr {
            span,
            kind: ExprKind::Literal(literal),
        })
    }

    fn parse_vector_literal(&mut self) -> Result<(Span, Literal), ParserError> {
        let left = self.consume_kind(
            TokenKind::LeftSquareBracket,
            CompilerErrorCode::ParsingConstantVector,
            "expected [ to start vector literal",
        )?;
        let mut values = [0.0_f32; 3];
        let mut count = 0;

        if self.matches_kind(&TokenKind::RightSquareBracket) {
            let right =
                self.advance_required(CompilerErrorCode::ParsingConstantVector, "expected ]")?;
            return Ok((join_spans(left.span, right.span), Literal::Vector(values)));
        }

        loop {
            let token = self.consume_kind(
                TokenKind::Float,
                CompilerErrorCode::ParsingConstantVector,
                "expected float literal in vector constant",
            )?;
            if count >= 3 {
                return Err(ParserError::new(
                    CompilerErrorCode::ParsingConstantVector,
                    token.span,
                    "vector literal cannot contain more than three elements",
                ));
            }
            let value = token.text.parse::<f32>().map_err(|_error| {
                ParserError::new(
                    CompilerErrorCode::ParsingConstantVector,
                    token.span,
                    format!("invalid vector component {:?}", token.text),
                )
            })?;
            let Some(slot) = values.get_mut(count) else {
                return Err(ParserError::new(
                    CompilerErrorCode::ParsingConstantVector,
                    token.span,
                    "vector literal cannot contain more than three elements",
                ));
            };
            *slot = value;
            count += 1;
            if self.matches_kind(&TokenKind::RightSquareBracket) {
                let right =
                    self.advance_required(CompilerErrorCode::ParsingConstantVector, "expected ]")?;
                return Ok((join_spans(left.span, right.span), Literal::Vector(values)));
            }
            self.consume_kind(
                TokenKind::Comma,
                CompilerErrorCode::ParsingConstantVector,
                "expected , or ] in vector constant",
            )?;
        }
    }

    fn parse_left_associative_binary(
        &mut self,
        subparser: fn(&mut Self) -> Result<Expr, ParserError>,
        operators: &[(TokenKind, BinaryOp)],
    ) -> Result<Expr, ParserError> {
        let mut expr = subparser(self)?;
        while let Some(op) = self.current_binary_op(operators) {
            self.advance();
            let right = subparser(self)?;
            expr = Expr {
                span: join_spans(expr.span, right.span),
                kind: ExprKind::Binary {
                    op,
                    left: Box::new(expr),
                    right: Box::new(right),
                },
            };
        }
        Ok(expr)
    }

    fn current_binary_op(&self, operators: &[(TokenKind, BinaryOp)]) -> Option<BinaryOp> {
        let token = self.peek()?;
        operators
            .iter()
            .find_map(|(kind, op)| (token.kind == *kind).then_some(*op))
    }

    fn current_assignment_op(&self) -> Option<AssignmentOp> {
        let token = self.peek()?;
        match token.kind {
            TokenKind::Assign => Some(AssignmentOp::Assign),
            TokenKind::AssignMinus => Some(AssignmentOp::AssignMinus),
            TokenKind::AssignPlus => Some(AssignmentOp::AssignPlus),
            TokenKind::AssignMultiply => Some(AssignmentOp::AssignMultiply),
            TokenKind::AssignDivide => Some(AssignmentOp::AssignDivide),
            TokenKind::AssignModulus => Some(AssignmentOp::AssignModulus),
            TokenKind::AssignAnd => Some(AssignmentOp::AssignAnd),
            TokenKind::AssignXor => Some(AssignmentOp::AssignXor),
            TokenKind::AssignOr => Some(AssignmentOp::AssignOr),
            TokenKind::AssignShiftLeft => Some(AssignmentOp::AssignShiftLeft),
            TokenKind::AssignShiftRight => Some(AssignmentOp::AssignShiftRight),
            TokenKind::AssignUnsignedShiftRight => Some(AssignmentOp::AssignUnsignedShiftRight),
            _ => None,
        }
    }

    fn starts_non_void_type_specifier(&self) -> bool {
        let Some(token) = self.peek() else {
            return false;
        };
        match token.kind {
            TokenKind::Keyword(
                Keyword::Const
                | Keyword::Int
                | Keyword::Float
                | Keyword::String
                | Keyword::Object
                | Keyword::Struct
                | Keyword::Vector,
            ) => true,
            TokenKind::Identifier => self.is_engine_structure_name(token),
            _ => false,
        }
    }

    fn is_engine_structure_name(&self, token: &Token) -> bool {
        matches!(token.kind, TokenKind::Identifier)
            && self.engine_structures.contains(token.text.as_str())
    }

    fn matches_keyword(&self, keyword: Keyword) -> bool {
        matches!(
            self.peek(),
            Some(Token {
                kind: TokenKind::Keyword(found),
                ..
            }) if *found == keyword
        )
    }

    fn matches_kind(&self, kind: &TokenKind) -> bool {
        self.peek().is_some_and(|token| token.kind == *kind)
    }

    fn consume_keyword(
        &mut self,
        keyword: Keyword,
        code: CompilerErrorCode,
        message: &str,
    ) -> Result<Token, ParserError> {
        if self.matches_keyword(keyword) {
            self.advance_required(code, message)
        } else {
            Err(self.error_here(code, message))
        }
    }

    #[allow(clippy::needless_pass_by_value)]
    fn consume_kind(
        &mut self,
        kind: TokenKind,
        code: CompilerErrorCode,
        message: &str,
    ) -> Result<Token, ParserError> {
        if self.matches_kind(&kind) {
            self.advance_required(code, message)
        } else {
            Err(self.error_here(code, message))
        }
    }

    fn consume_identifier(
        &mut self,
        code: CompilerErrorCode,
        message: &str,
    ) -> Result<Token, ParserError> {
        match self.peek() {
            Some(Token {
                kind: TokenKind::Identifier,
                ..
            }) => self.advance_required(code, message),
            _ => Err(self.error_here(code, message)),
        }
    }

    fn consume_string(
        &mut self,
        code: CompilerErrorCode,
        message: &str,
    ) -> Result<Token, ParserError> {
        match self.peek() {
            Some(Token {
                kind: TokenKind::String,
                ..
            }) => self.advance_required(code, message),
            _ => Err(self.error_here(code, message)),
        }
    }

    fn error_here(&self, code: CompilerErrorCode, message: impl Into<String>) -> ParserError {
        let span = self
            .peek()
            .map_or_else(|| self.eof_span(), |token| token.span);
        ParserError::new(code, span, message)
    }

    fn eof_span(&self) -> Span {
        self.tokens
            .last()
            .map_or_else(|| Span::new(SourceId::new(0), 0, 0), |token| token.span)
    }

    fn at_eof(&self) -> bool {
        matches!(
            self.peek(),
            Some(Token {
                kind: TokenKind::Eof,
                ..
            }) | None
        )
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.position)
    }

    fn advance(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.position)?.clone();
        self.position += 1;
        Some(token)
    }

    fn advance_required(
        &mut self,
        code: CompilerErrorCode,
        message: &str,
    ) -> Result<Token, ParserError> {
        self.advance().ok_or_else(|| self.error_here(code, message))
    }
}

impl Stmt {
    #[allow(clippy::match_same_arms)]
    fn span(&self) -> Span {
        match self {
            Self::Block(stmt) => stmt.span,
            Self::Declaration(stmt) => stmt.span,
            Self::Expression(stmt) => stmt.span,
            Self::If(stmt) => stmt.span,
            Self::Switch(stmt) => stmt.span,
            Self::Return(stmt) => stmt.span,
            Self::While(stmt) => stmt.span,
            Self::DoWhile(stmt) => stmt.span,
            Self::For(stmt) => stmt.span,
            Self::Case(stmt) => stmt.span,
            Self::Default(stmt) => stmt.span,
            Self::Break(stmt) => stmt.span,
            Self::Continue(stmt) => stmt.span,
            Self::Empty(stmt) => stmt.span,
        }
    }
}

fn join_spans(start: Span, end: Span) -> Span {
    debug_assert_eq!(start.source_id, end.source_id);
    Span::new(
        start.source_id,
        start.start.min(end.start),
        start.end.max(end.end),
    )
}

fn parse_decimal_integer(token: &Token) -> Result<i32, ParserError> {
    parse_wrapping_decimal_i32(&token.text).map_err(|_error| {
        ParserError::new(
            CompilerErrorCode::BadConstantType,
            token.span,
            format!("invalid integer literal {:?}", token.text),
        )
    })
}

fn parse_prefixed_integer(token: &Token, radix: u32) -> Result<i32, ParserError> {
    let value = parse_wrapping_prefixed_i32(&token.text, radix).map_err(|_error| {
        ParserError::new(
            CompilerErrorCode::BadConstantType,
            token.span,
            format!("invalid integer literal {:?}", token.text),
        )
    })?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::{
        ExprKind, Literal, ParseError, Stmt, TopLevelItem, parse_resolved_script, parse_text,
    };
    use crate::{InMemoryScriptResolver, LangSpec, SourceId, SourceLoadOptions, TypeKind};

    fn test_langspec() -> LangSpec {
        LangSpec {
            engine_num_structures: 3,
            engine_structures:     vec![
                "effect".to_string(),
                "location".to_string(),
                "json".to_string(),
            ],
            constants:             Vec::new(),
            functions:             Vec::new(),
        }
    }

    #[test]
    fn parses_top_level_items_using_upstream_shapes() -> Result<(), Box<dyn std::error::Error>> {
        let script = parse_text(
            SourceId::new(1),
            "#include \"x\"\nint VALUE = 1;\nvoid main(int n = -1) { return; }\neffect fx;",
            Some(&test_langspec()),
        )?;

        assert_eq!(script.items.len(), 4);
        assert!(matches!(
            script.items.first(),
            Some(TopLevelItem::Include(_))
        ));
        assert!(matches!(script.items.get(1), Some(TopLevelItem::Global(_))));
        assert!(matches!(
            script.items.get(2),
            Some(TopLevelItem::Function(_))
        ));
        match script.items.get(3) {
            Some(TopLevelItem::Global(decl)) => {
                assert_eq!(
                    decl.ty.kind,
                    TypeKind::EngineStructure("effect".to_string())
                );
                assert_eq!(
                    decl.declarators
                        .first()
                        .map(|declarator| declarator.name.as_str()),
                    Some("fx")
                );
            }
            other => {
                return Err(std::io::Error::other(format!(
                    "expected global declaration, got {other:?}"
                ))
                .into());
            }
        }
        Ok(())
    }

    #[test]
    fn parses_struct_definitions_and_fields() -> Result<(), Box<dyn std::error::Error>> {
        let script = parse_text(
            SourceId::new(2),
            "struct Foo { int a, b; vector dir; struct Bar child; };",
            Some(&test_langspec()),
        )?;

        match script.items.first() {
            Some(TopLevelItem::Global(decl)) => {
                return Err(std::io::Error::other(format!(
                    "expected struct declaration, got global {decl:?}"
                ))
                .into());
            }
            Some(TopLevelItem::Struct(def)) => {
                assert_eq!(def.name, "Foo");
                assert_eq!(def.fields.len(), 3);
                assert_eq!(def.fields.first().map(|field| field.names.len()), Some(2));
                assert_eq!(
                    def.fields.get(1).map(|field| field.ty.kind.clone()),
                    Some(TypeKind::Vector)
                );
                assert_eq!(
                    def.fields.get(2).map(|field| field.ty.kind.clone()),
                    Some(TypeKind::Struct("Bar".to_string()))
                );
            }
            other => {
                return Err(std::io::Error::other(format!(
                    "expected struct declaration, got {other:?}"
                ))
                .into());
            }
        }
        Ok(())
    }

    #[test]
    fn parses_expression_precedence_and_postfix_forms() -> Result<(), Box<dyn std::error::Error>> {
        let script = parse_text(
            SourceId::new(3),
            "void main() { a >>= b + c * d.e++ ? f : g; }",
            Some(&test_langspec()),
        )?;

        let function = match script.items.first() {
            Some(TopLevelItem::Function(function)) => function,
            other => {
                return Err(
                    std::io::Error::other(format!("expected function, got {other:?}")).into(),
                );
            }
        };
        let body = function
            .body
            .as_ref()
            .ok_or_else(|| std::io::Error::other("function body must exist"))?;
        let stmt = match body.statements.first() {
            Some(Stmt::Expression(stmt)) => stmt,
            other => {
                return Err(std::io::Error::other(format!(
                    "expected expression statement, got {other:?}"
                ))
                .into());
            }
        };

        match &stmt.expr.kind {
            ExprKind::Assignment {
                ..
            } => {}
            other => {
                return Err(std::io::Error::other(format!(
                    "expected assignment expression, got {other:?}"
                ))
                .into());
            }
        }
        Ok(())
    }

    #[test]
    fn parses_control_flow_statements() -> Result<(), Box<dyn std::error::Error>> {
        let script = parse_text(
            SourceId::new(4),
            "void main() { if (a) { return; } else { while (b) { continue; } } for (i = 0; i < 3; \
             i += 1) { break; } switch (n) { case 1: break; default: return; } do { n -= 1; } \
             while (n); }",
            Some(&test_langspec()),
        )?;

        let function = match script.items.first() {
            Some(TopLevelItem::Function(function)) => function,
            other => {
                return Err(
                    std::io::Error::other(format!("expected function, got {other:?}")).into(),
                );
            }
        };
        let body = function
            .body
            .as_ref()
            .ok_or_else(|| std::io::Error::other("function body must exist"))?;
        assert!(matches!(body.statements.first(), Some(Stmt::If(_))));
        assert!(matches!(body.statements.get(1), Some(Stmt::For(_))));
        assert!(matches!(body.statements.get(2), Some(Stmt::Switch(_))));
        assert!(matches!(body.statements.get(3), Some(Stmt::DoWhile(_))));
        Ok(())
    }

    #[test]
    fn preserves_magic_and_vector_literals() -> Result<(), Box<dyn std::error::Error>> {
        let script = parse_text(
            SourceId::new(5),
            "void main() { string a = __FILE__; vector v = [1.0, 2.0]; json j = JSON_OBJECT; }",
            Some(&test_langspec()),
        )?;

        let function = match script.items.first() {
            Some(TopLevelItem::Function(function)) => function,
            other => {
                return Err(
                    std::io::Error::other(format!("expected function, got {other:?}")).into(),
                );
            }
        };
        let body = function
            .body
            .as_ref()
            .ok_or_else(|| std::io::Error::other("function body must exist"))?;
        match body.statements.first() {
            Some(Stmt::Declaration(decl)) => match decl
                .declarators
                .first()
                .and_then(|declarator| declarator.initializer.as_ref())
            {
                Some(expr) => assert!(matches!(expr.kind, ExprKind::Literal(Literal::Magic(_)))),
                None => return Err(std::io::Error::other("expected initializer").into()),
            },
            other => {
                return Err(
                    std::io::Error::other(format!("expected declaration, got {other:?}")).into(),
                );
            }
        }
        Ok(())
    }

    #[test]
    fn rejects_null_statement_after_if_like_upstream() -> Result<(), Box<dyn std::error::Error>> {
        let error = parse_text(
            SourceId::new(6),
            "void main() { if (TRUE) ; }",
            Some(&test_langspec()),
        )
        .expect_err("parser should reject null if body");

        match error {
            ParseError::Parse(error) => {
                assert_eq!(
                    error.code,
                    crate::CompilerErrorCode::IfConditionCannotBeFollowedByANullStatement
                );
            }
            other => {
                return Err(
                    std::io::Error::other(format!("expected parse error, got {other:?}")).into(),
                );
            }
        }
        Ok(())
    }

    #[test]
    fn parses_resolved_script_through_includes_and_object_like_defines()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut resolver = InMemoryScriptResolver::new();
        resolver.insert_source(
            "root",
            br#"#define BASE 2
#include "util"
int main_value = UTIL_PLUS;
"#,
        );
        resolver.insert_source(
            "util",
            br#"#define UTIL_PLUS BASE + 3
int helper = UTIL_PLUS;
"#,
        );

        let script = parse_resolved_script(
            &resolver,
            "root",
            SourceLoadOptions::default(),
            Some(&test_langspec()),
        )?;

        assert_eq!(script.items.len(), 2);
        assert!(matches!(
            script.items.first(),
            Some(TopLevelItem::Global(_))
        ));
        assert!(matches!(script.items.get(1), Some(TopLevelItem::Global(_))));
        Ok(())
    }

    #[test]
    fn parses_full_constant_expressions_in_parameter_defaults()
    -> Result<(), Box<dyn std::error::Error>> {
        let script = parse_text(
            SourceId::new(7),
            "const int BASE = 1; void main(int nValue = BASE + 2 * 3) { return; }",
            Some(&test_langspec()),
        )?;

        let function = match script.items.get(1) {
            Some(TopLevelItem::Function(function)) => function,
            other => {
                return Err(
                    std::io::Error::other(format!("expected function, got {other:?}")).into(),
                );
            }
        };
        let default = function
            .parameters
            .first()
            .and_then(|parameter| parameter.default.as_ref())
            .ok_or_else(|| std::io::Error::other("expected parameter default"))?;
        assert!(matches!(default.kind, ExprKind::Binary { .. }));
        Ok(())
    }
}
