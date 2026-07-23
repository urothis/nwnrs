'use strict';

const assert = require('node:assert/strict');
const test = require('node:test');
const { LanguageRequestScheduler } = require('../dist/src/language-scheduler');

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

test('language scheduler serializes one package index while independent packages progress', async () => {
  const work = new Map<number, Deferred>([
    [1, deferred()],
    [2, deferred()],
    [3, deferred()],
  ]);
  const calls: number[] = [];
  const scheduler = new LanguageRequestScheduler(
    (request: { readonly id: number }) => {
      calls.push(request.id);
      const pending = work.get(request.id);
      if (!pending) throw new Error(`missing work for ${request.id}`);
      return pending.promise;
    },
    () => {},
    { backgroundConcurrency: 2 },
  );

  scheduler.enqueue({ id: 1, method: 'indexProject', request: {}, sessionKey: 'first' });
  scheduler.enqueue({ id: 2, method: 'indexProject', request: {}, sessionKey: 'first' });
  scheduler.enqueue({ id: 3, method: 'indexProject', request: {}, sessionKey: 'second' });
  assert.deepEqual(calls, [1, 3]);
  work.get(3)?.resolve('second');
  await turn();
  assert.deepEqual(calls, [1, 3]);
  work.get(1)?.resolve('first');
  await turn();
  assert.deepEqual(calls, [1, 3, 2]);
  work.get(2)?.resolve('next');
  await turn();
  assert.equal(scheduler.runningCount, 0);
});

test('language scheduler coalesces obsolete queued background work', async () => {
  const active = deferred();
  const latest = deferred();
  const calls: number[] = [];
  const responses: Array<{ readonly id: number; readonly error?: string }> = [];
  const scheduler = new LanguageRequestScheduler(
    (request: { readonly id: number }) => {
      calls.push(request.id);
      return request.id === 1 ? active.promise : latest.promise;
    },
    (response: { readonly id: number; readonly error?: string }) => responses.push(response),
    { backgroundConcurrency: 1 },
  );

  scheduler.enqueue({ id: 1, method: 'indexProject', request: {}, sessionKey: 'blocking' });
  scheduler.enqueue({
    id: 2,
    method: 'checkNss',
    request: { source_path: '/demo/script.nss' },
    sessionKey: 'demo',
  });
  scheduler.enqueue({
    id: 3,
    method: 'checkNss',
    request: { source_path: '/demo/script.nss' },
    sessionKey: 'demo',
  });
  assert.match(responses[0]?.error || '', /superseded/u);
  assert.equal(responses[0]?.id, 2);
  active.resolve('indexed');
  await turn();
  assert.deepEqual(calls, [1, 3]);
  latest.resolve('checked');
  await turn();
});

test('interactive language work safely preempts and later resumes project indexing', async () => {
  const firstIndex = deferred();
  const resumedIndex = deferred();
  const hover = deferred();
  const calls: string[] = [];
  const responses: Array<{ readonly id: number; readonly error?: string }> = [];
  let indexAttempt = 0;
  const scheduler = new LanguageRequestScheduler(
    (
      request: { readonly id: number; readonly method: string },
      signal: AbortSignal,
    ) => {
      calls.push(`${request.method}:${request.id}`);
      if (request.method === 'indexProject') {
        indexAttempt += 1;
        const work = indexAttempt === 1 ? firstIndex : resumedIndex;
        signal.addEventListener('abort', () => work.reject(new Error('preempted')), {
          once: true,
        });
        return work.promise;
      }
      return hover.promise;
    },
    (response: { readonly id: number; readonly error?: string }) => responses.push(response),
    { backgroundConcurrency: 1, interactiveConcurrency: 1 },
  );

  scheduler.enqueue({ id: 1, method: 'indexProject', request: {}, sessionKey: 'demo' });
  scheduler.enqueue({ id: 2, method: 'findDefinitions', request: {}, sessionKey: 'demo' });
  await turn();
  assert.deepEqual(calls, ['indexProject:1', 'findDefinitions:2']);
  hover.resolve('definition');
  await turn();
  assert.deepEqual(calls, ['indexProject:1', 'findDefinitions:2', 'indexProject:1']);
  resumedIndex.resolve('index');
  await turn();
  assert.deepEqual(responses.map(({ id }) => id), [2, 1]);

  const stale = deferred();
  const staleScheduler = new LanguageRequestScheduler(
    (_request: unknown, signal: AbortSignal) => {
      signal.addEventListener('abort', () => stale.reject(new Error('invalidated')), {
        once: true,
      });
      return stale.promise;
    },
    (response: { readonly id: number; readonly error?: string }) => responses.push(response),
  );
  staleScheduler.enqueue({ id: 3, method: 'indexProject', request: {}, sessionKey: 'stale' });
  staleScheduler.invalidate('stale');
  await turn();
  assert.match(responses.at(-1)?.error || '', /invalidated before completion/u);
});
