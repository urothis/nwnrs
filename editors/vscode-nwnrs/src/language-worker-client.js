'use strict';

const { WorkerClient } = require('./worker-client');

class LanguageWorkerClient extends WorkerClient {
  constructor(workerPath, bindingPath, output) {
    super(workerPath, { bindingPath, persistent: true }, output, 'language');
  }

  request(method, request, cancellationToken, sessionKey = '') {
    return super.request(method, request, cancellationToken, { sessionKey });
  }

  invalidate(sessionKey = '', changedPath) {
    this.post({ type: 'invalidate', sessionKey, changedPath });
  }

  release(sessionKey) {
    this.post({ type: 'release', sessionKey });
  }
}

module.exports = { LanguageWorkerClient };
