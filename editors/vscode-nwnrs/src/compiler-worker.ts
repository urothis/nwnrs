import { createRequire } from 'node:module';
import { parentPort, workerData, type MessagePort } from 'node:worker_threads';
import type {
  LanguageService as NativeLanguageService,
  NativeCheckRequest,
  NativeCheckResult,
  NativeCodeActionRequest,
  NativeCodeActionsResult,
  NativeCompletionsResult,
  NativeDefinitionsResult,
  NativeDependencyApplyRequest,
  NativeDependencyCleanOperationResult,
  NativeDependencyPlanResult,
  NativeDependencyProgress,
  NativeDependencyRequest,
  NativeDependencySourceContentsResult,
  NativeDependencySourceRequest,
  NativeDependencySourcesResult,
  NativeDependencyStatusesResult,
  NativeDiagnosticsResult,
  NativeDocumentAnalysisResult,
  NativeDocumentSymbolsResult,
  NativeEventCompletionsResult,
  NativeEventSymbolResult,
  NativeIncludeCandidatesResult,
  NativeManifestCompletionsResult,
  NativeManifestDefinitionResult,
  NativeManifestHoverResult,
  NativeManifestSymbolsResult,
  NativeOperationError,
  NativeOutgoingCallsResult,
  NativeDefinitionRequest,
  NativeEventPositionRequest,
  NativeManifestPathRequest,
  NativeManifestPositionRequest,
  NativePackageConfiguration,
  NativePositionRequest,
  NativeRenameRequest,
  NativeRenamePlanResult,
  NativePathResult,
  NativeProjectIndexResult,
  NativeProjectRootsRequest,
  NativeReferencesResult,
  NativeResolvedSourceResult,
  NativeSignatureHelpResult,
  NativeSourceRequest,
  NativeStringsResult,
  NativeUnitResult,
  NativeVirtualSourceResult,
  NativeWorkspacePackagesResult,
  NativeWorkspaceSymbolsResult,
} from './generated/native';
import { EXPECTED_NATIVE_CONTRACT_FINGERPRINT } from './generated/contract';
import { LanguageRequestScheduler, operationError } from './language-scheduler';

interface WorkerData {
  readonly bindingPath: string;
  readonly persistent?: boolean;
  readonly method?: string;
  readonly request?: unknown;
}

interface NativeCompilerBinding {
  readonly LanguageService?: new () => NativeLanguageService;
  readonly nativeContractFingerprint?: () => string;
}

interface RequestMessage {
  readonly type: 'request';
  readonly id: number;
  readonly method: string;
  readonly request: unknown;
  readonly sessionKey?: string;
}

function requireParentPort(): MessagePort {
  if (!parentPort) throw new Error('compiler worker requires a parent message port');
  return parentPort;
}

const port = requireParentPort();
const data = workerData as WorkerData;
const loadNativeModule = createRequire(__filename);

function loadBinding(bindingPath: string): NativeCompilerBinding {
  const binding = loadNativeModule(bindingPath) as NativeCompilerBinding;
  const actual = binding.nativeContractFingerprint?.();
  if (actual !== EXPECTED_NATIVE_CONTRACT_FINGERPRINT) {
    throw new Error(
      `native contract fingerprint mismatch: expected `
      + `${EXPECTED_NATIVE_CONTRACT_FINGERPRINT}, received ${actual ?? 'missing'}`,
    );
  }
  if (typeof binding.LanguageService !== 'function') {
    throw new Error('native compiler does not export LanguageService');
  }
  return binding;
}

async function invoke(
  service: NativeLanguageService,
  method: string,
  request: unknown,
  signal?: AbortSignal,
  progress?: (event: NativeDependencyProgress) => void,
): Promise<unknown> {
  switch (method) {
    case 'addWorkspace':
      return unwrapUnit(service.addWorkspace(requiredPath(request)));
    case 'removeWorkspace':
      return unwrapUnit(service.removeWorkspace(requiredPath(request)));
    case 'openDocument': {
      const lifecycle = documentLifecycleRequest(request);
      return unwrapUnit(service.openDocument(
        lifecycle.path,
        lifecycle.version,
        lifecycleBuffer(lifecycle),
      ));
    }
    case 'updateDocument': {
      const lifecycle = documentLifecycleRequest(request);
      return unwrapUnit(service.updateDocument(
        lifecycle.path,
        lifecycle.version,
        lifecycleBuffer(lifecycle),
      ));
    }
    case 'invalidateDocument': {
      const lifecycle = documentLifecycleRequest(request);
      return unwrapUnit(service.invalidateDocument(lifecycle.path, lifecycle.version));
    }
    case 'closeDocument':
      return unwrapUnit(service.closeDocument(requiredPath(request)));
    case 'invalidate':
      return unwrapUnit(service.invalidate(optionalPath(request)));
    case 'releasePackage':
      return unwrapUnit(service.releasePackage(requiredPath(request)));
    case 'checkNss':
      return unwrapOperation(await service.checkNss(request as NativeCheckRequest, signal));
    case 'findDefinitions':
      return unwrapOperation(
        await service.findDefinitions(request as NativeDefinitionRequest, signal),
      );
    case 'completeNwscript':
      return unwrapOperation(
        await service.completeNwscript(request as NativePositionRequest, signal),
      );
    case 'signatureHelp':
      return unwrapOperation(
        await service.signatureHelp(request as NativePositionRequest, signal),
      );
    case 'findIncludeCandidates':
      return unwrapOperation(
        await service.findIncludeCandidates(request as NativeDefinitionRequest, signal),
      );
    case 'findReferences':
      return unwrapOperation(
        await service.findReferences(request as NativePositionRequest, signal),
      );
    case 'codeActionsAt':
      return unwrapOperation(await service.codeActionsAt(
        request as NativeCodeActionRequest,
        signal,
      ));
    case 'renameAt':
      return unwrapOperation(await service.renameAt(request as NativeRenameRequest, signal));
    case 'resolvePackageRoot':
      return unwrapOperation(service.resolvePackageRoot(request as string));
    case 'findOutgoingCalls':
      return unwrapOperation(
        await service.findOutgoingCalls(request as NativePositionRequest, signal),
      );
    case 'listDocumentSymbols':
      return unwrapOperation(
        await service.listDocumentSymbols(request as NativePackageConfiguration, signal),
      );
    case 'indexProject':
      return unwrapOperation(
        await service.indexProject(request as NativePackageConfiguration, signal),
      );
    case 'analyzeDocument':
      return unwrapOperation(
        await service.analyzeDocument(request as NativePackageConfiguration, signal),
      );
    case 'deduplicateProjectRoots':
      return unwrapOperation(service.deduplicateProjectRoots(request as NativeProjectRootsRequest));
    case 'resolveWatchRoots':
      return unwrapOperation(service.resolveWatchRoots(request as NativeProjectRootsRequest));
    case 'readVirtualSource':
      return unwrapOperation(service.readVirtualSource(request as NativeSourceRequest));
    case 'resolveSource':
      return unwrapOperation(service.resolveSource(request as NativeSourceRequest));
    case 'manifestDiagnostics':
      return unwrapOperation(service.manifestDiagnostics(request as NativeManifestPathRequest));
    case 'manifestCompletions':
      return unwrapOperation(service.manifestCompletions(
        request as NativeManifestPositionRequest,
      ));
    case 'manifestHover':
      return unwrapOperation(service.manifestHover(request as NativeManifestPositionRequest));
    case 'manifestDefinition':
      return unwrapOperation(service.manifestDefinition(
        request as NativeManifestPositionRequest,
      ));
    case 'manifestSymbols':
      return unwrapOperation(service.manifestSymbols(request as NativeManifestPathRequest));
    case 'listPackageDependencies':
      return unwrapOperation(
        await service.listPackageDependencies(request as NativeDependencyRequest, signal),
      );
    case 'fetchPackageDependencies':
      return unwrapOperation(await service.fetchPackageDependencies(
        request as NativeDependencyRequest,
        signal,
        progressCallback(progress),
      ));
    case 'planPackageDependencyUpdate':
      return unwrapOperation(await service.planPackageDependencyUpdate(
        request as NativeDependencyRequest,
        signal,
        progressCallback(progress),
      ));
    case 'applyPackageDependencyUpdate':
      return unwrapUnit(
        service.applyPackageDependencyUpdate(request as NativeDependencyApplyRequest),
      );
    case 'cleanPackageDependencyCache':
      return unwrapOperation(
        await service.cleanPackageDependencyCache(request as NativeDependencyRequest, signal),
      );
    case 'listPackageDependencySources':
      return unwrapOperation(await service.listPackageDependencySources(
        request as NativeDependencySourceRequest,
        signal,
      ));
    case 'readPackageDependencySource':
      return unwrapOperation(
        service.readPackageDependencySource(request as NativeDependencySourceRequest),
      );
    case 'listEventCatalog': return service.listEventCatalog();
    case 'eventCompletionsAt':
      return unwrapOperation(
        await service.eventCompletionsAt(request as NativeEventPositionRequest, signal),
      );
    case 'resolveEventSymbolAt':
      return unwrapOperation(
        await service.resolveEventSymbolAt(request as NativeEventPositionRequest, signal),
      );
    case 'listWorkspacePackages':
      return unwrapOperation(await service.listWorkspacePackages(signal));
    case 'listWorkspaceSymbols':
      return unwrapOperation(
        await service.listWorkspaceSymbols(request as NativePackageConfiguration, signal),
      );
    default: throw new Error(`native compiler does not export ${method}`);
  }
}

type NativeValueResult =
  | NativeCheckResult
  | NativeCodeActionsResult
  | NativeCompletionsResult
  | NativeDefinitionsResult
  | NativeDependencyStatusesResult
  | NativeDependencyPlanResult
  | NativeDependencyCleanOperationResult
  | NativeDiagnosticsResult
  | NativeDocumentAnalysisResult
  | NativeDocumentSymbolsResult
  | NativeEventCompletionsResult
  | NativeEventSymbolResult
  | NativeIncludeCandidatesResult
  | NativeManifestCompletionsResult
  | NativeManifestDefinitionResult
  | NativeManifestHoverResult
  | NativeManifestSymbolsResult
  | NativeOutgoingCallsResult
  | NativeDependencySourcesResult
  | NativeDependencySourceContentsResult
  | NativeProjectIndexResult
  | NativeReferencesResult
  | NativeRenamePlanResult
  | NativeResolvedSourceResult
  | NativeSignatureHelpResult
  | NativeStringsResult
  | NativePathResult
  | NativeVirtualSourceResult
  | NativeWorkspacePackagesResult
  | NativeWorkspaceSymbolsResult;

class NativeOperationFailure extends Error {
  public readonly code: string;
  public readonly path: string | undefined;
  public readonly manifestField: readonly string[];

  public constructor(error: NativeOperationError) {
    super(error.message);
    this.name = 'NativeOperationFailure';
    this.code = error.code;
    this.path = error.path;
    this.manifestField = error.manifestField;
  }
}

function unwrapOperation(result: NativeValueResult): unknown {
  if (result.error) throw new NativeOperationFailure(result.error);
  return result.value;
}

function unwrapUnit(result: NativeUnitResult): void {
  if (result.error) throw new NativeOperationFailure(result.error);
  if (!result.succeeded) {
    throw new NativeOperationFailure({
      code: 'language.native.invalidResult',
      message: 'native dependency mutation did not report success',
      manifestField: [],
    });
  }
}

function progressCallback(
  progress: ((event: NativeDependencyProgress) => void) | undefined,
): ((error: Error | null, event: NativeDependencyProgress) => void) | undefined {
  if (!progress) return undefined;
  return (error, event) => {
    if (!error) progress(event);
  };
}

interface DocumentLifecycleRequest {
  readonly path: string;
  readonly version: number;
  readonly bytes?: Uint8Array;
}

function requiredPath(request: unknown): string {
  const path = field(request, 'path');
  if (typeof path !== 'string') {
    throw new TypeError('language lifecycle request is missing path');
  }
  return path;
}

function optionalPath(request: unknown): string | undefined {
  const path = field(request, 'path');
  if (path === undefined) return undefined;
  if (typeof path !== 'string') {
    throw new TypeError('language invalidation path must be a string');
  }
  return path;
}

function documentLifecycleRequest(request: unknown): DocumentLifecycleRequest {
  const path = requiredPath(request);
  const version = field(request, 'version');
  const bytes = field(request, 'bytes');
  if (typeof version !== 'number' || !Number.isSafeInteger(version)) {
    throw new TypeError('language document lifecycle request has an invalid version');
  }
  if (bytes !== undefined && !(bytes instanceof Uint8Array)) {
    throw new TypeError('language document lifecycle request has invalid source bytes');
  }
  return { path, version, ...(bytes === undefined ? {} : { bytes }) };
}

function lifecycleBuffer(request: DocumentLifecycleRequest): Buffer {
  if (!request.bytes) {
    throw new TypeError('language document lifecycle request is missing source bytes');
  }
  return Buffer.from(
    request.bytes.buffer,
    request.bytes.byteOffset,
    request.bytes.byteLength,
  );
}

function isRequestMessage(value: unknown): value is RequestMessage {
  return typeof value === 'object'
    && value !== null
    && 'type' in value
    && value.type === 'request'
    && 'id' in value
    && typeof value.id === 'number'
    && 'method' in value
    && typeof value.method === 'string'
    && 'request' in value;
}

function field(value: unknown, key: string): unknown {
  return typeof value === 'object' && value !== null && key in value
    ? (value as Record<string, unknown>)[key]
    : undefined;
}

function runOnce(): void {
  void (async () => {
    try {
      const binding = loadBinding(data.bindingPath);
      const service = new binding.LanguageService!();
      port.postMessage({
        response: await invoke(service, data.method ?? 'checkNss', data.request),
      });
    } catch (error) {
      port.postMessage({ error: error instanceof Error ? error.message : String(error) });
    }
  })();
}

function runPersistent(): void {
  const binding = loadBinding(data.bindingPath);
  const service = new binding.LanguageService!();
  const scheduler = new LanguageRequestScheduler(
    (request, signal) => invoke(
      service,
      request.method,
      request.request,
      signal,
      (event) => port.postMessage({
        type: 'progress',
        id: request.id,
        event,
      }),
    ),
    (response) => port.postMessage(response),
  );
  port.postMessage({ type: 'ready' });
  port.on('message', (message: unknown) => {
    if (field(message, 'type') === 'cancel') {
      const id = field(message, 'id');
      if (typeof id === 'number') scheduler.cancel(id);
      return;
    }
    if (!isRequestMessage(message)) return;
    if (isLifecycleMethod(message.method)) {
      if (message.method === 'invalidate') {
        scheduler.invalidate(message.sessionKey || undefined);
      } else if (message.method === 'releasePackage') {
        scheduler.invalidate(message.sessionKey || undefined, 'released');
      }
      void invoke(service, message.method, message.request).then(
        (response) => port.postMessage({ type: 'response', id: message.id, response }),
        (error: unknown) => port.postMessage({
          type: 'response',
          id: message.id,
          error: operationError(error),
        }),
      );
      return;
    }
    scheduler.enqueue({
      id: message.id,
      method: message.method,
      request: message.request,
      sessionKey: message.sessionKey ?? '',
    });
  });
}

function isLifecycleMethod(method: string): boolean {
  return [
    'addWorkspace',
    'removeWorkspace',
    'openDocument',
    'updateDocument',
    'invalidateDocument',
    'closeDocument',
    'invalidate',
    'releasePackage',
  ].includes(method);
}

if (data.persistent) runPersistent();
else runOnce();
