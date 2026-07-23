import { createRequire } from 'node:module';
import { parentPort, workerData, type MessagePort } from 'node:worker_threads';
import {
  ResourceEditorRequestScheduler,
  type ResourceEditorScheduledRequest,
} from './resource-editor-scheduler';

interface ResourceEditorService {
  readEntryBytes(
    documentId: string,
    resource: unknown,
    signal?: AbortSignal,
  ): Promise<Uint8Array>;
  execute(method: string, request: string, signal?: AbortSignal): Promise<string>;
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

async function execute(
  message: ResourceEditorScheduledRequest,
  signal: AbortSignal | undefined,
): Promise<unknown> {
  if (message.method === 'readEntryBytes') {
    if (!isReadEntryBytesRequest(message.request)) {
      throw new Error('readEntryBytes received an invalid request');
    }
    return Uint8Array.from(await service.readEntryBytes(
      message.request.documentId,
      message.request.resource,
      signal,
    ));
  }
  const response = await service.execute(
    message.method,
    JSON.stringify(message.request),
    signal,
  );
  return JSON.parse(response) as unknown;
}

const scheduler = new ResourceEditorRequestScheduler(
  execute,
  (response) => {
    if (response.response instanceof Uint8Array
      && response.response.buffer instanceof ArrayBuffer) {
      port.postMessage(response, [response.response.buffer]);
    } else {
      port.postMessage(response);
    }
  },
);

port.postMessage({ type: 'ready' });
port.on('message', (message: unknown) => {
  if (typeof message === 'object'
    && message !== null
    && 'type' in message
    && message.type === 'cancel'
    && 'id' in message
    && typeof message.id === 'number') {
    scheduler.cancel(message.id);
    return;
  }
  if (!isWorkerRequest(message)) return;
  scheduler.enqueue(message);
});
