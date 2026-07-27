'use strict';

const assert = require('node:assert/strict');
const test = require('node:test');
const {
  ResourceEditorRequestScheduler,
} = require('../dist/src/resource-editor-scheduler');

interface Deferred {
  readonly promise: Promise<unknown>;
  resolve(value?: unknown): void;
  reject(error: unknown): void;
}

function deferred(): Deferred {
  let resolve!: (value?: unknown) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<unknown>((accept, decline) => {
    resolve = accept;
    reject = decline;
  });
  return { promise, resolve, reject };
}

function turn(): Promise<void> {
  return new Promise((resolve) => setImmediate(resolve));
}

test('resource scheduler preserves document order while running independent documents', async () => {
  const work = new Map<number, Deferred>([
    [1, deferred()],
    [2, deferred()],
    [3, deferred()],
  ]);
  const calls: number[] = [];
  const responses: number[] = [];
  const scheduler = new ResourceEditorRequestScheduler(
    (request: { readonly id: number }) => {
      calls.push(request.id);
      const pending = work.get(request.id);
      if (!pending) throw new Error(`missing test work for ${request.id}`);
      return pending.promise;
    },
    (response: { readonly id: number }) => responses.push(response.id),
    { maxRunningDocuments: 2 },
  );

  scheduler.enqueue({ id: 1, method: 'snapshot', request: { documentId: 'first' } });
  scheduler.enqueue({ id: 2, method: 'gffNode', request: { documentId: 'first' } });
  scheduler.enqueue({ id: 3, method: 'snapshot', request: { documentId: 'second' } });
  assert.deepEqual(calls, [1, 3]);

  work.get(3)?.resolve('second');
  await turn();
  assert.deepEqual(calls, [1, 3]);
  work.get(1)?.resolve('first');
  await turn();
  assert.deepEqual(calls, [1, 3, 2]);
  work.get(2)?.resolve('next');
  await turn();
  assert.deepEqual(responses.sort(), [1, 2, 3]);
});

test('resource scheduler cancels reads but never drops accepted mutations', async () => {
  const activeRead = deferred();
  const mutation = deferred();
  const calls: number[] = [];
  const responses: Array<{ readonly id: number; readonly error?: string }> = [];
  const scheduler = new ResourceEditorRequestScheduler(
    (request: { readonly id: number }, signal: AbortSignal | undefined) => {
      calls.push(request.id);
      if (request.id === 1) {
        signal?.addEventListener('abort', () => {
          activeRead.reject(new Error('read cancelled'));
        }, { once: true });
        return activeRead.promise;
      }
      return mutation.promise;
    },
    (response: { readonly id: number; readonly error?: string }) => responses.push(response),
    { maxRunningDocuments: 1 },
  );

  scheduler.enqueue({ id: 1, method: 'snapshot', request: { documentId: 'demo' } });
  scheduler.enqueue({ id: 2, method: 'applyEdit', request: { documentId: 'demo' } });
  scheduler.cancel(2);
  scheduler.cancel(1);
  await turn();
  assert.deepEqual(calls, [1, 2], 'the queued mutation must still execute');
  mutation.resolve('edited');
  await turn();
  assert.deepEqual(responses.map(({ id }) => id), [2]);
});

test('resource scheduler reports its queue bound instead of silently dropping work', () => {
  const active = deferred();
  const responses: Array<{ readonly id: number; readonly error?: string }> = [];
  const scheduler = new ResourceEditorRequestScheduler(
    () => active.promise,
    (response: { readonly id: number; readonly error?: string }) => responses.push(response),
    { maxQueued: 1, maxRunningDocuments: 1 },
  );

  scheduler.enqueue({ id: 1, method: 'snapshot', request: { documentId: 'first' } });
  scheduler.enqueue({ id: 2, method: 'snapshot', request: { documentId: 'second' } });
  scheduler.enqueue({ id: 3, method: 'snapshot', request: { documentId: 'third' } });
  assert.equal(scheduler.runningCount, 1);
  assert.equal(scheduler.queuedCount, 1);
  assert.equal(responses[0]?.id, 3);
  assert.match(responses[0]?.error || '', /queue is full/u);
  scheduler.cancel(1);
  scheduler.cancel(2);
});
