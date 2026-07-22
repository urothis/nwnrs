'use strict';

const { parentPort, workerData } = require('node:worker_threads');

function invoke(binding, method, request) {
  if (typeof binding[method] !== 'function') {
    throw new Error(`native compiler does not export ${method}`);
  }
  return JSON.parse(binding[method](JSON.stringify(request)));
}

function runOnce() {
  try {
    const binding = require(workerData.bindingPath);
    const method = workerData.method || 'checkNss';
    const response = invoke(binding, method, workerData.request);
    parentPort.postMessage({ response });
  } catch (error) {
    parentPort.postMessage({
      error: error instanceof Error ? error.message : String(error),
    });
  }
}

function runPersistent() {
  const binding = require(workerData.bindingPath);
  if (typeof binding.LanguageService !== 'function') {
    throw new Error('native compiler does not export LanguageService');
  }
  const service = new binding.LanguageService();
  const queues = { interactive: [], background: [] };
  const active = new Map();
  const limits = { interactive: 2, background: 1 };
  const running = { interactive: 0, background: 0 };

  function requestClass(method) {
    return method === 'checkNss' || method === 'indexProject' ? 'background' : 'interactive';
  }

  function complete(entry, response, error) {
    active.delete(entry.id);
    running[entry.className] -= 1;
    if (entry.preempted && !entry.cancelled) {
      entry.preempted = false;
      entry.controller = undefined;
      queues[entry.className].unshift(entry);
      pump();
      return;
    }
    if (!entry.cancelled) {
      parentPort.postMessage({
        type: 'response',
        id: entry.id,
        response,
        error: error instanceof Error ? error.message : error ? String(error) : undefined,
      });
    }
    pump();
  }

  function start(entry) {
    entry.controller = new AbortController();
    active.set(entry.id, entry);
    running[entry.className] += 1;
    Promise.resolve(service.execute(
      entry.method,
      JSON.stringify(entry.request),
      entry.sessionKey,
      entry.controller.signal,
    )).then(
      (response) => complete(entry, JSON.parse(response), undefined),
      (error) => complete(entry, undefined, error),
    );
  }

  function pump() {
    for (const className of ['interactive', 'background']) {
      while (running[className] < limits[className] && queues[className].length > 0) {
        const entry = queues[className].shift();
        if (!entry.cancelled) {
          start(entry);
        }
      }
    }
  }

  function cancel(id) {
    const runningEntry = active.get(id);
    if (runningEntry) {
      runningEntry.cancelled = true;
      runningEntry.controller.abort();
      return;
    }
    for (const queue of Object.values(queues)) {
      const index = queue.findIndex((entry) => entry.id === id);
      if (index >= 0) {
        queue.splice(index, 1);
        return;
      }
    }
  }

  function preemptProjectIndexing() {
    for (const entry of active.values()) {
      if (entry.method === 'indexProject' && !entry.cancelled && !entry.preempted) {
        entry.preempted = true;
        entry.controller.abort();
      }
    }
  }

  parentPort.postMessage({ type: 'ready' });

  parentPort.on('message', (message) => {
    if (message?.type === 'cancel') {
      cancel(message.id);
      return;
    }
    if (message?.type === 'invalidate') {
      service.invalidate(message.sessionKey || undefined, message.changedPath || undefined);
      return;
    }
    if (message?.type === 'release') {
      service.release(message.sessionKey);
      return;
    }
    if (message?.type !== 'request') {
      return;
    }
    const { id, method, request, sessionKey = '' } = message;
    const className = requestClass(method);
    if (className === 'interactive') {
      preemptProjectIndexing();
    }
    queues[className].push({
      id,
      method,
      request,
      sessionKey,
      className,
      cancelled: false,
      preempted: false,
      controller: undefined,
    });
    pump();
  });
}

if (workerData.persistent) {
  runPersistent();
} else {
  runOnce();
}
