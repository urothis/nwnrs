#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

mod checking;
mod code_actions;
mod completion;
mod dependencies;
mod diagnostics;
mod events;
mod manifests;
mod operation_error;
mod service;
mod symbols;

pub use checking::{
    CompileCommandOptions, CompileScriptOptions, CompileScriptOutcome, DEFAULT_OPTIMIZATION,
    LanguageCompilerHost, NwScriptCheckOptions, SharedInstallResMan, autodetected_install_resman,
    build_install_resman, build_install_script_resolver, check_nwscript,
    check_nwscript_with_cancellation, compile_generated_script, compile_script_file,
    compile_script_file_with_skip, deduplicate_nwscript_project_roots, nwscript_watch_roots,
    parse_optimizations, run_compile,
};
pub use code_actions::{NwScriptCodeAction, NwScriptCodeActionDiagnostic, NwScriptTextEdit};
pub use completion::{
    NwScriptCompletion, NwScriptCompletionKind, NwScriptSignature, NwScriptSignatureHelp,
    NwScriptSignatureParameter,
};
pub use dependencies::{
    PackageDependencyChange, PackageDependencyPlan, PackageDependencySource,
    PackageDependencySourceContents, PackageDependencyStatus,
};
pub use diagnostics::{
    LanguageCheckReport, LanguageCheckSummary, LanguageDiagnostic, LanguageDiagnosticCode,
    LanguageDiagnosticSeverity, LanguagePosition, LanguageRange, LanguageSource,
    nwscript_utf16_position, nwscript_utf16_range, physical_diagnostic, utf16_position,
    utf16_range,
};
pub use events::{
    EventLanguageCompletionItem, EventLanguageCompletionItemKind, EventLanguageCompletionKind,
    EventLanguageCompletions, EventLanguageSymbol, EventLanguageSymbolKind,
    EventLanguageSymbolToken, event_handler_configuration,
};
pub use manifests::{
    ManifestCompletion, ManifestCompletionKind, ManifestDefinition, ManifestHover, ManifestSymbol,
    ManifestSymbolKind, analyze_manifest_diagnostics, manifest_completions, manifest_definition,
    manifest_hover, manifest_symbols,
};
pub use operation_error::{LanguageOperationError, LanguageResult};
pub use service::{
    LanguageService, OpenDocument, PackageConfiguration, WorkspacePackage, WorkspaceSymbol,
};
pub use symbols::{
    NwScriptDefinitionQuery, NwScriptDocumentSymbol, NwScriptDocumentSymbolKind,
    NwScriptIncludeCandidate, NwScriptInlayHint, NwScriptOutgoingCall, NwScriptProjectDocuments,
    NwScriptProjectIndex, NwScriptReference, NwScriptRenamePlan, NwScriptResolvedSource,
    NwScriptSemanticToken, NwScriptSemanticTokenKind, NwScriptSourceRange,
    NwScriptSymbolDefinition, NwScriptSymbolKind, NwScriptVirtualSource, analyze_nwscript_document,
    find_nwscript_definitions, find_nwscript_include_candidates, find_nwscript_outgoing_calls,
    find_nwscript_references, list_nwscript_document_symbols, list_nwscript_project_sources,
    load_nwscript_builtin_source, load_nwscript_virtual_source, resolve_nwscript_source,
};
