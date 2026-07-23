import {
  WorkerClient,
  type CancellationTokenLike,
  type OutputChannelLike,
} from './worker-client';

export class ResourceEditorWorkerClient extends WorkerClient {
  public constructor(workerPath: string, bindingPath: string, output?: OutputChannelLike) {
    super(workerPath, { bindingPath }, output, 'resource editor');
  }

  public override request<TResponse = unknown>(
    method: string,
    request: unknown,
    cancellationToken?: CancellationTokenLike,
  ): Promise<TResponse> {
    // Resource writes are transactional and must run to a definite completion.
    // VS Code cancellation is therefore observed before a request is queued by
    // the provider, not by abandoning an in-flight native mutation or save.
    if (cancellationToken?.isCancellationRequested) {
      return Promise.reject(new Error(`${method} cancelled before it was queued`));
    }
    return super.request<TResponse>(method, request);
  }

  public readEntryBytes<TResponse = Uint8Array>(
    documentId: string,
    resource: unknown,
    cancellationToken?: CancellationTokenLike,
  ): Promise<TResponse> {
    return this.request<TResponse>(
      'readEntryBytes',
      { documentId, resource },
      cancellationToken,
    );
  }
}
