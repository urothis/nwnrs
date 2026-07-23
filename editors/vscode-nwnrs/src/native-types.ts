/**
 * JSON contracts produced by `crates/vscode-native`.
 *
 * Keep these names and optional fields aligned with the corresponding Rust
 * `Serialize` structs. They describe a trusted, in-process native boundary;
 * untrusted worker messages are validated separately before reaching it.
 */

export interface NativeSourceRange {
  readonly start_line: number;
  readonly start_column: number;
  readonly end_line: number;
  readonly end_column: number;
}

export interface NativeSourceOverlay {
  readonly path: string;
  readonly contents: string;
}

export interface NativeLanguageRequestContext {
  readonly source_path: string;
  readonly project_root?: string | null;
  readonly include_dirs: readonly string[];
  readonly overlays: readonly NativeSourceOverlay[];
  readonly langspec?: string | null;
  readonly max_include_depth: number;
  readonly root?: string | null;
  readonly user?: string | null;
  readonly language: string;
  readonly load_ovr: boolean;
  readonly resource?: string | null;
}

export interface NativeDefinition extends NativeSourceRange {
  readonly name: string;
  readonly kind: string;
  readonly path: string;
  readonly signature: string;
  readonly documentation: string | null;
  readonly is_implementation: boolean;
  readonly uri: string | null;
  readonly resource: string | null;
}

export interface NativeReference {
  readonly name: string;
  readonly kind: string;
  readonly path: string;
  readonly range: NativeSourceRange;
  readonly is_declaration: boolean;
  readonly container: string | null;
  readonly uri: string | null;
  readonly resource: string | null;
}

export interface NativeOutgoingCall {
  readonly target: NativeDefinition;
  readonly ranges: readonly NativeSourceRange[];
}

export interface NativeDocumentSymbol {
  readonly name: string;
  readonly kind: string;
  readonly detail: string | null;
  readonly range: NativeSourceRange;
  readonly selection_range: NativeSourceRange;
  readonly children: readonly NativeDocumentSymbol[];
}

export interface NativeProjectIndexDocument {
  readonly path: string;
  readonly symbols: readonly NativeDocumentSymbol[];
}

export interface NativeProjectIndex {
  readonly documents: readonly NativeProjectIndexDocument[];
  readonly warnings: readonly string[];
}

export interface NativeSemanticToken {
  readonly range: NativeSourceRange;
  readonly kind: string;
  readonly is_declaration: boolean;
  readonly is_readonly: boolean;
  readonly is_default_library: boolean;
}

export interface NativeInlayHint {
  readonly line: number;
  readonly column: number;
  readonly label: string;
  readonly kind: string;
}

export interface NativeSemanticDocument {
  readonly tokens: readonly NativeSemanticToken[];
  readonly hints: readonly NativeInlayHint[];
}

export interface NativeCheckDiagnostic {
  readonly input: string;
  readonly severity: string;
  readonly code: number | null;
  readonly message: string;
  readonly file: string | null;
  readonly start_line: number | null;
  readonly start_column: number | null;
  readonly end_line: number | null;
  readonly end_column: number | null;
}

export interface NativeCheckResponse {
  readonly diagnostics: readonly NativeCheckDiagnostic[];
  readonly summary: {
    readonly compiled: number;
    readonly skipped: number;
    readonly failed: number;
  };
}

export interface NativeNwpkgDiagnostic extends NativeSourceRange {
  readonly severity: string;
  readonly message: string;
}

export interface NativeNwpkgCheckResponse {
  readonly diagnostics: readonly NativeNwpkgDiagnostic[];
}

export interface NativeVirtualSource {
  readonly uri: string;
  readonly contents: string;
}

export interface NativeResolvedSource {
  readonly path: string;
  readonly uri: string | null;
  readonly resource: string | null;
}

export interface NativeIncludeCandidate {
  readonly include_name: string;
  readonly path: string;
  readonly start_line: number;
  readonly start_column: number;
}

export interface NativeLanguageResponseMap {
  readonly checkNss: NativeCheckResponse;
  readonly findDefinitions: readonly NativeDefinition[];
  readonly findReferences: readonly NativeReference[];
  readonly findOutgoingCalls: readonly NativeOutgoingCall[];
  readonly findIncludeCandidates: readonly NativeIncludeCandidate[];
  readonly readVirtualSource: NativeVirtualSource;
  readonly resolveSource: NativeResolvedSource | null;
  readonly listDocumentSymbols: readonly NativeDocumentSymbol[];
  readonly indexProject: NativeProjectIndex;
  readonly analyzeDocument: NativeSemanticDocument;
  readonly deduplicateProjectRoots: readonly string[];
  readonly resolveWatchRoots: readonly string[];
  readonly checkNwpkg: NativeNwpkgCheckResponse;
}

export interface NativePackageDependency {
  readonly name: string;
  readonly root: string;
  readonly manifestPath: string;
}

export interface NativePackageInfo {
  readonly manifestPath: string;
  readonly root: string;
  readonly name: string;
  readonly kind: string;
  readonly sourcePath: string;
  readonly resourcePaths: readonly string[];
  readonly dependencies: readonly NativePackageDependency[];
}

export interface NativePackageSourceFile {
  readonly path: string;
  readonly relativePath: string;
  readonly kind: string;
}

export interface NativeAreaObject {
  readonly key: string;
  readonly kind: string;
  readonly label: string;
  readonly sourceIndex: number;
  readonly tag: string | null;
  readonly templateResref: string | null;
  readonly position: readonly [number, number, number];
  readonly rotationAxisAngle: readonly [number, number, number, number];
}

export interface NativePackageSourceArea {
  readonly resref: string;
  readonly registered: boolean;
  readonly files: readonly NativePackageSourceFile[];
  readonly missing: readonly string[];
  readonly conflicts: readonly string[];
  readonly objects: readonly NativeAreaObject[];
  readonly objectError: string | null;
}

export interface NativePackageSourceInfo {
  readonly sourcePath: string;
  readonly areas: readonly NativePackageSourceArea[];
  readonly dialogs: readonly NativePackageSourceFile[];
  readonly code: readonly NativePackageSourceFile[];
  readonly warnings: readonly string[];
}

export interface NativeResourceCatalogItem {
  readonly kind: string;
  readonly label: string;
  readonly count: number;
  readonly layer?: string;
  readonly family?: string;
  readonly extension?: string;
  readonly prefix?: string;
  readonly resource?: string;
  readonly origin?: string;
  readonly filePath?: string;
}

export interface NativeResourceCatalog {
  readonly items: readonly NativeResourceCatalogItem[];
}

export interface NativeResolvedResource {
  readonly resource: string;
  readonly origin: string;
  readonly file_path: string | null;
}
