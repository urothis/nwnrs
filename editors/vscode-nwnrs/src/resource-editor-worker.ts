import { createRequire } from 'node:module';
import { parentPort, workerData, type MessagePort } from 'node:worker_threads';

interface ResourceEditorService {
  readEntryBytes(documentId: string, resource: unknown): Promise<Uint8Array>;
  execute(method: string, request: string): Promise<string>;
}

interface ResourceEditorBinding {
  ResourceEditorService?: new () => ResourceEditorService;
}

interface WorkerRequest {
  readonly type: 'request';
  readonly id: number;
  readonly method: string;
  readonly request: unknown;
}

interface ReadEntryBytesRequest {
  readonly documentId: string;
  readonly resource: unknown;
}

function requireParentPort(): MessagePort {
  if (!parentPort) throw new Error('resource editor worker requires a parent message port');
  return parentPort;
}

const port = requireParentPort();

const data = workerData as { bindingPath: string };
const loadNativeModule = createRequire(__filename);
const binding = loadNativeModule(data.bindingPath) as ResourceEditorBinding;
if (typeof binding.ResourceEditorService !== 'function') {
  throw new Error('native binding does not export ResourceEditorService');
}
const service = new binding.ResourceEditorService();
const queue: WorkerRequest[] = [];
let running = false;

function isWorkerRequest(value: unknown): value is WorkerRequest {
  return typeof value === 'object'
    && value !== null
    && 'type' in value
    && value.type === 'request'
    && 'id' in value
    && typeof value.id === 'number'
    && 'method' in value
    && typeof value.method === 'string'
    && 'request' in value;
}

function isReadEntryBytesRequest(value: unknown): value is ReadEntryBytesRequest {
  return typeof value === 'object'
    && value !== null
    && 'documentId' in value
    && typeof value.documentId === 'string'
    && 'resource' in value;
}

async function pump(): Promise<void> {
  if (running || queue.length === 0) return;
  running = true;
  const entry = queue.shift();
  if (!entry) {
    running = false;
    return;
  }
  try {
    if (entry.method === 'readEntryBytes') {
      if (!isReadEntryBytesRequest(entry.request)) {
        throw new Error('readEntryBytes received an invalid request');
      }
      const bytes = Uint8Array.from(
        await service.readEntryBytes(entry.request.documentId, entry.request.resource),
      );
      port.postMessage({ type: 'response', id: entry.id, response: bytes }, [bytes.buffer]);
      return;
    }
    const response = await service.execute(entry.method, JSON.stringify(entry.request));
    port.postMessage({ type: 'response', id: entry.id, response: JSON.parse(response) });
  } catch (error) {
    port.postMessage({
      type: 'response',
      id: entry.id,
      error: error instanceof Error ? error.message : String(error),
    });
  } finally {
    running = false;
    void pump();
  }
}

port.postMessage({ type: 'ready' });
port.on('message', (message: unknown) => {
  if (!isWorkerRequest(message)) return;
  queue.push(message);
  void pump();
});
