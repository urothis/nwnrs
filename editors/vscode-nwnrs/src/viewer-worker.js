'use strict';

const { parentPort, workerData } = require('node:worker_threads');

const binding = require(workerData.bindingPath);
if (typeof binding.ViewerService !== 'function') {
  throw new Error('native binding does not export ViewerService');
}
const service = new binding.ViewerService();
const queue = [];
let running = false;

async function pump() {
  if (running || queue.length === 0) return;
  running = true;
  const entry = queue.shift();
  try {
    if (entry.method === 'resolveResource') {
      const resolved = JSON.parse(await service.resolveResource(JSON.stringify(entry.request)));
      parentPort.postMessage({ type: 'response', id: entry.id, response: resolved });
      return;
    }
    let packed;
    if (entry.method === 'loadSceneBytes') packed = await service.loadSceneBytes(
      JSON.stringify(entry.request),
      Buffer.from(entry.contents.buffer, entry.contents.byteOffset, entry.contents.byteLength),
    );
    else if (entry.method === 'loadAnimation') packed = await service.loadAnimation(JSON.stringify(entry.request));
    else if (entry.method === 'loadTexture') packed = await service.loadTexture(JSON.stringify(entry.request));
    else if (entry.method === 'readResource') packed = await service.readResource(JSON.stringify(entry.request));
    else packed = await service.loadScene(JSON.stringify(entry.request));
    // N-API may return an external ArrayBuffer owned by Rust. External
    // buffers are not safely transferable through a Node worker MessagePort,
    // even when the Buffer spans its complete backing store. Always copy into
    // a V8-owned ArrayBuffer before transferring ownership to the extension.
    const response = Uint8Array.from(packed);
    parentPort.postMessage(
      { type: 'response', id: entry.id, response },
      [response.buffer],
    );
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
  if (message?.type === 'invalidate') {
    service.invalidate(message.sessionKey || undefined);
    return;
  }
  if (message?.type !== 'request') return;
  queue.push(message);
  void pump();
});
