use super::*;

impl Parser<'_> {
    pub(super) fn parse_expression(&mut self) -> Result<Expr, ParserError> {
        self.with_syntax_depth(Self::parse_assignment_expression)
    }

    pub(super) fn parse_assignment_expression(&mut self) -> Result<Expr, ParserError> {
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

    pub(super) fn parse_conditional_expression(&mut self) -> Result<Expr, ParserError> {
        self.with_syntax_depth(Self::parse_conditional_expression_inner)
    }

    pub(super) fn parse_conditional_expression_inner(&mut self) -> Result<Expr, ParserError> {
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

    pub(super) fn parse_logical_or_expression(&mut self) -> Result<Expr, ParserError> {
        self.parse_left_associative_binary(
            Self::parse_logical_and_expression,
            &[(TokenKind::LogicalOr, BinaryOp::LogicalOr)],
        )
    }

    pub(super) fn parse_logical_and_expression(&mut self) -> Result<Expr, ParserError> {
        self.parse_left_associative_binary(
            Self::parse_inclusive_or_expression,
            &[(TokenKind::LogicalAnd, BinaryOp::LogicalAnd)],
        )
    }

    pub(super) fn parse_inclusive_or_expression(&mut self) -> Result<Expr, ParserError> {
        self.parse_left_associative_binary(
            Self::parse_exclusive_or_expression,
            &[(TokenKind::InclusiveOr, BinaryOp::InclusiveOr)],
        )
    }

    pub(super) fn parse_exclusive_or_expression(&mut self) -> Result<Expr, ParserError> {
        self.parse_left_associative_binary(
            Self::parse_boolean_and_expression,
            &[(TokenKind::ExclusiveOr, BinaryOp::ExclusiveOr)],
        )
    }

    pub(super) fn parse_boolean_and_expression(&mut self) -> Result<Expr, ParserError> {
        self.parse_left_associative_binary(
            Self::parse_equality_expression,
            &[(TokenKind::BooleanAnd, BinaryOp::BooleanAnd)],
        )
    }

    pub(super) fn parse_equality_expression(&mut self) -> Result<Expr, ParserError> {
        self.parse_left_associative_binary(
            Self::parse_relational_expression,
            &[
                (TokenKind::NotEqual, BinaryOp::NotEqual),
                (TokenKind::EqualEqual, BinaryOp::EqualEqual),
            ],
        )
    }

    pub(super) fn parse_relational_expression(&mut self) -> Result<Expr, ParserError> {
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

    pub(super) fn parse_shift_expression(&mut self) -> Result<Expr, ParserError> {
        self.parse_left_associative_binary(
            Self::parse_additive_expression,
            &[
                (TokenKind::ShiftLeft, BinaryOp::ShiftLeft),
                (TokenKind::ShiftRight, BinaryOp::ShiftRight),
                (TokenKind::UnsignedShiftRight, BinaryOp::UnsignedShiftRight),
            ],
        )
    }

    pub(super) fn parse_additive_expression(&mut self) -> Result<Expr, ParserError> {
        self.parse_left_associative_binary(
            Self::parse_multiplicative_expression,
            &[
                (TokenKind::Plus, BinaryOp::Add),
                (TokenKind::Minus, BinaryOp::Subtract),
            ],
        )
    }

    pub(super) fn parse_multiplicative_expression(&mut self) -> Result<Expr, ParserError> {
        self.parse_left_associative_binary(
            Self::parse_unary_expression,
            &[
                (TokenKind::Multiply, BinaryOp::Multiply),
                (TokenKind::Divide, BinaryOp::Divide),
                (TokenKind::Modulus, BinaryOp::Modulus),
            ],
        )
    }

    pub(super) fn parse_unary_expression(&mut self) -> Result<Expr, ParserError> {
        self.with_syntax_depth(Self::parse_unary_expression_inner)
    }

    pub(super) fn parse_unary_expression_inner(&mut self) -> Result<Expr, ParserError> {
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
        let expr = self.parse_unary_expression()?;
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

    pub(super) fn parse_postfix_expression(&mut self) -> Result<Expr, ParserError> {
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

    pub(super) fn parse_primary_expression(&mut self) -> Result<Expr, ParserError> {
        let token = self.peek().cloned().ok_or_else(|| {
            self.error_here(
                CompilerErrorCode::UnknownStateInCompiler,
                "unexpected EOF in expression",
            )
        })?;

        if token.kind == TokenKind::Identifier && token.text == "match" {
            return self.parse_match_expression();
        }

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
                if self.matches_kind(&TokenKind::Colon)
                    && self
                        .tokens
                        .get(self.position + 1)
                        .is_some_and(|token| token.kind == TokenKind::Colon)
                {
                    self.advance();
                    self.advance();
                    let variant = self.consume_identifier(
                        CompilerErrorCode::InvalidEnumOperation,
                        "expected enum variant after ::",
                    )?;
                    return Ok(Expr {
                        span: join_spans(identifier.span, variant.span),
                        kind: ExprKind::ScopedIdentifier {
                            scope: identifier.text,
                            name:  variant.text,
                        },
                    });
                }
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
            TokenKind::Keyword(Keyword::Int | Keyword::String) => {
                let conversion = self.advance_required(
                    CompilerErrorCode::InvalidEnumOperation,
                    "expected enum conversion type",
                )?;
                if !self.matches_kind(&TokenKind::LeftParen) {
                    return Err(ParserError::new(
                        CompilerErrorCode::InvalidEnumOperation,
                        conversion.span,
                        "int and string are expressions only when used as enum conversions",
                    ));
                }
                let (arguments, end_span) = self.parse_argument_list()?;
                Ok(Expr {
                    span: join_spans(conversion.span, end_span),
                    kind: ExprKind::Call {
                        callee: Box::new(Expr {
                            span: conversion.span,
                            kind: ExprKind::Identifier(conversion.text),
                        }),
                        arguments,
                    },
                })
            }
            _ => Err(self.error_here(
                CompilerErrorCode::UnknownStateInCompiler,
                "unexpected token in expression",
            )),
        }
    }

    pub(super) fn parse_match_expression(&mut self) -> Result<Expr, ParserError> {
        let match_token =
            self.consume_identifier(CompilerErrorCode::InvalidMatch, "expected match")?;
        let value = self.parse_expression()?;
        self.consume_kind(
            TokenKind::LeftBrace,
            CompilerErrorCode::InvalidMatch,
            "expected { after match value",
        )?;
        let mut arms = Vec::new();
        while !self.matches_kind(&TokenKind::RightBrace) && !self.at_eof() {
            let arm_start = self.peek().map_or(match_token.span, |token| token.span);
            let mut patterns = vec![self.parse_match_pattern()?];
            while self.matches_kind(&TokenKind::InclusiveOr) {
                self.advance();
                patterns.push(self.parse_match_pattern()?);
            }
            let guard = if self.matches_keyword(Keyword::If) {
                self.advance();
                // `=>` begins with `=`, so parsing an assignment expression here
                // would incorrectly consume the match-arm delimiter as an
                // assignment operator. Guards deliberately stop at the
                // conditional-expression precedence level.
                Some(self.parse_conditional_expression()?)
            } else {
                None
            };
            self.consume_kind(
                TokenKind::Assign,
                CompilerErrorCode::InvalidMatch,
                "expected => after match pattern",
            )?;
            self.consume_kind(
                TokenKind::GreaterThan,
                CompilerErrorCode::InvalidMatch,
                "expected => after match pattern",
            )?;
            let body = if self.matches_kind(&TokenKind::LeftBrace) {
                MatchArmBody::Block(self.parse_match_block()?)
            } else {
                MatchArmBody::Expr(self.parse_expression()?)
            };
            let arm_end = match &body {
                MatchArmBody::Expr(expression) => expression.span,
                MatchArmBody::Block(block) => block.span,
            };
            let block_body = matches!(body, MatchArmBody::Block(_));
            arms.push(MatchArm {
                span: join_spans(arm_start, arm_end),
                patterns,
                guard,
                body,
            });
            if self.matches_kind(&TokenKind::Comma) {
                self.advance();
            } else if !block_body && !self.matches_kind(&TokenKind::RightBrace) {
                return Err(self.error_here(
                    CompilerErrorCode::InvalidMatch,
                    "expected , after match expression arm",
                ));
            }
        }
        let right = self.consume_kind(
            TokenKind::RightBrace,
            CompilerErrorCode::InvalidMatch,
            "expected } after match arms",
        )?;
        Ok(Expr {
            span: join_spans(match_token.span, right.span),
            kind: ExprKind::Match(MatchExpr {
                value: Box::new(value),
                arms,
            }),
        })
    }

    pub(super) fn parse_match_pattern(&mut self) -> Result<MatchPattern, ParserError> {
        let scope = self.consume_identifier(
            CompilerErrorCode::InvalidMatch,
            "expected enum variant or _ match pattern",
        )?;
        if scope.text == "_" {
            return Ok(MatchPattern::Wildcard {
                span: scope.span
            });
        }
        self.consume_kind(
            TokenKind::Colon,
            CompilerErrorCode::InvalidMatch,
            "enum match patterns must use Enum::Variant",
        )?;
        self.consume_kind(
            TokenKind::Colon,
            CompilerErrorCode::InvalidMatch,
            "enum match patterns must use Enum::Variant",
        )?;
        let variant = self.consume_identifier(
            CompilerErrorCode::InvalidMatch,
            "expected enum variant after ::",
        )?;
        Ok(MatchPattern::Variant {
            span:  join_spans(scope.span, variant.span),
            scope: scope.text,
            name:  variant.text,
        })
    }

    pub(super) fn parse_match_block(&mut self) -> Result<MatchBlock, ParserError> {
        let left = self.consume_kind(
            TokenKind::LeftBrace,
            CompilerErrorCode::InvalidMatch,
            "expected { to start match arm block",
        )?;
        let mut statements = Vec::new();
        let mut tail = None;
        while !self.matches_kind(&TokenKind::RightBrace) && !self.at_eof() {
            if self.starts_definite_statement() {
                statements.push(self.parse_statement()?);
                continue;
            }
            let expression = self.parse_expression()?;
            if self.matches_kind(&TokenKind::Semicolon) {
                let semicolon = self.advance_required(
                    CompilerErrorCode::InvalidMatch,
                    "expected ; after match block expression",
                )?;
                statements.push(Stmt::Expression(ExpressionStmt {
                    span: join_spans(expression.span, semicolon.span),
                    expr: expression,
                }));
            } else if self.matches_kind(&TokenKind::RightBrace) {
                tail = Some(Box::new(expression));
                break;
            } else {
                return Err(self.error_here(
                    CompilerErrorCode::InvalidMatch,
                    "match block tail expression must be last or end with ;",
                ));
            }
        }
        let right = self.consume_kind(
            TokenKind::RightBrace,
            CompilerErrorCode::InvalidMatch,
            "expected } after match arm block",
        )?;
        Ok(MatchBlock {
            span: join_spans(left.span, right.span),
            statements,
            tail,
        })
    }

    pub(super) fn starts_definite_statement(&self) -> bool {
        self.matches_kind(&TokenKind::LeftBrace)
            || self.matches_kind(&TokenKind::Semicolon)
            || self.matches_keyword(Keyword::If)
            || self.matches_keyword(Keyword::Switch)
            || self.matches_keyword(Keyword::Return)
            || self.matches_keyword(Keyword::While)
            || self.matches_keyword(Keyword::Do)
            || self.matches_keyword(Keyword::For)
            || self.matches_keyword(Keyword::Case)
            || self.matches_keyword(Keyword::Default)
            || self.matches_keyword(Keyword::Break)
            || self.matches_keyword(Keyword::Continue)
            || self.matches_identifier_text("static_assert")
            || (!self.matches_identifier_text("match") && self.starts_non_void_type_specifier())
    }

    pub(super) fn parse_argument_list(&mut self) -> Result<(Vec<Expr>, Span), ParserError> {
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

    pub(super) fn parse_literal_expression(&mut self) -> Result<Expr, ParserError> {
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
                let value = crate::float_literal::parse_upstream_float_literal(&token.text);
                (token.span, Literal::Float(value))
            }
            TokenKind::String => {
                self.advance();
                (
                    token.span,
                    Literal::String(crate::ScriptString::from_lexed_text(&token.text)),
                )
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
                return Err(self.error_here(
                    CompilerErrorCode::BadConstantType,
                    "unexpected token in literal expression",
                ));
            }
        };
        Ok(Expr {
            span,
            kind: ExprKind::Literal(literal),
        })
    }

    pub(super) fn parse_vector_literal(&mut self) -> Result<(Span, Literal), ParserError> {
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
            let value = crate::float_literal::parse_upstream_float_literal(&token.text);
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
}
