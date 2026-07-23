//! In-process Node-API access to reusable nwnrs editor tooling.

mod resource_editor;
mod viewer;

mod resource_capabilities {
    include!(concat!(env!("OUT_DIR"), "/resource_capabilities.rs"));
}

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
};

use napi::{
    Task,
    bindgen_prelude::{AbortSignal, AsyncTask},
};
use napi_derive::napi;
use nwnrs::{
    NwScriptCheckOptions, NwScriptDefinitionQuery, NwScriptDocumentSymbol,
    NwScriptDocumentSymbolKind, NwScriptProjectIndex, NwScriptSemanticTokenKind,
    NwScriptSourceRange, NwScriptSymbolDefinition, NwScriptSymbolKind, analyze_nwscript_document,
    check_nwscript, check_nwscript_with_cancellation, deduplicate_nwscript_project_roots,
    find_nwscript_definitions, find_nwscript_include_candidates, find_nwscript_outgoing_calls,
    find_nwscript_references, list_nwscript_document_symbols, load_nwscript_virtual_source,
    nwscript_watch_roots, resolve_nwscript_source,
};
use nwnrs_nwscript::{BatchCompileStatus, CompilerDiagnostic};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
struct CheckRequest {
    paths: Vec<PathBuf>,
    #[serde(default = "enabled")]
    no_entrypoint_check: bool,
    #[serde(default)]
    langspec: Option<PathBuf>,
    #[serde(default)]
    include_dirs: Vec<PathBuf>,
    #[serde(default)]
    overlays: Vec<SourceOverlay>,
    #[serde(default = "default_optimization")]
    optimization: String,
    #[serde(default)]
    optimization_flags: Vec<String>,
    #[serde(default = "default_include_depth")]
    max_include_depth: usize,
    #[serde(default = "default_diagnostic_limit")]
    max_diagnostics_per_input: usize,
    #[serde(default)]
    recurse: bool,
    #[serde(default)]
    follow_symlinks: bool,
    #[serde(default)]
    jobs: Option<usize>,
    #[serde(default)]
    root: Option<PathBuf>,
    #[serde(default)]
    user: Option<PathBuf>,
    #[serde(default = "default_language")]
    language: String,
    #[serde(default)]
    load_ovr: bool,
}

#[derive(Debug, Deserialize)]
struct SourceOverlay {
    path:     PathBuf,
    contents: String,
}

#[derive(Debug, Serialize)]
struct CheckResponse {
    diagnostics: Vec<CheckDiagnostic>,
    summary:     CheckSummary,
}

#[derive(Debug, Serialize)]
struct CheckDiagnostic {
    input:        PathBuf,
    severity:     &'static str,
    code:         Option<i32>,
    message:      String,
    file:         Option<String>,
    start_line:   Option<usize>,
    start_column: Option<usize>,
    end_line:     Option<usize>,
    end_column:   Option<usize>,
}

#[derive(Debug, Serialize)]
struct CheckSummary {
    compiled: usize,
    skipped:  usize,
    failed:   usize,
}

#[derive(Debug, Deserialize)]
struct DefinitionRequest {
    source_path:       PathBuf,
    symbol:            String,
    #[serde(default)]
    qualifier:         Option<String>,
    #[serde(default)]
    project_root:      Option<PathBuf>,
    #[serde(default)]
    include_dirs:      Vec<PathBuf>,
    #[serde(default)]
    overlays:          Vec<SourceOverlay>,
    #[serde(default)]
    langspec:          Option<PathBuf>,
    #[serde(default = "default_include_depth")]
    max_include_depth: usize,
    #[serde(default)]
    root:              Option<PathBuf>,
    #[serde(default)]
    user:              Option<PathBuf>,
    #[serde(default = "default_language")]
    language:          String,
    #[serde(default)]
    load_ovr:          bool,
}

impl DefinitionRequest {
    fn into_query(self) -> NwScriptDefinitionQuery {
        let source_overlays = self
            .overlays
            .into_iter()
            .map(|overlay| (overlay.path, overlay.contents.into_bytes()))
            .collect::<BTreeMap<_, _>>();
        NwScriptDefinitionQuery {
            source_path: self.source_path,
            symbol: self.symbol,
            qualifier: self.qualifier,
            project_root: self.project_root,
            include_directories: self.include_dirs,
            source_overlays,
            langspec: self.langspec,
            max_include_depth: self.max_include_depth,
            root: self.root,
            user: self.user,
            language: self.language,
            load_ovr: self.load_ovr,
        }
    }
}

#[derive(Debug, Serialize)]
struct DefinitionResponse {
    name:              String,
    kind:              &'static str,
    path:              PathBuf,
    start_line:        usize,
    start_column:      usize,
    end_line:          usize,
    end_column:        usize,
    signature:         String,
    documentation:     Option<String>,
    is_implementation: bool,
    uri:               Option<String>,
    resource:          Option<String>,
}

#[derive(Debug, Serialize)]
struct VirtualSourceResponse {
    uri:      String,
    contents: String,
}

#[derive(Debug, Serialize)]
struct ResolvedSourceResponse {
    path:     PathBuf,
    uri:      Option<String>,
    resource: Option<String>,
}

#[derive(Debug, Serialize)]
struct IncludeCandidateResponse {
    include_name: String,
    path:         PathBuf,
    start_line:   usize,
    start_column: usize,
}

#[derive(Debug, Deserialize)]
struct ReferencesRequest {
    #[serde(flatten)]
    definition: DefinitionRequest,
    line:       usize,
    column:     usize,
}

#[derive(Default)]
struct LanguageServiceState {
    sessions: Mutex<BTreeMap<String, Arc<ProjectSession>>>,
}

impl LanguageServiceState {
    fn session(&self, key: &str) -> napi::Result<Arc<ProjectSession>> {
        let mut sessions = self.sessions.lock().map_err(|error| {
            napi::Error::from_reason(format!("language-service session map is poisoned: {error}"))
        })?;
        Ok(Arc::clone(
            sessions
                .entry(key.to_string())
                .or_insert_with(|| Arc::new(ProjectSession::default())),
        ))
    }
}

#[derive(Default)]
struct PendingInvalidations {
    clear_all: bool,
    paths:     BTreeSet<PathBuf>,
}

#[derive(Default)]
struct ProjectSession {
    index:   Mutex<NwScriptProjectIndex>,
    pending: Mutex<PendingInvalidations>,
}

impl ProjectSession {
    fn queue_invalidation(&self, changed_path: Option<&str>) {
        let mut pending = match self.pending.lock() {
            Ok(pending) => pending,
            Err(error) => error.into_inner(),
        };
        let Some(changed_path) = changed_path else {
            pending.clear_all = true;
            pending.paths.clear();
            return;
        };
        let path = PathBuf::from(changed_path);
        if path.file_name().is_some_and(|name| name == "nwpkg.toml") {
            pending.clear_all = true;
            pending.paths.clear();
        } else if !pending.clear_all {
            pending.paths.insert(path);
        }
    }

    fn lock_index(&self) -> napi::Result<MutexGuard<'_, NwScriptProjectIndex>> {
        let mut index = self.index.lock().map_err(|error| {
            napi::Error::from_reason(format!(
                "language-service project index is poisoned: {error}"
            ))
        })?;
        let mut pending = self.pending.lock().map_err(|error| {
            napi::Error::from_reason(format!(
                "language-service invalidation queue is poisoned: {error}"
            ))
        })?;
        if pending.clear_all {
            index.clear();
        } else {
            for path in &pending.paths {
                index.invalidate_path(path);
            }
        }
        *pending = PendingInvalidations::default();
        drop(pending);
        Ok(index)
    }
}

/// One persistent native language service with an independent compiler index
/// per package.
#[napi]
pub struct LanguageService {
    state: Arc<LanguageServiceState>,
}

#[napi]
impl LanguageService {
    /// Creates an empty persistent service.
    #[napi(constructor)]
    pub fn new() -> Self {
        Self {
            state: Arc::new(LanguageServiceState::default()),
        }
    }

    /// Executes one language request away from the JavaScript event loop.
    #[napi]
    pub fn execute(
        &self,
        method: String,
        request_json: String,
        session_key: String,
        signal: Option<AbortSignal>,
    ) -> AsyncTask<LanguageServiceTask> {
        let cancellation = nwnrs_nwscript::CancellationToken::new();
        if let Some(signal) = &signal {
            let cancellation = cancellation.clone();
            signal.on_abort(move || cancellation.cancel());
        }
        AsyncTask::with_optional_signal(
            LanguageServiceTask {
                state: Arc::clone(&self.state),
                method,
                request_json,
                session_key,
                cancellation,
            },
            signal,
        )
    }

    /// Invalidates every indexed unit that consumed a changed source path.
    #[napi]
    pub fn invalidate(&self, session_key: Option<String>, changed_path: Option<String>) {
        let Ok(sessions) = self.state.sessions.lock() else {
            return;
        };
        for (key, session) in sessions.iter() {
            if session_key
                .as_deref()
                .is_some_and(|requested| requested != key)
            {
                continue;
            }
            session.queue_invalidation(changed_path.as_deref());
        }
    }

    /// Releases a package session when its workspace closes.
    #[napi]
    pub fn release(&self, session_key: String) {
        if let Ok(mut sessions) = self.state.sessions.lock() {
            sessions.remove(&session_key);
        }
    }
}

impl Default for LanguageService {
    fn default() -> Self {
        Self::new()
    }
}

/// One asynchronous request owned by [`LanguageService`].
pub struct LanguageServiceTask {
    state:        Arc<LanguageServiceState>,
    method:       String,
    request_json: String,
    session_key:  String,
    cancellation: nwnrs_nwscript::CancellationToken,
}

impl Task for LanguageServiceTask {
    type JsValue = String;
    type Output = String;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        self.cancellation
            .check()
            .map_err(|error| napi::Error::from_reason(error.to_string()))?;
        let response = match self.method.as_str() {
            "checkNss" => check_nss_impl(self.request_json.clone(), Some(&self.cancellation)),
            "findDefinitions" => self.find_definitions(),
            "findIncludeCandidates" => self.find_include_candidates(),
            "findReferences" => self.find_references(),
            "findOutgoingCalls" => self.outgoing_calls(),
            "listDocumentSymbols" => self.document_symbols(),
            "indexProject" => self.index_project(),
            "analyzeDocument" => self.analyze_document(),
            _ => invoke_language_method(&self.method, self.request_json.clone()),
        }?;
        self.cancellation
            .check()
            .map_err(|error| napi::Error::from_reason(error.to_string()))?;
        Ok(response)
    }

    fn resolve(&mut self, _env: napi::Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        Ok(output)
    }
}

impl LanguageServiceTask {
    fn project_session(&self) -> napi::Result<Arc<ProjectSession>> {
        self.state.session(&self.session_key)
    }

    fn find_references(&self) -> napi::Result<String> {
        let request =
            serde_json::from_str::<ReferencesRequest>(&self.request_json).map_err(|error| {
                napi::Error::from_reason(format!("invalid references request: {error}"))
            })?;
        let session = self.project_session()?;
        let mut session = session.lock_index()?;
        let query = request.definition.into_query();
        let references = session
            .find_references(
                &query,
                request.line,
                request.column,
                Some(&self.cancellation),
            )
            .map_err(napi::Error::from_reason)?;
        encode_reference_response(references)
    }

    fn find_definitions(&self) -> napi::Result<String> {
        let request =
            serde_json::from_str::<DefinitionRequest>(&self.request_json).map_err(|error| {
                napi::Error::from_reason(format!("invalid definition request: {error}"))
            })?;
        let session = self.project_session()?;
        let mut session = session.lock_index()?;
        let definitions = session
            .definitions(&request.into_query(), Some(&self.cancellation))
            .map_err(napi::Error::from_reason)?
            .into_iter()
            .map(definition_response)
            .collect::<Vec<_>>();
        serde_json::to_string(&definitions).map_err(|error| {
            napi::Error::from_reason(format!("failed to encode definition response: {error}"))
        })
    }

    fn find_include_candidates(&self) -> napi::Result<String> {
        let request =
            serde_json::from_str::<DefinitionRequest>(&self.request_json).map_err(|error| {
                napi::Error::from_reason(format!("invalid include-candidate request: {error}"))
            })?;
        let session = self.project_session()?;
        let mut session = session.lock_index()?;
        let candidates = session
            .include_candidates(&request.into_query(), Some(&self.cancellation))
            .map_err(napi::Error::from_reason)?
            .into_iter()
            .map(|candidate| IncludeCandidateResponse {
                include_name: candidate.include_name,
                path:         candidate.definition.path,
                start_line:   candidate.definition.start_line,
                start_column: candidate.definition.start_column,
            })
            .collect::<Vec<_>>();
        serde_json::to_string(&candidates).map_err(|error| {
            napi::Error::from_reason(format!("failed to encode include candidates: {error}"))
        })
    }

    fn outgoing_calls(&self) -> napi::Result<String> {
        let request =
            serde_json::from_str::<ReferencesRequest>(&self.request_json).map_err(|error| {
                napi::Error::from_reason(format!("invalid outgoing-calls request: {error}"))
            })?;
        let session = self.project_session()?;
        let mut session = session.lock_index()?;
        let calls = session
            .outgoing_calls(
                &request.definition.into_query(),
                request.line,
                Some(&self.cancellation),
            )
            .map_err(napi::Error::from_reason)?
            .into_iter()
            .map(|call| OutgoingCallResponse {
                target: definition_response(call.target),
                ranges: call.ranges.into_iter().map(Into::into).collect(),
            })
            .collect::<Vec<_>>();
        serde_json::to_string(&calls).map_err(|error| {
            napi::Error::from_reason(format!("failed to encode outgoing calls: {error}"))
        })
    }

    fn document_symbols(&self) -> napi::Result<String> {
        let request = serde_json::from_str::<DocumentSymbolsRequest>(&self.request_json).map_err(
            |error| napi::Error::from_reason(format!("invalid document-symbol request: {error}")),
        )?;
        if request.resource.is_some() {
            return list_document_symbols(self.request_json.clone());
        }
        let session = self.project_session()?;
        let mut session = session.lock_index()?;
        let query = request.into_query();
        let symbols = session
            .document_symbols(&query, Some(&self.cancellation))
            .map_err(napi::Error::from_reason)?
            .into_iter()
            .map(Into::into)
            .collect::<Vec<DocumentSymbolResponse>>();
        serde_json::to_string(&symbols).map_err(|error| {
            napi::Error::from_reason(format!(
                "failed to encode document-symbol response: {error}"
            ))
        })
    }

    fn index_project(&self) -> napi::Result<String> {
        let request = serde_json::from_str::<DocumentSymbolsRequest>(&self.request_json).map_err(
            |error| napi::Error::from_reason(format!("invalid project-index request: {error}")),
        )?;
        let session = self.project_session()?;
        let mut session = session.lock_index()?;
        let (documents, warnings) = session
            .project_documents(&request.into_query(), Some(&self.cancellation))
            .map_err(napi::Error::from_reason)?;
        let documents = documents
            .into_iter()
            .map(|(path, symbols)| ProjectIndexDocumentResponse {
                path,
                symbols: symbols.into_iter().map(Into::into).collect(),
            })
            .collect();
        serde_json::to_string(&ProjectIndexResponse {
            documents,
            warnings,
        })
        .map_err(|error| {
            napi::Error::from_reason(format!("failed to encode project index: {error}"))
        })
    }

    fn analyze_document(&self) -> napi::Result<String> {
        let request = serde_json::from_str::<DocumentSymbolsRequest>(&self.request_json).map_err(
            |error| napi::Error::from_reason(format!("invalid semantic-document request: {error}")),
        )?;
        if request.resource.is_some() {
            return analyze_document(self.request_json.clone());
        }
        let session = self.project_session()?;
        let mut session = session.lock_index()?;
        let (tokens, hints) = session
            .analyze_document(&request.into_query(), Some(&self.cancellation))
            .map_err(napi::Error::from_reason)?;
        encode_semantic_document(tokens, hints)
    }
}

fn invoke_language_method(method: &str, request_json: String) -> napi::Result<String> {
    match method {
        "checkNss" => check_nss(request_json),
        "findDefinitions" => find_definitions(request_json),
        "findIncludeCandidates" => find_include_candidates(request_json),
        "findReferences" => find_references(request_json),
        "findOutgoingCalls" => find_outgoing_calls(request_json),
        "readVirtualSource" => read_virtual_source(request_json),
        "resolveSource" => resolve_source(request_json),
        "listDocumentSymbols" => list_document_symbols(request_json),
        "indexProject" => index_project(request_json),
        "analyzeDocument" => analyze_document(request_json),
        "deduplicateProjectRoots" => deduplicate_project_roots(request_json),
        "resolveWatchRoots" => resolve_watch_roots(request_json),
        "checkNwpkg" => check_nwpkg(request_json),
        _ => Err(napi::Error::from_reason(format!(
            "native compiler does not export {method}"
        ))),
    }
}

#[derive(Debug, Serialize)]
struct ReferenceResponse {
    name:           String,
    kind:           &'static str,
    path:           PathBuf,
    range:          SourceRangeResponse,
    is_declaration: bool,
    container:      Option<String>,
    uri:            Option<String>,
    resource:       Option<String>,
}

#[derive(Debug, Serialize)]
struct OutgoingCallResponse {
    target: DefinitionResponse,
    ranges: Vec<SourceRangeResponse>,
}

#[derive(Debug, Deserialize)]
struct VirtualSourceRequest {
    #[serde(flatten)]
    definition: DefinitionRequest,
    resource:   String,
}

#[derive(Debug, Deserialize)]
struct DocumentSymbolsRequest {
    source_path:       PathBuf,
    #[serde(default)]
    resource:          Option<String>,
    #[serde(default)]
    project_root:      Option<PathBuf>,
    #[serde(default)]
    include_dirs:      Vec<PathBuf>,
    #[serde(default)]
    overlays:          Vec<SourceOverlay>,
    #[serde(default)]
    langspec:          Option<PathBuf>,
    #[serde(default = "default_include_depth")]
    max_include_depth: usize,
    #[serde(default)]
    root:              Option<PathBuf>,
    #[serde(default)]
    user:              Option<PathBuf>,
    #[serde(default = "default_language")]
    language:          String,
    #[serde(default)]
    load_ovr:          bool,
}

impl DocumentSymbolsRequest {
    fn into_query(self) -> NwScriptDefinitionQuery {
        let source_overlays = self
            .overlays
            .into_iter()
            .map(|overlay| (overlay.path, overlay.contents.into_bytes()))
            .collect::<BTreeMap<_, _>>();
        NwScriptDefinitionQuery {
            source_path: self.source_path,
            symbol: String::new(),
            qualifier: None,
            project_root: self.project_root,
            include_directories: self.include_dirs,
            source_overlays,
            langspec: self.langspec,
            max_include_depth: self.max_include_depth,
            root: self.root,
            user: self.user,
            language: self.language,
            load_ovr: self.load_ovr,
        }
    }
}

#[derive(Debug, Serialize)]
struct DocumentSymbolResponse {
    name:            String,
    kind:            &'static str,
    detail:          Option<String>,
    range:           SourceRangeResponse,
    selection_range: SourceRangeResponse,
    children:        Vec<DocumentSymbolResponse>,
}

#[derive(Debug, Serialize)]
struct ProjectIndexResponse {
    documents: Vec<ProjectIndexDocumentResponse>,
    warnings:  Vec<String>,
}

#[derive(Debug, Serialize)]
struct ProjectIndexDocumentResponse {
    path:    PathBuf,
    symbols: Vec<DocumentSymbolResponse>,
}

#[derive(Debug, Serialize)]
struct SourceRangeResponse {
    start_line:   usize,
    start_column: usize,
    end_line:     usize,
    end_column:   usize,
}

#[derive(Debug, Serialize)]
struct SemanticDocumentResponse {
    tokens: Vec<SemanticTokenResponse>,
    hints:  Vec<InlayHintResponse>,
}

#[derive(Debug, Serialize)]
struct SemanticTokenResponse {
    range:              SourceRangeResponse,
    kind:               &'static str,
    is_declaration:     bool,
    is_readonly:        bool,
    is_default_library: bool,
}

#[derive(Debug, Serialize)]
struct InlayHintResponse {
    line:   usize,
    column: usize,
    label:  String,
    kind:   &'static str,
}

impl From<NwScriptSourceRange> for SourceRangeResponse {
    fn from(range: NwScriptSourceRange) -> Self {
        Self {
            start_line:   range.start_line,
            start_column: range.start_column,
            end_line:     range.end_line,
            end_column:   range.end_column,
        }
    }
}

impl From<NwScriptDocumentSymbol> for DocumentSymbolResponse {
    fn from(symbol: NwScriptDocumentSymbol) -> Self {
        Self {
            name:            symbol.name,
            kind:            match symbol.kind {
                NwScriptDocumentSymbolKind::Function => "function",
                NwScriptDocumentSymbolKind::Variable => "variable",
                NwScriptDocumentSymbolKind::Struct => "struct",
                NwScriptDocumentSymbolKind::Field => "field",
                NwScriptDocumentSymbolKind::Enum => "enum",
                NwScriptDocumentSymbolKind::EnumVariant => "enumVariant",
                NwScriptDocumentSymbolKind::TypeAlias => "typeAlias",
                NwScriptDocumentSymbolKind::Constant => "constant",
                NwScriptDocumentSymbolKind::Macro => "macro",
            },
            detail:          symbol.detail,
            range:           symbol.range.into(),
            selection_range: symbol.selection_range.into(),
            children:        symbol.children.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct ProjectRootsRequest {
    roots: Vec<PathBuf>,
}

#[derive(Debug, Deserialize)]
struct NwpkgCheckRequest {
    path:     PathBuf,
    contents: String,
}

#[derive(Debug, Serialize)]
struct NwpkgCheckResponse {
    diagnostics: Vec<NwpkgDiagnostic>,
}

#[derive(Debug, Serialize)]
struct NwpkgDiagnostic {
    severity:     &'static str,
    message:      String,
    start_line:   usize,
    start_column: usize,
    end_line:     usize,
    end_column:   usize,
}

const fn enabled() -> bool {
    true
}

const fn default_include_depth() -> usize {
    16
}

const fn default_diagnostic_limit() -> usize {
    50
}

fn default_optimization() -> String {
    "O1".to_string()
}

fn default_language() -> String {
    "english".to_string()
}

fn map_diagnostic(
    input: PathBuf,
    diagnostic: Option<CompilerDiagnostic>,
    fallback: Option<String>,
) -> CheckDiagnostic {
    let diagnostic = diagnostic.unwrap_or_else(|| CompilerDiagnostic {
        code:         None,
        message:      fallback.unwrap_or_else(|| "unknown compilation error".to_string()),
        file:         None,
        start_line:   None,
        start_column: None,
        end_line:     None,
        end_column:   None,
    });
    CheckDiagnostic {
        input,
        severity: "error",
        code: diagnostic.code,
        message: diagnostic.message,
        file: diagnostic.file,
        start_line: diagnostic.start_line,
        start_column: diagnostic.start_column,
        end_line: diagnostic.end_line,
        end_column: diagnostic.end_column,
    }
}

/// Checks NSS inputs in-process and returns one JSON response for JavaScript.
#[napi(js_name = "checkNss", strict)]
pub fn check_nss(request_json: String) -> napi::Result<String> {
    check_nss_impl(request_json, None)
}

fn check_nss_impl(
    request_json: String,
    cancellation: Option<&nwnrs_nwscript::CancellationToken>,
) -> napi::Result<String> {
    let request = serde_json::from_str::<CheckRequest>(&request_json)
        .map_err(|error| napi::Error::from_reason(format!("invalid check request: {error}")))?;
    let source_overlays = request
        .overlays
        .into_iter()
        .map(|overlay| (overlay.path, overlay.contents.into_bytes()))
        .collect::<BTreeMap<_, _>>();
    let options = NwScriptCheckOptions {
        paths: request.paths,
        no_entrypoint_check: request.no_entrypoint_check,
        langspec: request.langspec,
        include_dirs: request.include_dirs,
        source_overlays,
        optimization: request.optimization,
        optimization_flags: request.optimization_flags,
        max_include_depth: request.max_include_depth,
        max_diagnostics_per_input: request.max_diagnostics_per_input,
        recurse: request.recurse,
        follow_symlinks: request.follow_symlinks,
        jobs: request.jobs,
        root: request.root,
        user: request.user,
        language: request.language,
        load_ovr: request.load_ovr,
    };
    let report = cancellation
        .map_or_else(
            || check_nwscript(&options),
            |cancellation| check_nwscript_with_cancellation(&options, cancellation),
        )
        .map_err(napi::Error::from_reason)?;
    let diagnostics = report
        .entries
        .into_iter()
        .filter(|entry| entry.status == BatchCompileStatus::Error)
        .flat_map(|entry| {
            let input = entry.input;
            let mut diagnostics =
                vec![map_diagnostic(input.clone(), entry.diagnostic, entry.error)];
            diagnostics.extend(
                entry
                    .additional_diagnostics
                    .into_iter()
                    .map(|diagnostic| map_diagnostic(input.clone(), Some(diagnostic), None)),
            );
            diagnostics
        })
        .collect();
    serde_json::to_string(&CheckResponse {
        diagnostics,
        summary: CheckSummary {
            compiled: report.successes,
            skipped:  report.skips,
            failed:   report.errors,
        },
    })
    .map_err(|error| napi::Error::from_reason(format!("failed to encode check response: {error}")))
}

/// Finds project- and include-aware NSS symbol definitions in-process.
#[napi(js_name = "findDefinitions", strict)]
pub fn find_definitions(request_json: String) -> napi::Result<String> {
    let request = serde_json::from_str::<DefinitionRequest>(&request_json).map_err(|error| {
        napi::Error::from_reason(format!("invalid definition request: {error}"))
    })?;
    let definitions = find_nwscript_definitions(&request.into_query())
        .map_err(napi::Error::from_reason)?
        .into_iter()
        .map(definition_response)
        .collect::<Vec<_>>();
    serde_json::to_string(&definitions).map_err(|error| {
        napi::Error::from_reason(format!("failed to encode definition response: {error}"))
    })
}

/// Finds source files that uniquely provide an unresolved NSS symbol and can
/// therefore back a safe "Add missing include" code action.
#[napi(js_name = "findIncludeCandidates", strict)]
pub fn find_include_candidates(request_json: String) -> napi::Result<String> {
    let request = serde_json::from_str::<DefinitionRequest>(&request_json).map_err(|error| {
        napi::Error::from_reason(format!("invalid include-candidate request: {error}"))
    })?;
    let candidates = find_nwscript_include_candidates(&request.into_query())
        .map_err(napi::Error::from_reason)?
        .into_iter()
        .map(|candidate| IncludeCandidateResponse {
            include_name: candidate.include_name,
            path:         candidate.definition.path,
            start_line:   candidate.definition.start_line,
            start_column: candidate.definition.start_column,
        })
        .collect::<Vec<_>>();
    serde_json::to_string(&candidates).map_err(|error| {
        napi::Error::from_reason(format!("failed to encode include candidates: {error}"))
    })
}

/// Finds compiler-filtered references for the symbol at a source position.
#[napi(js_name = "findReferences", strict)]
pub fn find_references(request_json: String) -> napi::Result<String> {
    let request = serde_json::from_str::<ReferencesRequest>(&request_json).map_err(|error| {
        napi::Error::from_reason(format!("invalid references request: {error}"))
    })?;
    let references = find_nwscript_references(
        &request.definition.into_query(),
        request.line,
        request.column,
    )
    .map_err(napi::Error::from_reason)?;
    encode_reference_response(references)
}

fn encode_reference_response(references: Vec<nwnrs::NwScriptReference>) -> napi::Result<String> {
    let references = references
        .into_iter()
        .map(|reference| ReferenceResponse {
            name:           reference.name,
            kind:           symbol_kind_name(reference.kind),
            path:           reference.path,
            range:          reference.range.into(),
            is_declaration: reference.is_declaration,
            container:      reference.container,
            uri:            reference.virtual_uri,
            resource:       reference.resource_name,
        })
        .collect::<Vec<_>>();
    serde_json::to_string(&references).map_err(|error| {
        napi::Error::from_reason(format!("failed to encode references response: {error}"))
    })
}

fn symbol_kind_name(kind: NwScriptSymbolKind) -> &'static str {
    match kind {
        NwScriptSymbolKind::Function => "function",
        NwScriptSymbolKind::Macro => "macro",
        NwScriptSymbolKind::Enum => "enum",
        NwScriptSymbolKind::EnumVariant => "enumVariant",
        NwScriptSymbolKind::TypeAlias => "typeAlias",
        NwScriptSymbolKind::Constant => "constant",
        NwScriptSymbolKind::Variable => "variable",
        NwScriptSymbolKind::Parameter => "parameter",
        NwScriptSymbolKind::Struct => "struct",
        NwScriptSymbolKind::Field => "field",
        NwScriptSymbolKind::BuiltinFunction => "builtinFunction",
        NwScriptSymbolKind::BuiltinConstant => "builtinConstant",
        NwScriptSymbolKind::EngineStructure => "engineStructure",
    }
}

fn definition_response(definition: NwScriptSymbolDefinition) -> DefinitionResponse {
    DefinitionResponse {
        name:              definition.name,
        kind:              symbol_kind_name(definition.kind),
        path:              definition.path,
        start_line:        definition.start_line,
        start_column:      definition.start_column,
        end_line:          definition.end_line,
        end_column:        definition.end_column,
        signature:         definition.signature,
        documentation:     definition.documentation,
        is_implementation: definition.is_implementation,
        uri:               definition.virtual_uri,
        resource:          definition.resource_name,
    }
}

/// Resolves outgoing calls from the function containing a source position.
#[napi(js_name = "findOutgoingCalls", strict)]
pub fn find_outgoing_calls(request_json: String) -> napi::Result<String> {
    let request = serde_json::from_str::<ReferencesRequest>(&request_json).map_err(|error| {
        napi::Error::from_reason(format!("invalid outgoing-calls request: {error}"))
    })?;
    let calls = find_nwscript_outgoing_calls(&request.definition.into_query(), request.line)
        .map_err(napi::Error::from_reason)?
        .into_iter()
        .map(|call| OutgoingCallResponse {
            target: definition_response(call.target),
            ranges: call.ranges.into_iter().map(Into::into).collect(),
        })
        .collect::<Vec<_>>();
    serde_json::to_string(&calls).map_err(|error| {
        napi::Error::from_reason(format!("failed to encode outgoing calls: {error}"))
    })
}

/// Returns one immutable virtual NSS document resolved from a packed game
/// resource rather than a workspace path.
#[napi(js_name = "readVirtualSource", strict)]
pub fn read_virtual_source(request_json: String) -> napi::Result<String> {
    let request = serde_json::from_str::<VirtualSourceRequest>(&request_json).map_err(|error| {
        napi::Error::from_reason(format!("invalid virtual-source request: {error}"))
    })?;
    let response =
        load_nwscript_virtual_source(&request.definition.into_query(), &request.resource)
            .map_err(napi::Error::from_reason)?
            .map(|source| VirtualSourceResponse {
                uri:      source.uri,
                contents: source.contents,
            });
    serde_json::to_string(&response).map_err(|error| {
        napi::Error::from_reason(format!("failed to encode virtual-source response: {error}"))
    })
}

/// Resolves an include/script resource through editable project overrides,
/// dependencies, configured roots, and finally packed game resources.
#[napi(js_name = "resolveSource", strict)]
pub fn resolve_source(request_json: String) -> napi::Result<String> {
    let request = serde_json::from_str::<VirtualSourceRequest>(&request_json).map_err(|error| {
        napi::Error::from_reason(format!("invalid source-resolution request: {error}"))
    })?;
    let resource = request.resource;
    let response = resolve_nwscript_source(&request.definition.into_query(), &resource)
        .map_err(napi::Error::from_reason)?
        .map(|source| ResolvedSourceResponse {
            path:     source.path,
            uri:      source.virtual_uri,
            resource: source.resource_name,
        });
    serde_json::to_string(&response).map_err(|error| {
        napi::Error::from_reason(format!("failed to encode resolved source: {error}"))
    })
}

/// Returns the hierarchical, source-authored declarations for one NSS
/// document. Included declarations and synthetic macro output are omitted.
#[napi(js_name = "listDocumentSymbols", strict)]
pub fn list_document_symbols(request_json: String) -> napi::Result<String> {
    let request =
        serde_json::from_str::<DocumentSymbolsRequest>(&request_json).map_err(|error| {
            napi::Error::from_reason(format!("invalid document-symbol request: {error}"))
        })?;
    let resource = request.resource.clone();
    let symbols = list_nwscript_document_symbols(&request.into_query(), resource.as_deref())
        .map_err(napi::Error::from_reason)?
        .into_iter()
        .map(Into::into)
        .collect::<Vec<DocumentSymbolResponse>>();
    serde_json::to_string(&symbols).map_err(|error| {
        napi::Error::from_reason(format!(
            "failed to encode document-symbol response: {error}"
        ))
    })
}

/// Builds one bounded, package-aware symbol index covering project sources,
/// configured include roots, and transitive editable dependencies.
#[napi(js_name = "indexProject", strict)]
pub fn index_project(request_json: String) -> napi::Result<String> {
    let request =
        serde_json::from_str::<DocumentSymbolsRequest>(&request_json).map_err(|error| {
            napi::Error::from_reason(format!("invalid project-index request: {error}"))
        })?;
    let query = request.into_query();
    let mut index = NwScriptProjectIndex::new();
    let (documents, warnings) = index
        .project_documents(&query, None)
        .map_err(napi::Error::from_reason)?;
    let documents = documents
        .into_iter()
        .map(|(path, symbols)| ProjectIndexDocumentResponse {
            path,
            symbols: symbols.into_iter().map(Into::into).collect(),
        })
        .collect();
    serde_json::to_string(&ProjectIndexResponse {
        documents,
        warnings,
    })
    .map_err(|error| napi::Error::from_reason(format!("failed to encode project index: {error}")))
}

/// Returns compiler-backed semantic tokens and restrained inlay hints for one
/// physical or packed NSS document.
#[napi(js_name = "analyzeDocument", strict)]
pub fn analyze_document(request_json: String) -> napi::Result<String> {
    let request =
        serde_json::from_str::<DocumentSymbolsRequest>(&request_json).map_err(|error| {
            napi::Error::from_reason(format!("invalid semantic-document request: {error}"))
        })?;
    let resource = request.resource.clone();
    let (tokens, hints) = analyze_nwscript_document(&request.into_query(), resource.as_deref())
        .map_err(napi::Error::from_reason)?;
    encode_semantic_document(tokens, hints)
}

fn encode_semantic_document(
    tokens: Vec<nwnrs::NwScriptSemanticToken>,
    hints: Vec<nwnrs::NwScriptInlayHint>,
) -> napi::Result<String> {
    let response = SemanticDocumentResponse {
        tokens: tokens
            .into_iter()
            .map(|token| SemanticTokenResponse {
                range:              token.range.into(),
                kind:               match token.kind {
                    NwScriptSemanticTokenKind::Function => "function",
                    NwScriptSemanticTokenKind::Parameter => "parameter",
                    NwScriptSemanticTokenKind::Variable => "variable",
                    NwScriptSemanticTokenKind::Property => "property",
                    NwScriptSemanticTokenKind::Type => "type",
                    NwScriptSemanticTokenKind::Enum => "enum",
                    NwScriptSemanticTokenKind::EnumMember => "enumMember",
                    NwScriptSemanticTokenKind::Macro => "macro",
                },
                is_declaration:     token.is_declaration,
                is_readonly:        token.is_readonly,
                is_default_library: token.is_default_library,
            })
            .collect(),
        hints:  hints
            .into_iter()
            .map(|hint| InlayHintResponse {
                line:   hint.line,
                column: hint.column,
                label:  hint.label,
                kind:   hint.kind,
            })
            .collect(),
    };
    serde_json::to_string(&response).map_err(|error| {
        napi::Error::from_reason(format!(
            "failed to encode semantic-document response: {error}"
        ))
    })
}

/// Removes workspace projects already compiled through another project's
/// transitive local include dependency graph.
#[napi(js_name = "deduplicateProjectRoots", strict)]
pub fn deduplicate_project_roots(request_json: String) -> napi::Result<String> {
    let request = serde_json::from_str::<ProjectRootsRequest>(&request_json).map_err(|error| {
        napi::Error::from_reason(format!("invalid project-root request: {error}"))
    })?;
    let roots =
        deduplicate_nwscript_project_roots(&request.roots).map_err(napi::Error::from_reason)?;
    serde_json::to_string(&roots).map_err(|error| {
        napi::Error::from_reason(format!("failed to encode project roots: {error}"))
    })
}

/// Returns project/dependency directories that can invalidate editor checks.
#[napi(js_name = "resolveWatchRoots", strict)]
pub fn resolve_watch_roots(request_json: String) -> napi::Result<String> {
    let request = serde_json::from_str::<ProjectRootsRequest>(&request_json).map_err(|error| {
        napi::Error::from_reason(format!("invalid watch-root request: {error}"))
    })?;
    let roots = nwscript_watch_roots(&request.roots).map_err(napi::Error::from_reason)?;
    serde_json::to_string(&roots)
        .map_err(|error| napi::Error::from_reason(format!("failed to encode watch roots: {error}")))
}

/// Validates an in-memory `nwpkg.toml`, including local source and dependency
/// paths, without requiring the editor buffer to be saved.
#[napi(js_name = "checkNwpkg", strict)]
pub fn check_nwpkg(request_json: String) -> napi::Result<String> {
    let request = serde_json::from_str::<NwpkgCheckRequest>(&request_json).map_err(|error| {
        napi::Error::from_reason(format!("invalid nwpkg check request: {error}"))
    })?;
    let mut diagnostics = Vec::new();
    match toml::from_str::<nwnrs_nwpkg::ProjectManifest>(&request.contents) {
        Ok(manifest) => validate_nwpkg_manifest(&request, &manifest, &mut diagnostics),
        Err(error) => {
            let (start, end) = error
                .span()
                .map_or((0, 1), |span| (span.start, span.end.max(span.start + 1)));
            let range = byte_range(&request.contents, start, end);
            diagnostics.push(NwpkgDiagnostic {
                severity:     "error",
                message:      error.to_string(),
                start_line:   range.0,
                start_column: range.1,
                end_line:     range.2,
                end_column:   range.3,
            });
        }
    }
    serde_json::to_string(&NwpkgCheckResponse {
        diagnostics,
    })
    .map_err(|error| {
        napi::Error::from_reason(format!("failed to encode nwpkg diagnostics: {error}"))
    })
}

fn validate_nwpkg_manifest(
    request: &NwpkgCheckRequest,
    manifest: &nwnrs_nwpkg::ProjectManifest,
    diagnostics: &mut Vec<NwpkgDiagnostic>,
) {
    let root = request
        .path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    if manifest.project.name.trim().is_empty() {
        push_nwpkg_key_diagnostic(
            diagnostics,
            &request.contents,
            "name",
            "project.name must not be empty",
        );
    }
    if manifest.source.path.as_os_str().is_empty() {
        push_nwpkg_key_diagnostic(
            diagnostics,
            &request.contents,
            "path",
            "source.path must not be empty",
        );
    } else {
        let source = root.join(&manifest.source.path);
        if !source.is_dir() {
            push_nwpkg_value_diagnostic(
                diagnostics,
                &request.contents,
                &manifest.source.path.to_string_lossy(),
                format!("source directory does not exist: {}", source.display()),
            );
        } else if let (Ok(source), Ok(root)) = (source.canonicalize(), root.canonicalize())
            && !source.starts_with(&root)
        {
            push_nwpkg_value_diagnostic(
                diagnostics,
                &request.contents,
                &manifest.source.path.to_string_lossy(),
                format!(
                    "source directory escapes package root: {}",
                    source.display()
                ),
            );
        }
    }
    for (name, dependency) in &manifest.dependencies {
        let nwnrs_nwpkg::DependencySpec::Path(dependency) = dependency;
        let dependency_root = root.join(&dependency.path);
        if dependency.path.as_os_str().is_empty() || !dependency_root.is_dir() {
            push_nwpkg_value_diagnostic(
                diagnostics,
                &request.contents,
                &dependency.path.to_string_lossy(),
                format!(
                    "dependency {name:?} directory does not exist: {}",
                    dependency_root.display()
                ),
            );
            continue;
        }
        match nwnrs_nwpkg::read_project_manifest(&dependency_root) {
            Ok(Some(dependency_manifest))
                if dependency_manifest.project.kind == nwnrs_nwpkg::ProjectKind::Include => {}
            Ok(Some(dependency_manifest)) => push_nwpkg_value_diagnostic(
                diagnostics,
                &request.contents,
                &dependency.path.to_string_lossy(),
                format!(
                    "dependency {name:?} is kind {:?}; only include packages are supported",
                    dependency_manifest.project.kind
                ),
            ),
            Ok(None) => push_nwpkg_value_diagnostic(
                diagnostics,
                &request.contents,
                &dependency.path.to_string_lossy(),
                format!("dependency {name:?} does not contain nwpkg.toml"),
            ),
            Err(error) => push_nwpkg_value_diagnostic(
                diagnostics,
                &request.contents,
                &dependency.path.to_string_lossy(),
                error,
            ),
        }
    }
    if let Err(error) = validate_nwpkg_dependency_graph(root, manifest) {
        push_nwpkg_key_diagnostic(diagnostics, &request.contents, "dependencies", error);
    }
}

fn validate_nwpkg_dependency_graph(
    root: &Path,
    manifest: &nwnrs_nwpkg::ProjectManifest,
) -> Result<(), String> {
    fn visit(
        root: &Path,
        manifest: &nwnrs_nwpkg::ProjectManifest,
        active: &mut Vec<PathBuf>,
        visited: &mut BTreeSet<PathBuf>,
    ) -> Result<(), String> {
        let canonical = root
            .canonicalize()
            .map_err(|error| format!("failed to resolve package {}: {error}", root.display()))?;
        if let Some(start) = active.iter().position(|entry| entry == &canonical) {
            let mut cycle = active
                .get(start..)
                .unwrap_or_default()
                .iter()
                .map(|entry| entry.display().to_string())
                .collect::<Vec<_>>();
            cycle.push(canonical.display().to_string());
            return Err(format!("nwpkg dependency cycle: {}", cycle.join(" -> ")));
        }
        if !visited.insert(canonical.clone()) {
            return Ok(());
        }
        active.push(canonical.clone());
        for (name, dependency) in &manifest.dependencies {
            let nwnrs_nwpkg::DependencySpec::Path(dependency) = dependency;
            let dependency_root = canonical.join(&dependency.path);
            if !dependency_root.is_dir() {
                continue;
            }
            let Some(dependency_manifest) = nwnrs_nwpkg::read_project_manifest(&dependency_root)?
            else {
                continue;
            };
            if dependency_manifest.project.kind != nwnrs_nwpkg::ProjectKind::Include {
                continue;
            }
            let dependency_source = dependency_root.join(&dependency_manifest.source.path);
            if dependency_source.is_dir()
                && let (Ok(source), Ok(package)) = (
                    dependency_source.canonicalize(),
                    dependency_root.canonicalize(),
                )
                && !source.starts_with(&package)
            {
                return Err(format!(
                    "source for dependency {name:?} escapes package root: {}",
                    source.display()
                ));
            }
            visit(&dependency_root, &dependency_manifest, active, visited)?;
        }
        active.pop();
        Ok(())
    }

    visit(root, manifest, &mut Vec::new(), &mut BTreeSet::new())
}

fn push_nwpkg_key_diagnostic(
    diagnostics: &mut Vec<NwpkgDiagnostic>,
    source: &str,
    key: &str,
    message: impl Into<String>,
) {
    let start = source.find(key).unwrap_or(0);
    push_nwpkg_span_diagnostic(diagnostics, source, start, start + key.len(), message);
}

fn push_nwpkg_value_diagnostic(
    diagnostics: &mut Vec<NwpkgDiagnostic>,
    source: &str,
    value: &str,
    message: impl Into<String>,
) {
    let start = source.find(value).unwrap_or(0);
    push_nwpkg_span_diagnostic(
        diagnostics,
        source,
        start,
        start + value.len().max(1),
        message,
    );
}

fn push_nwpkg_span_diagnostic(
    diagnostics: &mut Vec<NwpkgDiagnostic>,
    source: &str,
    start: usize,
    end: usize,
    message: impl Into<String>,
) {
    let range = byte_range(source, start, end);
    diagnostics.push(NwpkgDiagnostic {
        severity:     "error",
        message:      message.into(),
        start_line:   range.0,
        start_column: range.1,
        end_line:     range.2,
        end_column:   range.3,
    });
}

fn byte_range(source: &str, start: usize, end: usize) -> (usize, usize, usize, usize) {
    let location = |offset: usize| {
        let prefix = source.get(..offset.min(source.len())).unwrap_or_default();
        let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
        let column = prefix
            .rfind('\n')
            .map_or(prefix.len() + 1, |newline| prefix.len() - newline);
        (line, column)
    };
    let start = location(start);
    let end = location(end);
    (start.0, start.1, end.0, end.1)
}
