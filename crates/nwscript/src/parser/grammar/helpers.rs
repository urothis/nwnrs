use super::*;

impl Parser<'_> {
    pub(super) fn parse_left_associative_binary(
        &mut self,
        subparser: fn(&mut Self) -> Result<Expr, ParserError>,
        operators: &[(TokenKind, BinaryOp)],
    ) -> Result<Expr, ParserError> {
        let mut expr = subparser(self)?;
        while let Some(op) = self.current_binary_op(operators) {
            self.check_operation()?;
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

    pub(super) fn with_syntax_depth<T>(
        &mut self,
        operation: fn(&mut Self) -> Result<T, ParserError>,
    ) -> Result<T, ParserError> {
        self.check_operation()?;
        if self.syntax_depth >= self.limits.max_syntax_nesting {
            return Err(self.error_here(
                CompilerErrorCode::UnknownStateInCompiler,
                format!(
                    "syntax nesting exceeded configured maximum of {}",
                    self.limits.max_syntax_nesting
                ),
            ));
        }
        self.syntax_depth += 1;
        let result = operation(self);
        self.syntax_depth -= 1;
        result
    }

    pub(super) fn check_operation(&self) -> Result<(), ParserError> {
        if self
            .cancellation
            .is_some_and(crate::CancellationToken::is_cancelled)
        {
            return Err(self.error_here(
                CompilerErrorCode::UnknownStateInCompiler,
                "operation cancelled",
            ));
        }
        Ok(())
    }

    pub(super) fn current_binary_op(
        &self,
        operators: &[(TokenKind, BinaryOp)],
    ) -> Option<BinaryOp> {
        let token = self.peek()?;
        operators
            .iter()
            .find_map(|(kind, op)| (token.kind == *kind).then_some(*op))
    }

    pub(super) fn current_assignment_op(&self) -> Option<AssignmentOp> {
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

    pub(super) fn starts_non_void_type_specifier(&self) -> bool {
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
            TokenKind::Identifier => {
                self.is_engine_structure_name(token)
                    || self
                        .tokens
                        .get(self.position + 1)
                        .is_some_and(|next| next.kind == TokenKind::Identifier)
            }
            _ => false,
        }
    }

    pub(super) fn is_engine_structure_name(&self, token: &Token) -> bool {
        matches!(token.kind, TokenKind::Identifier)
            && self.engine_structures.contains(token.text.as_str())
    }

    pub(super) fn matches_keyword(&self, keyword: Keyword) -> bool {
        matches!(
            self.peek(),
            Some(Token {
                kind: TokenKind::Keyword(found),
                ..
            }) if *found == keyword
        )
    }

    pub(super) fn matches_kind(&self, kind: &TokenKind) -> bool {
        self.peek().is_some_and(|token| token.kind == *kind)
    }

    pub(super) fn matches_identifier_text(&self, text: &str) -> bool {
        self.peek()
            .is_some_and(|token| token.kind == TokenKind::Identifier && token.text == text)
    }

    pub(super) fn consume_keyword(
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
    pub(super) fn consume_kind(
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

    pub(super) fn consume_identifier(
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

    pub(super) fn consume_string(
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

    pub(super) fn error_here(
        &self,
        code: CompilerErrorCode,
        message: impl Into<String>,
    ) -> ParserError {
        let span = match self.peek() {
            Some(Token {
                kind: TokenKind::Eof,
                ..
            })
            | None => self.previous_non_eof_span(),
            Some(token) => token.span,
        };
        ParserError::new(code, span, message)
    }

    pub(super) fn previous_non_eof_span(&self) -> Span {
        self.tokens
            .get(..self.position.min(self.tokens.len()))
            .unwrap_or_default()
            .iter()
            .rev()
            .find(|token| token.kind != TokenKind::Eof)
            .or_else(|| {
                self.tokens
                    .iter()
                    .find(|token| token.kind != TokenKind::Eof)
            })
            .map_or_else(|| Span::new(SourceId::new(0), 0, 0), |token| token.span)
    }

    pub(super) fn at_eof(&self) -> bool {
        matches!(
            self.peek(),
            Some(Token {
                kind: TokenKind::Eof,
                ..
            }) | None
        )
    }

    pub(super) fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.position)
    }

    pub(super) fn advance(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.position)?.clone();
        self.position += 1;
        Some(token)
    }

    pub(super) fn advance_required(
        &mut self,
        code: CompilerErrorCode,
        message: &str,
    ) -> Result<Token, ParserError> {
        if self.at_eof() {
            Err(self.error_here(code, message))
        } else {
            self.advance().ok_or_else(|| self.error_here(code, message))
        }
    }
}
