'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const Module = require('node:module');
const path = require('node:path');
const test = require('node:test');

type UnknownRecord = Record<string, unknown>;

interface ScriptDebugConfiguration {
  readonly ndb?: string;
  readonly langspec?: string;
  readonly sources?: Readonly<Record<string, string>>;
}

interface ScriptDebugSnapshot {
  readonly kind: string;
  readonly data: {
    readonly sourceFiles: Array<{ readonly name: string; readonly available: boolean }>;
    readonly diagnostics: string[];
  };
}

interface ScriptDebugDocument {
  readonly uri: { readonly path: string };
  snapshot: ScriptDebugSnapshot;
  request(method: string, configuration: ScriptDebugConfiguration): Promise<ScriptDebugSnapshot>;
}

interface ViewerDocument {
  viewer: boolean;
  uri: { readonly fsPath?: string; toString(): string };
  parent?: unknown;
  resource?: string;
  viewerContents?: Uint8Array;
  scenePacket?: Uint8Array;
  scenePacketPromise?: Promise<unknown>;
  sceneGeneration?: number;
  viewerDependencyResources?: Set<string>;
  viewerDependencyOrigins?: Set<string>;
}

interface AreaInspectionRequest {
  readonly sessionKey: string;
  readonly assetKey: string;
  readonly objectKey: string;
}

function loadResourceEditorWithoutVsCodeHost() {
  const originalLoad = Module._load;
  try {
    Module._load = function load(
      request: string,
      parent: NodeModule | null | undefined,
      isMain: boolean,
    ) {
      if (request === 'vscode') return {};
      return originalLoad.call(this, request, parent, isMain);
    };
    return require('../dist/src/resource-custom-editor');
  } finally {
    Module._load = originalLoad;
  }
}

function loadProviderWithoutVsCodeHost() {
  return loadResourceEditorWithoutVsCodeHost().ResourceCustomEditorProvider;
}

function scenePacket(manifest: UnknownRecord = {}): Buffer {
  const json = Buffer.from(JSON.stringify({ dependencies: { nodes: [] }, ...manifest }));
  const packet = Buffer.alloc(12 + json.length);
  packet.write('NWNRS3D\0', 0, 'binary');
  packet.writeUInt32LE(json.length, 8);
  json.copy(packet, 12);
  return packet;
}

test('webview renderer and stylesheet are inside the declared local resource roots', () => {
  const { RESOURCE_EDITOR_WEBVIEW_ASSETS: assets } = loadResourceEditorWithoutVsCodeHost();
  assert.deepEqual(assets.roots, [['media'], ['dist', 'media']]);
  for (const asset of [assets.script, assets.style]) {
    assert.ok(
      assets.roots.some((root: readonly string[]) =>
        root.every((segment, index) => asset[index] === segment)),
      `${asset.join('/')} is outside the webview local resource roots`,
    );
  }
});

test('resource browser opens only supported packed formats while allowing physical files', () => {
  const ResourceCustomEditorProvider = loadProviderWithoutVsCodeHost();
  const provider = Object.create(ResourceCustomEditorProvider.prototype);
  assert.equal(provider.canOpenResource('c_bodak.mdl'), true);
  assert.equal(provider.canOpenResource('nwscript.nss'), true);
  assert.equal(provider.canOpenResource('appearance.2da'), true);
  assert.equal(provider.canOpenResource('module_load.ncs'), true);
  assert.equal(provider.canOpenResource('module_load.ndb'), true);
  assert.equal(provider.canOpenResource('ambient.wav'), false);
  assert.equal(provider.canOpenResource('ambient.wav', '/workspace/ambient.wav'), true);
});

test('NCS workbench enrichment resolves matching debug, language, and source resources', async () => {
  const ResourceCustomEditorProvider = loadProviderWithoutVsCodeHost();
  const provider = Object.create(ResourceCustomEditorProvider.prototype);
  const requestedResources: string[] = [];
  const configurations: ScriptDebugConfiguration[] = [];
  provider.readScriptDebugResource = async (_document: unknown, resource: string) => {
    requestedResources.push(resource);
    return Buffer.from(resource);
  };
  let snapshot: ScriptDebugSnapshot = {
    kind: 'ncs',
    data: { sourceFiles: [], diagnostics: [] },
  };
  const document: ScriptDebugDocument = {
    uri: { path: '/virtual/demo.ncs' },
    snapshot,
    async request(method: string, configuration: ScriptDebugConfiguration) {
      assert.equal(method, 'configureScriptDebug');
      configurations.push(configuration);
      if (configuration.ndb) snapshot = {
        kind: 'ncs', data: { sourceFiles: [{ name: 'demo', available: false }], diagnostics: [] },
      };
      if (configuration.sources) snapshot = {
        kind: 'ncs', data: { sourceFiles: [{ name: 'demo', available: true }], diagnostics: [] },
      };
      return snapshot;
    },
  };

  await provider.enrichScriptDebugDocument(document);

  assert.deepEqual(requestedResources, ['demo.ndb', 'nwscript.nss', 'demo.nss']);
  assert.ok(configurations.some((configuration) => configuration.ndb));
  assert.ok(configurations.some((configuration) => configuration.langspec));
  assert.ok(configurations.some((configuration) => configuration.sources?.demo));
  assert.equal(document.snapshot.data.sourceFiles[0]?.available, true);
});

test('provider handles the webview ready message with the owning view', async () => {
  const ResourceCustomEditorProvider = loadProviderWithoutVsCodeHost();
  const provider = Object.create(ResourceCustomEditorProvider.prototype);
  const logged: string[] = [];
  const posted: Array<{ document: unknown; view: unknown }> = [];
  provider.output = { appendLine: (message: string) => logged.push(message) };
  provider.postSnapshot = async (document: unknown, view: unknown) => {
    posted.push({ document, view });
  };
  const document = { snapshot: { kind: 'gff' } };
  const view = { ready: true, webview: {} };

  await provider.handleMessage(document, view, { type: 'ready' });

  assert.deepEqual(posted, [{ document, view }]);
  assert.deepEqual(logged, []);
});

test('area inspection requests use the cached scene identity and return only to the requesting view', async () => {
  const ResourceCustomEditorProvider = loadProviderWithoutVsCodeHost();
  const provider = Object.create(ResourceCustomEditorProvider.prototype);
  const requests: AreaInspectionRequest[] = []; const posted: unknown[] = [];
  provider.output = { appendLine: (message: string) => assert.fail(message) };
  provider.viewerRequest = () => ({ session_key: '/workspace/nwpkg.toml' });
  provider.viewerWorker = {
    inspectAreaObject: async (request: AreaInspectionRequest) => {
      requests.push(request);
      return { schema: 'nwnrs.area-object-inspection', key: request.objectKey };
    },
  };
  const document = { viewer: true, sceneGeneration: 3 };
  const view = { ready: true, webview: { postMessage: async (message: unknown) => posted.push(message) } };

  await provider.handleMessage(document, view, {
    type: 'inspectAreaObject', assetKey: 'scene-key', objectKey: 'placeable:7',
  });

  assert.deepEqual(requests, [{
    sessionKey: '/workspace/nwpkg.toml', assetKey: 'scene-key', objectKey: 'placeable:7',
  }]);
  assert.deepEqual(posted, [{
    type: 'areaObjectInspection',
    assetKey: 'scene-key',
    objectKey: 'placeable:7',
    inspection: { schema: 'nwnrs.area-object-inspection', key: 'placeable:7' },
  }]);
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
    async readEntryBytes(resource: string) {
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
  const broadcasts: unknown[] = [];
  provider.documents = new Map<string, unknown>([['parent', parent], ['viewer', viewer]]);
  provider.viewerWorker = { invalidate: () => { invalidated += 1; } };
  provider.viewerRequest = () => ({ session_key: '/workspace/nwpkg.toml', project_root: '/workspace' });
  provider.output = { appendLine: (message: string) => assert.fail(message) };
  provider.broadcast = async (document: unknown) => { broadcasts.push(document); };

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
  let loads = 0;
  let completeLoad: ((packet: Buffer) => void) | undefined;
  provider.viewerWorker = {
    loadScene: async () => {
      loads += 1;
      return new Promise<Buffer>((resolve) => { completeLoad = resolve; });
    },
  };
  provider.viewerRequest = () => ({ session_key: '/workspace/nwpkg.toml' });
  const document: {
    scenePacket: Uint8Array | undefined;
    scenePacketPromise: Promise<unknown> | undefined;
    sceneGeneration: number;
    viewerContents: Uint8Array | undefined;
  } = {
    scenePacket: undefined,
    scenePacketPromise: undefined,
    sceneGeneration: 0,
    viewerContents: undefined,
  };

  const first = provider.scenePacket(document);
  const second = provider.scenePacket(document);
  assert.equal(loads, 1);
  if (!completeLoad) throw new Error('Scene load resolver was not captured');
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

test('virtual resource URIs retain enough validated context to survive extension restarts', () => {
  const {
    decodeVirtualResourceDescriptor,
    virtualResourceQuery,
  } = loadResourceEditorWithoutVsCodeHost();
  const request = {
    session_key: '/workspace/module/nwpkg.toml',
    path: '/workspace/module/start.are',
    project_root: '/workspace/module',
    area: null,
    root: '/game',
    user: '/user',
    language: 'english',
    load_ovr: false,
    archives: [],
    include_project_resources: true,
    authored_area: {
      resref: 'start',
      are: '/workspace/module/start.are.json',
      git: '/workspace/module/start.git.json',
      gic: '/workspace/module/start.gic.json',
    },
  };
  const query = virtualResourceQuery('area-id', 'start.are', request);
  assert.deepEqual(decodeVirtualResourceDescriptor({ path: '/start.are', query }), {
    resource: 'start.are',
    request,
  });
  assert.equal(decodeVirtualResourceDescriptor({ path: '/other.are', query }), undefined);
  assert.equal(decodeVirtualResourceDescriptor({
    path: '/start.are',
    query: virtualResourceQuery('area-id', 'start.are', { ...request, path: 'start.are' }),
  }), undefined);
  assert.equal(decodeVirtualResourceDescriptor({ path: '/start.are', query: 'id=legacy' }), undefined);
});

test('virtual binary and text resources rehydrate from their self-contained URI context', async () => {
  const {
    ResourceCustomEditorProvider,
    virtualResourceQuery,
  } = loadResourceEditorWithoutVsCodeHost();
  const provider = Object.create(ResourceCustomEditorProvider.prototype);
  provider.viewerResources = new Map();
  provider.viewerTextResources = new Map();
  let reads = 0;
  provider.viewerWorker = {
    readResource: async (request: { readonly session_key: string }) => {
      reads += 1;
      assert.equal(request.session_key, '/workspace/nwpkg.toml');
      return Uint8Array.from([110, 119, 110, 114, 115]);
    },
  };
  const request = {
    session_key: '/workspace/nwpkg.toml',
    path: '/workspace/a_ba.mdl',
    project_root: '/workspace',
    archives: [],
  };
  const binaryUri = {
    path: '/a_ba.mdl',
    query: virtualResourceQuery('binary-id', 'a_ba.mdl', request),
  };
  const restored = await provider.resolveVirtualResource(binaryUri);
  assert.deepEqual(Array.from(restored.contents), [110, 119, 110, 114, 115]);
  assert.equal((await provider.resolveVirtualResource(binaryUri)), restored);
  const textRequest = { ...request, path: '/workspace/model.mtr' };
  const textUri = {
    path: '/model.mtr',
    query: virtualResourceQuery('text-id', 'model.mtr', textRequest),
  };
  assert.equal(await provider.virtualTextContents(textUri), 'nwnrs');
  assert.equal(await provider.virtualTextContents(textUri), 'nwnrs');
  assert.equal(reads, 2);
});

test('authored areas become one in-memory ARE/GIT/GIC viewer request', () => {
  const { authoredAreaRequest, authoredAreaVirtualId } = loadResourceEditorWithoutVsCodeHost();
  const base = {
    session_key: '/workspace/module/nwpkg.toml',
    project_root: '/workspace/module',
    path: '/workspace/module/.catalog',
  };
  const area = {
    resref: 'start',
    files: [
      { kind: 'are', path: '/workspace/module/areas/start.are.json' },
      { kind: 'git', path: '/workspace/module/areas/start.git.json' },
      { kind: 'gic', path: '/workspace/module/areas/start.gic.json' },
    ],
  };

  const request = authoredAreaRequest(base, area);
  assert.deepEqual(request, {
    ...base,
    path: '/workspace/module/start.are',
    area: null,
    authored_area: {
      resref: 'start',
      are: '/workspace/module/areas/start.are.json',
      git: '/workspace/module/areas/start.git.json',
      gic: '/workspace/module/areas/start.gic.json',
    },
  });
  assert.equal(authoredAreaVirtualId(request), authoredAreaVirtualId({
    ...request,
    path: '/a/different/non-identity/path.are',
  }));
  assert.notEqual(authoredAreaVirtualId(request), authoredAreaVirtualId({
    ...request,
    authored_area: { ...request.authored_area, resref: 'another' },
  }));
  assert.throws(
    () => authoredAreaRequest(base, { resref: 'broken', files: [area.files[0]] }),
    /requires both ARE and GIT/u,
  );
  assert.throws(
    () => authoredAreaRequest(base, { resref: 'duplicate', files: [area.files[0], area.files[0], area.files[1]] }),
    /more than one ARE/u,
  );
});

test('viewport area selection is retained and emitted for sidebar reveal', async () => {
  const ResourceCustomEditorProvider = loadProviderWithoutVsCodeHost();
  const provider = Object.create(ResourceCustomEditorProvider.prototype);
  const emitted: unknown[] = [];
  provider.output = { appendLine: (message: string) => assert.fail(message) };
  provider._onDidSelectAreaObject = { fire: (selection: unknown) => emitted.push(selection) };
  provider.viewerRequest = () => ({
    session_key: '/workspace/nwpkg.toml',
    authored_area: { resref: 'start' },
  });
  const currentView = { ready: true, webview: { postMessage: async () => {} } };
  const otherMessages: unknown[] = [];
  const document: {
    viewerRequestOverride: { readonly authored_area: { readonly resref: string } };
    views: Set<{
      readonly ready: boolean;
      readonly webview: { postMessage(message: unknown): Promise<unknown> };
    }>;
    selectedAreaObjectKey?: string;
  } = {
    viewerRequestOverride: { authored_area: { resref: 'start' } },
    views: new Set([
      currentView,
      { ready: true, webview: { postMessage: async (message: unknown) => otherMessages.push(message) } },
    ]),
  };

  await provider.handleMessage(document, currentView, {
    type: 'selectAreaObject',
    objectKey: 'placeable:0',
  });

  assert.equal(document.selectedAreaObjectKey, 'placeable:0');
  assert.deepEqual(emitted, [{
    manifestPath: '/workspace/nwpkg.toml',
    resref: 'start',
    objectKey: 'placeable:0',
  }]);
  assert.deepEqual(otherMessages, [{
    type: 'selectAreaObject', objectKey: 'placeable:0', frame: false,
  }]);
});

test('viewer refresh targets only scenes whose source or dependency changed', async () => {
  const ResourceCustomEditorProvider = loadProviderWithoutVsCodeHost();
  const provider = Object.create(ResourceCustomEditorProvider.prototype);
  const root = path.resolve('/workspace');
  const changed = path.join(root, 'textures', 'cat.dds');
  const makeViewer = (name: string, dependency: string): ViewerDocument => ({
    viewer: true,
    uri: { fsPath: path.join(root, `${name}.mdl`), toString: () => name },
    viewerDependencyResources: new Set([dependency]),
    viewerDependencyOrigins: new Set(),
  });
  const cat = makeViewer('cat', 'cat.dds');
  const dog = makeViewer('dog', 'dog.dds');
  provider.documents = new Map([['cat', cat], ['dog', dog]]);
  provider.viewerRequest = () => ({ session_key: path.join(root, 'nwpkg.toml'), project_root: root });
  const invalidated: string[] = []; const broadcasts: unknown[] = [];
  provider.viewerWorker = { invalidate: (session: string) => invalidated.push(session) };
  provider.broadcast = async (document: unknown) => broadcasts.push(document);
  provider.output = { appendLine: (message: string) => assert.fail(message) };

  await provider.refreshViewerDocuments(new Set([changed]));

  assert.deepEqual(broadcasts, [cat]);
  assert.deepEqual(invalidated, [path.join(root, 'nwpkg.toml')]);
  assert.equal(cat.scenePacket, undefined);
  assert.equal(dog.scenePacket, undefined);
});

test('viewer refresh recognizes compound JSON dependency source names outside the package root', () => {
  const { viewerAffectedByPaths } = loadResourceEditorWithoutVsCodeHost();
  const changed = path.resolve('/workspace/shared/blueprints/bodak.utc.json');
  assert.equal(viewerAffectedByPaths({
    viewerDependencyResources: new Set(['bodak.utc']),
    viewerDependencyOrigins: new Set(),
    viewerSourcePaths: [],
    uri: {},
  }, new Set([changed]), { project_root: '/workspace/module' }), true);
});

test('hidden custom editors release their webview context', () => {
  const source = fs.readFileSync(
    path.resolve(__dirname, '..', 'dist', 'src', 'resource-custom-editor.js'),
    'utf8',
  );
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
  provider.output = { appendLine: (message: string) => assert.fail(message) };
  const document = { viewer: true, sceneGeneration: 4 };
  const view = { ready: true, webview: { postMessage: async () => {} } };

  await provider.handleMessage(document, view, {
    type: 'loadAnimation', assetKey: 'evicted', modelIndex: 0, animationIndex: 0,
  });

  assert.equal(recovered, 1);
});
