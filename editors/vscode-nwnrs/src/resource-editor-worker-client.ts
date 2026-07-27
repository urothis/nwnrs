import {
  WorkerClient,
  type CancellationTokenLike,
  type OutputChannelLike,
} from './worker-client';
import { isResourceEditorMutation } from './resource-editor-scheduler';
import type {
  ResourceEditorWorkerMethod,
  ResourceEditorWorkerRequest,
  ResourceEditorWorkerResponse,
} from './resource-editor-protocol';

export class ResourceEditorWorkerClient extends WorkerClient {
  public constructor(workerPath: string, bindingPath: string, output?: OutputChannelLike) {
    super(workerPath, { bindingPath }, output, 'resource editor');
  }

  public requestResource<Method extends ResourceEditorWorkerMethod>(
    method: Method,
    request: ResourceEditorWorkerRequest<Method>,
    cancellationToken?: CancellationTokenLike,
  ): Promise<ResourceEditorWorkerResponse<Method>> {
    if (cancellationToken?.isCancellationRequested) {
      return Promise.reject(new Error(`${method} cancelled before it was queued`));
    }
    // Mutations run to a definite completion once queued. Read-only work stays
    // cooperatively cancellable through the worker and native task.
    return super.request<ResourceEditorWorkerResponse<Method>>(
      method,
      request,
      isResourceEditorMutation(method) ? undefined : cancellationToken,
    );
  }

  public readEntryBytes(
    documentId: string,
    resource: string,
    cancellationToken?: CancellationTokenLike,
  ): Promise<Uint8Array> {
    return this.requestResource(
      'readEntryBytes',
      { documentId, resource },
      cancellationToken,
    );
  }
}
