use std::collections::HashSet;

use crate::{
    AssignmentOp, BinaryOp, BlockStmt, CaseStmt, CompilerErrorCode, Declaration, DefaultStmt,
    DoWhileStmt, EnumBackingType, EnumDecl, EnumVariantDecl, Expr, ExprKind, ExpressionStmt,
    ForStmt, FunctionDecl, IfStmt, IncludeDirective, Keyword, LangSpec, Literal, MagicLiteral,
    MatchArm, MatchArmBody, MatchBlock, MatchExpr, MatchPattern, NamedItem, Parameter, ReturnStmt,
    Script, SimpleStmt, Span, StaticAssertDecl, Stmt, StructDecl, StructFieldDecl, SwitchStmt,
    Token, TokenKind, TopLevelItem, TypeAliasDecl, TypeKind, TypeSpec, UnaryOp, VarDeclarator,
    WhileStmt,
    int_literal::{parse_wrapping_decimal_i32, parse_wrapping_prefixed_i32},
    lexer::{LexerError, lex_source_with_limits_and_cancellation},
    preprocess::{PreprocessError, preprocess_source_bundle},
    source::{SourceFile, SourceId},
};

mod api;
mod grammar;

pub use api::*;

struct Parser<'a> {
    tokens:            Vec<Token>,
    position:          usize,
    engine_structures: HashSet<&'a str>,
    limits:            crate::CompilerResourceLimits,
    cancellation:      Option<&'a crate::CancellationToken>,
    syntax_depth:      usize,
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
            Self::StaticAssert(stmt) => stmt.span,
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
mod tests;
