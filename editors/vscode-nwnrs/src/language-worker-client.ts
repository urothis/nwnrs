import {
  WorkerClient,
  type CancellationTokenLike,
  type OutputChannelLike,
  type WorkerRequestOptions,
} from './worker-client';
import type { LanguageService as NativeLanguageService } from './generated/native';
import type {
  NativeEventSymbolResult,
  NativeManifestDefinitionResult,
  NativeManifestHoverResult,
  NativeOperationError,
  NativeResolvedSourceResult,
  NativeSignatureHelpResult,
  NativeUnitResult,
  NativeVirtualSourceResult,
} from './generated/native';

export type NativeLanguageMethod = {
  [K in keyof NativeLanguageService]:
  NativeLanguageService[K] extends (...arguments_: infer _P) => infer _R ? K : never
}[keyof NativeLanguageService];

export type WorkerLanguageMethod = Exclude<
  NativeLanguageMethod,
  | 'addWorkspace'
  | 'removeWorkspace'
  | 'openDocument'
  | 'updateDocument'
  | 'invalidateDocument'
  | 'closeDocument'
  | 'invalidate'
  | 'releasePackage'
>;

export type NativeLanguageRequest<K extends WorkerLanguageMethod> =
  NativeLanguageService[K] extends (...arguments_: infer P) => unknown
    ? P extends [] ? undefined : P[0]
    : never;

type NativeOptionalOperationEnvelope =
  | NativeEventSymbolResult
  | NativeManifestDefinitionResult
  | NativeManifestHoverResult
  | NativeResolvedSourceResult
  | NativeSignatureHelpResult
  | NativeVirtualSourceResult;

type UnwrappedNativeResponse<R> =
  R extends NativeUnitResult ? void
    : R extends NativeOptionalOperationEnvelope
      ? R['value']
    : R extends { readonly value?: unknown; readonly error?: NativeOperationError }
      ? NonNullable<R['value']>
      : R;

export type NativeLanguageResponse<K extends WorkerLanguageMethod> =
  NativeLanguageService[K] extends (...arguments_: infer _P) => infer R
    ? UnwrappedNativeResponse<Awaited<R>>
    : never;

export class LanguageWorkerClient extends WorkerClient {
  private readyListener: (() => void) | undefined;
  private readonly lifecycleOutput: OutputChannelLike | undefined;

  public constructor(workerPath: string, bindingPath: string, output?: OutputChannelLike) {
    super(workerPath, { bindingPath, persistent: true }, output, 'language');
    this.lifecycleOutput = output;
  }

  public requestTyped<K extends WorkerLanguageMethod>(
    method: K,
    request: NativeLanguageRequest<K>,
    cancellationToken?: CancellationTokenLike,
    sessionKeyOrExtra?: string | WorkerRequestOptions,
  ): Promise<NativeLanguageResponse<K>> {
    return this.request<NativeLanguageResponse<K>>(
      method,
      request,
      cancellationToken,
      sessionKeyOrExtra,
    );
  }

  public override request<TResponse = unknown>(
    method: string,
    request: unknown,
    cancellationToken?: CancellationTokenLike,
    sessionKeyOrExtra: string | WorkerRequestOptions = '',
  ): Promise<TResponse> {
    const extra = typeof sessionKeyOrExtra === 'string'
      ? { sessionKey: sessionKeyOrExtra }
      : sessionKeyOrExtra;
    return super.request<TResponse>(method, request, cancellationToken, extra);
  }

  public invalidate(sessionKey = '', changedPath?: string): void {
    this.sendLifecycle(
      'invalidate',
      { ...(changedPath === undefined ? {} : { path: changedPath }) },
      { sessionKey },
    );
  }

  public release(sessionKey: string): void {
    this.sendLifecycle('releasePackage', { path: sessionKey }, { sessionKey });
  }

  public addWorkspace(path: string): void {
    this.sendLifecycle('addWorkspace', { path });
  }

  public removeWorkspace(path: string): void {
    this.sendLifecycle('removeWorkspace', { path });
  }

  public openDocument(path: string, version: number, bytes: Uint8Array): void {
    this.sendLifecycle(
      'openDocument',
      { path, version, bytes },
      { sessionKey: path, transferList: transferableBuffer(bytes) },
    );
  }

  public updateDocument(path: string, version: number, bytes: Uint8Array): void {
    this.sendLifecycle(
      'updateDocument',
      { path, version, bytes },
      { sessionKey: path, transferList: transferableBuffer(bytes) },
    );
  }

  public invalidateDocument(path: string, version: number): void {
    this.sendLifecycle('invalidateDocument', { path, version }, { sessionKey: path });
  }

  public closeDocument(path: string): void {
    this.sendLifecycle('closeDocument', { path }, { sessionKey: path });
  }

  public onReady(listener: () => void): void {
    this.readyListener = listener;
  }

  protected override onWorkerReady(): void {
    this.readyListener?.();
  }

  private sendLifecycle(
    method: string,
    request: unknown,
    extra: WorkerRequestOptions = {},
  ): void {
    void super.request<void>(method, request, undefined, extra).catch((error: unknown) => {
      const detail = error instanceof Error ? error.message : String(error);
      const code = error instanceof Error && 'code' in error && typeof error.code === 'string'
        ? ` [${error.code}]`
        : '';
      this.lifecycleOutput?.appendLine(`nwnrs ${method} failed${code}: ${detail}`);
    });
  }
}

function transferableBuffer(bytes: Uint8Array): readonly ArrayBuffer[] {
  return bytes.buffer instanceof ArrayBuffer ? [bytes.buffer] : [];
}
