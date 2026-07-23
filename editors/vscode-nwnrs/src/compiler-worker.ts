import { createRequire } from 'node:module';
import { parentPort, workerData, type MessagePort } from 'node:worker_threads';

type RequestClass = 'interactive' | 'background';

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

interface QueuedRequest {
  readonly id: number;
  readonly method: string;
  readonly request: unknown;
  readonly sessionKey: string;
  readonly className: RequestClass;
  cancelled: boolean;
  preempted: boolean;
  controller?: AbortController;
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
  const queues: Record<RequestClass, QueuedRequest[]> = {
    interactive: [],
    background: [],
  };
  const active = new Map<number, QueuedRequest>();
  const limits: Record<RequestClass, number> = { interactive: 2, background: 1 };
  const running: Record<RequestClass, number> = { interactive: 0, background: 0 };

  function requestClass(method: string): RequestClass {
    return method === 'checkNss' || method === 'indexProject' ? 'background' : 'interactive';
  }

  function complete(entry: QueuedRequest, response?: unknown, error?: unknown): void {
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
      port.postMessage({
        type: 'response',
        id: entry.id,
        response,
        error: error instanceof Error ? error.message : error ? String(error) : undefined,
      });
    }
    pump();
  }

  function start(entry: QueuedRequest): void {
    entry.controller = new AbortController();
    active.set(entry.id, entry);
    running[entry.className] += 1;
    Promise.resolve(service.execute(
      entry.method,
      JSON.stringify(entry.request),
      entry.sessionKey,
      entry.controller.signal,
    )).then(
      (response: string) => complete(entry, JSON.parse(response)),
      (error: unknown) => complete(entry, undefined, error),
    );
  }

  function pump(): void {
    for (const className of ['interactive', 'background'] as const) {
      while (running[className] < limits[className] && queues[className].length > 0) {
        const entry = queues[className].shift();
        if (entry && !entry.cancelled) start(entry);
      }
    }
  }

  function cancel(id: number): void {
    const runningEntry = active.get(id);
    if (runningEntry) {
      runningEntry.cancelled = true;
      runningEntry.controller?.abort();
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

  function preemptProjectIndexing(): void {
    for (const entry of active.values()) {
      if (entry.method === 'indexProject' && !entry.cancelled && !entry.preempted) {
        entry.preempted = true;
        entry.controller?.abort();
      }
    }
  }

  port.postMessage({ type: 'ready' });

  port.on('message', (message: unknown) => {
    const type = messageType(message);
    if (type === 'cancel') {
      const id = messageNumber(message, 'id');
      if (id !== undefined) cancel(id);
      return;
    }
    if (type === 'invalidate') {
      service.invalidate(
        messageString(message, 'sessionKey'),
        messageString(message, 'changedPath'),
      );
      return;
    }
    if (type === 'release') {
      const sessionKey = messageString(message, 'sessionKey');
      if (sessionKey !== undefined) service.release(sessionKey);
      return;
    }
    if (!isRequestMessage(message)) return;
    const { id, method, request, sessionKey = '' } = message;
    const className = requestClass(method);
    if (className === 'interactive') preemptProjectIndexing();
    queues[className].push({
      id,
      method,
      request,
      sessionKey,
      className,
      cancelled: false,
      preempted: false,
    });
    pump();
  });
}

if (data.persistent) runPersistent();
else runOnce();
