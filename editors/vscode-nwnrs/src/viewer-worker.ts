import { createRequire } from 'node:module';
import { parentPort, workerData, type MessagePort } from 'node:worker_threads';
import type {
  NativeViewerAreaInspectionRequest,
  NativeViewerAssetRequest,
  NativeViewerLoadRequest,
  NativeViewerPackageRequest,
  NativeViewerPackageSourceRequest,
  NativeViewerResourceCatalogRequest,
  ViewerService as NativeViewerService,
} from './generated/native';
import { EXPECTED_NATIVE_CONTRACT_FINGERPRINT } from './generated/contract';
import {
  ViewerRequestScheduler,
  type ViewerScheduledRequest,
  type ViewerSchedulerResponse,
} from './viewer-scheduler';

interface ViewerBinding {
  readonly ViewerService?: new () => NativeViewerService;
  readonly nativeContractFingerprint?: () => string;
}

interface ViewerWorkerRequest {
  readonly type: 'request';
  readonly id: number;
  readonly method: string;
  readonly request: unknown;
  readonly contents?: Uint8Array;
}

function requireParentPort(): MessagePort {
  if (!parentPort) throw new Error('viewer worker requires a parent message port');
  return parentPort;
}

const port = requireParentPort();
const data = workerData as { bindingPath: string };
const loadNativeModule = createRequire(__filename);
const binding = loadNativeModule(data.bindingPath) as ViewerBinding;
const fingerprint = binding.nativeContractFingerprint?.();
if (fingerprint !== EXPECTED_NATIVE_CONTRACT_FINGERPRINT) {
  throw new Error(
    `native contract fingerprint mismatch: expected ${EXPECTED_NATIVE_CONTRACT_FINGERPRINT}, `
    + `received ${fingerprint ?? 'missing'}`,
  );
}
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

function field(value: unknown, name: string): unknown {
  return typeof value === 'object' && value !== null && name in value
    ? (value as Record<string, unknown>)[name]
    : undefined;
}

async function executeViewerRequest(
  entry: ViewerScheduledRequest,
  signal: AbortSignal,
): Promise<unknown> {
  let response: unknown;
  switch (entry.method) {
    case 'loadSceneBytes':
      if (!entry.contents) throw new Error('loadSceneBytes requires binary contents');
      response = await service.loadSceneBytes(
        entry.request as NativeViewerLoadRequest,
        Buffer.from(
          entry.contents.buffer,
          entry.contents.byteOffset,
          entry.contents.byteLength,
        ),
        signal,
      );
      break;
    case 'loadAnimation':
      response = await service.loadAnimation(entry.request as NativeViewerAssetRequest, signal);
      break;
    case 'loadTexture':
      response = await service.loadTexture(entry.request as NativeViewerAssetRequest, signal);
      break;
    case 'readResource':
      response = await service.readResource(entry.request as NativeViewerLoadRequest, signal);
      break;
    case 'loadScene':
      response = await service.loadScene(entry.request as NativeViewerLoadRequest, signal);
      break;
    case 'resolveResource':
      return service.resolveResource(entry.request as NativeViewerLoadRequest, signal);
    case 'inspectAreaObject':
      return service.inspectAreaObject(
        entry.request as NativeViewerAreaInspectionRequest,
        signal,
      );
    case 'inspectPackage':
      return service.inspectPackage(entry.request as NativeViewerPackageRequest, signal);
    case 'inspectPackageSource':
      return service.inspectPackageSource(
        entry.request as NativeViewerPackageSourceRequest,
        signal,
      );
    case 'listResources':
      return service.listResources(
        entry.request as NativeViewerResourceCatalogRequest,
        signal,
      );
    default:
      throw new Error(`unsupported viewer worker method: ${entry.method}`);
  }
  return Uint8Array.from(response as Uint8Array);
}

function postSchedulerResponse(message: ViewerSchedulerResponse): void {
  if (message.response instanceof Uint8Array
    && message.response.buffer instanceof ArrayBuffer) {
    port.postMessage(message, [message.response.buffer]);
  } else {
    port.postMessage(message);
  }
}

const scheduler = new ViewerRequestScheduler(executeViewerRequest, postSchedulerResponse);
port.postMessage({ type: 'ready' });
port.on('message', (message: unknown) => {
  const type = field(message, 'type');
  if (type === 'cancel') {
    const id = field(message, 'id');
    if (typeof id === 'number') scheduler.cancel(id);
    return;
  }
  if (type === 'invalidate') {
    const sessionKey = field(message, 'sessionKey');
    scheduler.invalidateSession(typeof sessionKey === 'string' ? sessionKey : undefined);
    service.invalidate(typeof sessionKey === 'string' ? sessionKey : undefined);
    return;
  }
  if (isViewerWorkerRequest(message)) scheduler.enqueue(message);
});
port.on('close', () => scheduler.dispose());
