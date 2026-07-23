type RequestClass = 'interactive' | 'background';

export interface LanguageScheduledRequest {
  readonly id: number;
  readonly method: string;
  readonly request: unknown;
  readonly sessionKey: string;
}

export interface LanguageSchedulerResponse {
  readonly type: 'response';
  readonly id: number;
  readonly response?: unknown;
  readonly error?: string;
}

export interface LanguageSchedulerOptions {
  readonly interactiveConcurrency?: number;
  readonly backgroundConcurrency?: number;
  readonly interactiveQueueLimit?: number;
  readonly backgroundQueueLimit?: number;
  readonly maxIndexPreemptions?: number;
}

interface QueueEntry extends LanguageScheduledRequest {
  readonly className: RequestClass;
  readonly generation: number;
  controller?: AbortController;
  cancelled: boolean;
  preempted: boolean;
  preemptions: number;
}

type LanguageExecutor = (
  request: LanguageScheduledRequest,
  signal: AbortSignal,
) => Promise<unknown>;

type LanguageResponder = (response: LanguageSchedulerResponse) => void;

function positiveInteger(value: number, label: string): number {
  if (!Number.isSafeInteger(value) || value < 1) {
    throw new RangeError(`${label} must be a positive safe integer`);
  }
  return value;
}

function requestClass(method: string): RequestClass {
  return method === 'checkNss' || method === 'indexProject' ? 'background' : 'interactive';
}

function requestSourcePath(request: unknown): string | undefined {
  return typeof request === 'object'
    && request !== null
    && 'source_path' in request
    && typeof request.source_path === 'string'
    ? request.source_path
    : undefined;
}

/**
 * Bounded, generation-safe scheduler for the persistent language service.
 *
 * Package indexes are serialized per `nwpkg.toml` session while independent
 * packages run concurrently. Interactive work gets a dedicated lane and can
 * cooperatively preempt long project indexing without starving it forever.
 * Invalidated generations never publish stale analysis.
 */
export class LanguageRequestScheduler {
  private readonly queues: Record<RequestClass, QueueEntry[]> = {
    interactive: [],
    background: [],
  };
  private readonly active = new Map<number, QueueEntry>();
  private readonly activeIndexedSessions = new Set<string>();
  private readonly running: Record<RequestClass, number> = {
    interactive: 0,
    background: 0,
  };
  private readonly limits: Record<RequestClass, number>;
  private readonly queueLimits: Record<RequestClass, number>;
  private readonly maxIndexPreemptions: number;
  private readonly generations = new Map<string, number>();
  private disposed = false;

  public constructor(
    private readonly execute: LanguageExecutor,
    private readonly respond: LanguageResponder,
    options: LanguageSchedulerOptions = {},
  ) {
    this.limits = {
      interactive: positiveInteger(
        options.interactiveConcurrency ?? 4,
        'interactiveConcurrency',
      ),
      background: positiveInteger(
        options.backgroundConcurrency ?? 2,
        'backgroundConcurrency',
      ),
    };
    this.queueLimits = {
      interactive: positiveInteger(
        options.interactiveQueueLimit ?? 256,
        'interactiveQueueLimit',
      ),
      background: positiveInteger(
        options.backgroundQueueLimit ?? 64,
        'backgroundQueueLimit',
      ),
    };
    this.maxIndexPreemptions = positiveInteger(
      options.maxIndexPreemptions ?? 3,
      'maxIndexPreemptions',
    );
  }

  public enqueue(request: LanguageScheduledRequest): void {
    if (this.disposed) {
      this.respondError(request.id, 'language scheduler is disposed');
      return;
    }
    if (this.active.has(request.id)
      || Object.values(this.queues).some((queue) =>
        queue.some((entry) => entry.id === request.id))) {
      this.respondError(request.id, `duplicate language request id ${request.id}`);
      return;
    }
    const className = requestClass(request.method);
    if (className === 'interactive') this.preemptProjectIndexing(request.sessionKey);
    const entry: QueueEntry = {
      ...request,
      className,
      generation: this.generations.get(request.sessionKey) ?? 0,
      cancelled: false,
      preempted: false,
      preemptions: 0,
    };
    this.coalesceBackground(entry);
    if (this.queues[className].length >= this.queueLimits[className]) {
      this.respondError(
        request.id,
        `${className} language queue is full (${this.queueLimits[className]} requests)`,
      );
      return;
    }
    this.queues[className].push(entry);
    this.pump();
  }

  public cancel(id: number): void {
    const running = this.active.get(id);
    if (running) {
      running.cancelled = true;
      running.controller?.abort();
      return;
    }
    for (const queue of Object.values(this.queues)) {
      const index = queue.findIndex((entry) => entry.id === id);
      if (index >= 0) {
        queue.splice(index, 1);
        return;
      }
    }
  }

  public invalidate(sessionKey: string | undefined, reason = 'invalidated'): void {
    const affected = (entry: QueueEntry): boolean =>
      sessionKey === undefined || entry.sessionKey === sessionKey;
    const keys = new Set<string>();
    if (sessionKey === undefined) {
      for (const key of this.generations.keys()) keys.add(key);
      for (const entry of this.active.values()) keys.add(entry.sessionKey);
      for (const queue of Object.values(this.queues)) {
        for (const entry of queue) keys.add(entry.sessionKey);
      }
    } else {
      keys.add(sessionKey);
    }
    for (const key of keys) {
      this.generations.set(key, (this.generations.get(key) ?? 0) + 1);
    }
    for (const queue of Object.values(this.queues)) {
      for (let index = queue.length - 1; index >= 0; index -= 1) {
        const entry = queue[index];
        if (!entry || !affected(entry)) continue;
        queue.splice(index, 1);
        this.reject(entry, `${entry.method} ${reason}`);
      }
    }
    for (const entry of this.active.values()) {
      if (affected(entry)) entry.controller?.abort();
    }
  }

  public dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.invalidate(undefined, 'disposed');
  }

  public get queuedCount(): number {
    return this.queues.interactive.length + this.queues.background.length;
  }

  public get runningCount(): number {
    return this.active.size;
  }

  private pump(): void {
    if (this.disposed) return;
    for (const className of ['interactive', 'background'] as const) {
      while (this.running[className] < this.limits[className]
        && this.queues[className].length > 0) {
        const index = this.queues[className].findIndex((candidate) =>
          candidate.method === 'checkNss'
          || !this.activeIndexedSessions.has(candidate.sessionKey));
        if (index < 0) break;
        const [entry] = this.queues[className].splice(index, 1);
        if (entry && !entry.cancelled) this.start(entry);
      }
    }
  }

  private start(entry: QueueEntry): void {
    entry.controller = new AbortController();
    this.active.set(entry.id, entry);
    if (entry.method !== 'checkNss') {
      this.activeIndexedSessions.add(entry.sessionKey);
    }
    this.running[entry.className] += 1;
    Promise.resolve(this.execute(entry, entry.controller.signal)).then(
      (response) => this.complete(entry, response),
      (error: unknown) => this.complete(entry, undefined, error),
    );
  }

  private complete(entry: QueueEntry, response?: unknown, error?: unknown): void {
    if (!this.active.delete(entry.id)) return;
    if (entry.method !== 'checkNss') {
      this.activeIndexedSessions.delete(entry.sessionKey);
    }
    this.running[entry.className] -= 1;
    const stale = entry.generation !== (this.generations.get(entry.sessionKey) ?? 0);
    if (entry.preempted && !entry.cancelled) {
      entry.preempted = false;
      entry.controller = undefined;
      if (stale) {
        this.respondError(entry.id, `${entry.method} invalidated before completion`);
      } else if (error === undefined) {
        // The native task can win a race with AbortSignal delivery. Preserve
        // that completed index instead of throwing it away and indexing twice.
        this.respond({
          type: 'response',
          id: entry.id,
          response,
        });
      } else {
        entry.preemptions += 1;
        this.queues[entry.className].push(entry);
      }
      this.pump();
      return;
    }
    if (!entry.cancelled) {
      this.respond({
        type: 'response',
        id: entry.id,
        response: stale ? undefined : response,
        error: stale
          ? `${entry.method} invalidated before completion`
          : error instanceof Error ? error.message : error ? String(error) : undefined,
      });
    }
    this.pump();
  }

  private preemptProjectIndexing(sessionKey: string): void {
    for (const entry of this.active.values()) {
      if (entry.method === 'indexProject'
        && entry.sessionKey === sessionKey
        && entry.preemptions < this.maxIndexPreemptions
        && !entry.cancelled
        && !entry.preempted) {
        entry.preempted = true;
        entry.controller?.abort();
      }
    }
  }

  private coalesceBackground(entry: QueueEntry): void {
    if (entry.className !== 'background') return;
    const sourcePath = requestSourcePath(entry.request);
    const queue = this.queues.background;
    for (let index = queue.length - 1; index >= 0; index -= 1) {
      const candidate = queue[index];
      if (!candidate
        || candidate.sessionKey !== entry.sessionKey
        || candidate.method !== entry.method
        || (entry.method === 'checkNss'
          && requestSourcePath(candidate.request) !== sourcePath)) {
        continue;
      }
      queue.splice(index, 1);
      this.reject(candidate, `${candidate.method} superseded by a newer request`);
    }
  }

  private reject(entry: QueueEntry, reason: string): void {
    entry.cancelled = true;
    this.respondError(entry.id, reason);
  }

  private respondError(id: number, error: string): void {
    this.respond({ type: 'response', id, error });
  }
}
