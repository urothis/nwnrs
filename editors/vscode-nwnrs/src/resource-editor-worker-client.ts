import {
  WorkerClient,
  type CancellationTokenLike,
  type OutputChannelLike,
} from './worker-client';
import { isResourceEditorMutation } from './resource-editor-methods';

export class ResourceEditorWorkerClient extends WorkerClient {
  public constructor(workerPath: string, bindingPath: string, output?: OutputChannelLike) {
    super(workerPath, { bindingPath }, output, 'resource editor');
  }

  public override request<TResponse = unknown>(
    method: string,
    request: unknown,
    cancellationToken?: CancellationTokenLike,
  ): Promise<TResponse> {
    if (cancellationToken?.isCancellationRequested) {
      return Promise.reject(new Error(`${method} cancelled before it was queued`));
    }
    // Mutations run to a definite completion once queued. Read-only work stays
    // cooperatively cancellable through the worker and native task.
    return super.request<TResponse>(
      method,
      request,
      isResourceEditorMutation(method) ? undefined : cancellationToken,
    );
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
