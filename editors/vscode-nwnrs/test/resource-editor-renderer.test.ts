'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const test = require('node:test');
const vm = require('node:vm');
const { ResourceEditorWorkerClient } = require('../dist/src/resource-editor-worker-client');
const { ViewerWorkerClient } = require('../dist/src/viewer-worker-client');

const extensionRoot = path.resolve(__dirname, '..');
const repositoryRoot = path.resolve(extensionRoot, '..', '..');
const bindingPath = path.join(extensionRoot, 'native', 'nwnrs-vscode.darwin-arm64.node');
const rendererSource = fs.readFileSync(
  path.join(extensionRoot, 'dist', 'media', 'resource-editor.js'),
  'utf8',
);
const rendererStyle = fs.readFileSync(
  path.join(extensionRoot, 'media', 'resource-editor.css'),
  'utf8',
);
const installedAcceptanceResources = [
  ['cloakmodel.2da', '2da'],
  ['quickchat.gff', 'gff'],
  ['voiceset.gff', 'gff'],
] as const;

interface TestContext {
  skip(message?: string): void;
}

interface TestElement {
  readonly id: string;
  innerHTML: string;
  textContent: string;
  value: string;
  readonly dataset: Record<string, string>;
  type: string;
  hidden: boolean;
  scrollTop: number;
  clientWidth: number;
  readonly style: { setProperty(name: string, value: string): void };
  readonly classList: {
    add(...names: string[]): void;
    remove(...names: string[]): void;
    toggle(name: string, force?: boolean): void;
  };
  getContext(): object;
  querySelector(): TestElement | null;
  querySelectorAll(): TestElement[];
  scrollIntoView(): void;
  setAttribute(name: string, value: string): void;
  setPointerCapture(pointerId: number): void;
  focus(): void;
}

type RendererMessageListener = (event: { readonly data: unknown }) => unknown;

function dispatchRendererMessage(
  listeners: ReadonlyMap<string, RendererMessageListener>,
  data: unknown,
): unknown {
  const listener = listeners.get('message');
  if (!listener) throw new Error('Renderer message listener was not registered');
  return listener({ data });
}

function isPostedMessage(value: unknown): value is { readonly type: string } {
  return typeof value === 'object'
    && value !== null
    && 'type' in value
    && typeof value.type === 'string';
}

function createRendererHarness() {
  const elements = new Map<string, TestElement>();
  const listeners = new Map<string, RendererMessageListener>();
  const posted: unknown[] = [];
  let persistedState: unknown;
  const element = (id: string): TestElement => {
    const existing = elements.get(id);
    if (existing) return existing;
    const created: TestElement = {
      id,
      innerHTML: '',
      textContent: '',
      value: '',
      dataset: {},
      type: '',
      hidden: false,
      scrollTop: 0,
      clientWidth: 1280,
      style: { setProperty() {} },
      classList: { add() {}, remove() {}, toggle() {} },
      getContext() { return {}; },
      querySelector() { return null; },
      querySelectorAll() { return []; },
      scrollIntoView() {},
      setAttribute() {},
      setPointerCapture() {},
      focus() {},
    };
    elements.set(id, created);
    return created;
  };
  element('app').innerHTML = '<div class="loading">Loading resource…</div>';
  const sandbox: import('node:vm').Context = {
    acquireVsCodeApi: () => ({
      postMessage: (message: unknown) => posted.push(message),
      getState: () => persistedState,
      setState: (value: unknown) => { persistedState = value; },
    }),
    atob,
    btoa,
    Blob,
    clearTimeout,
    confirm: () => true,
    console,
    createImageBitmap: async () => { throw new Error('not used by acceptance fixtures'); },
    document: {
      createElement: () => element(`created-${elements.size}`),
      getElementById: element,
      querySelector: () => null,
      querySelectorAll: () => [],
    },
    prompt: () => null,
    setTimeout,
    structuredClone,
    DataView,
    Float32Array,
    Int32Array,
    TextDecoder,
    TextEncoder,
    Uint32Array,
    Uint8Array,
    Uint8ClampedArray,
    devicePixelRatio: 1,
    performance,
    requestAnimationFrame: () => 1,
    cancelAnimationFrame() {},
    ResizeObserver: class {
      observe() {}
      disconnect() {}
    },
    window: {
      addEventListener(
        type: string,
        listener: (event: { readonly data: unknown }) => unknown,
      ) {
        listeners.set(type, listener);
      },
      removeEventListener(type: string) {
        listeners.delete(type);
      },
    },
  };
  vm.runInNewContext(rendererSource, sandbox, { filename: 'resource-editor.js' });
  return {
    app: element('app'),
    content: element('content'),
    element,
    listeners,
    posted,
    sandbox,
    getPersistedState: () => persistedState,
  };
}

function createViewerElements(element: (id: string) => TestElement) {
  return {
    status: element('test-viewer-status'),
    animationTime: element('test-viewer-animation-time'),
    animationEvent: element('test-viewer-animation-event'),
    workbench: element('test-viewer-workbench'),
    inspector: element('test-viewer-inspector'),
    inspectorContent: element('test-viewer-inspector-content'),
    inspectorContext: element('test-viewer-inspector-context'),
    inspectorScope: element('test-viewer-inspector-scope'),
    inspectorSearch: element('test-viewer-inspector-search'),
    inspectorJump: element('test-viewer-inspector-jump'),
    inspectorTechnical: element('test-viewer-inspector-technical'),
    inspectorCollapse: element('test-viewer-inspector-collapse'),
    inspectorReopen: element('test-viewer-inspector-reopen'),
    inspectorSash: element('test-viewer-inspector-sash'),
  };
}

function createWebGlHarness() {
  let constant = 1;
  const values: Record<PropertyKey, unknown> = {
    COMPILE_STATUS: 1,
    LINK_STATUS: 2,
    MAX_TEXTURE_SIZE: 3,
    TEXTURE0: 100,
  };
  return new Proxy(values, {
    get(target, property) {
      if (property === 'getShaderParameter' || property === 'getProgramParameter') return () => true;
      if (property === 'getParameter') return () => 4096;
      if (property === 'getShaderInfoLog' || property === 'getProgramInfoLog') return () => '';
      if (property === 'getUniformLocation') return () => ({});
      if (String(property).startsWith('create')) return () => ({});
      if (property in target) return target[property];
      if (/^[A-Z0-9_]+$/u.test(String(property))) {
        target[property] = constant += 1;
        return target[property];
      }
      return () => {};
    },
  });
}

test('viewer animation tracks interpolate and matrix inversion remains stable', () => {
  const { sandbox, listeners } = createRendererHarness();
  const binary = new Uint8Array(32);
  new Float32Array(binary.buffer, 0, 2).set([0, 1]);
  new Float32Array(binary.buffer, 8, 6).set([0, 0, 0, 2, 4, 6]);
  const track = {
    times: { byteOffset: 0, byteLength: 8, component: 'f32', componentsPerElement: 1 },
    values: { byteOffset: 8, byteLength: 24, component: 'f32', componentsPerElement: 3 },
  };
  const preparedTrack = sandbox.preparePackedTrack(binary, track);
  const sampledTrack = new Float32Array(3);
  sandbox.samplePreparedTrackInto(preparedTrack, 0.25, [9, 9, 9], sampledTrack);
  assert.deepEqual(Array.from(sampledTrack), [0.5, 1, 1.5]);
  const transform = sandbox.multiply4(sandbox.translation4([2, 3, 4]), sandbox.scale4([2, 4, 8]));
  const product = sandbox.multiply4(transform, sandbox.inverse4(transform));
  assert.deepEqual(
    Array.from(product as Float32Array).map((value) => Math.round(value * 1000) / 1000),
    Array.from(sandbox.identity4()),
  );
  const emitter = { properties: [{ name: 'birthrate', values: [{ kind: 'float', value: 12 }] }] };
  assert.equal(sandbox.emitterProperty(emitter, 'BIRTHRATE', 0), 12);
  const emitterBinary = new Uint8Array(28);
  new Float32Array(emitterBinary.buffer, 0, 2).set([0, 1]);
  new Float32Array(emitterBinary.buffer, 8, 2).set([2, 4]);
  new Uint32Array(emitterBinary.buffer, 16, 3).set([0, 1, 2]);
  const emitterTrack = { emitterControllers: [{
    controller: 'velocity',
    times: { byteOffset: 0, byteLength: 8, component: 'f32', componentsPerElement: 1 },
    values: {
      values: { byteOffset: 8, byteLength: 8, component: 'f32', componentsPerElement: 1 },
      rowOffsets: { byteOffset: 16, byteLength: 12, component: 'u32', componentsPerElement: 1 },
    },
  }] };
  const preparedEmitter = sandbox.preparedEmitterTrack(
    emitterBinary,
    emitterTrack,
    'velocity',
  );
  assert.equal(sandbox.samplePreparedEmitterValue(preparedEmitter, 0.25, 0), 2.5);
  assert.deepEqual(Array.from(sandbox.lightOverrideForNode('mainlight2', [[1, 0, 0], [0, 1, 0]], 0)), [0, 1, 0]);
  const globalLight = sandbox.globalIllumination();
  assert.deepEqual(Array.from(globalLight.environmentLight), [1, 1, 1]);
  assert.equal(globalLight.fogEnabled, false);
  assert.deepEqual(Array.from(globalLight.background), [0.035, 0.045, 0.06]);
  const areaLight = sandbox.globalIllumination({ isNight: true, moonAmbientColor: 0x00102030, moonDiffuseColor: 0x00405060, moonFogColor: 0x00708090, fogClipDistance: 72 });
  assert.equal(areaLight.fogEnabled, true);
  assert.equal(areaLight.fogEnd, 72);
  assert.notDeepEqual(Array.from(areaLight.environmentLight), Array.from(globalLight.environmentLight));
  assert.ok(areaLight.environmentLight.every(
    (value: number) => Number.isFinite(value) && value >= 0.35 && value <= 1.15,
  ));
  assert.doesNotMatch(rendererSource, /viewer-lighting|setLighting|uKeyDirection|uFillDirection|uRimColor|keyDiffuse|fillDiffuse/u);
  assert.match(rendererSource, /vec3 lit=base\.rgb\*uEnvironmentLight\*uMaterialAmbient\+emissive/u);
  assert.notDeepEqual(Array.from(sandbox.surfaceColor(2)), Array.from(sandbox.surfaceColor(6)));

  const animMeshBinary = new Uint8Array(20);
  new Float32Array(animMeshBinary.buffer, 0, 3).set([2, 4, 6]);
  new Float32Array(animMeshBinary.buffer, 12, 2).set([0.25, 0.75]);
  const sourceVertex = new Float32Array(19);
  sourceVertex.set([0.2, 0.4, 0.6], 16);
  const animMeshGpu = {
    vertices: sourceVertex,
    indices: new Uint32Array([0]),
    uvIndices: new Uint32Array([0]),
    stride: 19,
  };
  const preparedAnimMesh = sandbox.prepareAnimMeshTrack(animMeshBinary, {
    vertexFrameCount: 1,
    verticesPerFrame: 1,
    vertexSamples: { byteOffset: 0, byteLength: 12, component: 'f32', componentsPerElement: 3 },
    uvFrameCount: 1,
    uvsPerFrame: 1,
    uvSamples: { byteOffset: 12, byteLength: 8, component: 'f32', componentsPerElement: 2 },
    samplePeriod: 1,
  });
  const animatedVertex = sandbox.updatePreparedAnimMesh(
    animMeshGpu,
    preparedAnimMesh,
    0,
    1,
    undefined,
    0,
    1,
    1,
  );
  assert.deepEqual(Array.from(animatedVertex.slice(0, 3)), [2, 4, 6]);
  assert.deepEqual(Array.from(animatedVertex.slice(6, 8)), [0.25, 0.75]);
  assert.deepEqual(
    Array.from(animatedVertex.slice(16, 19) as Float32Array)
      .map((value) => Math.round(value * 10) / 10),
    [0.2, 0.4, 0.6],
  );
  dispatchRendererMessage(
    listeners,
    { type: 'snapshot', snapshot: { kind: 'unknown', path: 'done' } },
  );
});

test('authored object inspections use compact searchable rows and lossless drill-down pages', () => {
  const { sandbox } = createRendererHarness();
  const inspection = {
    schema: 'nwnrs.area-object-inspection',
    key: 'placeable:0',
    sections: [{
      id: 'identity',
      label: 'Identity & Text',
      defaultOpen: true,
      fields: [{
        name: 'Description',
        label: 'Description',
        kind: 'cexolocstring',
        display: '<script>bad()</script>',
        localized: {
          strRef: 42,
          source: 'dialog.tlk',
          languageId: 0,
          gender: 'male',
          entries: [{ id: 0, text: 'Inline <English>' }],
        },
        provenance: { layer: 'instance', resource: 'start.git', origin: 'module.mod' },
        resource: { resource: 'inspect_me.dlg', resolved: true, origin: 'module.mod' },
      }, {
        name: 'Opaque',
        label: 'Opaque',
        kind: 'void',
        display: '3 bytes',
        value64: 'AP8Q',
        provenance: { layer: 'instance', resource: 'start.git', origin: 'module.mod' },
      }],
    }],
    sources: [{ layer: 'instance', resource: 'start.git', origin: 'module.mod', data: { id: 1, fields: [] } }],
    references: [{ resource: 'inspect_me.dlg', resolved: true, origin: 'module.mod' }],
    diagnostics: [],
  };
  const shell = sandbox.inspectionContent({ status: 'ready', data: inspection });
  assert.match(shell, /data-section-id="inspection-identity"/u);
  assert.match(shell, /&lt;script&gt;bad\(\)&lt;\/script&gt;/u);
  assert.match(shell, /inspection-property-row inspection-field-row/u);
  assert.match(shell, /title="instance · start\.git · module\.mod">GIT/u);
  assert.match(shell, /data-resource="inspect_me\.dlg"/u);
  assert.doesNotMatch(shell, /AP8Q/u, 'opaque bytes belong to the explicit field drill-down page');
  assert.doesNotMatch(shell, /Inline &lt;English&gt;/u, 'localized variants belong to field drill-down');

  const descriptionRoute = { page: 'field', root: 'section', rootIndex: 0, trail: [{ kind: 'field', index: 0 }] };
  const description = sandbox.inspectionContent({ status: 'ready', data: inspection }, { route: descriptionRoute, technicalNames: true });
  assert.match(description, /GFF name/u);
  assert.match(description, /Description/u);
  assert.match(description, /String reference/u);
  assert.match(description, /dialog\.tlk/u);
  assert.match(description, /Inline Localized Values/u);
  assert.match(description, /Inline &lt;English&gt;/u);
  assert.match(description, /module\.mod/u);

  const opaqueRoute = { page: 'field', root: 'section', rootIndex: 0, trail: [{ kind: 'field', index: 1 }] };
  assert.match(sandbox.inspectionContent({ status: 'ready', data: inspection }, { route: opaqueRoute }), /AP8Q/u);
  const filtered = sandbox.inspectionContent({ status: 'ready', data: inspection }, { query: 'opaque' });
  assert.match(filtered, /Opaque/u);
  assert.doesNotMatch(filtered, /Inspect Description/u);
});

test('Aurora emitter curves, extents, and linked ribbons preserve engine semantics', () => {
  const { sandbox } = createRendererHarness();
  assert.equal(sandbox.emitterCurve(0.25, 0.5, 0, 100, 0.8, false), 0.2);
  assert.equal(sandbox.emitterCurve(0.25, 0.5, 0, 1, 0, true), 0.5);

  const emitter = {
    xSize: 1000,
    ySize: 200,
    properties: [
      { name: 'lifeexp', values: [{ kind: 'float', value: 4 }] },
      { name: 'velocity', values: [{ kind: 'float', value: 0.2 }] },
      { name: 'randvel', values: [{ kind: 'float', value: 0.1 }] },
      { name: 'mass', values: [{ kind: 'float', value: 0 }] },
      { name: 'sizestart', values: [{ kind: 'float', value: 0.5 }] },
      { name: 'sizeend', values: [{ kind: 'float', value: 0.25 }] },
    ],
  };
  assert.deepEqual(Array.from(sandbox.emitterSpatialExtent(emitter)), [6.25, 2.25, 1.25]);

  const particles = new Float32Array(30);
  particles.set([
    0, 0, 0, 0.25, 0.5, 0, 1, 1, 0.5, 0.25, 0, 0, 0.25, 0.25, 0,
    0, 0, 2, 0.125, 0.25, 0, 0.5, 0.25, 0.5, 1, 0.25, 0, 0.25, 0.25, 0,
  ]);
  const ribbon = sandbox.buildLinkedParticleVertices(particles, 2, [4, 0, 1]);
  assert.equal(ribbon.vertexCount, 6);
  assert.ok(Array.from(ribbon.values.subarray(0, ribbon.vertexCount * 9)).every(Number.isFinite));
  assert.equal(ribbon.values[8], 1);
  assert.equal(ribbon.values[2 * 9 + 8], 0.5);
  const retainedValues = ribbon.values;
  assert.equal(
    sandbox.buildLinkedParticleVertices(particles, 2, [4, 0, 1], ribbon).values,
    retainedValues,
  );
  assert.match(rendererSource, /aRenderMode>0\.5/u);
  assert.match(rendererSource, /renderMode === 'billboard_to_world_z'/u);
  assert.match(rendererSource, /renderMode === 'linked'/u);
});

test('area-object bounds remain logical across visual fragments and support ray picking', () => {
  const { sandbox } = createRendererHarness();
  const scene = {
    manifest: {
      models: [],
      areaObjects: [
        { key: 'placeable:0', position: [4, 5, 0] },
        { key: 'trigger:0', position: [10, 0, 0] },
      ],
      instances: [
        {
          objectKey: 'placeable:0', kind: 'placeable', model: null,
          position: [4, 5, 0], rotationAxisAngle: [0, 0, 1, 0], scale: [1, 1, 1], polygon: [],
        },
        {
          objectKey: 'placeable:0', kind: 'collision', model: null,
          position: [4.1, 5.2, 0], rotationAxisAngle: [0, 0, 1, 0], scale: [1, 1, 1], polygon: [],
        },
        {
          objectKey: 'trigger:0', kind: 'trigger', model: null,
          position: [10, 0, 0], rotationAxisAngle: [0, 0, 1, 0], scale: [1, 1, 1],
          polygon: [[-1, -1, 0], [1, -1, 0], [1, 1, 0], [-1, 1, 0]],
        },
      ],
    },
    binary: new Uint8Array(),
  };
  const catalog = sandbox.sceneBoundsCatalog(scene);
  const placeable = catalog.objects.get('placeable:0');
  assert.ok(placeable.min[0] < 4 && placeable.max[0] > 4.1);
  assert.ok(placeable.min[2] < 0 && placeable.max[2] > 0);
  assert.equal(
    sandbox.rayBoundsDistance([10, 0, 5], [0, 0, -1], catalog.objects.get('trigger:0')),
    4.875,
  );
  assert.equal(sandbox.rayBoundsDistance([20, 20, 5], [0, 0, -1], placeable), undefined);
  assert.equal(sandbox.boxLineVertices(placeable).length, 24);
});

test('emitter volume affects scene framing without resizing selectable placeables', () => {
  const { sandbox } = createRendererHarness();
  const scene = {
    manifest: {
      models: [{
        nodes: [{
          name: 'mist', parent: null, translation: [0, 0, 0],
          rotationAxisAngle: [0, 0, 1, 0], scale: [1, 1, 1],
          emitter: {
            xSize: 1000, ySize: 200,
            properties: [
              { name: 'lifeexp', values: [{ kind: 'float', value: 0 }] },
              { name: 'sizestart', values: [{ kind: 'float', value: 2 }] },
            ],
          },
        }],
        meshes: [], attachments: [],
      }],
      areaObjects: [{ key: 'placeable:mist', position: [10, 20, 0] }],
      instances: [{
        objectKey: 'placeable:mist', kind: 'placeable', model: 0,
        position: [10, 20, 0], rotationAxisAngle: [0, 0, 1, 0], scale: [1, 1, 1], polygon: [],
      }],
    },
    binary: new Uint8Array(),
  };
  const catalog = sandbox.sceneBoundsCatalog(scene);
  assert.deepEqual(Array.from(catalog.scene.min), [4, 18, -1]);
  assert.deepEqual(Array.from(catalog.scene.max), [16, 22, 1]);
  assert.deepEqual(Array.from(catalog.objects.get('placeable:mist').min), [9.875, 19.875, -0.125]);
  assert.deepEqual(Array.from(catalog.objects.get('placeable:mist').max), [10.125, 20.125, 0.125]);
});

test('selection geometry retains authored rotation for objects and components', () => {
  const { sandbox } = createRendererHarness();
  const scene = {
    manifest: {
      models: [],
      areaObjects: [{
        key: 'trigger:0', position: [10, 20, 0], rotationAxisAngle: [0, 0, 1, Math.PI / 2],
      }],
      instances: [{
        id: 7, objectKey: 'trigger:0', kind: 'trigger', model: null,
        position: [10, 20, 0], rotationAxisAngle: [0, 0, 1, Math.PI / 2], scale: [1, 1, 1],
        polygon: [[0, 0, 0], [2, 0, 0], [2, 1, 0], [0, 1, 0]],
      }],
    },
    binary: new Uint8Array(),
  };
  const catalog = sandbox.sceneBoundsCatalog(scene);
  const objectSelection = catalog.objectSelections.get('trigger:0');
  const componentSelection = catalog.componentSelections.get(7);
  for (const selection of [objectSelection, componentSelection]) {
    const first = selection.vertices[0]; const second = selection.vertices[1];
    assert.ok(Math.abs(second[0] - first[0]) < 1e-5);
    assert.ok(Math.abs(Math.abs(second[1] - first[1]) - 2) < 1e-5);
  }
  assert.ok(Math.abs((objectSelection.bounds.max[0] - objectSelection.bounds.min[0]) - 1) < 1e-5);
  assert.ok(Math.abs((objectSelection.bounds.max[1] - objectSelection.bounds.min[1]) - 2) < 1e-5);
});

test('WASD and QE provide elapsed-time fly-camera translation', () => {
  const { sandbox } = createRendererHarness();
  interface TestKeyEvent {
    readonly key: string;
    preventDefault(): void;
  }
  const listeners = new Map<string, (event: TestKeyEvent) => void>();
  const dispatch = (type: string, event: TestKeyEvent): void => {
    const listener = listeners.get(type);
    if (!listener) throw new Error(`Viewport listener ${type} was not registered`);
    listener(event);
  };
  let changes = 0; let draws = 0;
  const canvas = {
    addEventListener: (type: string, listener: (event: TestKeyEvent) => void) =>
      listeners.set(type, listener),
    removeEventListener: (type: string, listener: (event: TestKeyEvent) => void) => {
      if (listeners.get(type) === listener) listeners.delete(type);
    },
    setPointerCapture() {}, focus() {},
  };
  const camera = { yaw: 0, pitch: 0, distance: 10, target: [0, 0, 0] };
  const key = (value: string): TestKeyEvent => ({ key: value, preventDefault() {} });
  const controls = sandbox.bindViewportControls(canvas, camera, () => { draws += 1; }, () => { changes += 1; });
  dispatch('keydown', key('w'));
  assert.equal(controls.update(1), true);
  assert.deepEqual(camera.target.map((value) => Math.round(value * 10) / 10), [-7.5, 0, 0]);
  dispatch('keyup', key('w'));
  dispatch('keydown', key('d'));
  controls.update(1);
  dispatch('keyup', key('d'));
  dispatch('keydown', key('e'));
  controls.update(1);
  dispatch('keyup', key('e'));
  assert.deepEqual(camera.target.map((value) => Math.round(value * 10) / 10), [-7.5, 7.5, 7.5]);
  assert.equal(draws, 0, 'continuous movement is drawn by the retained viewer frame loop');
  assert.equal(changes, 3, 'camera state is persisted when each held key is released');
  controls.dispose();
  assert.equal(listeners.size, 0);
});

test('packet decoder realigns legacy binary payloads before creating typed views', () => {
  const { sandbox } = createRendererHarness();
  const manifest = {
    schema: 'nwnrs.scene',
    paddingProbe: '',
    name: 'alignment-test',
    source: 'model',
    environment: 'studio',
    module: null,
    instances: [],
    areaObjects: [],
    models: [],
    rootModels: [],
    textures: [],
    shaders: [],
    dependencies: { nodes: [], edges: [] },
    diagnostics: [],
  };
  let manifestBytes = Buffer.from(JSON.stringify(manifest));
  while ((12 + manifestBytes.length) % 4 === 0) {
    manifest.paddingProbe += 'x';
    manifestBytes = Buffer.from(JSON.stringify(manifest));
  }
  const floatBytes = Buffer.alloc(8);
  floatBytes.writeFloatLE(2.5, 0);
  floatBytes.writeFloatLE(7.5, 4);
  const packet = Buffer.alloc(12 + manifestBytes.length + floatBytes.length);
  Buffer.from('NWNRS3D\0', 'binary').copy(packet, 0);
  packet.writeUInt32LE(manifestBytes.length, 8);
  manifestBytes.copy(packet, 12);
  floatBytes.copy(packet, 12 + manifestBytes.length);

  const decoded = sandbox.decodeScenePacket(packet);
  assert.equal(decoded.binary.byteOffset % 4, 0);
  assert.deepEqual(Array.from(sandbox.numericView(decoded.binary, {
    byteOffset: 0, byteLength: 8, component: 'f32', componentsPerElement: 1,
  })), [2.5, 7.5]);
});

test('lazy animation assets retain their own binary and leave the scene catalog immutable', () => {
  const { sandbox, posted, element } = createRendererHarness();
  const gl = createWebGlHarness();
  const listeners = new Map<string, (event: unknown) => void>();
  const lightUploads: Float32Array[] = [];
  gl.texImage2D = (...args: unknown[]) => {
    const values = args.at(-1);
    if (values instanceof Float32Array) lightUploads.push(Float32Array.from(values));
  };
  const canvas = {
    clientWidth: 640, clientHeight: 480, width: 0, height: 0,
    getContext: (kind: string) => kind === 'webgl2' ? gl : null,
    addEventListener: (type: string, listener: (event: unknown) => void) =>
      listeners.set(type, listener),
    removeEventListener: (type: string) => listeners.delete(type),
  };
  const catalogAnimation = {
    name: 'walk', length: 1, transitionTime: 0, rootName: null, rootNode: 0,
    events: [], tracksLoaded: false, nodeTracks: [],
  };
  const model = {
    name: 'actor', supermodel: null, classification: null, animationScale: 1, ignoreFog: 0,
    nodes: [{
      name: 'root', kind: 'dummy', parent: null, translation: [0, 0, 0],
      rotationAxisAngle: [0, 1, 0, 0], scale: [1, 1, 1], color: [1, 1, 1],
      alpha: null, radius: 10,
      light: { multiplier: 1, shadowRadius: 0, verticalDisplacement: 0, negativeLight: false, flareRadius: 0, ambientOnly: false, affectDynamic: true, lightPriority: 0, lensFlares: false },
      emitter: null, dangly: null,
    }],
    meshes: [], materials: [], resolvedMaterials: [], nodeTextures: [], hiddenGeometryNodes: [],
    attachments: [], animations: [catalogAnimation],
  };
  const sceneBinary = new Uint8Array(32); new Float32Array(sceneBinary.buffer).fill(99);
  const scene = {
    binary: sceneBinary,
    manifest: {
      schema: 'nwnrs.scene', assetKey: 'scene:actor', name: 'actor', source: 'model', environment: 'studio',
      models: [model], rootModels: [0], textures: [], shaders: [], diagnostics: [], module: null,
      dependencies: { nodes: [], edges: [] },
      instances: [{ kind: 'model', model: 0, position: [0, 0, 0], rotationAxisAngle: [0, 1, 0, 0], scale: [1, 1, 1], polygon: [] }],
    },
  };
  const session = sandbox.createViewerSession(scene);
  const viewer = sandbox.createViewer(
    canvas,
    scene,
    createViewerElements(element),
    'model',
    session,
  );
  viewer.setAnimation(0, 0);
  assert.deepEqual(structuredClone(posted.at(-1)), { type: 'loadAnimation', assetKey: 'scene:actor', modelIndex: 0, animationIndex: 0 });

  const animationBinary = new Uint8Array(32);
  new Float32Array(animationBinary.buffer, 0, 2).set([0, 1]);
  new Float32Array(animationBinary.buffer, 8, 6).set([5, 0, 0, 10, 0, 0]);
  viewer.applyAnimation({
    binary: animationBinary,
    manifest: {
      schema: 'nwnrs.scene.animation', assetKey: 'scene:actor', modelIndex: 0, animationIndex: 0,
      animation: {
        ...catalogAnimation, tracksLoaded: true,
        nodeTracks: [{
          targetName: 'root', targetNode: 0, bezierControllers: [], emitterControllers: [], animmesh: null,
          translation: {
            times: { byteOffset: 0, byteLength: 8, component: 'f32', componentsPerElement: 1 },
            values: { byteOffset: 8, byteLength: 24, component: 'f32', componentsPerElement: 3 },
          },
          ...Object.fromEntries(['rotationAxisAngle', 'scale', 'color', 'radius', 'alpha', 'selfIllumColor', 'multiplier', 'shadowRadius', 'verticalDisplacement'].map((name) => [name, {
            times: { byteOffset: 32, byteLength: 0, component: 'f32', componentsPerElement: 1 },
            values: { byteOffset: 32, byteLength: 0, component: 'f32', componentsPerElement: name === 'rotationAxisAngle' ? 4 : ['scale', 'color', 'selfIllumColor'].includes(name) ? 3 : 1 },
          }])),
        }],
      },
    },
  });

  assert.equal(model.animations[0], catalogAnimation);
  assert.equal(model.animations[0]?.tracksLoaded, false);
  const retained = session.animationAssets.get('0:0');
  assert.equal(retained.binary, animationBinary);
  const runtime = sandbox.createModelRuntime(model);
  const installed = sandbox.installAnimationAsset(runtime, retained);
  sandbox.sampleModelPoseInto(runtime, model, installed, 0.5);
  assert.deepEqual(Array.from(runtime.pose.nodes[0].translation), [7.5, 0, 0]);
  assert.equal(lightUploads.at(-1)?.[0], 5, 'the first post-load draw reused a stale bind pose');
  viewer.dispose();
});

test('animation scope, transitions, events, Bezier timing, and chunk ordering are deterministic', () => {
  const { sandbox } = createRendererHarness();
  const animation = (name: string) => ({ name, events: [], length: 1 });
  const scene = { manifest: { models: [
    { animations: [animation('idle'), animation('idle')], attachments: [{ model: 1 }] },
    { animations: [animation('idle')], attachments: [] },
    { animations: [animation('idle')], attachments: [] },
  ] } };
  assert.deepEqual([...sandbox.animationPlaybackScope(scene, 0, 1)].map((entry) => Array.from(entry)), [[0, 1], [1, 0]]);

  const events: Array<[string, number | undefined]> = [];
  assert.equal(sandbox.dispatchAnimationEvents({
    length: 1,
    events: [{ time: 0, name: 'start' }, { time: 0.5, name: 'middle' }],
  }, -Number.EPSILON, 1.1, (event: PacketAnimationEvent) =>
    events.push([event.name, event.cycle])), 3);
  assert.deepEqual(events, [['start', 0], ['middle', 0], ['start', 1]]);

  const binary = new Uint8Array(16);
  new Float32Array(binary.buffer, 0, 2).set([0, 1]);
  new Float32Array(binary.buffer, 8, 2).set([0, 1]);
  const bezier = sandbox.preparePackedTrack(binary, {
    times: { byteOffset: 0, byteLength: 8, component: 'f32', componentsPerElement: 1 },
    values: { byteOffset: 8, byteLength: 8, component: 'f32', componentsPerElement: 1 },
  }, true);
  const sampled = new Float32Array(1);
  sandbox.samplePreparedTrackInto(bezier, 0.25, [0], sampled);
  assert.equal(Math.round((sampled[0] ?? 0) * 100000) / 100000, 0.15625);

  const target = { nodes: [{ translation: new Float32Array([10, 0, 0]), rotationAxisAngle: new Float32Array([0, 1, 0, 0]), scale: new Float32Array([1, 1, 1]), color: new Float32Array([1, 1, 1]) }], materials: [] };
  const source = { nodes: [{ translation: new Float32Array([0, 0, 0]), rotationAxisAngle: new Float32Array([0, 1, 0, 0]), scale: new Float32Array([1, 1, 1]), color: new Float32Array([1, 1, 1]) }], materials: [] };
  sandbox.blendPoseInto(target, source, 0.25, { materials: [] });
  assert.deepEqual(Array.from(target.nodes[0]?.translation ?? []), [2.5, 0, 0]);

  const drawBody = rendererSource.match(
    /function draw\(\) \{[\s\S]*?\n\s+function drawModel/u,
  )?.[0] || '';
  assert.ok(drawBody.indexOf('drawModel(instance.model') < drawBody.indexOf('drawChunkBatches(viewProjection'));
});

test('texture upload scopes vertical row flipping to color pixels', () => {
  const { sandbox } = createRendererHarness();
  const pixelStoreCalls: Array<[unknown, unknown]> = [];
  const gl = createWebGlHarness();
  gl.pixelStorei = (parameter: unknown, value: unknown) =>
    pixelStoreCalls.push([parameter, value]);
  const binary = new Uint8Array([255, 0, 0, 255]);
  const texture = {
    width: 1,
    height: 1,
    rgba8: { byteOffset: 0, byteLength: 4, component: 'u8', componentsPerElement: 4 },
  };

  sandbox.createTexture(gl, texture, binary);

  assert.deepEqual(pixelStoreCalls, [
    [gl.UNPACK_FLIP_Y_WEBGL, true],
    [gl.UNPACK_FLIP_Y_WEBGL, false],
  ]);
});

test('texture upload keeps authored compressed mip chains on the GPU', () => {
  const { sandbox } = createRendererHarness();
  const uploads: unknown[][] = []; const generated: unknown[][] = [];
  const gl = createWebGlHarness();
  gl.compressedTexImage2D = (...args: unknown[]) => uploads.push(args);
  gl.generateMipmap = (...args: unknown[]) => generated.push(args);
  const s3tc = {
    COMPRESSED_RGBA_S3TC_DXT1_EXT: 0x83f1,
    COMPRESSED_RGBA_S3TC_DXT5_EXT: 0x83f3,
  };
  const binary = new Uint8Array(40);
  const texture = {
    width: 8,
    height: 8,
    compression: 'dxt1',
    mipLevels: [
      { width: 8, height: 8, data: { byteOffset: 0, byteLength: 32, component: 'u8', componentsPerElement: 1 } },
      { width: 4, height: 4, data: { byteOffset: 32, byteLength: 8, component: 'u8', componentsPerElement: 1 } },
    ],
    rgba8: null,
  };

  sandbox.createTexture(gl, texture, binary, s3tc);

  assert.equal(uploads.length, 2);
  assert.equal(uploads[0]?.[2], s3tc.COMPRESSED_RGBA_S3TC_DXT1_EXT);
  assert.deepEqual(uploads.map((entry) => {
    const data = entry[6];
    return [entry[1], entry[3], entry[4], data instanceof Uint8Array ? data.byteLength : undefined];
  }), [
    [0, 8, 8, 32],
    [1, 4, 4, 8],
  ]);
  assert.deepEqual(generated, []);
});

test('viewer batches chunk transforms and compiles feature-specific shader paths', () => {
  const { sandbox } = createRendererHarness();
  const batch = { values: new Float32Array(16), count: 0 };
  const first = sandbox.identity4(); const second = sandbox.translation4([1, 2, 3]);
  sandbox.appendChunkInstance(batch, first); sandbox.appendChunkInstance(batch, second);
  assert.equal(batch.count, 2);
  assert.ok(batch.values.length >= 32);
  assert.deepEqual(Array.from(batch.values.slice(16, 32)), Array.from(second));
  assert.match(rendererSource, /sceneHasSkinning \? `#version 300 es/u);
  assert.match(rendererSource, /#define HAS_POINT_LIGHTS/u);
  assert.match(rendererSource, /drawArraysInstanced\(gl\.TRIANGLES, 0, gpu\.count, batch\.count\)/u);
  const overlayDraw = rendererSource.match(/function drawOverlays[\s\S]*?function drawEffects/u)?.[0] || '';
  assert.doesNotMatch(overlayDraw, /createVertexArray|createBuffer|deleteVertexArray|deleteBuffer/u);
});

test('viewer creates, draws, and disposes a directly opened standalone scene', () => {
  const { sandbox, element } = createRendererHarness();
  const gl = createWebGlHarness();
  const listeners = new Map<string, (event: unknown) => void>();
  const canvas = {
    clientWidth: 800,
    clientHeight: 600,
    width: 0,
    height: 0,
    getContext: (kind: string) => kind === 'webgl2' ? gl : null,
    addEventListener: (type: string, listener: (event: unknown) => void) =>
      listeners.set(type, listener),
    removeEventListener: (type: string) => listeners.delete(type),
  };
  const scene = {
    binary: new Uint8Array(),
    manifest: {
      source: 'model',
      environment: 'studio',
      models: [],
      textures: [],
      shaders: [],
      instances: [],
      diagnostics: [],
      dependencies: { nodes: [], edges: [] },
      module: null,
    },
  };
  const elements = createViewerElements(element);
  const viewer = sandbox.createViewer(canvas, scene, elements);
  assert.equal(elements.status.textContent, '0 models · 0 textures · 0 instances');
  assert.equal(typeof viewer.setAnimation, 'function');
  assert.doesNotThrow(() => viewer.dispose());

  const collisionViewer = sandbox.createViewer(canvas, {
    ...scene,
    manifest: { ...scene.manifest, source: 'walkmesh' },
  }, elements, 'collision');
  assert.doesNotThrow(() => collisionViewer.dispose());
});

test('installed start area completes a full renderer draw with every resolved emitter', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, async (context: TestContext) => {
  const moduleRoot = path.join(repositoryRoot, 'module');
  if (!fs.existsSync(path.join(moduleRoot, 'start.are.json'))) {
    context.skip('repository authored-area fixture is unavailable');
    return;
  }
  const binding = require(bindingPath);
  const service = new binding.ViewerService();
  let packet;
  try {
    packet = Buffer.from(await service.loadScene(JSON.stringify({
      session_key: path.join(moduleRoot, 'nwpkg.toml'),
      path: path.join(moduleRoot, 'start.are'),
      project_root: moduleRoot,
      area: null,
      root: null,
      user: null,
      language: 'english',
      load_ovr: false,
      archives: [],
      include_project_resources: true,
      authored_area: {
        resref: 'start',
        are: path.join(moduleRoot, 'start.are.json'),
        git: path.join(moduleRoot, 'start.git.json'),
        gic: path.join(moduleRoot, 'start.gic.json'),
      },
    })));
  } catch (error) {
    if (/installation|root|language directory/iu.test(String(error))) {
      context.skip('Neverwinter Nights installation was not discovered');
      return;
    }
    throw error;
  }
  const { sandbox, element } = createRendererHarness();
  const scene = sandbox.decodeScenePacket(packet);
  const gl = createWebGlHarness();
  const canvas = {
    clientWidth: 800, clientHeight: 600, width: 0, height: 0,
    getContext: (kind: string) => kind === 'webgl2' ? gl : null,
    addEventListener() {}, removeEventListener() {},
  };
  const elements = createViewerElements(element);
  const viewer = sandbox.createViewer(canvas, scene, elements);
  assert.match(elements.status.textContent, /models/u);
  assert.doesNotThrow(() => viewer.dispose());
});

test('viewer chrome uses a resizable single-context inspector with responsive presentation', () => {
  const { sandbox } = createRendererHarness();
  const scene = {
    manifest: {
      source: 'model',
      models: [{
        name: 'cat', animations: [{ name: 'idle' }], nodes: [], meshes: [], materials: [],
        resolvedMaterials: [], nodeTextures: [],
      }],
      textures: [], shaders: [], instances: [], diagnostics: [], environment: 'studio',
      dependencies: {
        nodes: [{ id: 1, resource: 'cat.tga', kind: 'texture', state: 'resolved', origin: '/game/cat.tga' }],
        edges: [{ to: 1, relationship: 'diffuse' }],
      },
    },
  };
  const animations = sandbox.viewerAnimations(scene);
  assert.equal(animations.length, 1);
  assert.equal(animations[0].name, 'idle');
  assert.match(sandbox.sceneInspectorContent(scene), /Overview/u);
  assert.match(sandbox.sceneInspectorContent(scene), /Source/u);
  assert.match(sandbox.dependenciesInspectorContent(scene), /cat\.tga/u);
  assert.doesNotMatch(sandbox.dependenciesInspectorContent(scene, 'missing-value'), /cat\.tga/u);
  assert.doesNotMatch(rendererSource, /viewer-overlay-stack|viewer-disclosure/u);
  assert.match(rendererSource, /id="viewer-inspector-scope"/u);
  assert.match(rendererSource, /id="viewer-inspector-sash"/u);
  assert.match(rendererSource, /id="viewer-inspector-search"/u);
  assert.match(rendererSource, /data-inspector-collapsed/u);
  assert.match(rendererSource, /id="viewer-animation"/u);
  assert.match(rendererStyle, /grid-template-columns: minmax\(320px, 1fr\) 5px var\(--viewer-inspector-width\)/u);
  assert.match(rendererStyle, /@media \(max-width: 760px\)/u);
  assert.match(rendererStyle, /position: absolute; z-index: 6/u);
});

test('inspector navigation and persisted-state helpers retain bounded, meaningful state', () => {
  const { sandbox } = createRendererHarness();
  assert.equal(sandbox.validInspectorWidth(340), true);
  assert.equal(sandbox.validInspectorWidth(720), true);
  assert.equal(sandbox.validInspectorWidth(339), false);
  assert.equal(sandbox.validInspectorWidth(Number.NaN), false);
  assert.deepEqual(
    { ...sandbox.parentInspectorRoute({ page: 'field', root: 'source', rootIndex: 2, trail: [{ kind: 'field', index: 4 }] }) },
    { page: 'raw-source', sourceIndex: 2 },
  );
  assert.deepEqual(
    { ...sandbox.parentInspectorRoute({ page: 'raw-source', sourceIndex: 2 }) },
    { page: 'raw-sources' },
  );
  const state = new Map(Array.from({ length: 40 }, (_, index) => [`object:${index}`, index]));
  const bounded = sandbox.boundedStateEntries(state);
  assert.equal(Object.keys(bounded).length, 32);
  assert.equal(bounded['object:0'], undefined);
  assert.equal(bounded['object:39'], 39);
});

test('selected-object pages avoid duplicate summaries and drill into rendered components', () => {
  const { sandbox } = createRendererHarness();
  const scene = {
    manifest: {
      areaObjects: [{
        key: 'placeable:2', label: 'Mist', kind: 'placeable', sourceIndex: 2,
        tag: 'MIST_TAG', templateResref: 'x3_plc_mist', position: [4, 5, 6],
        rotationAxisAngle: [0, 0, 1, Math.PI / 2],
      }],
      models: [{ name: 'tnp_gmist' }],
      instances: [{
        id: 8, objectKey: 'placeable:2', label: 'Mist visual', kind: 'placeable', model: 0,
        resource: 'tnp_gmist.mdl',
        position: [4, 5, 6], scale: [1, 1, 1],
      }, {
        id: 9, objectKey: 'placeable:2', label: 'Mist collision', kind: 'collision', model: null,
        resource: 'tnp_gmist.pwk',
        position: [4, 5, 6], scale: [1, 1, 1],
      }],
    },
  };
  assert.equal(sandbox.blueprintResourceForObject(scene.manifest.areaObjects[0]), 'x3_plc_mist.utp');
  const content = sandbox.inspectionComponentsPage(scene, 'placeable:2', 8, '');
  assert.match(content, /tnp_gmist\.mdl/u);
  assert.match(content, /tnp_gmist\.pwk/u);
  assert.match(content, /data-component-id="8"/u);
  assert.match(content, /selected-component selected/u);
  assert.match(content, /component-select/u);
  assert.match(content, /component-open/u);
  assert.match(content, /Mist visual/u);
  assert.match(content, /Mist collision/u);
  assert.doesNotMatch(content, /GIT index|Rotation angle|Blueprint/u);
  assert.match(sandbox.animationControl([{ label: 'walk' }]), /id="viewer-animation"/u);
});

test('NCS workbench renders structured navigation without editable or export controls', () => {
  const { sandbox, listeners, content } = createRendererHarness();
  dispatchRendererMessage(listeners, { type: 'snapshot', snapshot: {
    path: '/virtual/demo.ncs', kind: 'ncs', revision: 0, data: {
      primary: 'ncs',
      hasNcs: true, hasNdb: true, hasLangspec: true,
      header: {
        format: 'NCS V1.0',
        fileSize: 23,
        declaredSize: 23,
        codeSize: 10,
        instructionCount: 1,
      },
      summary: {
        files: 1,
        structs: 0,
        functions: 1,
        variables: 0,
        lineMappings: 1,
        structEntries: [],
        variableEntries: [],
      },
      sourceFiles: [{ name: 'demo', isRoot: true, available: true }],
      functions: [{
        index: 0, name: 'main', start: 0, end: 10, returnType: 'v', arguments: [], synthetic: false,
        blocks: [{ id: 'f0b0', start: 0, end: 10, instructionIndices: [0], successors: [], calls: [] }],
      }],
      instructions: [{
        index: 0, offset: 0, localOffset: 0, size: 2, label: '', opcode: 'RET',
        opcodeInternal: 'RET', auxcode: 'NONE', auxcodeInternal: 'NONE', operand: '',
        action: null, rawHex: '20 00', jumpTarget: null, callTarget: null, successors: [],
        functionIndex: 0,
        source: { file: 'demo', line: 1, text: 'void main() {}', available: true },
      }],
      diagnostics: [],
    },
  } });
  assert.match(content.innerHTML, /ncs-workbench/u);
  assert.match(content.innerHTML, /Control Flow/u);
  assert.match(content.innerHTML, /Functions/u);
  assert.doesNotMatch(content.innerHTML, /Export|Save|textarea/u);
  assert.match(sandbox.scriptDebugOutline({ functions: [], sourceFiles: [], summary: {} }), /No function information/u);
});

test('authoritative installed resources open through the worker and render their custom view', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, async (context: TestContext) => {
  const resourceClient = new ResourceEditorWorkerClient(
    path.join(extensionRoot, 'dist', 'src', 'resource-editor-worker.js'),
    bindingPath,
    { appendLine() {} },
  );
  const viewerClient = new ViewerWorkerClient(
    path.join(extensionRoot, 'dist', 'src', 'viewer-worker.js'),
    bindingPath,
    { appendLine() {} },
  );
  const moduleRoot = path.join(repositoryRoot, 'module');
  const request = {
    session_key: path.join(moduleRoot, 'nwpkg.toml'),
    path: path.join(moduleRoot, '.nwnrs-resource-catalog'),
    project_root: moduleRoot,
    area: null,
    root: null,
    user: null,
    language: 'english',
    load_ovr: false,
    archives: [],
    include_project_resources: true,
  };
  try {
    for (const [index, [name, expectedKind]] of installedAcceptanceResources.entries()) {
      const resourceRequest = { ...request, path: path.join(moduleRoot, name) };
      const resolved = await viewerClient.resolveResource(resourceRequest);
      assert.equal(resolved.resource, name);
      assert.equal(resolved.file_path, null);
      assert.match(resolved.origin, /KeyTable:/u, name);
      const contents: Uint8Array = await viewerClient.readResource(resourceRequest);
      const documentId = `installed-acceptance-${index}`;
      let opened = false;
      try {
        const snapshot = await resourceClient.request('openDocumentBytes', {
          documentId,
          path: `/${name}`,
          contents: Buffer.from(contents).toString('base64'),
        });
        opened = true;
        assert.equal(snapshot.kind, expectedKind, name);

        const harness = createRendererHarness();
        assert.equal(harness.posted.length, 1, name);
        const readyMessage = harness.posted[0];
        assert.ok(isPostedMessage(readyMessage), name);
        assert.equal(isPostedMessage(readyMessage) ? readyMessage.type : undefined, 'ready', name);
        assert.doesNotThrow(
          () => dispatchRendererMessage(
            harness.listeners,
            { type: 'snapshot', snapshot },
          ),
          name,
        );
        assert.match(harness.app.innerHTML, /class="shell"/u, name);
        assert.ok(harness.content.innerHTML.length > 0, name);
        assert.equal(
          harness.posted.some(
            (message) => isPostedMessage(message) && message.type === 'showError',
          ),
          false,
          name,
        );
      } finally {
        if (opened) await resourceClient.request('closeDocument', { documentId });
      }
    }
  } catch (error) {
    if (/installation|root|language directory/iu.test(String(error))) {
      context.skip('Neverwinter Nights installation was not discovered');
      return;
    }
    throw error;
  } finally {
    resourceClient.dispose();
    viewerClient.dispose();
  }
});
