'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const Module = require('node:module');
const path = require('node:path');
const test = require('node:test');

function loadProviderWithoutVsCodeHost() {
  const originalLoad = Module._load;
  try {
    Module._load = function load(request, parent, isMain) {
      if (request === 'vscode') return {};
      return originalLoad.call(this, request, parent, isMain);
    };
    return require('../src/resource-custom-editor').ResourceCustomEditorProvider;
  } finally {
    Module._load = originalLoad;
  }
}

function scenePacket(manifest = {}) {
  const json = Buffer.from(JSON.stringify({ dependencies: { nodes: [] }, ...manifest }));
  const packet = Buffer.alloc(12 + json.length);
  packet.write('NWNRS3D\0', 0, 'binary');
  packet.writeUInt32LE(json.length, 8);
  json.copy(packet, 12);
  return packet;
}

test('provider handles the webview ready message with the owning view', async () => {
  const ResourceCustomEditorProvider = loadProviderWithoutVsCodeHost();
  const provider = Object.create(ResourceCustomEditorProvider.prototype);
  const logged = [];
  const posted = [];
  provider.output = { appendLine: (message) => logged.push(message) };
  provider.postSnapshot = async (document, view) => {
    posted.push({ document, view });
  };
  const document = { snapshot: { kind: 'gff' } };
  const view = { ready: true, webview: {} };

  await provider.handleMessage(document, view, { type: 'ready' });

  assert.deepEqual(posted, [{ document, view }]);
  assert.deepEqual(logged, []);
});

test('automatic viewer refresh reloads changed archives and replaces cached entry bytes', async () => {
  const ResourceCustomEditorProvider = loadProviderWithoutVsCodeHost();
  const provider = Object.create(ResourceCustomEditorProvider.prototype);
  const archivePath = path.resolve('/workspace/module.mod');
  let reverted = 0; let invalidated = 0;
  const parent = {
    dirty: false,
    uri: { scheme: 'file', fsPath: archivePath, toString: () => archivePath },
    async revert() { reverted += 1; },
    async readEntryBytes(resource) {
      assert.equal(resource, 'cat.mdl');
      return Uint8Array.from([4, 5, 6]);
    },
  };
  const viewer = {
    viewer: true,
    parent,
    resource: 'cat.mdl',
    viewerContents: Buffer.from([1, 2, 3]),
    scenePacket: Buffer.from([9]),
    uri: { toString: () => 'nwnrs-resource:/cat.mdl' },
  };
  const broadcasts = [];
  provider.documents = new Map([['parent', parent], ['viewer', viewer]]);
  provider.viewerWorker = { invalidate: () => { invalidated += 1; } };
  provider.viewerRequest = () => ({ session_key: '/workspace/nwpkg.toml', project_root: '/workspace' });
  provider.output = { appendLine: (message) => assert.fail(message) };
  provider.broadcast = async (document) => { broadcasts.push(document); };

  await provider.refreshViewerDocuments(new Set([archivePath]));

  assert.equal(reverted, 1);
  assert.equal(invalidated, 1);
  assert.deepEqual(Array.from(viewer.viewerContents), [4, 5, 6]);
  assert.equal(viewer.scenePacket, undefined);
  assert.deepEqual(broadcasts, [parent, viewer]);
});

test('concurrent scene posts share one generation-safe worker request', async () => {
  const ResourceCustomEditorProvider = loadProviderWithoutVsCodeHost();
  const provider = Object.create(ResourceCustomEditorProvider.prototype);
  let loads = 0; let completeLoad;
  provider.viewerWorker = {
    loadScene: async () => {
      loads += 1;
      return new Promise((resolve) => { completeLoad = resolve; });
    },
  };
  provider.viewerRequest = () => ({ session_key: '/workspace/nwpkg.toml' });
  const document = {
    scenePacket: undefined,
    scenePacketPromise: undefined,
    sceneGeneration: 0,
    viewerContents: undefined,
  };

  const first = provider.scenePacket(document);
  const second = provider.scenePacket(document);
  assert.equal(loads, 1);
  completeLoad(scenePacket());

  assert.deepEqual(Array.from(await first), Array.from(scenePacket()));
  assert.deepEqual(Array.from(await second), Array.from(scenePacket()));
  assert.equal(loads, 1);
});

test('virtual viewer documents retain their package resolution path', () => {
  const ResourceCustomEditorProvider = loadProviderWithoutVsCodeHost();
  const provider = Object.create(ResourceCustomEditorProvider.prototype);
  const request = {
    session_key: '/workspace/nwpkg.toml',
    path: '/workspace/a_ba.mdl',
    project_root: '/workspace',
    area: null,
  };
  const document = {
    uri: { path: '/a_ba.mdl' },
    viewerRequestOverride: request,
    sceneArea: undefined,
  };

  const resolved = provider.viewerRequest(document);

  assert.equal(resolved.path, '/workspace/a_ba.mdl');
  assert.equal(resolved.project_root, '/workspace');
  assert.notEqual(resolved, request);
});

test('viewer refresh targets only scenes whose source or dependency changed', async () => {
  const ResourceCustomEditorProvider = loadProviderWithoutVsCodeHost();
  const provider = Object.create(ResourceCustomEditorProvider.prototype);
  const root = path.resolve('/workspace');
  const changed = path.join(root, 'textures', 'cat.dds');
  const makeViewer = (name, dependency) => ({
    viewer: true,
    uri: { fsPath: path.join(root, `${name}.mdl`), toString: () => name },
    viewerDependencyResources: new Set([dependency]),
    viewerDependencyOrigins: new Set(),
  });
  const cat = makeViewer('cat', 'cat.dds');
  const dog = makeViewer('dog', 'dog.dds');
  provider.documents = new Map([['cat', cat], ['dog', dog]]);
  provider.viewerRequest = () => ({ session_key: path.join(root, 'nwpkg.toml'), project_root: root });
  const invalidated = []; const broadcasts = [];
  provider.viewerWorker = { invalidate: (session) => invalidated.push(session) };
  provider.broadcast = async (document) => broadcasts.push(document);
  provider.output = { appendLine: (message) => assert.fail(message) };

  await provider.refreshViewerDocuments(new Set([changed]));

  assert.deepEqual(broadcasts, [cat]);
  assert.deepEqual(invalidated, [path.join(root, 'nwpkg.toml')]);
  assert.equal(cat.scenePacket, undefined);
  assert.equal(dog.scenePacket, undefined);
});

test('hidden custom editors release their webview context', () => {
  const source = fs.readFileSync(path.resolve(__dirname, '..', 'src', 'resource-custom-editor.js'), 'utf8');
  assert.match(source, /retainContextWhenHidden: false/u);
  assert.doesNotMatch(source, /retainContextWhenHidden: true/u);
});

test('lazy viewer cache misses transparently rehydrate the scene', async () => {
  const ResourceCustomEditorProvider = loadProviderWithoutVsCodeHost();
  const provider = Object.create(ResourceCustomEditorProvider.prototype);
  let recovered = 0;
  provider.viewerWorker = {
    loadAnimation: async () => { throw new Error('viewer scene assets were evicted; reload the scene'); },
  };
  provider.viewerRequest = () => ({ session_key: '/workspace/nwpkg.toml' });
  provider.recoverViewerScene = async () => { recovered += 1; };
  provider.output = { appendLine: (message) => assert.fail(message) };
  const document = { viewer: true, sceneGeneration: 4 };
  const view = { ready: true, webview: { postMessage: async () => {} } };

  await provider.handleMessage(document, view, {
    type: 'loadAnimation', assetKey: 'evicted', modelIndex: 0, animationIndex: 0,
  });

  assert.equal(recovered, 1);
});
