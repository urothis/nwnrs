'use strict';

const { parentPort, workerData } = require('node:worker_threads');

const binding = require(workerData.bindingPath);
if (typeof binding.ResourceEditorService !== 'function') {
  throw new Error('native binding does not export ResourceEditorService');
}
const service = new binding.ResourceEditorService();
const queue = [];
let running = false;

async function pump() {
  if (running || queue.length === 0) return;
  running = true;
  const entry = queue.shift();
  try {
    if (entry.method === 'readEntryBytes') {
      const bytes = Uint8Array.from(await service.readEntryBytes(entry.request.documentId, entry.request.resource));
      parentPort.postMessage({ type: 'response', id: entry.id, response: bytes }, [bytes.buffer]);
      return;
    }
    const response = await service.execute(entry.method, JSON.stringify(entry.request));
    parentPort.postMessage({ type: 'response', id: entry.id, response: JSON.parse(response) });
  } catch (error) {
    parentPort.postMessage({
      type: 'response',
      id: entry.id,
      error: error instanceof Error ? error.message : String(error),
    });
  } finally {
    running = false;
    void pump();
  }
}

parentPort.postMessage({ type: 'ready' });
parentPort.on('message', (message) => {
  if (message?.type !== 'request') return;
  queue.push(message);
  void pump();
});
