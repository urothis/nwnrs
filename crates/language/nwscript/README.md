# nwnrs-nwscript

`nwnrs-nwscript` is the home of the workspace's pure Rust `NWScript` frontend
and compiler toolchain.

## Scope

- source loading and preprocessing
- lexing, parsing, semantic analysis, and optimization
- emission of `NCS` and `NDB` artifacts
- `NCS` asm and disassembly through the upstream-style `nwasm` text layer
- typed vocabulary for NWScript-related binary formats

## Internal Structure

- `source` and `preprocess`: source loading and include handling
- `lexer`, `token`, and `parser`: tokenization and syntax construction
- `ast`, `hir`, `sema`, and `ir`: progressively richer semantic and lowered
  program forms
- `opt` and `codegen`: optimization and NCS emission
- `ncs`, `ndb`, and `nwasm`: binary artifact support plus text asm and disasm
- `diag`, `langspec`, and `hash`: diagnostics, builtin language specification,
  and NWScript-specific string hashing

## Public Surface by Stage

### Source loading and preprocessing

- `ScriptResolver`
- `SourceBundle`
- `SourceFile`
- `SourceId`
- `SourceMap`
- `SourceLocation`
- `SourceLoadOptions`
- `SourceError`
- `PreprocessedSource`
- `PreprocessError`
- `IncludeDirective`
- `load_source_bundle`
- `preprocess_source_bundle`

### Lexing and parsing

- `Lexer`
- `LexerError`
- `Token`
- `TokenKind`
- `Keyword`
- `MAX_TOKEN_LENGTH`
- `ParseError`
- `ParserError`
- `ResolvedParseError`
- `parse_bytes`
- `parse_text`
- `parse_tokens`
- `parse_source`
- `parse_source_bundle`
- `lex_bytes`
- `lex_text`
- `lex_source`

### Syntax-layer vocabulary

- `Script`
- `TopLevelItem`
- `Declaration`
- `FunctionDecl`
- `StructDecl`
- `StructFieldDecl`
- `Parameter`
- `TypeSpec`
- `TypeKind`
- `Stmt`
- `SimpleStmt`
- `BlockStmt`
- `IfStmt`
- `ForStmt`
- `WhileStmt`
- `DoWhileStmt`
- `SwitchStmt`
- `CaseStmt`
- `DefaultStmt`
- `ReturnStmt`
- `ExpressionStmt`
- `Expr`
- `ExprKind`
- `UnaryOp`
- `BinaryOp`
- `AssignmentOp`
- `VarDeclarator`
- `MagicLiteral`
- `NamedItem`

### Semantic and lowered representations

- `SemanticModel`
- `SemanticFunction`
- `SemanticGlobal`
- `SemanticStruct`
- `SemanticField`
- `SemanticParameter`
- `SemanticType`
- `SemanticOptions`
- `SemanticError`
- `HirModule`
- `HirFunction`
- `HirStmt`
- `HirExpr`
- `HirExprKind`
- `HirStruct`
- `HirField`
- `HirGlobal`
- `HirLocal`
- `HirLocalId`
- `HirLocalKind`
- `HirValueRef`
- `HirBlock`
- `HirParameter`
- `HirCallTarget`
- `HirDeclarator`
- `HirDeclareStmt`
- `HirIfStmt`
- `HirForStmt`
- `HirDoWhileStmt`
- `HirReturnStmt`
- `HirSwitchStmt`
- `HirLowerError`
- `IrModule`
- `IrFunction`
- `IrBlock`
- `IrBlockId`
- `IrInstruction`
- `IrTerminator`
- `IrValueId`
- `IrLocalId`
- `IrGlobal`
- `IrLowerError`
- `analyze_script`
- `analyze_script_with_options`
- `lower_to_hir`
- `lower_hir_to_ir`

### Builtins and language specification

- `LangSpec`
- `LangSpecError`
- `BuiltinType`
- `BuiltinFunction`
- `BuiltinParameter`
- `BuiltinConstant`
- `BuiltinValue`
- `DEFAULT_LANGSPEC_SCRIPT_NAME`
- `NW_SCRIPT_SOURCE_RES_TYPE`
- `load_langspec`
- `parse_langspec`
- `parse_langspec_bytes`
- `parse_langspec_from_source_map`

### Compile and optimization

- `CompileOptions`
- `CompileArtifacts`
- `CompileError`
- `CodegenError`
- `CompilerErrorCode`
- `OptimizationLevel`
- `compile_script`
- `compile_script_with_source_map`
- `compile_source_bundle`
- `compile_hir_to_ncs`

### Artifacts: `NCS`, `NDB`, and asm

- `NCS_HEADER`
- `NCS_BINARY_HEADER_SIZE`
- `NCS_OPERATION_BASE_SIZE`
- `NcsHeader`
- `NcsHeaderError`
- `NcsInstruction`
- `NcsOpcode`
- `NcsAuxCode`
- `NcsReadError`
- `NcsAsmError`
- `NcsAsmLine`
- `NcsDisassemblyOptions`
- `Ndb`
- `NdbFile`
- `NdbFunction`
- `NdbLine`
- `NdbStruct`
- `NdbStructField`
- `NdbType`
- `NdbVariable`
- `NdbError`
- `decode_ncs_header`
- `decode_ncs_instructions`
- `encode_ncs_instructions`
- `disassemble_ncs`
- `render_disassembly_lines`
- `render_ncs_disassembly`
- `render_ncs_disassembly_with_ndb`
- `assemble_ncs_bytes`
- `assemble_ncs_text`
- `read_ndb`
- `parse_ndb_str`
- `write_ndb`

### Miscellaneous semantic helpers

- `Span`
- `nwscript_string_hash`
- `nwscript_string_hash_bytes`

## Logical Edges

- the crate intentionally exposes multiple representations instead of
  pretending parsing and compilation are one stage
- `LangSpec` is part of normal compilation, not an optional afterthought
- HIR and IR are real public control points
- `nwasm` is both an artifact boundary and a debugging surface
- source loading and include handling are operationally significant

## See also

- [`nwnrs-resman`](https://docs.rs/nwnrs-resman), which provides the resource
  layer through which source and compiled script files are typically loaded
- [`nwnrs-install`](https://docs.rs/nwnrs-install), which resolves the install
  root and language root needed to locate `nwscript.nss` for compilation

## Why This Crate Exists

The point of `nwnrs-nwscript` is to make the language subsystem inspectable and
operable at every meaningful stage, from raw source bundle through lowered IR
and into bytecode and debug artifacts.
