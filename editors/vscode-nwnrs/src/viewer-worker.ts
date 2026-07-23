import { createRequire } from 'node:module';
import { parentPort, workerData, type MessagePort } from 'node:worker_threads';
import {
  ViewerRequestScheduler,
  type ViewerScheduledRequest,
  type ViewerSchedulerResponse,
} from './viewer-scheduler';

interface ViewerService {
  loadScene(request: string, signal: AbortSignal): Promise<Uint8Array>;
  loadSceneBytes(request: string, contents: Buffer, signal: AbortSignal): Promise<Uint8Array>;
  loadAnimation(request: string, signal: AbortSignal): Promise<Uint8Array>;
  loadTexture(request: string, signal: AbortSignal): Promise<Uint8Array>;
  readResource(request: string, signal: AbortSignal): Promise<Uint8Array>;
  resolveResource(request: string, signal: AbortSignal): Promise<string>;
  inspectAreaObject(request: string, signal: AbortSignal): Promise<string>;
  inspectPackage(request: string, signal: AbortSignal): Promise<string>;
  inspectPackageSource(request: string, signal: AbortSignal): Promise<string>;
  listResources(request: string, signal: AbortSignal): Promise<string>;
  invalidate(sessionKey?: string): void;
}

interface ViewerBinding {
  ViewerService?: new () => ViewerService;
}

interface ViewerWorkerRequest {
  readonly type: 'request';
  readonly id: number;
  readonly method: string;
  readonly request: unknown;
  readonly contents?: Uint8Array;
}

type ViewerJsonMethod =
  | 'resolveResource'
  | 'inspectAreaObject'
  | 'inspectPackage'
  | 'inspectPackageSource'
  | 'listResources';

function requireParentPort(): MessagePort {
  if (!parentPort) throw new Error('viewer worker requires a parent message port');
  return parentPort;
}

const port = requireParentPort();

const data = workerData as { bindingPath: string };
const loadNativeModule = createRequire(__filename);
const binding = loadNativeModule(data.bindingPath) as ViewerBinding;
if (typeof binding.ViewerService !== 'function') {
  throw new Error('native binding does not export ViewerService');
}
const service = new binding.ViewerService();

function isViewerWorkerRequest(value: unknown): value is ViewerWorkerRequest {
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

function isJsonMethod(method: string): method is ViewerJsonMethod {
  return [
    'resolveResource',
    'inspectAreaObject',
    'inspectPackage',
    'inspectPackageSource',
    'listResources',
  ].includes(method);
}

function messageType(value: unknown): string | undefined {
  if (typeof value !== 'object' || value === null || !('type' in value)) return undefined;
  return typeof value.type === 'string' ? value.type : undefined;
}

function messageId(value: unknown): number | undefined {
  if (typeof value !== 'object' || value === null || !('id' in value)) return undefined;
  return typeof value.id === 'number' ? value.id : undefined;
}

function messageSessionKey(value: unknown): string | undefined {
  if (typeof value !== 'object' || value === null || !('sessionKey' in value)) return undefined;
  return typeof value.sessionKey === 'string' ? value.sessionKey : undefined;
}

async function executeViewerRequest(
  entry: ViewerScheduledRequest,
  signal: AbortSignal,
): Promise<unknown> {
  if (isJsonMethod(entry.method)) {
    const encoded = await service[entry.method](JSON.stringify(entry.request), signal);
    return JSON.parse(encoded);
  }
  let packed: Uint8Array;
  if (entry.method === 'loadSceneBytes') {
    if (!entry.contents) throw new Error('loadSceneBytes requires binary contents');
    packed = await service.loadSceneBytes(
      JSON.stringify(entry.request),
      Buffer.from(
        entry.contents.buffer,
        entry.contents.byteOffset,
        entry.contents.byteLength,
      ),
      signal,
    );
  } else if (entry.method === 'loadAnimation') {
    packed = await service.loadAnimation(JSON.stringify(entry.request), signal);
  } else if (entry.method === 'loadTexture') {
    packed = await service.loadTexture(JSON.stringify(entry.request), signal);
  } else if (entry.method === 'readResource') {
    packed = await service.readResource(JSON.stringify(entry.request), signal);
  } else if (entry.method === 'loadScene') {
    packed = await service.loadScene(JSON.stringify(entry.request), signal);
  } else {
    throw new Error(`unsupported viewer worker method: ${entry.method}`);
  }
  // N-API may return an external ArrayBuffer owned by Rust. External buffers
  // are not safely transferable through a Node worker MessagePort.
  return Uint8Array.from(packed);
}

function postSchedulerResponse(message: ViewerSchedulerResponse): void {
  if (
    message.response instanceof Uint8Array
    && message.response.buffer instanceof ArrayBuffer
  ) {
    port.postMessage(message, [message.response.buffer]);
  } else {
    port.postMessage(message);
  }
}

const scheduler = new ViewerRequestScheduler(executeViewerRequest, postSchedulerResponse);

port.postMessage({ type: 'ready' });
port.on('message', (message: unknown) => {
  const type = messageType(message);
  if (type === 'cancel') {
    const id = messageId(message);
    if (id !== undefined) scheduler.cancel(id);
    return;
  }
  if (type === 'invalidate') {
    const sessionKey = messageSessionKey(message);
    scheduler.invalidateSession(sessionKey);
    service.invalidate(sessionKey);
    return;
  }
  if (!isViewerWorkerRequest(message)) return;
  scheduler.enqueue(message);
});
port.on('close', () => scheduler.dispose());
