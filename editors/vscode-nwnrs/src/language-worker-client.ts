import {
  WorkerClient,
  type CancellationTokenLike,
  type OutputChannelLike,
  type WorkerRequestOptions,
} from './worker-client';
import type { NativeLanguageResponseMap } from './native-types';

export class LanguageWorkerClient extends WorkerClient {
  public constructor(workerPath: string, bindingPath: string, output?: OutputChannelLike) {
    super(workerPath, { bindingPath, persistent: true }, output, 'language');
  }

  public requestTyped<K extends keyof NativeLanguageResponseMap>(
    method: K,
    request: unknown,
    cancellationToken?: CancellationTokenLike,
    sessionKeyOrExtra?: string | WorkerRequestOptions,
  ): Promise<NativeLanguageResponseMap[K]> {
    return this.request<NativeLanguageResponseMap[K]>(
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
    this.post({ type: 'invalidate', sessionKey, changedPath });
  }

  public release(sessionKey: string): void {
    this.post({ type: 'release', sessionKey });
  }
}
