import { isResourceEditorMutation } from './resource-editor-methods';

export interface ResourceEditorScheduledRequest {
  readonly id: number;
  readonly method: string;
  readonly request: unknown;
}

export interface ResourceEditorSchedulerResponse {
  readonly type: 'response';
  readonly id: number;
  readonly response?: unknown;
  readonly error?: string;
}

export interface ResourceEditorSchedulerOptions {
  readonly maxQueued?: number;
  readonly maxRunningDocuments?: number;
}

interface QueueEntry extends ResourceEditorScheduledRequest {
  readonly documentId: string;
  readonly cancellable: boolean;
  controller?: AbortController;
  suppressResponse: boolean;
}

type ResourceEditorExecutor = (
  request: ResourceEditorScheduledRequest,
  signal: AbortSignal | undefined,
) => Promise<unknown>;

type ResourceEditorResponder = (response: ResourceEditorSchedulerResponse) => void;

function documentId(request: ResourceEditorScheduledRequest): string {
  if (typeof request.request === 'object'
    && request.request !== null
    && 'documentId' in request.request
    && typeof request.request.documentId === 'string'
    && request.request.documentId.length > 0) {
    return request.request.documentId;
  }
  return `request:${request.id}`;
}

/**
 * Bounded per-document scheduler for the persistent resource editor.
 *
 * Requests for one custom document retain strict arrival order. Independent
 * documents run concurrently, so a large archive operation cannot stall every
 * open editor. Read-only work is cancellable; mutations become definite once
 * accepted and therefore cannot leave document state ambiguous.
 */
export class ResourceEditorRequestScheduler {
  private readonly queues = new Map<string, QueueEntry[]>();
  private readonly queuedById = new Map<number, QueueEntry>();
  private readonly activeById = new Map<number, QueueEntry>();
  private readonly activeDocuments = new Set<string>();
  private readonly readyDocuments: string[] = [];
  private readonly readySet = new Set<string>();
  private readonly maxQueued: number;
  private readonly maxRunningDocuments: number;
  private queued = 0;
  private disposed = false;

  public constructor(
    private readonly execute: ResourceEditorExecutor,
    private readonly respond: ResourceEditorResponder,
    options: ResourceEditorSchedulerOptions = {},
  ) {
    this.maxQueued = options.maxQueued ?? 512;
    this.maxRunningDocuments = options.maxRunningDocuments ?? 4;
    if (!Number.isSafeInteger(this.maxQueued) || this.maxQueued < 1) {
      throw new RangeError('resource editor scheduler maxQueued must be a positive safe integer');
    }
    if (!Number.isSafeInteger(this.maxRunningDocuments)
      || this.maxRunningDocuments < 1) {
      throw new RangeError(
        'resource editor scheduler maxRunningDocuments must be a positive safe integer',
      );
    }
  }

  public enqueue(request: ResourceEditorScheduledRequest): void {
    if (this.disposed) {
      this.respondError(request.id, 'resource editor scheduler is disposed');
      return;
    }
    if (this.queuedById.has(request.id) || this.activeById.has(request.id)) {
      this.respondError(request.id, `duplicate resource editor request id ${request.id}`);
      return;
    }
    if (this.queued >= this.maxQueued) {
      this.respondError(
        request.id,
        `resource editor queue is full (${this.maxQueued} requests)`,
      );
      return;
    }
    const owner = documentId(request);
    const entry: QueueEntry = {
      ...request,
      documentId: owner,
      cancellable: !isResourceEditorMutation(request.method),
      suppressResponse: false,
    };
    const queue = this.queues.get(owner) ?? [];
    queue.push(entry);
    this.queues.set(owner, queue);
    this.queuedById.set(entry.id, entry);
    this.queued += 1;
    this.markReady(owner);
    this.pump();
  }

  public cancel(id: number): void {
    const active = this.activeById.get(id);
    if (active) {
      if (!active.cancellable) return;
      active.suppressResponse = true;
      active.controller?.abort();
      return;
    }
    const queued = this.queuedById.get(id);
    if (!queued || !queued.cancellable) return;
    const queue = this.queues.get(queued.documentId);
    const index = queue?.indexOf(queued) ?? -1;
    if (queue && index >= 0) {
      queue.splice(index, 1);
      this.queued -= 1;
      if (queue.length === 0 && !this.activeDocuments.has(queued.documentId)) {
        this.queues.delete(queued.documentId);
        this.readySet.delete(queued.documentId);
      }
    }
    this.queuedById.delete(id);
    this.pump();
  }

  public dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    for (const queue of this.queues.values()) {
      for (const entry of queue) {
        if (!entry.cancellable) continue;
        entry.suppressResponse = true;
      }
    }
    for (const entry of this.activeById.values()) {
      if (!entry.cancellable) continue;
      entry.suppressResponse = true;
      entry.controller?.abort();
    }
  }

  public get queuedCount(): number {
    return this.queued;
  }

  public get runningCount(): number {
    return this.activeById.size;
  }

  private markReady(owner: string): void {
    if (this.activeDocuments.has(owner) || this.readySet.has(owner)) return;
    this.readySet.add(owner);
    this.readyDocuments.push(owner);
  }

  private pump(): void {
    while (!this.disposed
      && this.activeDocuments.size < this.maxRunningDocuments
      && this.readyDocuments.length > 0) {
      const owner = this.readyDocuments.shift();
      if (!owner) continue;
      this.readySet.delete(owner);
      if (this.activeDocuments.has(owner)) continue;
      const queue = this.queues.get(owner);
      const entry = queue?.shift();
      if (!entry) {
        this.queues.delete(owner);
        continue;
      }
      this.queuedById.delete(entry.id);
      this.queued -= 1;
      this.start(entry);
    }
  }

  private start(entry: QueueEntry): void {
    if (entry.cancellable) entry.controller = new AbortController();
    this.activeById.set(entry.id, entry);
    this.activeDocuments.add(entry.documentId);
    Promise.resolve(this.execute(entry, entry.controller?.signal)).then(
      (response) => this.complete(entry, response),
      (error: unknown) => this.complete(entry, undefined, error),
    );
  }

  private complete(entry: QueueEntry, response?: unknown, error?: unknown): void {
    if (!this.activeById.delete(entry.id)) return;
    this.activeDocuments.delete(entry.documentId);
    if (!entry.suppressResponse) {
      this.respond({
        type: 'response',
        id: entry.id,
        response,
        error: error instanceof Error ? error.message : error ? String(error) : undefined,
      });
    }
    const queue = this.queues.get(entry.documentId);
    if (queue?.length) this.markReady(entry.documentId);
    else this.queues.delete(entry.documentId);
    this.pump();
  }

  private respondError(id: number, error: string): void {
    this.respond({ type: 'response', id, error });
  }
}
