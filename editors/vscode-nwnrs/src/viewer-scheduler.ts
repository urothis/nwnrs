type ViewerRequestClass = 'interactive' | 'catalog' | 'scene';

export interface ViewerScheduledRequest {
  readonly id: number;
  readonly method: string;
  readonly request: unknown;
  readonly contents?: Uint8Array;
}

interface ViewerQueueEntry extends ViewerScheduledRequest {
  readonly className: ViewerRequestClass;
  readonly sessionKey: string;
  readonly concurrencyKey: string;
  controller?: AbortController;
  suppressResponse: boolean;
  responseSent: boolean;
}

export interface ViewerSchedulerResponse {
  readonly type: 'response';
  readonly id: number;
  readonly response?: unknown;
  readonly error?: string;
}

export interface ViewerSchedulerOptions {
  readonly maxQueued?: number;
  readonly maxRunning?: number;
}

type ViewerExecutor = (
  request: ViewerScheduledRequest,
  signal: AbortSignal,
) => Promise<unknown>;

type ViewerResponder = (response: ViewerSchedulerResponse) => void;

const SCHEDULE: readonly ViewerRequestClass[] = [
  'interactive',
  'interactive',
  'interactive',
  'catalog',
  'catalog',
  'scene',
];

const CLASS_LIMITS: Readonly<Record<ViewerRequestClass, number>> = {
  interactive: 3,
  catalog: 2,
  scene: 2,
};

function requestClass(method: string): ViewerRequestClass {
  if (method === 'loadScene' || method === 'loadSceneBytes') return 'scene';
  if (
    method === 'inspectPackage'
    || method === 'inspectPackageSource'
    || method === 'listResources'
  ) {
    return 'catalog';
  }
  return 'interactive';
}

function recordString(value: unknown, ...keys: readonly string[]): string | undefined {
  if (typeof value !== 'object' || value === null) return undefined;
  const record = value as Readonly<Record<string, unknown>>;
  for (const key of keys) {
    const candidate = record[key];
    if (typeof candidate === 'string' && candidate.length > 0) return candidate;
  }
  return undefined;
}

function sessionKey(request: ViewerScheduledRequest): string {
  return recordString(
    request.request,
    'session_key',
    'sessionKey',
    'manifestPath',
    'project_root',
    'projectRoot',
    'path',
  ) ?? `request:${request.id}`;
}

function concurrencyKey(
  request: ViewerScheduledRequest,
  className: ViewerRequestClass,
  session: string,
): string {
  if (className === 'scene') return `scene:${session}`;
  if (className === 'catalog') return `catalog:${session}`;
  const assetKey = recordString(request.request, 'asset_key', 'assetKey');
  return assetKey ? `asset:${session}:${assetKey}` : `resource:${session}`;
}

/**
 * Bounded, cancellation-aware scheduler for the persistent viewer worker.
 *
 * The scheduler gives interactive work more turns without starving catalog or
 * scene work. Requests that mutate or consume the same session lane retain
 * arrival order, while independent lanes and packages can run concurrently.
 */
export class ViewerRequestScheduler {
  private readonly queues: Record<ViewerRequestClass, ViewerQueueEntry[]> = {
    interactive: [],
    catalog: [],
    scene: [],
  };
  private readonly active = new Map<number, ViewerQueueEntry>();
  private readonly activeConcurrencyKeys = new Set<string>();
  private readonly running: Record<ViewerRequestClass, number> = {
    interactive: 0,
    catalog: 0,
    scene: 0,
  };
  private readonly maxQueued: number;
  private readonly maxRunning: number;
  private scheduleIndex = 0;
  private disposed = false;

  public constructor(
    private readonly execute: ViewerExecutor,
    private readonly respond: ViewerResponder,
    options: ViewerSchedulerOptions = {},
  ) {
    this.maxQueued = options.maxQueued ?? 256;
    this.maxRunning = options.maxRunning ?? 4;
    if (!Number.isSafeInteger(this.maxQueued) || this.maxQueued < 1) {
      throw new RangeError('viewer scheduler maxQueued must be a positive safe integer');
    }
    if (!Number.isSafeInteger(this.maxRunning) || this.maxRunning < 1) {
      throw new RangeError('viewer scheduler maxRunning must be a positive safe integer');
    }
  }

  public enqueue(request: ViewerScheduledRequest): void {
    if (this.disposed) {
      this.respondError(request.id, 'viewer scheduler is disposed');
      return;
    }
    if (this.queuedCount >= this.maxQueued) {
      this.respondError(
        request.id,
        `viewer request queue reached its ${this.maxQueued}-request safety limit`,
      );
      return;
    }
    const className = requestClass(request.method);
    const session = sessionKey(request);
    this.queues[className].push({
      ...request,
      className,
      sessionKey: session,
      concurrencyKey: concurrencyKey(request, className, session),
      suppressResponse: false,
      responseSent: false,
    });
    this.pump();
  }

  public cancel(id: number): void {
    const active = this.active.get(id);
    if (active) {
      active.suppressResponse = true;
      active.controller?.abort();
      return;
    }
    for (const queue of Object.values(this.queues)) {
      const index = queue.findIndex((entry) => entry.id === id);
      if (index >= 0) {
        queue.splice(index, 1);
        this.pump();
        return;
      }
    }
  }

  public invalidateSession(session: string | undefined): void {
    const reason = session
      ? `viewer session ${session} was invalidated`
      : 'all viewer sessions were invalidated';
    for (const queue of Object.values(this.queues)) {
      for (let index = queue.length - 1; index >= 0; index -= 1) {
        const entry = queue[index];
        if (!entry || (session && entry.sessionKey !== session)) continue;
        queue.splice(index, 1);
        this.respondEntryError(entry, reason);
      }
    }
    for (const entry of this.active.values()) {
      if (session && entry.sessionKey !== session) continue;
      this.respondEntryError(entry, reason);
      entry.controller?.abort();
    }
    this.pump();
  }

  public dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    for (const queue of Object.values(this.queues)) {
      for (const entry of queue.splice(0)) {
        this.respondEntryError(entry, 'viewer scheduler was disposed');
      }
    }
    for (const entry of this.active.values()) {
      this.respondEntryError(entry, 'viewer scheduler was disposed');
      entry.controller?.abort();
    }
  }

  public get queuedCount(): number {
    return Object.values(this.queues).reduce((total, queue) => total + queue.length, 0);
  }

  public get runningCount(): number {
    return this.active.size;
  }

  private pump(): void {
    while (!this.disposed && this.active.size < this.maxRunning) {
      const entry = this.nextRunnable();
      if (!entry) return;
      this.start(entry);
    }
  }

  private nextRunnable(): ViewerQueueEntry | undefined {
    for (let offset = 0; offset < SCHEDULE.length; offset += 1) {
      const scheduleIndex = (this.scheduleIndex + offset) % SCHEDULE.length;
      const className = SCHEDULE[scheduleIndex];
      if (!className || this.running[className] >= CLASS_LIMITS[className]) continue;
      const queue = this.queues[className];
      const index = queue.findIndex(
        (entry) => !this.activeConcurrencyKeys.has(entry.concurrencyKey),
      );
      if (index < 0) continue;
      const [entry] = queue.splice(index, 1);
      if (!entry) continue;
      this.scheduleIndex = (scheduleIndex + 1) % SCHEDULE.length;
      return entry;
    }
    return undefined;
  }

  private start(entry: ViewerQueueEntry): void {
    const controller = new AbortController();
    entry.controller = controller;
    this.active.set(entry.id, entry);
    this.activeConcurrencyKeys.add(entry.concurrencyKey);
    this.running[entry.className] += 1;
    Promise.resolve(this.execute(entry, controller.signal)).then(
      (response) => this.complete(entry, response),
      (error: unknown) => this.complete(entry, undefined, error),
    );
  }

  private complete(entry: ViewerQueueEntry, response?: unknown, error?: unknown): void {
    if (!this.active.delete(entry.id)) return;
    this.activeConcurrencyKeys.delete(entry.concurrencyKey);
    this.running[entry.className] -= 1;
    if (!entry.suppressResponse && !entry.responseSent) {
      entry.responseSent = true;
      this.respond({
        type: 'response',
        id: entry.id,
        response,
        error: error instanceof Error ? error.message : error ? String(error) : undefined,
      });
    }
    this.pump();
  }

  private respondEntryError(entry: ViewerQueueEntry, error: string): void {
    if (entry.responseSent || entry.suppressResponse) return;
    entry.responseSent = true;
    this.respondError(entry.id, error);
  }

  private respondError(id: number, error: string): void {
    this.respond({ type: 'response', id, error });
  }
}
