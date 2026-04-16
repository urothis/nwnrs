# NWScript Compiler

Docs:

- crate: `nwnrs-nwscript`
- [crate docs](https://docs.rs/nwnrs-nwscript/latest/nwnrs_nwscript/)
- [README](https://github.com/urothis/nwnrs/blob/main/crates/language/nwscript/README.md)
- [source](https://github.com/urothis/nwnrs/blob/main/crates/language/nwscript/src/lib.rs)

## Scope

`nwnrs-nwscript` is a compiler subsystem, not just a parser. The public surface is large because the crate intentionally exposes multiple stages and artifact types.

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

- The crate intentionally exposes multiple representations instead of pretending parsing and compilation are one stage.
- `LangSpec` is part of normal compilation, not an optional afterthought.
- HIR and IR are real public control points. They are how the compiler constrains meaning between stages.
- `nwasm` is both an artifact boundary and a debugging surface. The asm layer is part of the correctness story, not just a convenience.
- Source loading and include handling are operationally significant. The compiler is designed to survive real-world script trees, not just idealized single-file input.

## Why This Crate Exists

The point of `nwnrs-nwscript` is to make the language subsystem inspectable and operable at every meaningful stage, from raw source bundle through lowered IR and into bytecode and debug artifacts.
