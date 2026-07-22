'use strict';

const { WorkerClient } = require('./worker-client');

/** Binary scene client for the persistent native viewer worker. */
class ViewerWorkerClient extends WorkerClient {
  constructor(workerPath, bindingPath, output) {
    super(workerPath, { bindingPath }, output, '3D viewer');
  }

  loadScene(request, contents, cancellationToken) {
    if (!contents) return this.request('loadScene', request, cancellationToken);
    const bytes = Uint8Array.from(contents);
    return this.request(
      'loadSceneBytes',
      request,
      cancellationToken,
      { contents: bytes, transferList: [bytes.buffer] },
    );
  }

  invalidate(sessionKey) {
    this.post({ type: 'invalidate', sessionKey });
  }

  loadAnimation(request, cancellationToken) {
    return this.request('loadAnimation', request, cancellationToken);
  }

  loadTexture(request, cancellationToken) {
    return this.request('loadTexture', request, cancellationToken);
  }

  readResource(request, cancellationToken) {
    return this.request('readResource', request, cancellationToken);
  }

  resolveResource(request, cancellationToken) {
    return this.request('resolveResource', request, cancellationToken);
  }
}

module.exports = { ViewerWorkerClient };
