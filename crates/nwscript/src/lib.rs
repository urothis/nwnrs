#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

extern crate self as nwnrs_nwscript;

mod ast;
mod codegen;
mod diag;
mod driver;
mod float_literal;
mod graphviz;
mod hash;
mod hir;
mod int_literal;
mod langspec;
mod lexer;
mod ncs;
mod ndb;
mod nwasm;
mod opt;
mod parser;
mod preprocess;
mod script_string;
mod sema;
mod session;
mod source;
mod token;
mod vm;

pub use ast::*;
pub use codegen::*;
pub use diag::*;
pub use driver::*;
pub use graphviz::*;
pub use hash::*;
pub use hir::*;
pub use langspec::*;
pub use lexer::*;
pub use ncs::*;
pub use ndb::*;
pub use nwasm::*;
pub use parser::*;
pub use preprocess::*;
pub use script_string::*;
pub use sema::*;
pub use session::*;
pub use source::*;
pub use token::*;
pub use vm::*;

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::{
        AssignmentOp, BatchCompileEntry, BatchCompileError, BatchCompileOptions,
        BatchCompileReport, BatchCompileStatus, BinaryOp, BlockStmt, BuiltinConstant,
        BuiltinFunction, BuiltinParameter, BuiltinType, BuiltinValue, CaseStmt, CodegenError,
        CompileArtifacts, CompileError, CompileFileOutcome, CompileOptions, CompilerDriverError,
        CompilerDriverOptions, CompilerErrorCode, CompilerHost, CompilerHostError, CompilerSession,
        CompilerSessionError, CompilerSessionOptions, DEFAULT_LANGSPEC_SCRIPT_NAME, Declaration,
        DefaultStmt, DirectoryCompilerHost, DoWhileStmt, Expr, ExprKind, ExpressionStmt,
        FileSystemScriptResolver, ForStmt, FunctionDecl, HirBlock, HirCallTarget, HirDeclarator,
        HirDeclareStmt, HirDoWhileStmt, HirExpr, HirExprKind, HirField, HirForStmt, HirFunction,
        HirGlobal, HirIfStmt, HirLocal, HirLocalId, HirLocalKind, HirLowerError, HirModule,
        HirParameter, HirReturnStmt, HirStmt, HirStruct, HirSwitchStmt, HirValueRef, IfStmt,
        IncludeDirective, Keyword, LangSpec, LangSpecError, Lexer, LexerError, MAX_TOKEN_LENGTH,
        MagicLiteral, NCS_BINARY_HEADER_SIZE, NCS_HEADER, NCS_OPERATION_BASE_SIZE,
        NW_SCRIPT_BINARY_RES_TYPE, NW_SCRIPT_DEBUG_RES_TYPE, NW_SCRIPT_SOURCE_RES_TYPE, NamedItem,
        NcsAsmError, NcsAsmLine, NcsAuxCode, NcsDisassemblyOptions, NcsHeader, NcsHeaderError,
        NcsInstruction, NcsOpcode, NcsReadError, Ndb, NdbError, NdbFile, NdbFunction, NdbLine,
        NdbStruct, NdbStructField, NdbType, NdbVariable, OptimizationFlag, OptimizationFlags,
        OptimizationLevel, Parameter, ParseError, ParserError, PreprocessError, PreprocessedSource,
        ResolvedParseError, ReturnStmt, Script, ScriptResolver, ScriptString, SemanticError,
        SemanticField, SemanticFunction, SemanticGlobal, SemanticModel, SemanticOptions,
        SemanticParameter, SemanticStruct, SemanticType, SimpleStmt, SourceBundle, SourceError,
        SourceFile, SourceId, SourceLoadOptions, SourceLocation, SourceMap, Span, Stmt, StructDecl,
        StructFieldDecl, SwitchStmt, Token, TokenKind, TopLevelItem, TypeKind, TypeSpec, UnaryOp,
        VarDeclarator, Vm, VmCommandHandler, VmEngineStructureComparer, VmEngineStructureFactory,
        VmEngineStructureValue, VmError, VmFunctionInfo, VmObjectId, VmRunOptions, VmScript,
        VmSituation, VmSourceLocation, VmStepOutcome, VmTraceEvent, VmTraceHook, VmValue,
        WhileStmt, analyze_script, analyze_script_with_options, assemble_ncs_bytes,
        assemble_ncs_text, compile_file_with_host, compile_hir_to_ncs, compile_paths,
        compile_script, compile_script_with_source_map, compile_source_bundle, decode_ncs_header,
        decode_ncs_instructions, disassemble_ncs, encode_ncs_instructions, lex_bytes, lex_source,
        lex_text, load_langspec, load_source_bundle, lower_to_hir, nwscript_string_hash,
        nwscript_string_hash_bytes, parse_bytes, parse_langspec, parse_langspec_bytes,
        parse_langspec_from_source_map, parse_ndb_str, parse_resolved_script, parse_source,
        parse_source_bundle, parse_text, parse_tokens, preprocess_source_bundle, read_ndb,
        render_disassembly_lines, render_ncs_disassembly, render_ncs_disassembly_with_ndb,
        render_script_graphviz, write_ndb,
    };
}
