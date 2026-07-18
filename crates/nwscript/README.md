# nwnrs-nwscript

`nwnrs-nwscript` is the workspace's pure Rust `NWScript` frontend and compiler
toolchain. It depends on `nwnrs-types` for shared IO and resource vocabulary,
but owns the language pipeline itself.

## Scope

- source loading and preprocessing
- lexing, parsing, semantic analysis, and optimization
- emission of `NCS` and `NDB` artifacts
- `NCS` asm and disassembly through the upstream-style `nwasm` text layer
- a lightweight `NCS` VM and test-runner layer for exercising compiled scripts
- typed vocabulary for NWScript-related binary formats

## Internal Structure

- `source` and `preprocess`: source loading and include handling
- `lexer`, `token`, and `parser`: tokenization and syntax construction
- `ast`, `sema`, and `hir`: syntax, semantic analysis, and the lowered program
  form consumed directly by code generation
- `opt` and `codegen`: optimization and NCS emission
- `ncs`, `ndb`, and `nwasm`: binary artifact support plus text asm and disasm
- `vm`: lightweight bytecode execution and action-command test harness
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
- `Literal`
- `NamedItem`
- `ScriptString`

`ScriptString` stores the exact bytes carried by source literals, NCS
constants, folded expressions, and VM string values. It deliberately does not
select UTF-8 or a legacy code page; callers can request UTF-8 with
`ScriptString::as_str` when their boundary requires text.

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
- `analyze_script`
- `analyze_script_with_options`
- `lower_to_hir`

### Builtins and language specification

- `LangSpec`
- `LangSpecError`
- `BuiltinType`
- `BuiltinFunction`
- `BuiltinParameter`
- `BuiltinConstant`
- `BuiltinValue`
- `DEFAULT_LANGSPEC_SCRIPT_NAME`
- `NW_SCRIPT_BINARY_RES_TYPE`
- `NW_SCRIPT_DEBUG_RES_TYPE`
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
- `MAX_COMPILER_IDENTIFIERS`
- `MAX_COMPILER_RUNTIME_CELLS`
- `OptimizationFlag`
- `OptimizationFlags`
- `OptimizationLevel`
- `compile_script`
- `compile_script_with_source_map`
- `compile_source_bundle`
- `compile_hir_to_ncs`

The three native optimization passes can be selected independently with
`OptimizationFlags`. The `OptimizationLevel` convenience presets map O1 to
unreachable-function removal, O2 additionally to constant dead-branch
removal, and O3 additionally to instruction melding. When a source map is
supplied, every flag combination emits matching NDB metadata.
Compiler-limit failures expose their upstream-aligned code through
`CodegenError::code`.

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

### Compiler sessions

- `CompilerSession`
- `CompilerSessionError`
- `CompilerSessionOptions`

### Compiler driver

- `CompilerHost`
- `CompilerHostError`
- `CompilerDriverOptions`
- `CompilerDriverError`
- `CompileFileOutcome`
- `compile_file_with_host`
- `FileSystemScriptResolver`
- `DirectoryCompilerHost`
- `SharedScriptResolver`
- `BatchCompileOptions`
- `BatchCompileEntry`
- `BatchCompileStatus`
- `BatchCompileReport`
- `BatchCompileError`
- `GraphvizOutputFormat`
- `format_source_aware_driver_error`
- `compile_paths`

`compile_paths` accepts files and directory trees, preserves relative output
paths, supports bounded parallel workers, validates output collisions before
writing, and can consult a shared fallback resolver after local source roots.
This is the same public batch path consumed by the `nwnrs compile` command.
Graphviz output can remain DOT source or be rendered through the `dot`
executable as SVG, PNG, or PDF while preserving the same directory hierarchy.

### Graphviz

- `render_script_graphviz`

### VM and test execution

- `Vm`
- `VmRunOptions`
- `VmScript`
- `VmSituation`
- `VmStepOutcome`
- `VmTraceEvent`
- `VmTraceHook`
- `VmValue`
- `VmObjectId`
- `VmEngineStructureComparer`
- `VmEngineStructureFactory`
- `VmEngineStructureValue`
- `VmFunctionInfo`
- `VmCommandHandler`
- `VmError`
- `VmSourceLocation`

Normal compiled scripts use the native callee-clean function ABI and run
without `NDB` metadata. The direct named-function runner uses `NDB` to locate
the requested function and describe its arguments and return value; it also
boots global initializers automatically for
`main()` and `StartingConditional()` entry loaders. The debugger-oriented VM
surface also supports single-step execution plus `step_over`, `step_out`,
`run_until_offset`, `run_until_function`, and `run_until_line` when attached
`NDB` metadata is available. `VmRunOptions` can bound instructions, recursion,
and stack cells; budget failures and division by zero map back to the matching
native `CompilerErrorCode` through `VmError::code()`.

## VM Spec

`nwnrs-nwscript` includes a debugger-oriented `NCS V1.0` VM. It is intended
for compiler validation, script inspection, action-command testing, and
host-driven execution. This section documents the behavior implemented in
[`vm.rs`](./src/vm.rs).

### Runtime Model

- `ip` is a byte offset into the decoded code section, not an instruction
  index.
- `sp` and `bp` are measured in stack cells. One cell is one logical 4-byte
  slot.
- Scalar runtime values occupy one cell: `int`, `float`, `string`, `object`,
  and engine structures.
- Vectors are stored as three adjacent float cells in `x`, `y`, `z` order.
  They are not represented as a dedicated `VmValue` variant.
- Engine structures are represented as `VmValue::EngineStructure { index,
  value }`. The payload is either one opaque `u32` word or one text value.
- The VM keeps a separate return-frame stack from the value stack.
- The first `step()` or `run()` call synthesizes one outer halt frame.
  Returning from that frame halts the script.

### Program Loading And Metadata

- `VmScript::from_bytes` decodes one full `NCS V1.0` stream and assigns each
  instruction a stable byte offset.
- Relative branches and call targets are resolved from the current
  instruction's byte offset.
- `VmScript::attach_ndb` adds function ranges and source line mappings. NDB
  file-relative offsets are normalized to code-section-relative VM offsets at
  this boundary.
- Compiled user functions follow the native callee-clean ABI, so ordinary
  execution does not require NDB metadata.

### Stack And Frame Conventions

- `SAVEBP` pushes the previous base pointer as an integer cell, then repoints
  `bp` at that saved cell.
- `RESTOREBP` pops one integer cell and installs it as the new base pointer.
- `RSADD` pushes one zero/default value selected by auxcode.
- `CONST` pushes one immediate constant selected by auxcode.
- `CPTOPSP` and `CPTOPBP` copy one or more cells from a negative,
  4-byte-aligned offset relative to `sp` or `bp` to the current stack top.
- `CPDOWNSP` and `CPDOWNBP` copy the current top cells back into an earlier
  stack window addressed relative to `sp` or `bp`.
- `MOVSP` with a positive delta grows the stack by pushing zeroed integer
  cells. `MOVSP` with a negative delta truncates the stack.
- `INCSP`, `DECSP`, `INCBP`, and `DECBP` mutate one addressed integer cell by
  `+1` or `-1`.
- `DESTRUCT` removes part of a struct-shaped stack region and compacts the
  surviving cells down.
- `RET` pops one return frame. Compiler-emitted `MOVSP` instructions remove
  parameters and locals before `RET`, preserving any caller-allocated return
  slot.

### Instruction Semantics

| Group | Instructions | Behavior |
| --- | --- | --- |
| Control flow | `NOP`, `JMP`, `JSR`, `JZ`, `JNZ`, `RET` | Jumps and calls operate on byte offsets. `JZ` and `JNZ` pop one integer condition. |
| Saved continuations | `STOREIP`, `STORESTATE` | `STOREIP` records one future byte offset. `STORESTATE` also snapshots the leading `(global_bytes + stack_bytes) / 4` stack cells into one `VmSituation`. |
| Integer and boolean ops | `LOGAND`, `LOGOR`, `INCOR`, `EXCOR`, `BOOLAND`, `NOT`, `COMP`, `SHLEFT`, `SHRIGHT`, `USHRIGHT`, `MOD` | These operate on integer cells. Boolean results are normalized to `0` or `1`. |
| Arithmetic | `ADD`, `SUB`, `MUL`, `DIV`, `NEG` | Mixed `int`/`float` arithmetic is supported. `ADD` also supports `string + string` and `vector + vector`. `SUB` supports `vector - vector`. `MUL` supports `vector * float` and `float * vector`. `DIV` supports `vector / float`. Integer `ADD`, `SUB`, and `MUL` use wrapping semantics. |
| Comparison | `EQUAL`, `NEQUAL`, `LT`, `GT`, `LEQ`, `GEQ` | Ordered comparisons are supported for numeric values and strings. Objects support equality only. Vectors, engine structures, and raw struct cell-ranges support equality and inequality only. |
| Host interop | `ACTION` | Dispatches one registered host command by numeric action id and encoded argument count. |

Unsupported auxcodes, invalid operand type combinations, and unimplemented
behavior return `VmError::Unsupported` rather than silently approximating
native behavior. Division or modulus by zero reports `VmError::DivideByZero`
with the native `VmDivideByZero` diagnostic mapping.

### Runtime Values

- `int` values are `i32`.
- `float` values are `f32`.
- `string` values preserve the exact bytes from `CONST` payloads in
  `ScriptString`.
- `object` values are opaque `u32` ids.
- Engine-structure defaults come from `Vm::define_engine_structure`, falling
  back to zeroed words for most indices and empty text for index `7`.
- Engine-structure equality can be overridden with
  `Vm::define_engine_structure_comparer`.

### Actions And Abort Semantics

- `ACTION` looks up one handler by numeric command id. Missing handlers produce
  `VmError::InvalidCommand`.
- Handlers receive `&mut VmScript`, the command id, and the encoded argument
  count.
- A handler may mutate the stack directly, inspect debug state, or request a
  clean stop through `VmScript::abort()`.
- Abort requests are consumed at the dispatcher boundary. They clear the return
  stack and end the run with `VmStepOutcome::Aborted`.

### Saved Situations

- `STORESTATE` captures one resumable `VmSituation`.
- The saved continuation stores the script label, decoded program, target
  instruction pointer, current base pointer, saved stack pointer, and a clone
  of the visible stack prefix.
- `Vm::run_situation` resumes from that snapshot by rehydrating a fresh
  `VmScript`.

### Direct Function Invocation

- `Vm::run_function_bytes` and `VmScript::prepare_function_call` can invoke one
  named user function directly from attached `NDB` metadata.
- If the script has globals, the VM first bootstraps them by running the entry
  loader (`main` or `StartingConditional`) until the loader's first
  instruction is patched to `RET`, then it keeps only the initialized globals
  frame.
- Direct calls support scalar, object, string, engine-structure, and recursively
  represented `VmValue::Struct` arguments and return values.

### Debugger Surface

- `VmTraceHook` fires before each instruction executes and exposes `ip`, `sp`,
  `bp`, the current byte offset, and the decoded instruction.
- `step_over` executes through one user-function call and stops when control
  returns to the caller.
- `step_out` runs until the current function returns.
- `run_until_offset`, `run_until_function`, and `run_until_line` provide
  debugger-style break-on-target behavior.
- `VmRunOptions::max_instructions` enforces a hard instruction budget and
  reports `VmError::InstructionLimitExceeded` when the budget is exhausted.

### Miscellaneous semantic helpers

- `Span`
- `nwscript_string_hash`
- `nwscript_string_hash_bytes`

## Logical Edges

- the crate intentionally exposes multiple representations instead of
  pretending parsing and compilation are one stage
- `LangSpec` is part of normal compilation, not an optional afterthought
- HIR is the lowered public control point consumed by code generation
- `nwasm` is both an artifact boundary and a debugging surface
- source loading and include handling are operationally significant
- focused installation-backed tests compare compilation and runtime behavior
  against matching shipped source and bytecode

## Reference Compiler Parity Audit

On 2026-07-17, every unique NSS resource exposed by the standard NWN
installation resource manager was compiled independently with both the current
reference C++ compiler used by `neverwinter.nim` and this Rust compiler at O1.
The installation contained 4,144 unique NSS resources. The reference compiler
accepted 4,000 of them, and all 4,000 Rust NCS outputs were byte-for-byte
identical to the corresponding reference-compiler outputs.

The remaining 144 resources were exempt from byte comparison because the
reference compiler did not produce NCS output for them:

- 92 returned `-623` because they are include or library sources without a
  `main()` or `StartingConditional()` entrypoint.
- 25 returned `-582` for declarations without a type.
- 15 returned `-622` for undefined identifiers.
- 7 returned `-566` for an unknown compiler state in malformed legacy input.
- 1 returned `-6804` for a case label jumping over a declaration.
- 1 returned `-617` for a function definition/implementation mismatch.
- 1 returned `-603` for a missing included file.
- 1 returned `-567` for an invalid declaration type; this is `nwscript.nss`,
  the compiler language specification rather than an executable script.
- 1 returned `-565` while parsing a malformed variable list.

These are reference-compiler rejections, not Rust-only exclusions. With no
reference NCS bytes, they cannot participate in a byte-equality comparison.
The exhaustive audit is recorded here as validation evidence; it is not kept as
an integration test or coupled to the untracked reference source tree.

## See also

- [`nwnrs_types::resman`](https://docs.rs/nwnrs-types/latest/nwnrs_types/resman/), which provides the resource
  layer through which source and compiled script files are typically loaded
- [`nwnrs_types::install`](https://docs.rs/nwnrs-types/latest/nwnrs_types/install/),
  which resolves the install root and language root needed to locate
  `nwscript.nss` for compilation

## Why This Crate Exists

The point of `nwnrs-nwscript` is to make the language subsystem inspectable and
operable at every meaningful stage, from raw source bundle through lowered HIR
and into bytecode and debug artifacts.
