use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use napi::{
    Task,
    bindgen_prelude::{AbortSignal, AsyncTask, Buffer, Either, ToNapiValue, TypeName},
    threadsafe_function::{ThreadsafeFunction, ThreadsafeFunctionCallMode},
};
use napi_derive::napi;
use nwnrs_language::{
    EventLanguageCompletionItemKind, EventLanguageCompletionKind, EventLanguageSymbolKind,
    EventLanguageSymbolToken, LanguageOperationError, LanguageService as CoreLanguageService,
    ManifestCompletionKind, ManifestSymbolKind, NwScriptCheckOptions, NwScriptCompletionKind,
    NwScriptDefinitionQuery, NwScriptDocumentSymbolKind, NwScriptSemanticTokenKind,
    NwScriptSymbolKind, PackageConfiguration, PackageDependencyChange, PackageDependencyPlan,
    PackageDependencySource, PackageDependencyStatus,
};
use nwnrs_nwpkg::{DependencyProgressEvent, DependencyProgressReporter};
use nwnrs_nwscript::CancellationToken;
use nwnrs_runtime::{
    EventControlPolicy, EventFilterPolicy, EventFrequency, EventResultKind, EventResultRole,
    EventSourceKind,
};

type NativeJob<T> = Box<dyn FnOnce(&CancellationToken) -> napi::Result<T> + Send>;

/// Returns the content-derived native/TypeScript contract fingerprint.
#[napi]
#[cfg_attr(test, allow(dead_code))]
pub fn native_contract_fingerprint() -> &'static str {
    env!("NWNRS_CONTRACT_FINGERPRINT")
}

pub struct CompilerTask<T> {
    job:          Option<NativeJob<T>>,
    cancellation: CancellationToken,
}

impl<T> CompilerTask<T> {
    fn new(job: impl FnOnce(&CancellationToken) -> napi::Result<T> + Send + 'static) -> Self {
        Self {
            job:          Some(Box::new(job)),
            cancellation: CancellationToken::new(),
        }
    }
}

impl<T> Task for CompilerTask<T>
where
    T: Send + ToNapiValue + TypeName + 'static,
{
    type JsValue = T;
    type Output = T;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        self.job
            .take()
            .ok_or_else(|| napi::Error::from_reason("native compiler task was already consumed"))?(
            &self.cancellation,
        )
    }

    fn resolve(&mut self, _env: napi::Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        Ok(output)
    }
}

fn async_query<T>(
    signal: Option<AbortSignal>,
    job: impl FnOnce(&CancellationToken) -> napi::Result<T> + Send + 'static,
) -> AsyncTask<CompilerTask<T>>
where
    T: Send + ToNapiValue + TypeName + 'static,
{
    let task = CompilerTask::new(job);
    if let Some(signal) = &signal {
        let cancellation = task.cancellation.clone();
        signal.on_abort(move || cancellation.cancel());
    }
    AsyncTask::with_optional_signal(task, signal)
}

#[napi(object)]
#[derive(Clone)]
pub struct NativePackageConfiguration {
    pub source_path:         String,
    pub project_root:        Option<String>,
    pub include_directories: Vec<String>,
    pub langspec:            Option<String>,
    pub max_include_depth:   u32,
    pub root:                Option<String>,
    pub user:                Option<String>,
    pub language:            String,
    pub load_ovr:            bool,
    pub git_executable:      Option<String>,
    pub package_cache:       Option<String>,
}

impl NativePackageConfiguration {
    fn core(&self) -> PackageConfiguration {
        PackageConfiguration {
            source_path:         PathBuf::from(&self.source_path),
            project_root:        self.project_root.as_deref().map(PathBuf::from),
            include_directories: self.include_directories.iter().map(PathBuf::from).collect(),
            langspec:            self.langspec.as_deref().map(PathBuf::from),
            max_include_depth:   usize::try_from(self.max_include_depth).unwrap_or(usize::MAX),
            root:                self.root.as_deref().map(PathBuf::from),
            user:                self.user.as_deref().map(PathBuf::from),
            language:            self.language.clone(),
            load_ovr:            self.load_ovr,
            git_executable:      self.git_executable.as_deref().map(PathBuf::from),
            package_cache:       self.package_cache.as_deref().map(PathBuf::from),
        }
    }
}

#[napi(object)]
#[derive(Clone, Copy)]
pub struct NativePosition {
    pub line:      u32,
    pub character: u32,
}

#[napi(object)]
#[derive(Clone, Copy)]
pub struct NativeRange {
    pub start: NativePosition,
    pub end:   NativePosition,
}

#[napi(object)]
pub struct NativeDefinitionRequest {
    pub package:   NativePackageConfiguration,
    pub symbol:    String,
    pub qualifier: Option<String>,
}

#[napi(object)]
pub struct NativePositionRequest {
    pub package: NativePackageConfiguration,
    pub line:    u32,
    pub column:  u32,
}

#[napi(object)]
pub struct NativeRenameRequest {
    pub package:  NativePackageConfiguration,
    pub line:     u32,
    pub column:   u32,
    pub new_name: Option<String>,
}

#[napi(object)]
pub struct NativeCodeActionDiagnostic {
    pub code:  i32,
    pub range: NativeRange,
}

#[napi(object)]
pub struct NativeCodeActionRequest {
    pub package:     NativePackageConfiguration,
    pub diagnostics: Vec<NativeCodeActionDiagnostic>,
}

#[napi(object)]
pub struct NativeCompletion {
    pub label:         String,
    pub detail:        String,
    pub documentation: Option<String>,
    pub insert_text:   String,
    pub sort_text:     String,
    pub replacement:   NativeRange,
    pub kind:          String,
}

#[napi(object)]
pub struct NativeSignatureParameter {
    pub label:         String,
    pub documentation: Option<String>,
}

#[napi(object)]
pub struct NativeSignature {
    pub label:         String,
    pub documentation: Option<String>,
    pub parameters:    Vec<NativeSignatureParameter>,
}

#[napi(object)]
pub struct NativeSignatureHelp {
    pub signatures:       Vec<NativeSignature>,
    pub active_signature: u32,
    pub active_parameter: u32,
}

#[napi(object)]
pub struct NativeDefinition {
    pub name:              String,
    pub kind:              String,
    pub path:              String,
    pub range:             NativeRange,
    pub signature:         String,
    pub documentation:     Option<String>,
    pub is_implementation: bool,
    pub uri:               Option<String>,
    pub resource:          Option<String>,
}

#[napi(object)]
pub struct NativeReference {
    pub name:           String,
    pub kind:           String,
    pub path:           String,
    pub range:          NativeRange,
    pub is_declaration: bool,
    pub container:      Option<String>,
    pub uri:            Option<String>,
    pub resource:       Option<String>,
}

#[napi(object)]
pub struct NativeIncludeCandidate {
    pub include_name: String,
    pub definition:   NativeDefinition,
}

#[napi(object)]
pub struct NativeOutgoingCall {
    pub target: NativeDefinition,
    pub ranges: Vec<NativeRange>,
}

#[napi(object)]
pub struct NativeDocumentSymbol {
    pub name:      String,
    pub detail:    Option<String>,
    pub kind:      String,
    pub range:     NativeRange,
    pub selection: NativeRange,
    pub children:  Vec<NativeDocumentSymbol>,
}

#[napi(object)]
pub struct NativeProjectDocument {
    pub path:    String,
    pub symbols: Vec<NativeDocumentSymbol>,
}

#[napi(object)]
pub struct NativeProjectIndex {
    pub documents: Vec<NativeProjectDocument>,
    pub warnings:  Vec<String>,
}

#[napi(object)]
pub struct NativeWorkspacePackage {
    pub root:          String,
    pub manifest_path: String,
}

#[napi(object)]
pub struct NativeWorkspaceSymbol {
    pub package_root: String,
    pub source_path:  String,
    pub symbol:       NativeDocumentSymbol,
}

#[napi(object)]
pub struct NativeSemanticToken {
    pub range:              NativeRange,
    pub kind:               String,
    pub is_declaration:     bool,
    pub is_readonly:        bool,
    pub is_default_library: bool,
}

#[napi(object)]
pub struct NativeInlayHint {
    pub position: NativePosition,
    pub label:    String,
    pub kind:     String,
}

#[napi(object)]
pub struct NativeDocumentAnalysis {
    pub tokens: Vec<NativeSemanticToken>,
    pub hints:  Vec<NativeInlayHint>,
}

#[napi(object)]
pub struct NativeCheckRequest {
    pub paths: Vec<String>,
    pub no_entrypoint_check: bool,
    pub langspec: Option<String>,
    pub include_directories: Vec<String>,
    pub optimization: String,
    pub optimization_flags: Vec<String>,
    pub max_include_depth: u32,
    pub max_diagnostics_per_input: u32,
    pub recurse: bool,
    pub follow_symlinks: bool,
    pub jobs: Option<u32>,
    pub root: Option<String>,
    pub user: Option<String>,
    pub language: String,
    pub load_ovr: bool,
    pub git_executable: Option<String>,
    pub package_cache: Option<String>,
}

#[napi(object)]
pub struct NativeCheckSummary {
    pub compiled: u32,
    pub skipped:  u32,
    pub failed:   u32,
}

#[napi(object)]
pub struct NativeCheckResponse {
    pub diagnostics: Vec<NativeLanguageDiagnostic>,
    pub summary:     NativeCheckSummary,
}

#[napi(object)]
pub struct NativeManifestPathRequest {
    pub path:           String,
    pub git_executable: Option<String>,
    pub package_cache:  Option<String>,
}

#[napi(object)]
pub struct NativeManifestPositionRequest {
    pub path:      String,
    pub line:      u32,
    pub character: u32,
}

#[napi(object)]
pub struct NativeManifestCompletion {
    pub label:         String,
    pub detail:        String,
    pub documentation: String,
    pub insert_text:   String,
    pub replacement:   NativeRange,
    pub kind:          String,
    pub retrigger:     bool,
}

#[napi(object)]
pub struct NativeManifestHover {
    pub documentation: String,
    pub range:         NativeRange,
}

#[napi(object)]
pub struct NativeManifestDefinition {
    pub path:  String,
    pub range: NativeRange,
}

#[napi(object)]
pub struct NativeManifestSymbol {
    pub name:      String,
    pub detail:    String,
    pub kind:      String,
    pub range:     NativeRange,
    pub selection: NativeRange,
    pub children:  Vec<NativeManifestSymbol>,
}

#[napi(object)]
#[derive(Clone)]
pub struct NativeDependencyRequest {
    pub manifest_path: String,
    pub git_path:      Option<String>,
    pub cache_path:    Option<String>,
    pub dependency:    Option<String>,
}

#[napi(object)]
pub struct NativeDependencyApplyRequest {
    pub manifest_path: String,
    pub token:         String,
}

#[napi(object)]
pub struct NativeDependencyStatus {
    pub route:         String,
    pub name:          String,
    pub parent:        Option<String>,
    pub source_kind:   String,
    pub selector:      Option<String>,
    pub commit:        Option<String>,
    pub state:         String,
    pub root:          Option<String>,
    pub manifest_path: Option<String>,
    pub message:       Option<String>,
}

#[napi(object)]
pub struct NativeDependencyChange {
    pub route:  String,
    pub before: Option<String>,
    pub after:  String,
}

#[napi(object)]
pub struct NativeDependencyPlan {
    pub token:   String,
    pub changes: Vec<NativeDependencyChange>,
    pub status:  Vec<NativeDependencyStatus>,
}

#[napi(object)]
pub struct NativeDependencyCleanResult {
    pub removed: Vec<String>,
}

#[napi(object)]
pub struct NativeOperationError {
    pub code:           String,
    pub message:        String,
    pub path:           Option<String>,
    pub manifest_field: Vec<String>,
}

macro_rules! native_value_result {
    ($name:ident, $value:ty) => {
        #[napi(object)]
        pub struct $name {
            pub value: Option<$value>,
            pub error: Option<NativeOperationError>,
        }
    };
}

macro_rules! native_result {
    ($type:ident, $result:expr, | $value:pat_param | $map:expr) => {
        match $result {
            Ok($value) => $type {
                value: Some($map),
                error: None,
            },
            Err(error) => $type {
                value: None,
                error: Some(native_operation_error(error)),
            },
        }
    };
    ($type:ident, $result:expr, $map:expr) => {
        match $result {
            Ok(value) => $type {
                value: Some(($map)(value)),
                error: None,
            },
            Err(error) => $type {
                value: None,
                error: Some(native_operation_error(error)),
            },
        }
    };
}

macro_rules! native_optional_result {
    ($type:ident, $result:expr, | $value:pat_param | $map:expr) => {
        match $result {
            Ok(value) => $type {
                value: value.map(|$value| $map),
                error: None,
            },
            Err(error) => $type {
                value: None,
                error: Some(native_operation_error(error)),
            },
        }
    };
    ($type:ident, $result:expr, $map:expr) => {
        match $result {
            Ok(value) => $type {
                value: value.map($map),
                error: None,
            },
            Err(error) => $type {
                value: None,
                error: Some(native_operation_error(error)),
            },
        }
    };
}

native_value_result!(NativeCheckResult, NativeCheckResponse);
native_value_result!(NativeDefinitionsResult, Vec<NativeDefinition>);
native_value_result!(NativeCompletionsResult, Vec<NativeCompletion>);
native_value_result!(NativeSignatureHelpResult, NativeSignatureHelp);
native_value_result!(NativeIncludeCandidatesResult, Vec<NativeIncludeCandidate>);
native_value_result!(NativeReferencesResult, Vec<NativeReference>);
native_value_result!(NativeOutgoingCallsResult, Vec<NativeOutgoingCall>);
native_value_result!(NativeDocumentSymbolsResult, Vec<NativeDocumentSymbol>);
native_value_result!(NativeProjectIndexResult, NativeProjectIndex);
native_value_result!(NativeWorkspacePackagesResult, Vec<NativeWorkspacePackage>);
native_value_result!(NativeWorkspaceSymbolsResult, Vec<NativeWorkspaceSymbol>);
native_value_result!(NativeStringsResult, Vec<String>);
native_value_result!(NativeDocumentAnalysisResult, NativeDocumentAnalysis);
native_value_result!(NativeVirtualSourceResult, NativeVirtualSource);
native_value_result!(NativeResolvedSourceResult, NativeResolvedSource);
native_value_result!(NativeDiagnosticsResult, Vec<NativeLanguageDiagnostic>);
native_value_result!(
    NativeManifestCompletionsResult,
    Vec<NativeManifestCompletion>
);
native_value_result!(NativeManifestHoverResult, NativeManifestHover);
native_value_result!(NativeManifestDefinitionResult, NativeManifestDefinition);
native_value_result!(NativeManifestSymbolsResult, Vec<NativeManifestSymbol>);
native_value_result!(NativeEventCompletionsResult, NativeEventCompletions);
native_value_result!(NativeEventSymbolResult, NativeEventSymbol);

#[napi(object)]
pub struct NativeUnitResult {
    pub succeeded: bool,
    pub error:     Option<NativeOperationError>,
}

#[napi(object)]
pub struct NativeDependencyStatusesResult {
    pub value: Option<Vec<NativeDependencyStatus>>,
    pub error: Option<NativeOperationError>,
}

#[napi(object)]
pub struct NativeDependencyPlanResult {
    pub value: Option<NativeDependencyPlan>,
    pub error: Option<NativeOperationError>,
}

#[napi(object)]
pub struct NativeDependencyCleanOperationResult {
    pub value: Option<NativeDependencyCleanResult>,
    pub error: Option<NativeOperationError>,
}

#[napi(object)]
pub struct NativeDependencySourcesResult {
    pub value: Option<Vec<NativeDependencySource>>,
    pub error: Option<NativeOperationError>,
}

#[napi(object)]
pub struct NativeDependencySourceContentsResult {
    pub value: Option<NativeDependencySourceContents>,
    pub error: Option<NativeOperationError>,
}

#[napi(object)]
pub struct NativePathResult {
    pub value: Option<String>,
    pub error: Option<NativeOperationError>,
}

#[napi(object)]
pub struct NativeRenamePlan {
    pub name:  String,
    pub range: NativeRange,
    pub edits: Vec<NativeReference>,
}

#[napi(object)]
pub struct NativeRenamePlanResult {
    pub value: Option<NativeRenamePlan>,
    pub error: Option<NativeOperationError>,
}

#[napi(object)]
pub struct NativeTextEdit {
    pub path:     String,
    pub range:    NativeRange,
    pub new_text: String,
}

#[napi(object)]
pub struct NativeCodeAction {
    pub title:        String,
    pub is_preferred: bool,
    pub edits:        Vec<NativeTextEdit>,
}

#[napi(object)]
pub struct NativeCodeActionsResult {
    pub value: Option<Vec<NativeCodeAction>>,
    pub error: Option<NativeOperationError>,
}

#[napi(object)]
pub struct NativeDependencyProgress {
    pub phase:   String,
    pub message: String,
}

type DependencyProgressCallback = Arc<ThreadsafeFunction<NativeDependencyProgress, ()>>;

fn dependency_progress_reporter(
    callback: Option<DependencyProgressCallback>,
) -> Option<DependencyProgressReporter> {
    callback.map(|callback| {
        DependencyProgressReporter::new(move |event: DependencyProgressEvent| {
            let _ = callback.call(
                Ok(NativeDependencyProgress {
                    phase:   event.phase.to_string(),
                    message: event.message,
                }),
                ThreadsafeFunctionCallMode::NonBlocking,
            );
        })
    })
}

fn native_operation_error(error: LanguageOperationError) -> NativeOperationError {
    NativeOperationError {
        code:           error.code,
        message:        error.message,
        path:           error
            .path
            .as_deref()
            .map(nwnrs_nwscript::physical_path_display),
        manifest_field: error.manifest_field,
    }
}

fn native_unit_result(result: Result<(), LanguageOperationError>) -> NativeUnitResult {
    match result {
        Ok(()) => NativeUnitResult {
            succeeded: true,
            error:     None,
        },
        Err(error) => NativeUnitResult {
            succeeded: false,
            error:     Some(native_operation_error(error)),
        },
    }
}

#[napi(object)]
#[derive(Clone)]
pub struct NativeDependencySourceRequest {
    pub manifest_path: String,
    pub route:         String,
    pub relative_path: Option<String>,
    pub git_path:      Option<String>,
    pub cache_path:    Option<String>,
}

#[napi(object)]
pub struct NativeDependencySource {
    pub route:         String,
    pub relative_path: String,
    pub physical_path: String,
    pub encoding:      String,
}

#[napi(object)]
pub struct NativeDependencySourceContents {
    pub contents: Buffer,
    pub encoding: String,
}

#[napi(object)]
pub struct NativeLanguageDiagnostic {
    pub severity: String,
    pub code:     Option<Either<i32, String>>,
    pub path:     Option<String>,
    pub uri:      Option<String>,
    pub message:  String,
    pub range:    NativeRange,
}

#[napi(object)]
pub struct NativeVirtualSource {
    pub uri:      String,
    pub contents: Buffer,
}

#[napi(object)]
pub struct NativeResolvedSource {
    pub path:     String,
    pub uri:      Option<String>,
    pub resource: Option<String>,
}

#[napi(object)]
pub struct NativeSourceRequest {
    pub package:  NativePackageConfiguration,
    pub resource: String,
}

#[napi(object)]
pub struct NativeProjectRootsRequest {
    pub roots: Vec<String>,
}

#[napi(object)]
pub struct NativeEventCatalogEntry {
    pub subject:     String,
    pub name:        String,
    pub phase:       String,
    pub skippable:   bool,
    pub result_kind: String,
    pub control:     String,
    pub source:      String,
    pub frequency:   String,
    pub filter:      String,
}

#[napi(object)]
pub struct NativeEventPositionRequest {
    pub package: NativePackageConfiguration,
    pub offset:  u32,
}

#[napi(object)]
pub struct NativeEventCompletionItem {
    pub label:         String,
    pub kind:          String,
    pub detail:        String,
    pub documentation: Option<String>,
    pub sort_text:     String,
    pub definition:    Option<NativeDefinition>,
}

#[napi(object)]
pub struct NativeEventCompletions {
    pub kind:              String,
    pub prefix:            String,
    pub replacement_start: u32,
    pub replacement_end:   u32,
    pub subject:           Option<String>,
    pub phase:             Option<String>,
    pub modifier:          Option<String>,
    pub items:             Vec<NativeEventCompletionItem>,
}

#[napi(object)]
pub struct NativeEventSymbol {
    pub kind:       String,
    pub subject:    String,
    pub phase:      String,
    pub token:      String,
    pub range:      NativeRange,
    pub definition: Option<NativeDefinition>,
}

/// Persistent typed Node-API language service.
#[napi]
pub struct LanguageService {
    core: CoreLanguageService,
}

#[napi]
impl LanguageService {
    #[napi(constructor)]
    pub fn new() -> Self {
        Self {
            core: CoreLanguageService::new(),
        }
    }

    #[napi]
    pub fn resolve_package_root(&self, path: String) -> NativePathResult {
        match self.core.resolve_package_root(Path::new(&path)) {
            Ok(root) => NativePathResult {
                value: Some(nwnrs_nwscript::physical_path_display(&root)),
                error: None,
            },
            Err(error) => NativePathResult {
                value: None,
                error: Some(native_operation_error(error)),
            },
        }
    }

    #[napi]
    pub fn add_workspace(&self, root: String) -> NativeUnitResult {
        native_unit_result(self.core.add_workspace(Path::new(&root)))
    }

    #[napi]
    pub fn remove_workspace(&self, root: String) -> NativeUnitResult {
        native_unit_result(self.core.remove_workspace(Path::new(&root)))
    }

    #[napi]
    pub fn open_document(&self, path: String, version: i64, bytes: Buffer) -> NativeUnitResult {
        native_unit_result(self.core.open_document(
            Path::new(&path),
            version,
            Arc::<[u8]>::from(bytes.as_ref()),
        ))
    }

    #[napi]
    pub fn update_document(&self, path: String, version: i64, bytes: Buffer) -> NativeUnitResult {
        native_unit_result(self.core.update_document(
            Path::new(&path),
            version,
            Arc::<[u8]>::from(bytes.as_ref()),
        ))
    }

    #[napi]
    pub fn invalidate_document(&self, path: String, version: i64) -> NativeUnitResult {
        native_unit_result(self.core.invalidate_document(Path::new(&path), version))
    }

    #[napi]
    pub fn close_document(&self, path: String) -> NativeUnitResult {
        native_unit_result(self.core.close_document(Path::new(&path)))
    }

    #[napi]
    pub fn invalidate(&self, path: Option<String>) -> NativeUnitResult {
        native_unit_result(path.map_or_else(
            || self.core.invalidate_all(),
            |path| self.core.invalidate_source(Path::new(&path)),
        ))
    }

    #[napi]
    pub fn release_package(&self, path: String) -> NativeUnitResult {
        native_unit_result(self.core.release_package(Path::new(&path)))
    }

    #[napi(ts_return_type = "Promise<NativeCheckResult>")]
    pub fn check_nss(
        &self,
        request: NativeCheckRequest,
        signal: Option<AbortSignal>,
    ) -> AsyncTask<CompilerTask<NativeCheckResult>> {
        let service = self.core.clone();
        async_query(signal, move |cancellation| {
            Ok(native_result!(
                NativeCheckResult,
                service.check(check_options(request), cancellation),
                check_response
            ))
        })
    }

    #[napi(ts_return_type = "Promise<NativeDefinitionsResult>")]
    pub fn find_definitions(
        &self,
        request: NativeDefinitionRequest,
        signal: Option<AbortSignal>,
    ) -> AsyncTask<CompilerTask<NativeDefinitionsResult>> {
        let service = self.core.clone();
        async_query(signal, move |cancellation| {
            Ok(native_result!(
                NativeDefinitionsResult,
                service.definitions(
                    &request.package.core(),
                    request.symbol,
                    request.qualifier,
                    Some(cancellation),
                ),
                |values| values.into_iter().map(native_definition).collect()
            ))
        })
    }

    #[napi(ts_return_type = "Promise<NativeCompletionsResult>")]
    pub fn complete_nwscript(
        &self,
        request: NativePositionRequest,
        signal: Option<AbortSignal>,
    ) -> AsyncTask<CompilerTask<NativeCompletionsResult>> {
        let service = self.core.clone();
        async_query(signal, move |cancellation| {
            Ok(native_result!(
                NativeCompletionsResult,
                service.completions(
                    &request.package.core(),
                    usize::try_from(request.line)
                        .unwrap_or(usize::MAX)
                        .saturating_add(1),
                    usize::try_from(request.column)
                        .unwrap_or(usize::MAX)
                        .saturating_add(1),
                    Some(cancellation),
                ),
                |values| values.into_iter().map(native_completion).collect()
            ))
        })
    }

    #[napi(ts_return_type = "Promise<NativeSignatureHelpResult>")]
    pub fn signature_help(
        &self,
        request: NativePositionRequest,
        signal: Option<AbortSignal>,
    ) -> AsyncTask<CompilerTask<NativeSignatureHelpResult>> {
        let service = self.core.clone();
        async_query(signal, move |cancellation| {
            Ok(native_optional_result!(
                NativeSignatureHelpResult,
                service.signature_help(
                    &request.package.core(),
                    usize::try_from(request.line)
                        .unwrap_or(usize::MAX)
                        .saturating_add(1),
                    usize::try_from(request.column)
                        .unwrap_or(usize::MAX)
                        .saturating_add(1),
                    Some(cancellation),
                ),
                native_signature_help
            ))
        })
    }

    #[napi(ts_return_type = "Promise<NativeIncludeCandidatesResult>")]
    pub fn find_include_candidates(
        &self,
        request: NativeDefinitionRequest,
        signal: Option<AbortSignal>,
    ) -> AsyncTask<CompilerTask<NativeIncludeCandidatesResult>> {
        let service = self.core.clone();
        async_query(signal, move |cancellation| {
            Ok(native_result!(
                NativeIncludeCandidatesResult,
                service.include_candidates(
                    &request.package.core(),
                    request.symbol,
                    Some(cancellation)
                ),
                |values| {
                    values
                        .into_iter()
                        .map(|value| NativeIncludeCandidate {
                            include_name: value.include_name,
                            definition:   native_definition(value.definition),
                        })
                        .collect()
                }
            ))
        })
    }

    #[napi(ts_return_type = "Promise<NativeReferencesResult>")]
    pub fn find_references(
        &self,
        request: NativePositionRequest,
        signal: Option<AbortSignal>,
    ) -> AsyncTask<CompilerTask<NativeReferencesResult>> {
        let service = self.core.clone();
        async_query(signal, move |cancellation| {
            Ok(native_result!(
                NativeReferencesResult,
                service.references(
                    &request.package.core(),
                    usize::try_from(request.line).unwrap_or(usize::MAX),
                    usize::try_from(request.column).unwrap_or(usize::MAX),
                    Some(cancellation),
                ),
                |values| values.into_iter().map(native_reference).collect()
            ))
        })
    }

    #[napi(ts_return_type = "Promise<NativeRenamePlanResult>")]
    pub fn rename_at(
        &self,
        request: NativeRenameRequest,
        signal: Option<AbortSignal>,
    ) -> AsyncTask<CompilerTask<NativeRenamePlanResult>> {
        let service = self.core.clone();
        async_query(signal, move |cancellation| {
            Ok(
                match service.rename_plan(
                    &request.package.core(),
                    usize::try_from(request.line)
                        .unwrap_or(usize::MAX)
                        .saturating_add(1),
                    usize::try_from(request.column)
                        .unwrap_or(usize::MAX)
                        .saturating_add(1),
                    request.new_name.as_deref(),
                    Some(cancellation),
                ) {
                    Ok(plan) => NativeRenamePlanResult {
                        value: Some(NativeRenamePlan {
                            name:  plan.name,
                            range: native_source_range(plan.range),
                            edits: plan.edits.into_iter().map(native_reference).collect(),
                        }),
                        error: None,
                    },
                    Err(error) => NativeRenamePlanResult {
                        value: None,
                        error: Some(native_operation_error(error)),
                    },
                },
            )
        })
    }

    #[napi(ts_return_type = "Promise<NativeCodeActionsResult>")]
    pub fn code_actions_at(
        &self,
        request: NativeCodeActionRequest,
        signal: Option<AbortSignal>,
    ) -> AsyncTask<CompilerTask<NativeCodeActionsResult>> {
        let service = self.core.clone();
        async_query(signal, move |cancellation| {
            let diagnostics = request
                .diagnostics
                .into_iter()
                .map(|diagnostic| nwnrs_language::NwScriptCodeActionDiagnostic {
                    code:  diagnostic.code,
                    range: core_source_range(diagnostic.range),
                })
                .collect::<Vec<_>>();
            Ok(
                match service.code_actions(
                    &request.package.core(),
                    &diagnostics,
                    Some(cancellation),
                ) {
                    Ok(actions) => NativeCodeActionsResult {
                        value: Some(
                            actions
                                .into_iter()
                                .map(|action| NativeCodeAction {
                                    title:        action.title,
                                    is_preferred: action.is_preferred,
                                    edits:        action
                                        .edits
                                        .into_iter()
                                        .map(|edit| NativeTextEdit {
                                            path:     nwnrs_nwscript::physical_path_display(
                                                &edit.path,
                                            ),
                                            range:    native_source_range(edit.range),
                                            new_text: edit.new_text,
                                        })
                                        .collect(),
                                })
                                .collect(),
                        ),
                        error: None,
                    },
                    Err(error) => NativeCodeActionsResult {
                        value: None,
                        error: Some(native_operation_error(error)),
                    },
                },
            )
        })
    }

    #[napi(ts_return_type = "Promise<NativeOutgoingCallsResult>")]
    pub fn find_outgoing_calls(
        &self,
        request: NativePositionRequest,
        signal: Option<AbortSignal>,
    ) -> AsyncTask<CompilerTask<NativeOutgoingCallsResult>> {
        let service = self.core.clone();
        async_query(signal, move |cancellation| {
            Ok(native_result!(
                NativeOutgoingCallsResult,
                service.outgoing_calls(
                    &request.package.core(),
                    usize::try_from(request.line).unwrap_or(usize::MAX),
                    Some(cancellation),
                ),
                |values| {
                    values
                        .into_iter()
                        .map(|value| NativeOutgoingCall {
                            target: native_definition(value.target),
                            ranges: value.ranges.into_iter().map(native_source_range).collect(),
                        })
                        .collect()
                }
            ))
        })
    }

    #[napi(ts_return_type = "Promise<NativeDocumentSymbolsResult>")]
    pub fn list_document_symbols(
        &self,
        package: NativePackageConfiguration,
        signal: Option<AbortSignal>,
    ) -> AsyncTask<CompilerTask<NativeDocumentSymbolsResult>> {
        let service = self.core.clone();
        async_query(signal, move |cancellation| {
            Ok(native_result!(
                NativeDocumentSymbolsResult,
                service.document_symbols(&package.core(), Some(cancellation)),
                |values| values.into_iter().map(native_document_symbol).collect()
            ))
        })
    }

    #[napi(ts_return_type = "Promise<NativeProjectIndexResult>")]
    pub fn index_project(
        &self,
        package: NativePackageConfiguration,
        signal: Option<AbortSignal>,
    ) -> AsyncTask<CompilerTask<NativeProjectIndexResult>> {
        let service = self.core.clone();
        async_query(signal, move |cancellation| {
            Ok(native_result!(
                NativeProjectIndexResult,
                service.project_documents(&package.core(), Some(cancellation)),
                |(documents, warnings)| NativeProjectIndex {
                    documents: documents
                        .into_iter()
                        .map(|(path, symbols)| NativeProjectDocument {
                            path:    nwnrs_nwscript::physical_path_display(&path),
                            symbols: symbols.into_iter().map(native_document_symbol).collect(),
                        })
                        .collect(),
                    warnings,
                }
            ))
        })
    }

    #[napi(ts_return_type = "Promise<NativeWorkspacePackagesResult>")]
    pub fn list_workspace_packages(
        &self,
        signal: Option<AbortSignal>,
    ) -> AsyncTask<CompilerTask<NativeWorkspacePackagesResult>> {
        let service = self.core.clone();
        async_query(signal, move |cancellation| {
            Ok(native_result!(
                NativeWorkspacePackagesResult,
                service.workspace_packages(Some(cancellation)),
                |packages| {
                    packages
                        .into_iter()
                        .map(|package| NativeWorkspacePackage {
                            root:          nwnrs_nwscript::physical_path_display(&package.root),
                            manifest_path: nwnrs_nwscript::physical_path_display(
                                &package.manifest_path,
                            ),
                        })
                        .collect()
                }
            ))
        })
    }

    #[napi(ts_return_type = "Promise<NativeWorkspaceSymbolsResult>")]
    pub fn list_workspace_symbols(
        &self,
        package: NativePackageConfiguration,
        signal: Option<AbortSignal>,
    ) -> AsyncTask<CompilerTask<NativeWorkspaceSymbolsResult>> {
        let service = self.core.clone();
        async_query(signal, move |cancellation| {
            Ok(native_result!(
                NativeWorkspaceSymbolsResult,
                service.workspace_symbols(&package.core(), Some(cancellation)),
                |symbols| {
                    symbols
                        .into_iter()
                        .map(|entry| NativeWorkspaceSymbol {
                            package_root: nwnrs_nwscript::physical_path_display(
                                &entry.package_root,
                            ),
                            source_path:  nwnrs_nwscript::physical_path_display(&entry.source_path),
                            symbol:       native_document_symbol(entry.symbol),
                        })
                        .collect()
                }
            ))
        })
    }

    #[napi]
    pub fn deduplicate_project_roots(
        &self,
        request: NativeProjectRootsRequest,
    ) -> NativeStringsResult {
        native_result!(
            NativeStringsResult,
            nwnrs_language::deduplicate_nwscript_project_roots(
                &request
                    .roots
                    .into_iter()
                    .map(PathBuf::from)
                    .collect::<Vec<_>>(),
            ),
            |roots| roots
                .into_iter()
                .map(|root| nwnrs_nwscript::physical_path_display(&root))
                .collect()
        )
    }

    #[napi]
    pub fn resolve_watch_roots(&self, request: NativeProjectRootsRequest) -> NativeStringsResult {
        native_result!(
            NativeStringsResult,
            nwnrs_language::nwscript_watch_roots(
                &request
                    .roots
                    .into_iter()
                    .map(PathBuf::from)
                    .collect::<Vec<_>>(),
            ),
            |roots| roots
                .into_iter()
                .map(|root| nwnrs_nwscript::physical_path_display(&root))
                .collect()
        )
    }

    #[napi(ts_return_type = "Promise<NativeDocumentAnalysisResult>")]
    pub fn analyze_document(
        &self,
        package: NativePackageConfiguration,
        signal: Option<AbortSignal>,
    ) -> AsyncTask<CompilerTask<NativeDocumentAnalysisResult>> {
        let service = self.core.clone();
        async_query(signal, move |cancellation| {
            Ok(native_result!(
                NativeDocumentAnalysisResult,
                service.analyze_document(&package.core(), Some(cancellation)),
                |(tokens, hints)| NativeDocumentAnalysis {
                    tokens: tokens
                        .into_iter()
                        .map(|token| NativeSemanticToken {
                            range:              native_source_range(token.range),
                            kind:               semantic_kind(token.kind).to_string(),
                            is_declaration:     token.is_declaration,
                            is_readonly:        token.is_readonly,
                            is_default_library: token.is_default_library,
                        })
                        .collect(),
                    hints:  hints
                        .into_iter()
                        .map(|hint| NativeInlayHint {
                            position: NativePosition {
                                line:      u32_value(hint.line),
                                character: u32_value(hint.column),
                            },
                            label:    hint.label,
                            kind:     hint.kind.to_string(),
                        })
                        .collect(),
                }
            ))
        })
    }

    #[napi]
    pub fn read_virtual_source(&self, request: NativeSourceRequest) -> NativeVirtualSourceResult {
        let query = definition_query(&request.package, String::new(), None);
        native_optional_result!(
            NativeVirtualSourceResult,
            nwnrs_language::load_nwscript_virtual_source(&query, &request.resource),
            |source| NativeVirtualSource {
                uri:      source.uri,
                contents: source.contents.into(),
            }
        )
    }

    #[napi]
    pub fn resolve_source(&self, request: NativeSourceRequest) -> NativeResolvedSourceResult {
        let query = definition_query(&request.package, String::new(), None);
        native_optional_result!(
            NativeResolvedSourceResult,
            nwnrs_language::resolve_nwscript_source(&query, &request.resource),
            |source| NativeResolvedSource {
                path:     nwnrs_nwscript::physical_path_display(&source.path),
                uri:      source.virtual_uri,
                resource: source.resource_name,
            }
        )
    }

    #[napi]
    pub fn manifest_diagnostics(
        &self,
        request: NativeManifestPathRequest,
    ) -> NativeDiagnosticsResult {
        native_result!(
            NativeDiagnosticsResult,
            self.core.manifest_diagnostics(
                Path::new(&request.path),
                request.git_executable.map(PathBuf::from),
                request.package_cache.map(PathBuf::from),
            ),
            |values| values.into_iter().map(native_language_diagnostic).collect()
        )
    }

    #[napi]
    pub fn manifest_completions(
        &self,
        request: NativeManifestPositionRequest,
    ) -> NativeManifestCompletionsResult {
        native_result!(
            NativeManifestCompletionsResult,
            self.core.manifest_completions(
                Path::new(&request.path),
                request.line,
                request.character
            ),
            |values| {
                values
                    .into_iter()
                    .map(|completion| NativeManifestCompletion {
                        label:         completion.label,
                        detail:        completion.detail,
                        documentation: completion.documentation,
                        insert_text:   completion.insert_text,
                        replacement:   native_language_range(completion.replacement),
                        kind:          match completion.kind {
                            ManifestCompletionKind::Section => "section",
                            ManifestCompletionKind::Field => "field",
                            ManifestCompletionKind::ProjectKind => "projectKind",
                            ManifestCompletionKind::ProjectLanguage => "projectLanguage",
                            ManifestCompletionKind::Folder => "folder",
                        }
                        .to_string(),
                        retrigger:     completion.retrigger,
                    })
                    .collect()
            }
        )
    }

    #[napi]
    pub fn manifest_hover(
        &self,
        request: NativeManifestPositionRequest,
    ) -> NativeManifestHoverResult {
        native_optional_result!(
            NativeManifestHoverResult,
            self.core
                .manifest_hover(Path::new(&request.path), request.line, request.character),
            |hover| NativeManifestHover {
                documentation: hover.documentation,
                range:         native_language_range(hover.range),
            }
        )
    }

    #[napi]
    pub fn manifest_definition(
        &self,
        request: NativeManifestPositionRequest,
    ) -> NativeManifestDefinitionResult {
        native_optional_result!(
            NativeManifestDefinitionResult,
            self.core.manifest_definition(
                Path::new(&request.path),
                request.line,
                request.character
            ),
            |definition| NativeManifestDefinition {
                path:  nwnrs_nwscript::physical_path_display(&definition.path),
                range: native_language_range(definition.range),
            }
        )
    }

    #[napi]
    pub fn manifest_symbols(
        &self,
        request: NativeManifestPathRequest,
    ) -> NativeManifestSymbolsResult {
        native_result!(
            NativeManifestSymbolsResult,
            self.core.manifest_symbols(Path::new(&request.path)),
            |values| values.into_iter().map(native_manifest_symbol).collect()
        )
    }

    #[napi(
        ts_args_type = "request: NativeDependencyRequest, signal?: AbortSignal | null, progress?: \
                        (error: Error | null, event: NativeDependencyProgress) => void",
        ts_return_type = "Promise<NativeDependencyStatusesResult>"
    )]
    pub fn list_package_dependencies(
        &self,
        request: NativeDependencyRequest,
        signal: Option<AbortSignal>,
    ) -> AsyncTask<CompilerTask<NativeDependencyStatusesResult>> {
        let service = self.core.clone();
        async_query(signal, move |cancellation| {
            Ok(
                match cancellation
                    .check()
                    .map_err(LanguageOperationError::from)
                    .and_then(|()| {
                        service.package_dependencies(
                            Path::new(&request.manifest_path),
                            request.git_path.map(PathBuf::from),
                            request.cache_path.map(PathBuf::from),
                        )
                    }) {
                    Ok(values) => NativeDependencyStatusesResult {
                        value: Some(values.into_iter().map(native_dependency_status).collect()),
                        error: None,
                    },
                    Err(error) => NativeDependencyStatusesResult {
                        value: None,
                        error: Some(native_operation_error(error)),
                    },
                },
            )
        })
    }

    #[napi(ts_return_type = "Promise<NativeDependencyStatusesResult>")]
    pub fn fetch_package_dependencies(
        &self,
        request: NativeDependencyRequest,
        signal: Option<AbortSignal>,
        progress: Option<DependencyProgressCallback>,
    ) -> AsyncTask<CompilerTask<NativeDependencyStatusesResult>> {
        let service = self.core.clone();
        let reporter = dependency_progress_reporter(progress);
        async_query(signal, move |cancellation| {
            Ok(
                match service.fetch_package_dependencies(
                    Path::new(&request.manifest_path),
                    request.git_path.map(PathBuf::from),
                    request.cache_path.map(PathBuf::from),
                    request.dependency.as_deref(),
                    cancellation,
                    reporter.as_ref(),
                ) {
                    Ok(values) => NativeDependencyStatusesResult {
                        value: Some(values.into_iter().map(native_dependency_status).collect()),
                        error: None,
                    },
                    Err(error) => NativeDependencyStatusesResult {
                        value: None,
                        error: Some(native_operation_error(error)),
                    },
                },
            )
        })
    }

    #[napi(
        ts_args_type = "request: NativeDependencyRequest, signal?: AbortSignal | null, progress?: \
                        (error: Error | null, event: NativeDependencyProgress) => void",
        ts_return_type = "Promise<NativeDependencyPlanResult>"
    )]
    pub fn plan_package_dependency_update(
        &self,
        request: NativeDependencyRequest,
        signal: Option<AbortSignal>,
        progress: Option<DependencyProgressCallback>,
    ) -> AsyncTask<CompilerTask<NativeDependencyPlanResult>> {
        let service = self.core.clone();
        let reporter = dependency_progress_reporter(progress);
        async_query(signal, move |cancellation| {
            Ok(
                match service.plan_package_dependency_update(
                    Path::new(&request.manifest_path),
                    request.git_path.map(PathBuf::from),
                    request.cache_path.map(PathBuf::from),
                    request.dependency.as_deref(),
                    cancellation,
                    reporter.as_ref(),
                ) {
                    Ok(plan) => NativeDependencyPlanResult {
                        value: Some(native_dependency_plan(plan)),
                        error: None,
                    },
                    Err(error) => NativeDependencyPlanResult {
                        value: None,
                        error: Some(native_operation_error(error)),
                    },
                },
            )
        })
    }

    #[napi]
    pub fn apply_package_dependency_update(
        &self,
        request: NativeDependencyApplyRequest,
    ) -> NativeUnitResult {
        native_unit_result(
            self.core
                .apply_package_dependency_update(Path::new(&request.manifest_path), &request.token),
        )
    }

    #[napi(ts_return_type = "Promise<NativeDependencyCleanOperationResult>")]
    pub fn clean_package_dependency_cache(
        &self,
        request: NativeDependencyRequest,
        signal: Option<AbortSignal>,
    ) -> AsyncTask<CompilerTask<NativeDependencyCleanOperationResult>> {
        let service = self.core.clone();
        async_query(signal, move |cancellation| {
            Ok(
                match service.clean_package_dependency_cache(
                    request.git_path.map(PathBuf::from),
                    request.cache_path.map(PathBuf::from),
                    cancellation,
                ) {
                    Ok(removed) => NativeDependencyCleanOperationResult {
                        value: Some(NativeDependencyCleanResult {
                            removed: removed
                                .into_iter()
                                .map(|path| nwnrs_nwscript::physical_path_display(&path))
                                .collect(),
                        }),
                        error: None,
                    },
                    Err(error) => NativeDependencyCleanOperationResult {
                        value: None,
                        error: Some(native_operation_error(error)),
                    },
                },
            )
        })
    }

    #[napi(ts_return_type = "Promise<NativeDependencySourcesResult>")]
    pub fn list_package_dependency_sources(
        &self,
        request: NativeDependencySourceRequest,
        signal: Option<AbortSignal>,
    ) -> AsyncTask<CompilerTask<NativeDependencySourcesResult>> {
        let service = self.core.clone();
        async_query(signal, move |cancellation| {
            Ok(
                match service.package_dependency_sources(
                    Path::new(&request.manifest_path),
                    &request.route,
                    request.git_path.map(PathBuf::from),
                    request.cache_path.map(PathBuf::from),
                    cancellation,
                ) {
                    Ok(values) => NativeDependencySourcesResult {
                        value: Some(values.into_iter().map(native_dependency_source).collect()),
                        error: None,
                    },
                    Err(error) => NativeDependencySourcesResult {
                        value: None,
                        error: Some(native_operation_error(error)),
                    },
                },
            )
        })
    }

    #[napi]
    pub fn read_package_dependency_source(
        &self,
        request: NativeDependencySourceRequest,
    ) -> NativeDependencySourceContentsResult {
        let Some(relative_path) = request.relative_path else {
            return NativeDependencySourceContentsResult {
                value: None,
                error: Some(NativeOperationError {
                    code:           "nwpkg.source.missingPath".to_string(),
                    message:        "dependency source request is missing relativePath".to_string(),
                    path:           None,
                    manifest_field: Vec::new(),
                }),
            };
        };
        match self.core.read_package_dependency_source(
            Path::new(&request.manifest_path),
            &request.route,
            &relative_path,
            request.git_path.map(PathBuf::from),
            request.cache_path.map(PathBuf::from),
        ) {
            Ok(value) => NativeDependencySourceContentsResult {
                value: Some(NativeDependencySourceContents {
                    contents: Buffer::from(value.bytes),
                    encoding: value.encoding,
                }),
                error: None,
            },
            Err(error) => NativeDependencySourceContentsResult {
                value: None,
                error: Some(native_operation_error(error)),
            },
        }
    }

    #[napi]
    pub fn list_event_catalog(&self) -> Vec<NativeEventCatalogEntry> {
        nwnrs_runtime::EVENT_CATALOG
            .iter()
            .map(native_event_catalog_entry)
            .collect()
    }

    #[napi(ts_return_type = "Promise<NativeEventCompletionsResult>")]
    pub fn event_completions_at(
        &self,
        request: NativeEventPositionRequest,
        signal: Option<AbortSignal>,
    ) -> AsyncTask<CompilerTask<NativeEventCompletionsResult>> {
        let service = self.core.clone();
        async_query(signal, move |cancellation| {
            Ok(native_optional_result!(
                NativeEventCompletionsResult,
                service.event_completions_at(
                    &request.package.core(),
                    usize::try_from(request.offset).unwrap_or(usize::MAX),
                    cancellation,
                ),
                native_event_completions
            ))
        })
    }

    #[napi(ts_return_type = "Promise<NativeEventSymbolResult>")]
    pub fn resolve_event_symbol_at(
        &self,
        request: NativeEventPositionRequest,
        signal: Option<AbortSignal>,
    ) -> AsyncTask<CompilerTask<NativeEventSymbolResult>> {
        let service = self.core.clone();
        async_query(signal, move |cancellation| {
            Ok(native_optional_result!(
                NativeEventSymbolResult,
                service.resolve_event_symbol_at(
                    &request.package.core(),
                    usize::try_from(request.offset).unwrap_or(usize::MAX),
                    cancellation,
                ),
                native_event_symbol
            ))
        })
    }
}

impl Default for LanguageService {
    fn default() -> Self {
        Self::new()
    }
}

fn check_options(request: NativeCheckRequest) -> NwScriptCheckOptions {
    NwScriptCheckOptions {
        paths: request.paths.into_iter().map(PathBuf::from).collect(),
        no_entrypoint_check: request.no_entrypoint_check,
        langspec: request.langspec.map(PathBuf::from),
        include_dirs: request
            .include_directories
            .into_iter()
            .map(PathBuf::from)
            .collect(),
        source_overlays: Default::default(),
        optimization: request.optimization,
        optimization_flags: request.optimization_flags,
        max_include_depth: usize::try_from(request.max_include_depth).unwrap_or(usize::MAX),
        max_diagnostics_per_input: usize::try_from(request.max_diagnostics_per_input)
            .unwrap_or(usize::MAX),
        recurse: request.recurse,
        follow_symlinks: request.follow_symlinks,
        jobs: request
            .jobs
            .map(|jobs| usize::try_from(jobs).unwrap_or(usize::MAX)),
        root: request.root.map(PathBuf::from),
        user: request.user.map(PathBuf::from),
        language: request.language,
        load_ovr: request.load_ovr,
        git_executable: request.git_executable.map(PathBuf::from),
        package_cache: request.package_cache.map(PathBuf::from),
    }
}

fn check_response(report: nwnrs_language::LanguageCheckReport) -> NativeCheckResponse {
    NativeCheckResponse {
        diagnostics: report
            .diagnostics
            .into_iter()
            .map(native_language_diagnostic)
            .collect(),
        summary:     NativeCheckSummary {
            compiled: u32_value(report.summary.compiled),
            skipped:  u32_value(report.summary.skipped),
            failed:   u32_value(report.summary.failed),
        },
    }
}

fn definition_query(
    package: &NativePackageConfiguration,
    symbol: String,
    qualifier: Option<String>,
) -> NwScriptDefinitionQuery {
    let package = package.core();
    NwScriptDefinitionQuery {
        source_path: package.source_path,
        symbol,
        qualifier,
        project_root: package.project_root,
        include_directories: package.include_directories,
        source_overlays: Default::default(),
        langspec: package.langspec,
        max_include_depth: package.max_include_depth,
        root: package.root,
        user: package.user,
        language: package.language,
        load_ovr: package.load_ovr,
    }
}

fn native_completion(value: nwnrs_language::NwScriptCompletion) -> NativeCompletion {
    NativeCompletion {
        label:         value.label,
        detail:        value.detail,
        documentation: value.documentation,
        insert_text:   value.insert_text,
        sort_text:     value.sort_text,
        replacement:   native_source_range(value.replacement),
        kind:          match value.kind {
            NwScriptCompletionKind::Function => "function",
            NwScriptCompletionKind::Variable => "variable",
            NwScriptCompletionKind::Constant => "constant",
            NwScriptCompletionKind::Parameter => "parameter",
            NwScriptCompletionKind::Field => "field",
            NwScriptCompletionKind::Type => "type",
            NwScriptCompletionKind::Enum => "enum",
            NwScriptCompletionKind::EnumMember => "enumMember",
            NwScriptCompletionKind::Macro => "macro",
        }
        .to_string(),
    }
}

fn native_signature_help(value: nwnrs_language::NwScriptSignatureHelp) -> NativeSignatureHelp {
    NativeSignatureHelp {
        signatures:       value
            .signatures
            .into_iter()
            .map(|signature| NativeSignature {
                label:         signature.label,
                documentation: signature.documentation,
                parameters:    signature
                    .parameters
                    .into_iter()
                    .map(|parameter| NativeSignatureParameter {
                        label:         parameter.label,
                        documentation: parameter.documentation,
                    })
                    .collect(),
            })
            .collect(),
        active_signature: u32_value(value.active_signature),
        active_parameter: u32_value(value.active_parameter),
    }
}

fn native_definition(value: nwnrs_language::NwScriptSymbolDefinition) -> NativeDefinition {
    NativeDefinition {
        name:              value.name,
        kind:              symbol_kind(value.kind).to_string(),
        path:              nwnrs_nwscript::physical_path_display(&value.path),
        range:             NativeRange {
            start: NativePosition {
                line:      one_based_to_zero(value.start_line),
                character: one_based_to_zero(value.start_column),
            },
            end:   NativePosition {
                line:      one_based_to_zero(value.end_line),
                character: one_based_to_zero(value.end_column),
            },
        },
        signature:         value.signature,
        documentation:     value.documentation,
        is_implementation: value.is_implementation,
        uri:               value.virtual_uri,
        resource:          value.resource_name,
    }
}

fn native_reference(value: nwnrs_language::NwScriptReference) -> NativeReference {
    NativeReference {
        name:           value.name,
        kind:           symbol_kind(value.kind).to_string(),
        path:           nwnrs_nwscript::physical_path_display(&value.path),
        range:          native_source_range(value.range),
        is_declaration: value.is_declaration,
        container:      value.container,
        uri:            value.virtual_uri,
        resource:       value.resource_name,
    }
}

fn native_document_symbol(value: nwnrs_language::NwScriptDocumentSymbol) -> NativeDocumentSymbol {
    NativeDocumentSymbol {
        name:      value.name,
        detail:    value.detail,
        kind:      document_symbol_kind(value.kind).to_string(),
        range:     native_source_range(value.range),
        selection: native_source_range(value.selection_range),
        children:  value
            .children
            .into_iter()
            .map(native_document_symbol)
            .collect(),
    }
}

fn native_source_range(value: nwnrs_language::NwScriptSourceRange) -> NativeRange {
    NativeRange {
        start: NativePosition {
            line:      one_based_to_zero(value.start_line),
            character: one_based_to_zero(value.start_column),
        },
        end:   NativePosition {
            line:      one_based_to_zero(value.end_line),
            character: one_based_to_zero(value.end_column),
        },
    }
}

fn core_source_range(value: NativeRange) -> nwnrs_language::NwScriptSourceRange {
    nwnrs_language::NwScriptSourceRange {
        start_line:   usize::try_from(value.start.line)
            .unwrap_or(usize::MAX)
            .saturating_add(1),
        start_column: usize::try_from(value.start.character)
            .unwrap_or(usize::MAX)
            .saturating_add(1),
        end_line:     usize::try_from(value.end.line)
            .unwrap_or(usize::MAX)
            .saturating_add(1),
        end_column:   usize::try_from(value.end.character)
            .unwrap_or(usize::MAX)
            .saturating_add(1),
    }
}

fn native_language_range(value: nwnrs_language::LanguageRange) -> NativeRange {
    NativeRange {
        start: NativePosition {
            line:      value.start.line,
            character: value.start.character,
        },
        end:   NativePosition {
            line:      value.end.line,
            character: value.end.character,
        },
    }
}

fn native_manifest_symbol(value: nwnrs_language::ManifestSymbol) -> NativeManifestSymbol {
    NativeManifestSymbol {
        name:      value.name,
        detail:    value.detail,
        kind:      match value.kind {
            ManifestSymbolKind::Section => "section",
            ManifestSymbolKind::Field => "field",
        }
        .to_string(),
        range:     native_language_range(value.range),
        selection: native_language_range(value.selection),
        children:  value
            .children
            .into_iter()
            .map(native_manifest_symbol)
            .collect(),
    }
}

fn native_dependency_status(value: PackageDependencyStatus) -> NativeDependencyStatus {
    NativeDependencyStatus {
        route:         value.route,
        name:          value.name,
        parent:        value.parent,
        source_kind:   value.source_kind,
        selector:      value.selector,
        commit:        value.commit,
        state:         value.state,
        root:          value
            .root
            .map(|path| nwnrs_nwscript::physical_path_display(&path)),
        manifest_path: value
            .manifest_path
            .map(|path| nwnrs_nwscript::physical_path_display(&path)),
        message:       value.message,
    }
}

fn native_dependency_change(value: PackageDependencyChange) -> NativeDependencyChange {
    NativeDependencyChange {
        route:  value.route,
        before: value.before,
        after:  value.after,
    }
}

fn native_dependency_plan(value: PackageDependencyPlan) -> NativeDependencyPlan {
    NativeDependencyPlan {
        token:   value.token,
        changes: value
            .changes
            .into_iter()
            .map(native_dependency_change)
            .collect(),
        status:  value
            .status
            .into_iter()
            .map(native_dependency_status)
            .collect(),
    }
}

fn native_dependency_source(value: PackageDependencySource) -> NativeDependencySource {
    NativeDependencySource {
        route:         value.route,
        relative_path: value.relative_path,
        physical_path: nwnrs_nwscript::physical_path_display(&value.physical_path),
        encoding:      value.encoding,
    }
}

fn native_language_diagnostic(
    value: nwnrs_language::LanguageDiagnostic,
) -> NativeLanguageDiagnostic {
    let (path, uri) = match value.source {
        nwnrs_language::LanguageSource::Physical(path) => {
            (Some(nwnrs_nwscript::physical_path_display(&path)), None)
        }
        nwnrs_language::LanguageSource::Virtual(uri) => (None, Some(uri)),
    };
    NativeLanguageDiagnostic {
        severity: match value.severity {
            nwnrs_language::LanguageDiagnosticSeverity::Error => "error",
            nwnrs_language::LanguageDiagnosticSeverity::Warning => "warning",
            nwnrs_language::LanguageDiagnosticSeverity::Information => "information",
            nwnrs_language::LanguageDiagnosticSeverity::Hint => "hint",
        }
        .to_string(),
        code: value.code.map(|code| match code {
            nwnrs_language::LanguageDiagnosticCode::Numeric(code) => Either::A(code),
            nwnrs_language::LanguageDiagnosticCode::Named(code) => Either::B(code),
        }),
        path,
        uri,
        message: value.message,
        range: native_language_range(value.range),
    }
}

fn native_event_catalog_entry(event: &nwnrs_runtime::EventDefinition) -> NativeEventCatalogEntry {
    NativeEventCatalogEntry {
        subject:     event.subject.to_string(),
        name:        event.name.to_string(),
        phase:       event.phase.to_string(),
        skippable:   event.skippable,
        result_kind: event_result_kind(event.result_kind).to_string(),
        control:     event_control(event.control_policy),
        source:      match event.source_kind {
            EventSourceKind::NativeDetour => "native detour",
            EventSourceKind::EventPump => "event pump",
        }
        .to_string(),
        frequency:   match event.frequency {
            EventFrequency::Normal => "normal",
            EventFrequency::Hot => "hot",
        }
        .to_string(),
        filter:      event_filter(event.filter_policy),
    }
}

fn native_event_completions(
    value: nwnrs_language::EventLanguageCompletions,
) -> NativeEventCompletions {
    NativeEventCompletions {
        kind:              match value.kind {
            EventLanguageCompletionKind::Construct => "construct",
            EventLanguageCompletionKind::Subject => "subject",
            EventLanguageCompletionKind::Phase => "phase",
            EventLanguageCompletionKind::Modifier => "modifier",
            EventLanguageCompletionKind::Target => "target",
        }
        .to_string(),
        prefix:            value.prefix,
        replacement_start: u32_value(value.replacement_start),
        replacement_end:   u32_value(value.replacement_end),
        subject:           value.subject,
        phase:             value.phase,
        modifier:          value.modifier,
        items:             value
            .items
            .into_iter()
            .map(|item| NativeEventCompletionItem {
                label:         item.label,
                kind:          match item.kind {
                    EventLanguageCompletionItemKind::Keyword => "keyword",
                    EventLanguageCompletionItemKind::Event => "event",
                    EventLanguageCompletionItemKind::Phase => "enumMember",
                    EventLanguageCompletionItemKind::Function => "function",
                }
                .to_string(),
                detail:        item.detail,
                documentation: item.documentation,
                sort_text:     format!("{:08}", item.sort_order),
                definition:    item.definition.map(native_definition),
            })
            .collect(),
    }
}

fn native_event_symbol(value: nwnrs_language::EventLanguageSymbol) -> NativeEventSymbol {
    NativeEventSymbol {
        kind:       match value.kind {
            EventLanguageSymbolKind::Route => "route",
            EventLanguageSymbolKind::Handler => "handler",
        }
        .to_string(),
        subject:    value.subject,
        phase:      value.phase,
        token:      match value.token {
            EventLanguageSymbolToken::Subject => "subject",
            EventLanguageSymbolToken::Phase => "phase",
            EventLanguageSymbolToken::Handler => "handler",
        }
        .to_string(),
        range:      NativeRange {
            start: NativePosition {
                line:      u32_value(value.range.start_line),
                character: u32_value(value.range.start_column),
            },
            end:   NativePosition {
                line:      u32_value(value.range.end_line),
                character: u32_value(value.range.end_column),
            },
        },
        definition: value.definition.map(native_definition),
    }
}

fn symbol_kind(kind: NwScriptSymbolKind) -> &'static str {
    match kind {
        NwScriptSymbolKind::Function => "function",
        NwScriptSymbolKind::BuiltinFunction => "builtinFunction",
        NwScriptSymbolKind::Macro => "macro",
        NwScriptSymbolKind::Struct => "struct",
        NwScriptSymbolKind::Enum => "enum",
        NwScriptSymbolKind::EnumVariant => "enumVariant",
        NwScriptSymbolKind::TypeAlias => "typeAlias",
        NwScriptSymbolKind::Field => "field",
        NwScriptSymbolKind::Variable => "variable",
        NwScriptSymbolKind::Constant => "constant",
        NwScriptSymbolKind::Parameter => "parameter",
        NwScriptSymbolKind::BuiltinConstant => "builtinConstant",
        NwScriptSymbolKind::EngineStructure => "engineStructure",
    }
}

fn document_symbol_kind(kind: NwScriptDocumentSymbolKind) -> &'static str {
    match kind {
        NwScriptDocumentSymbolKind::Function => "function",
        NwScriptDocumentSymbolKind::Macro => "macro",
        NwScriptDocumentSymbolKind::Struct => "struct",
        NwScriptDocumentSymbolKind::Enum => "enum",
        NwScriptDocumentSymbolKind::EnumVariant => "enumVariant",
        NwScriptDocumentSymbolKind::TypeAlias => "typeAlias",
        NwScriptDocumentSymbolKind::Field => "field",
        NwScriptDocumentSymbolKind::Variable => "variable",
        NwScriptDocumentSymbolKind::Constant => "constant",
    }
}

fn semantic_kind(kind: NwScriptSemanticTokenKind) -> &'static str {
    match kind {
        NwScriptSemanticTokenKind::Function => "function",
        NwScriptSemanticTokenKind::Parameter => "parameter",
        NwScriptSemanticTokenKind::Variable => "variable",
        NwScriptSemanticTokenKind::Property => "property",
        NwScriptSemanticTokenKind::Type => "type",
        NwScriptSemanticTokenKind::Enum => "enum",
        NwScriptSemanticTokenKind::EnumMember => "enumMember",
        NwScriptSemanticTokenKind::Macro => "macro",
        NwScriptSemanticTokenKind::Event => "event",
    }
}

const fn event_result_kind(kind: EventResultKind) -> &'static str {
    match kind {
        EventResultKind::None => "none",
        EventResultKind::Boolean => "boolean",
        EventResultKind::Unsigned => "unsigned integer",
        EventResultKind::Signed => "signed integer",
        EventResultKind::Float => "float",
        EventResultKind::String => "string",
        EventResultKind::ObjectId => "object id",
    }
}

fn event_control(policy: EventControlPolicy) -> String {
    match policy {
        EventControlPolicy::Observe => "observe only".to_string(),
        EventControlPolicy::SuppressOriginal => "may suppress the original operation".to_string(),
        EventControlPolicy::TypedResult(EventResultRole::None) => {
            "typed control without a replacement result".to_string()
        }
        EventControlPolicy::TypedResult(EventResultRole::CancellationReturn) => {
            "suppression uses the typed result as the native return value".to_string()
        }
        EventControlPolicy::TypedResult(EventResultRole::ReplaceReturn) => {
            "may replace the native return value".to_string()
        }
        EventControlPolicy::TypedResult(EventResultRole::ReplaceArgument(argument)) => {
            format!("may replace the `{argument}` input argument")
        }
    }
}

fn event_filter(policy: EventFilterPolicy) -> String {
    match policy {
        EventFilterPolicy::SubscriptionOnly => "subscription only".to_string(),
        EventFilterPolicy::OptionalIntegerWhitelists(names) => {
            format!("optional integer whitelists: {}", names.join(", "))
        }
        EventFilterPolicy::RequiredIntegerWhitelist(name) => {
            format!("required integer whitelist: {name}")
        }
    }
}

fn u32_value(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn one_based_to_zero(value: usize) -> u32 {
    u32::try_from(value.saturating_sub(1)).unwrap_or(u32::MAX)
}
