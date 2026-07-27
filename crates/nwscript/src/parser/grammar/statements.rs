use super::*;

impl Parser<'_> {
    pub(super) fn parse_block_statement(&mut self) -> Result<BlockStmt, ParserError> {
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

    pub(super) fn parse_statement(&mut self) -> Result<Stmt, ParserError> {
        self.with_syntax_depth(Self::parse_statement_inner)
    }

    pub(super) fn parse_statement_inner(&mut self) -> Result<Stmt, ParserError> {
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
        if self.matches_identifier_text("static_assert") {
            return self.parse_static_assert().map(Stmt::StaticAssert);
        }
        if self.matches_kind(&TokenKind::Semicolon) {
            let semicolon =
                self.advance_required(CompilerErrorCode::NoSemicolonAfterStatement, "expected ;")?;
            return Ok(Stmt::Empty(SimpleStmt {
                span: semicolon.span,
            }));
        }
        if !self.matches_identifier_text("match") && self.starts_non_void_type_specifier() {
            return self.parse_statement_declaration().map(Stmt::Declaration);
        }

        let expr = self.parse_expression()?;
        if matches!(expr.kind, ExprKind::Match(_)) && !self.matches_kind(&TokenKind::Semicolon) {
            return Ok(Stmt::Expression(ExpressionStmt {
                span: expr.span,
                expr,
            }));
        }
        let semicolon = self
            .consume_kind(
                TokenKind::Semicolon,
                CompilerErrorCode::NoSemicolonAfterExpression,
                "expected ; after expression",
            )
            .map_err(|mut error| {
                if matches!(
                    self.peek().map(|token| &token.kind),
                    Some(TokenKind::RightBrace | TokenKind::Eof) | None
                ) {
                    error.span = expr.span;
                }
                error
            })?;
        Ok(Stmt::Expression(ExpressionStmt {
            span: join_spans(expr.span, semicolon.span),
            expr,
        }))
    }

    pub(super) fn parse_statement_declaration(&mut self) -> Result<Declaration, ParserError> {
        let ty = self.parse_non_void_type_specifier()?;
        let name = self.consume_identifier(
            CompilerErrorCode::ParsingVariableList,
            "expected variable name in declaration",
        )?;
        self.parse_declaration_after_name(ty, name)
    }

    pub(super) fn parse_if_statement(&mut self) -> Result<IfStmt, ParserError> {
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

    pub(super) fn parse_switch_statement(&mut self) -> Result<SwitchStmt, ParserError> {
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

    pub(super) fn parse_return_statement(&mut self) -> Result<ReturnStmt, ParserError> {
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

    pub(super) fn parse_while_statement(&mut self) -> Result<WhileStmt, ParserError> {
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

    pub(super) fn parse_do_while_statement(&mut self) -> Result<DoWhileStmt, ParserError> {
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

    pub(super) fn parse_for_statement(&mut self) -> Result<ForStmt, ParserError> {
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

    pub(super) fn parse_case_statement(&mut self) -> Result<CaseStmt, ParserError> {
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

    pub(super) fn parse_default_statement(&mut self) -> Result<DefaultStmt, ParserError> {
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
}
