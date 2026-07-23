'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const test = require('node:test');
const { ViewerRequestScheduler } = require('../dist/src/viewer-scheduler');
const {
  isCustomEditorResource,
  isTextResource,
  isViewerResource,
} = require('../dist/src/resource-capabilities.generated');

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

test('viewer scheduler cancels queued and running native work', async () => {
  const active = deferred();
  let aborted = false;
  const calls: number[] = [];
  const responses: unknown[] = [];
  const scheduler = new ViewerRequestScheduler(
    (request: { id: number }, signal: AbortSignal) => {
      calls.push(request.id);
      signal.addEventListener('abort', () => {
        aborted = true;
        active.reject(new Error('operation cancelled'));
      }, { once: true });
      return active.promise;
    },
    (response: unknown) => responses.push(response),
    { maxRunning: 1 },
  );

  scheduler.enqueue({
    id: 1,
    method: 'loadScene',
    request: { session_key: 'demo' },
  });
  scheduler.enqueue({
    id: 2,
    method: 'loadScene',
    request: { session_key: 'demo' },
  });
  scheduler.cancel(2);
  scheduler.cancel(1);
  await turn();

  assert.deepEqual(calls, [1]);
  assert.equal(aborted, true);
  assert.deepEqual(responses, []);
  assert.equal(scheduler.runningCount, 0);
  assert.equal(scheduler.queuedCount, 0);
});

test('viewer scheduler preserves scene order without blocking interactive resource work', async () => {
  const firstScene = deferred();
  const secondScene = deferred();
  const interactive = deferred();
  const calls: number[] = [];
  const responses: Array<{ id: number }> = [];
  const scheduler = new ViewerRequestScheduler(
    (request: { id: number }) => {
      calls.push(request.id);
      if (request.id === 1) return firstScene.promise;
      if (request.id === 2) return interactive.promise;
      return secondScene.promise;
    },
    (response: { id: number }) => responses.push(response),
    { maxRunning: 2 },
  );

  scheduler.enqueue({
    id: 1,
    method: 'loadScene',
    request: { session_key: 'demo' },
  });
  scheduler.enqueue({
    id: 2,
    method: 'resolveResource',
    request: { session_key: 'demo' },
  });
  scheduler.enqueue({
    id: 3,
    method: 'loadScene',
    request: { session_key: 'demo' },
  });

  assert.deepEqual(calls, [1, 2]);
  interactive.resolve('resource');
  await turn();
  assert.deepEqual(calls, [1, 2]);
  firstScene.resolve('scene one');
  await turn();
  assert.deepEqual(calls, [1, 2, 3]);
  secondScene.resolve('scene two');
  await turn();
  assert.deepEqual(responses.map((response) => response.id).sort(), [1, 2, 3]);
});

test('viewer scheduler enforces its queue safety bound without dropping a request silently', () => {
  const active = deferred();
  const responses: Array<{ id: number; error?: string }> = [];
  const scheduler = new ViewerRequestScheduler(
    () => active.promise,
    (response: { id: number; error?: string }) => responses.push(response),
    { maxRunning: 1, maxQueued: 1 },
  );

  scheduler.enqueue({ id: 1, method: 'loadScene', request: { session_key: 'demo' } });
  scheduler.enqueue({ id: 2, method: 'loadScene', request: { session_key: 'other' } });
  scheduler.enqueue({ id: 3, method: 'loadScene', request: { session_key: 'third' } });

  assert.equal(scheduler.runningCount, 1);
  assert.equal(scheduler.queuedCount, 1);
  assert.equal(responses.length, 1);
  assert.equal(responses[0]?.id, 3);
  assert.match(responses[0]?.error || '', /safety limit/u);
  scheduler.cancel(1);
  scheduler.cancel(2);
});

test('resource capability registry drives package and runtime support', () => {
  const extensionRoot = path.resolve(__dirname, '..');
  const registry = JSON.parse(
    fs.readFileSync(path.join(extensionRoot, 'resource-capabilities.json'), 'utf8'),
  );
  const manifest = JSON.parse(fs.readFileSync(path.join(extensionRoot, 'package.json'), 'utf8'));
  const editor = manifest.contributes.customEditors.find(
    (candidate: { viewType?: string }) => candidate.viewType === 'nwnrs.resourceEditor',
  );
  const expectedPatterns = registry
    .filter((entry: { customEditor: boolean }) => entry.customEditor)
    .map((entry: { extension: string }) => `*.${entry.extension}`);

  assert.deepEqual(
    editor.selector.map((entry: { filenamePattern: string }) => entry.filenamePattern),
    expectedPatterns,
  );
  assert.equal(isCustomEditorResource('script.ncs'), true);
  assert.equal(isCustomEditorResource('script.ndb'), true);
  assert.equal(isCustomEditorResource('conversation.dlg.json'), true);
  assert.equal(isViewerResource('creature.utc'), true);
  assert.equal(isTextResource('material.mtr'), true);
});
