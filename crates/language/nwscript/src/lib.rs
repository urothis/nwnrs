#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

mod ast;
mod codegen;
mod diag;
mod hash;
mod hir;
mod int_literal;
mod ir;
mod langspec;
mod lexer;
mod ncs;
mod ndb;
mod opt;
mod parser;
mod preprocess;
mod sema;
mod source;
mod token;

pub use ast::*;
pub use codegen::*;
pub use diag::*;
pub use hash::*;
pub use hir::*;
pub use ir::*;
pub use langspec::*;
pub use lexer::*;
pub use ncs::*;
pub use ndb::*;
pub use parser::*;
pub use preprocess::*;
pub use sema::*;
pub use source::*;
pub use token::*;

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::{
        AssignmentOp, BinaryOp, BlockStmt, BuiltinConstant, BuiltinFunction, BuiltinParameter,
        BuiltinType, BuiltinValue, CaseStmt, CodegenError, CompileArtifacts, CompileError,
        CompileOptions, CompilerErrorCode, DEFAULT_LANGSPEC_SCRIPT_NAME, Declaration, DefaultStmt,
        DoWhileStmt, Expr, ExprKind, ExpressionStmt, ForStmt, FunctionDecl, HirBlock,
        HirCallTarget, HirDeclarator, HirDeclareStmt, HirDoWhileStmt, HirExpr, HirExprKind,
        HirField, HirForStmt, HirFunction, HirGlobal, HirIfStmt, HirLocal, HirLocalId,
        HirLocalKind, HirLowerError, HirModule, HirParameter, HirReturnStmt, HirStmt, HirStruct,
        HirSwitchStmt, HirValueRef, IfStmt, IncludeDirective, IrBlock, IrBlockId, IrFunction,
        IrGlobal, IrInstruction, IrLocalId, IrLowerError, IrModule, IrTerminator, IrValueId,
        Keyword, LangSpec, LangSpecError, Lexer, LexerError, MAX_TOKEN_LENGTH, MagicLiteral,
        NCS_BINARY_HEADER_SIZE, NCS_HEADER, NCS_OPERATION_BASE_SIZE, NW_SCRIPT_SOURCE_RES_TYPE,
        NamedItem, NcsAuxCode, NcsHeader, NcsHeaderError, NcsInstruction, NcsOpcode, NcsReadError,
        Ndb, NdbError, NdbFile, NdbFunction, NdbLine, NdbStruct, NdbStructField, NdbType,
        NdbVariable, OptimizationLevel, Parameter, ParseError, ParserError, PreprocessError,
        PreprocessedSource, ResolvedParseError, ReturnStmt, Script, ScriptResolver, SemanticError,
        SemanticField, SemanticFunction, SemanticGlobal, SemanticModel, SemanticOptions,
        SemanticParameter, SemanticStruct, SemanticType, SimpleStmt, SourceBundle, SourceError,
        SourceFile, SourceId, SourceLoadOptions, SourceLocation, SourceMap, Span, Stmt, StructDecl,
        StructFieldDecl, SwitchStmt, Token, TokenKind, TopLevelItem, TypeKind, TypeSpec, UnaryOp,
        VarDeclarator, WhileStmt, analyze_script, analyze_script_with_options, compile_hir_to_ncs,
        compile_script, compile_script_with_source_map, compile_source_bundle, decode_ncs_header,
        decode_ncs_instructions, encode_ncs_instructions, lex_bytes, lex_source, lex_text,
        load_langspec, load_source_bundle, lower_hir_to_ir, lower_to_hir, nwscript_string_hash,
        nwscript_string_hash_bytes, parse_bytes, parse_langspec, parse_langspec_bytes,
        parse_langspec_from_source_map, parse_ndb_str, parse_resolved_script, parse_source,
        parse_source_bundle, parse_text, parse_tokens, preprocess_source_bundle, read_ndb,
        write_ndb,
    };
}
