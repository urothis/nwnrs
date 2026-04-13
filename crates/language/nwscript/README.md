# nwnrs-nwscript

`nwnrs-nwscript` is the home of the workspace's pure Rust NWScript frontend and
compiler toolchain.

## Scope

- source loading and preprocessing
- lexing, parsing, semantic analysis, and optimization
- emission of `NCS` and `NDB` artifacts
- `NCS` asm/disasm through the upstream-style `nwasm` text layer
- typed vocabulary for NWScript-related binary formats

## Internal Structure

- `source` and `preprocess`: source loading and include handling
- `lexer`, `token`, and `parser`: tokenization and syntax construction
- `ast`, `hir`, `sema`, and `ir`: progressively richer semantic and lowered
  program forms
- `opt` and `codegen`: optimization and NCS emission
- `ncs`, `ndb`, and `nwasm`: binary artifact support plus text asm/disasm
- `diag`, `langspec`, and `hash`: diagnostics, builtin language specification,
  and NWScript-specific string hashing

## Non-goals

- act as a runtime or VM for executing compiled scripts
- define editor or IDE behavior on its own
