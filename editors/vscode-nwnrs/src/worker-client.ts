import { Worker, type Transferable } from 'node:worker_threads';

export interface Disposable {
  dispose(): void;
}

export interface CancellationTokenLike {
  readonly isCancellationRequested: boolean;
  onCancellationRequested(listener: () => void): Disposable;
}

export interface OutputChannelLike {
  appendLine(value: string): void;
}

export interface WorkerRequestOptions {
  readonly sessionKey?: string;
  readonly contents?: Uint8Array;
  readonly transferList?: readonly Transferable[];
}

interface PendingRequest<T = unknown> {
  readonly resolve: (value: T) => void;
  readonly reject: (reason: Error) => void;
  readonly cancellation?: Disposable;
  readonly method: string;
}

interface ReadyMessage {
  readonly type: 'ready';
}

interface ResponseMessage {
  readonly type: 'response';
  readonly id: number;
  readonly response?: unknown;
  readonly error?: string;
}

type IncomingWorkerMessage = ReadyMessage | ResponseMessage;

function isIncomingWorkerMessage(value: unknown): value is IncomingWorkerMessage {
  if (typeof value !== 'object' || value === null || !('type' in value)) return false;
  const type = (value as { type?: unknown }).type;
  if (type === 'ready') return true;
  return type === 'response'
    && 'id' in value
    && Number.isInteger((value as { id?: unknown }).id);
}

/** A crash-resilient request client for a persistent Node worker. */
export class WorkerClient {
  private worker: Worker | undefined;
  private nextRequestId = 1;
  private readonly pending = new Map<number, PendingRequest>();
  private disposed = false;
  private restartAttempts = 0;
  private restartTimer: NodeJS.Timeout | undefined;
  private healthyTimer: NodeJS.Timeout | undefined;

  public constructor(
    private readonly workerPath: string,
    private readonly workerData: Readonly<Record<string, unknown>>,
    private readonly output: OutputChannelLike | undefined,
    private readonly label: string,
  ) {
    this.start();
  }

  private start(): void {
    if (this.disposed) return;
    const worker = new Worker(this.workerPath, { workerData: this.workerData });
    this.worker = worker;
    worker.on('message', (message: unknown) => this.handleMessage(message));
    worker.on('error', (error: Error) => this.handleFailure(error));
    worker.on('exit', (code: number) => {
      if (!this.disposed && this.worker === worker) {
        this.handleFailure(new Error(`${this.label} worker exited with code ${code}`));
      }
    });
  }

  public request<TResponse = unknown>(
    method: string,
    request: unknown,
    cancellationToken?: CancellationTokenLike,
    extra: WorkerRequestOptions = {},
  ): Promise<TResponse> {
    if (this.disposed) return Promise.reject(new Error(`${this.label} worker is disposed`));
    if (cancellationToken?.isCancellationRequested) {
      return Promise.reject(new Error(`${method} cancelled`));
    }
    if (!this.worker) return Promise.reject(new Error(`${this.label} worker is restarting`));
    const id = this.nextRequestId++;
    return new Promise<TResponse>((resolve, reject) => {
      const cancellation = cancellationToken?.onCancellationRequested(() => {
        const pending = this.pending.get(id);
        if (!pending) return;
        this.pending.delete(id);
        pending.cancellation?.dispose();
        this.worker?.postMessage({ type: 'cancel', id });
        reject(new Error(`${method} cancelled`));
      });
      const pending: PendingRequest<TResponse> = { resolve, reject, cancellation, method };
      this.pending.set(id, pending as PendingRequest);
      const { transferList = [], ...messageExtra } = extra;
      this.worker?.postMessage(
        { type: 'request', id, method, request, ...messageExtra },
        [...transferList],
      );
    });
  }

  protected post(message: unknown): void {
    this.worker?.postMessage(message);
  }

  public async restart(): Promise<void> {
    if (this.disposed) throw new Error(`${this.label} worker is disposed`);
    const previousWorker = this.worker;
    this.worker = undefined;
    clearTimeout(this.restartTimer);
    clearTimeout(this.healthyTimer);
    this.restartAttempts = 0;
    this.rejectPending(new Error(`${this.label} worker restarted`));
    if (previousWorker) await previousWorker.terminate();
    if (this.disposed) return;
    this.start();
    await this.waitUntilReady(this.worker);
  }

  public get isRunning(): boolean {
    return this.worker !== undefined && !this.disposed;
  }

  private waitUntilReady(worker: Worker | undefined): Promise<void> {
    if (!worker) return Promise.reject(new Error(`${this.label} worker did not start`));
    return new Promise<void>((resolve, reject) => {
      const timeout = setTimeout(
        () => finish(new Error(`${this.label} worker restart timed out`)),
        10_000,
      );
      const onMessage = (message: unknown): void => {
        if (isIncomingWorkerMessage(message) && message.type === 'ready') finish();
      };
      const onError = (error: Error): void => finish(error);
      const onExit = (code: number): void => {
        finish(new Error(`${this.label} worker exited with code ${code}`));
      };
      const finish = (error?: Error): void => {
        clearTimeout(timeout);
        worker.off('message', onMessage);
        worker.off('error', onError);
        worker.off('exit', onExit);
        if (error) reject(error);
        else resolve();
      };
      worker.on('message', onMessage);
      worker.once('error', onError);
      worker.once('exit', onExit);
    });
  }

  private handleMessage(message: unknown): void {
    if (!isIncomingWorkerMessage(message)) return;
    if (message.type === 'ready') {
      clearTimeout(this.healthyTimer);
      this.healthyTimer = setTimeout(() => {
        this.healthyTimer = undefined;
        this.restartAttempts = 0;
      }, 30_000);
      return;
    }
    const pending = this.pending.get(message.id);
    if (!pending) return;
    this.pending.delete(message.id);
    pending.cancellation?.dispose();
    if (message.error) pending.reject(new Error(message.error));
    else pending.resolve(message.response);
  }

  private rejectPending(error: Error): void {
    for (const pending of this.pending.values()) {
      pending.cancellation?.dispose();
      pending.reject(error);
    }
    this.pending.clear();
  }

  private handleFailure(error: Error): void {
    if (this.disposed) return;
    const failedWorker = this.worker;
    this.worker = undefined;
    clearTimeout(this.healthyTimer);
    this.healthyTimer = undefined;
    this.rejectPending(error);
    this.restartAttempts += 1;
    const delay = Math.min(5_000, 100 * (2 ** Math.min(this.restartAttempts - 1, 6)));
    this.output?.appendLine(
      `nwnrs ${this.label} worker failed; restarting in ${delay}ms: ${String(error)}`,
    );
    void failedWorker?.terminate();
    clearTimeout(this.restartTimer);
    this.restartTimer = setTimeout(() => {
      this.restartTimer = undefined;
      this.start();
    }, delay);
  }

  public dispose(): void {
    this.disposed = true;
    this.rejectPending(new Error(`${this.label} worker disposed`));
    clearTimeout(this.restartTimer);
    clearTimeout(this.healthyTimer);
    void this.worker?.terminate();
    this.worker = undefined;
  }
}
