'use strict';

const { WorkerClient } = require('./worker-client');

class ResourceEditorWorkerClient extends WorkerClient {
  constructor(workerPath, bindingPath, output) {
    super(workerPath, { bindingPath }, output, 'resource editor');
  }

  request(method, request, cancellationToken) {
    // Resource writes are transactional and must run to a definite completion.
    // VS Code cancellation is therefore observed before a request is queued by
    // the provider, not by abandoning an in-flight native mutation or save.
    if (cancellationToken?.isCancellationRequested) {
      return Promise.reject(new Error(`${method} cancelled before it was queued`));
    }
    return super.request(method, request, undefined);
  }

  readEntryBytes(documentId, resource, cancellationToken) {
    return this.request('readEntryBytes', { documentId, resource }, cancellationToken);
  }
}

module.exports = { ResourceEditorWorkerClient };
