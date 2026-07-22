'use strict';

const { Worker } = require('node:worker_threads');

class LanguageWorkerClient {
  constructor(workerPath, bindingPath, output) {
    this.workerPath = workerPath;
    this.bindingPath = bindingPath;
    this.output = output;
    this.nextRequestId = 1;
    this.pending = new Map();
    this.disposed = false;
    this.restartAttempts = 0;
    this.restartTimer = undefined;
    this.healthyTimer = undefined;
    this.start();
  }

  start() {
    if (this.disposed) {
      return;
    }
    const worker = new Worker(this.workerPath, {
      workerData: { bindingPath: this.bindingPath, persistent: true },
    });
    this.worker = worker;
    worker.on('message', (message) => this.handleMessage(message));
    worker.on('error', (error) => this.handleFailure(error));
    worker.on('exit', (code) => {
      if (!this.disposed && this.worker === worker) {
        this.handleFailure(new Error(`language worker exited with code ${code}`));
      }
    });
  }

  request(method, request, cancellationToken, sessionKey = '') {
    if (this.disposed) {
      return Promise.reject(new Error('language worker is disposed'));
    }
    if (cancellationToken?.isCancellationRequested) {
      return Promise.reject(new Error(`${method} cancelled`));
    }
    if (!this.worker) {
      return Promise.reject(new Error('language worker is restarting'));
    }
    const id = this.nextRequestId++;
    return new Promise((resolve, reject) => {
      const cancellation = cancellationToken?.onCancellationRequested(() => {
        const pending = this.pending.get(id);
        if (!pending) {
          return;
        }
        this.pending.delete(id);
        pending.cancellation?.dispose();
        this.worker?.postMessage({ type: 'cancel', id });
        reject(new Error(`${method} cancelled`));
      });
      this.pending.set(id, { resolve, reject, cancellation, method });
      this.worker.postMessage({ type: 'request', id, method, request, sessionKey });
    });
  }

  invalidate(sessionKey = '', changedPath) {
    this.worker?.postMessage({ type: 'invalidate', sessionKey, changedPath });
  }

  release(sessionKey) {
    this.worker?.postMessage({ type: 'release', sessionKey });
  }

  async restart() {
    if (this.disposed) {
      throw new Error('language worker is disposed');
    }
    const previousWorker = this.worker;
    this.worker = undefined;
    clearTimeout(this.restartTimer);
    clearTimeout(this.healthyTimer);
    this.restartTimer = undefined;
    this.healthyTimer = undefined;
    this.restartAttempts = 0;
    for (const pending of this.pending.values()) {
      pending.cancellation?.dispose();
      pending.reject(new Error('language worker restarted'));
    }
    this.pending.clear();
    if (previousWorker) {
      await previousWorker.terminate();
    }
    if (this.disposed) {
      return;
    }
    this.start();
    await this.waitUntilReady(this.worker);
  }

  waitUntilReady(worker) {
    if (!worker) {
      return Promise.reject(new Error('language worker did not start'));
    }
    return new Promise((resolve, reject) => {
      const timeout = setTimeout(() => finish(new Error('language worker restart timed out')), 10000);
      const onMessage = (message) => {
        if (message?.type === 'ready') {
          finish();
        }
      };
      const onError = (error) => finish(error);
      const onExit = (code) => finish(new Error(`language worker exited with code ${code}`));
      const finish = (error) => {
        clearTimeout(timeout);
        worker.off('message', onMessage);
        worker.off('error', onError);
        worker.off('exit', onExit);
        if (error) {
          reject(error);
        } else {
          resolve();
        }
      };
      worker.on('message', onMessage);
      worker.once('error', onError);
      worker.once('exit', onExit);
    });
  }

  handleMessage(message) {
    if (message?.type === 'ready') {
      clearTimeout(this.healthyTimer);
      this.healthyTimer = setTimeout(() => {
        this.healthyTimer = undefined;
        this.restartAttempts = 0;
      }, 30000);
      return;
    }
    if (!message || message.type !== 'response') {
      return;
    }
    const pending = this.pending.get(message.id);
    if (!pending) {
      return;
    }
    this.pending.delete(message.id);
    pending.cancellation?.dispose();
    if (message.error) {
      pending.reject(new Error(message.error));
    } else {
      pending.resolve(message.response);
    }
  }

  handleFailure(error) {
    if (this.disposed) {
      return;
    }
    const failedWorker = this.worker;
    this.worker = undefined;
    clearTimeout(this.healthyTimer);
    this.healthyTimer = undefined;
    for (const pending of this.pending.values()) {
      pending.cancellation?.dispose();
      pending.reject(error);
    }
    this.pending.clear();
    this.restartAttempts += 1;
    const delay = Math.min(5000, 100 * (2 ** Math.min(this.restartAttempts - 1, 6)));
    this.output?.appendLine(
      `nwnrs language worker failed; restarting in ${delay}ms: ${String(error)}`,
    );
    void failedWorker?.terminate();
    clearTimeout(this.restartTimer);
    this.restartTimer = setTimeout(() => {
      this.restartTimer = undefined;
      this.start();
    }, delay);
  }

  dispose() {
    this.disposed = true;
    for (const pending of this.pending.values()) {
      pending.cancellation?.dispose();
      pending.reject(new Error('language worker disposed'));
    }
    this.pending.clear();
    clearTimeout(this.restartTimer);
    clearTimeout(this.healthyTimer);
    this.restartTimer = undefined;
    this.healthyTimer = undefined;
    void this.worker?.terminate();
    this.worker = undefined;
  }
}

module.exports = { LanguageWorkerClient };
