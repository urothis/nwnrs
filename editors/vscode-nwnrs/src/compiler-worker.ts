import { createRequire } from 'node:module';
import { parentPort, workerData, type MessagePort } from 'node:worker_threads';
import { LanguageRequestScheduler } from './language-scheduler';

interface OneShotWorkerData {
  readonly bindingPath: string;
  readonly persistent?: false;
  readonly method?: string;
  readonly request?: unknown;
}

interface PersistentWorkerData {
  readonly bindingPath: string;
  readonly persistent: true;
}

interface LanguageService {
  execute(
    method: string,
    request: string,
    sessionKey: string,
    signal: AbortSignal,
  ): Promise<string>;
  invalidate(sessionKey?: string, changedPath?: string): void;
  release(sessionKey: string): void;
}

interface NativeCompilerBinding {
  readonly LanguageService?: new () => LanguageService;
  readonly [method: string]: unknown;
}

interface RequestMessage {
  readonly type: 'request';
  readonly id: number;
  readonly method: string;
  readonly request: unknown;
  readonly sessionKey?: string;
}

function requireParentPort(): MessagePort {
  if (!parentPort) throw new Error('compiler worker requires a parent message port');
  return parentPort;
}

const port = requireParentPort();
const data = workerData as OneShotWorkerData | PersistentWorkerData;
const loadNativeModule = createRequire(__filename);

function loadBinding(bindingPath: string): NativeCompilerBinding {
  return loadNativeModule(bindingPath) as NativeCompilerBinding;
}

function invoke(
  binding: NativeCompilerBinding,
  method: string,
  request: unknown,
): unknown {
  const implementation = binding[method];
  if (typeof implementation !== 'function') {
    throw new Error(`native compiler does not export ${method}`);
  }
  return JSON.parse(String(implementation(JSON.stringify(request))));
}

function runOnce(): void {
  try {
    const binding = loadBinding(data.bindingPath);
    const method = 'method' in data ? data.method ?? 'checkNss' : 'checkNss';
    const request = 'request' in data ? data.request : undefined;
    const response = invoke(binding, method, request);
    port.postMessage({ response });
  } catch (error) {
    port.postMessage({
      error: error instanceof Error ? error.message : String(error),
    });
  }
}

function isRequestMessage(value: unknown): value is RequestMessage {
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

function messageType(value: unknown): string | undefined {
  return typeof value === 'object'
    && value !== null
    && 'type' in value
    && typeof value.type === 'string'
    ? value.type
    : undefined;
}

function messageNumber(value: unknown, key: string): number | undefined {
  if (typeof value !== 'object' || value === null || !(key in value)) return undefined;
  const candidate = (value as Record<string, unknown>)[key];
  return typeof candidate === 'number' ? candidate : undefined;
}

function messageString(value: unknown, key: string): string | undefined {
  if (typeof value !== 'object' || value === null || !(key in value)) return undefined;
  const candidate = (value as Record<string, unknown>)[key];
  return typeof candidate === 'string' ? candidate : undefined;
}

function runPersistent(): void {
  const binding = loadBinding(data.bindingPath);
  if (typeof binding.LanguageService !== 'function') {
    throw new Error('native compiler does not export LanguageService');
  }
  const service = new binding.LanguageService();
  const scheduler = new LanguageRequestScheduler(
    async (request, signal) => {
      const response = await service.execute(
        request.method,
        JSON.stringify(request.request),
        request.sessionKey,
        signal,
      );
      return JSON.parse(response) as unknown;
    },
    (response) => port.postMessage(response),
  );

  port.postMessage({ type: 'ready' });

  port.on('message', (message: unknown) => {
    const type = messageType(message);
    if (type === 'cancel') {
      const id = messageNumber(message, 'id');
      if (id !== undefined) scheduler.cancel(id);
      return;
    }
    if (type === 'invalidate') {
      const sessionKey = messageString(message, 'sessionKey');
      scheduler.invalidate(sessionKey);
      service.invalidate(
        sessionKey,
        messageString(message, 'changedPath'),
      );
      return;
    }
    if (type === 'release') {
      const sessionKey = messageString(message, 'sessionKey');
      if (sessionKey !== undefined) {
        scheduler.invalidate(sessionKey, 'released');
        service.release(sessionKey);
      }
      return;
    }
    if (!isRequestMessage(message)) return;
    const { id, method, request, sessionKey = '' } = message;
    scheduler.enqueue({ id, method, request, sessionKey });
  });
}

if (data.persistent) runPersistent();
else runOnce();
