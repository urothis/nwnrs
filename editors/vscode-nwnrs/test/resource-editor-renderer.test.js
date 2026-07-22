'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const test = require('node:test');
const vm = require('node:vm');
const { ResourceEditorWorkerClient } = require('../src/resource-editor-worker-client');

const extensionRoot = path.resolve(__dirname, '..');
const repositoryRoot = path.resolve(extensionRoot, '..', '..');
const bindingPath = path.join(extensionRoot, 'native', 'nwnrs-vscode.darwin-arm64.node');
const rendererSource = fs.readFileSync(
  path.join(extensionRoot, 'media', 'resource-editor.js'),
  'utf8',
);

function createRendererHarness() {
  const elements = new Map();
  const listeners = new Map();
  const posted = [];
  let persistedState;
  const element = (id) => {
    if (!elements.has(id)) {
      elements.set(id, {
        id,
        innerHTML: '',
        value: '',
        dataset: {},
        type: '',
        getContext() { return {}; },
      });
    }
    return elements.get(id);
  };
  element('app').innerHTML = '<div class="loading">Loading resource…</div>';
  const sandbox = {
    acquireVsCodeApi: () => ({
      postMessage: (message) => posted.push(message),
      getState: () => persistedState,
      setState: (value) => { persistedState = value; },
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
      addEventListener(type, listener) {
        listeners.set(type, listener);
      },
    },
  };
  vm.runInNewContext(rendererSource, sandbox, { filename: 'resource-editor.js' });
  return { app: element('app'), content: element('content'), listeners, posted, sandbox, getPersistedState: () => persistedState };
}

function createWebGlHarness() {
  let constant = 1;
  const values = {
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
  assert.deepEqual(Array.from(sandbox.samplePackedTrack(binary, track, 0.25, [9, 9, 9])), [0.5, 1, 1.5]);
  const transform = sandbox.multiply4(sandbox.translation4([2, 3, 4]), sandbox.scale4([2, 4, 8]));
  const product = sandbox.multiply4(transform, sandbox.inverse4(transform));
  assert.deepEqual(Array.from(product).map((value) => Math.round(value * 1000) / 1000), Array.from(sandbox.identity4()));
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
  assert.equal(sandbox.sampleEmitterValue(emitterBinary, emitterTrack, 'velocity', 0.25, 0), 2.5);
  assert.deepEqual(Array.from(sandbox.lightOverrideForNode('mainlight2', [[1, 0, 0], [0, 1, 0]], 0)), [0, 1, 0]);
  const globalLight = sandbox.globalIllumination();
  assert.deepEqual(Array.from(globalLight.environmentLight), [1, 1, 1]);
  assert.equal(globalLight.fogEnabled, false);
  assert.deepEqual(Array.from(globalLight.background), [0.035, 0.045, 0.06]);
  const areaLight = sandbox.globalIllumination({ isNight: true, moonAmbientColor: 0x00102030, moonDiffuseColor: 0x00405060, moonFogColor: 0x00708090, fogClipDistance: 72 });
  assert.equal(areaLight.fogEnabled, true);
  assert.equal(areaLight.fogEnd, 72);
  assert.notDeepEqual(Array.from(areaLight.environmentLight), Array.from(globalLight.environmentLight));
  assert.ok(areaLight.environmentLight.every((value) => Number.isFinite(value) && value >= 0.35 && value <= 1.15));
  assert.doesNotMatch(rendererSource, /viewer-lighting|setLighting|uKeyDirection|uFillDirection|uRimColor|keyDiffuse|fillDiffuse/u);
  assert.match(rendererSource, /vec3 lit=base\.rgb\*uEnvironmentLight\*uMaterialAmbient\+emissive/u);
  assert.notDeepEqual(Array.from(sandbox.surfaceColor(2)), Array.from(sandbox.surfaceColor(6)));

  const animMeshBinary = new Uint8Array(20);
  new Float32Array(animMeshBinary.buffer, 0, 3).set([2, 4, 6]);
  new Float32Array(animMeshBinary.buffer, 12, 2).set([0.25, 0.75]);
  const sourceVertex = new Float32Array(19);
  sourceVertex.set([0.2, 0.4, 0.6], 16);
  const animatedVertex = sandbox.updateAnimMesh({
    vertices: sourceVertex,
    indices: new Uint32Array([0]),
    uvIndices: new Uint32Array([0]),
    stride: 19,
  }, {
    vertexFrameCount: 1,
    verticesPerFrame: 1,
    vertexSamples: { byteOffset: 0, byteLength: 12, component: 'f32', componentsPerElement: 3 },
    uvFrameCount: 1,
    uvsPerFrame: 1,
    uvSamples: { byteOffset: 12, byteLength: 8, component: 'f32', componentsPerElement: 2 },
    samplePeriod: 1,
  }, 0, 1, animMeshBinary);
  assert.deepEqual(Array.from(animatedVertex.slice(0, 3)), [2, 4, 6]);
  assert.deepEqual(Array.from(animatedVertex.slice(6, 8)), [0.25, 0.75]);
  assert.deepEqual(
    Array.from(animatedVertex.slice(16, 19)).map((value) => Math.round(value * 10) / 10),
    [0.2, 0.4, 0.6],
  );
  listeners.get('message')({ data: { type: 'snapshot', snapshot: { kind: 'unknown', path: 'done' } } });
});

test('packet decoder realigns legacy binary payloads before creating typed views', () => {
  const { sandbox } = createRendererHarness();
  const manifest = { schema: 'nwnrs.scene.animation', paddingProbe: '' };
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
  const { sandbox, posted } = createRendererHarness();
  const gl = createWebGlHarness(); const listeners = new Map(); const lightUploads = [];
  gl.texImage2D = (...args) => {
    const values = args.at(-1);
    if (values instanceof Float32Array) lightUploads.push(Float32Array.from(values));
  };
  const canvas = {
    clientWidth: 640, clientHeight: 480, width: 0, height: 0,
    getContext: (kind) => kind === 'webgl2' ? gl : null,
    addEventListener: (type, listener) => listeners.set(type, listener),
    removeEventListener: (type) => listeners.delete(type),
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
  const viewer = sandbox.createViewer(canvas, scene, { status: {}, animationTime: {}, animationEvent: {} }, 'model', session);
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
  assert.equal(model.animations[0].tracksLoaded, false);
  const retained = session.animationAssets.get('0:0');
  assert.equal(retained.binary, animationBinary);
  const runtime = sandbox.createModelRuntime(model);
  const installed = sandbox.installAnimationAsset(runtime, retained);
  sandbox.sampleModelPoseInto(runtime, model, installed, 0.5);
  assert.deepEqual(Array.from(runtime.pose.nodes[0].translation), [7.5, 0, 0]);
  assert.equal(lightUploads.at(-1)[0], 5, 'the first post-load draw reused a stale bind pose');
  viewer.dispose();
});

test('animation scope, transitions, events, Bezier timing, and chunk ordering are deterministic', () => {
  const { sandbox } = createRendererHarness();
  const animation = (name) => ({ name, events: [], length: 1 });
  const scene = { manifest: { models: [
    { animations: [animation('idle'), animation('idle')], attachments: [{ model: 1 }] },
    { animations: [animation('idle')], attachments: [] },
    { animations: [animation('idle')], attachments: [] },
  ] } };
  assert.deepEqual([...sandbox.animationPlaybackScope(scene, 0, 1)].map((entry) => Array.from(entry)), [[0, 1], [1, 0]]);

  const events = [];
  assert.equal(sandbox.dispatchAnimationEvents({
    length: 1,
    events: [{ time: 0, name: 'start' }, { time: 0.5, name: 'middle' }],
  }, -Number.EPSILON, 1.1, (event) => events.push([event.name, event.cycle])), 3);
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
  assert.equal(Math.round(sampled[0] * 100000) / 100000, 0.15625);

  const target = { nodes: [{ translation: new Float32Array([10, 0, 0]), rotationAxisAngle: new Float32Array([0, 1, 0, 0]), scale: new Float32Array([1, 1, 1]), color: new Float32Array([1, 1, 1]) }], materials: [] };
  const source = { nodes: [{ translation: new Float32Array([0, 0, 0]), rotationAxisAngle: new Float32Array([0, 1, 0, 0]), scale: new Float32Array([1, 1, 1]), color: new Float32Array([1, 1, 1]) }], materials: [] };
  sandbox.blendPoseInto(target, source, 0.25, { materials: [] });
  assert.deepEqual(Array.from(target.nodes[0].translation), [2.5, 0, 0]);

  const drawBody = rendererSource.match(/function draw\(\) \{[\s\S]*?\n  function drawModel/u)?.[0] || '';
  assert.ok(drawBody.indexOf('drawModel(instance.model') < drawBody.indexOf('drawChunkBatches(viewProjection'));
});

test('texture upload scopes vertical row flipping to color pixels', () => {
  const { sandbox } = createRendererHarness();
  const pixelStoreCalls = [];
  const gl = createWebGlHarness();
  gl.pixelStorei = (parameter, value) => pixelStoreCalls.push([parameter, value]);
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
  const uploads = []; const generated = [];
  const gl = createWebGlHarness();
  gl.compressedTexImage2D = (...args) => uploads.push(args);
  gl.generateMipmap = (...args) => generated.push(args);
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
  assert.equal(uploads[0][2], s3tc.COMPRESSED_RGBA_S3TC_DXT1_EXT);
  assert.deepEqual(uploads.map((entry) => [entry[1], entry[3], entry[4], entry[6].byteLength]), [
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
  const { sandbox } = createRendererHarness();
  const gl = createWebGlHarness();
  const listeners = new Map();
  const canvas = {
    clientWidth: 800,
    clientHeight: 600,
    width: 0,
    height: 0,
    getContext: (kind) => kind === 'webgl2' ? gl : null,
    addEventListener: (type, listener) => listeners.set(type, listener),
    removeEventListener: (type) => listeners.delete(type),
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
  const elements = {
    status: { textContent: '' },
    animationTime: { textContent: '' },
  };
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

test('viewer chrome uses an animation selector and collapsed in-viewport disclosures', () => {
  const { sandbox } = createRendererHarness();
  const scene = {
    manifest: {
      source: 'model',
      models: [{
        name: 'cat', animations: [{ name: 'idle' }], nodes: [], meshes: [], materials: [],
        resolvedMaterials: [], nodeTextures: [],
      }],
      textures: [], shaders: [], instances: [], diagnostics: [], environment: null,
      dependencies: {
        nodes: [{ id: 1, resource: 'cat.tga', kind: 'texture', state: 'resolved', origin: '/game/cat.tga' }],
        edges: [{ to: 1, relationship: 'diffuse' }],
      },
    },
  };
  const animations = sandbox.viewerAnimations(scene);
  assert.equal(animations.length, 1);
  assert.equal(animations[0].name, 'idle');
  assert.match(sandbox.sceneDisclosure(scene), /^<details id="viewer-scene-data" class="viewer-disclosure">/u);
  assert.match(sandbox.dependenciesDisclosure(scene), /<span>Dependencies<\/span><small>1<\/small>/u);
  assert.doesNotMatch(sandbox.dependenciesDisclosure(scene), /cat\.tga/u);
  assert.match(sandbox.dependenciesDisclosureContent(scene), /cat\.tga/u);
  assert.match(sandbox.sceneDisclosureContent(scene), /<dt>Source<\/dt>/u);
  assert.doesNotMatch(sandbox.sceneDisclosure(scene), /^<details[^>]+ open/u);
  assert.doesNotMatch(sandbox.dependenciesDisclosure(scene), /^<details[^>]+ open/u);
  assert.doesNotMatch(rendererSource, /viewer-modes|viewer-mode|viewer-inspector|viewer-fit|viewer-reload|reloadScene/u);
  assert.match(rendererSource, /id="viewer-animation"/u);
  assert.match(rendererSource, /viewer-overlay-stack/u);
});

test('all root acceptance resources open through the worker and render their custom view', {
  skip: !fs.existsSync(bindingPath) && 'run npm run build-native first',
}, async () => {
  const fixtures = [
    ['cloakmodel.2da', '2da'],
    ['nwnrs.mod', 'erf'],
    ['quickchat.gff', 'gff'],
    ['voiceset.gff', 'gff'],
  ];
  for (const [name] of fixtures) {
    assert.equal(fs.existsSync(path.join(repositoryRoot, name)), true, `${name} is missing`);
  }

  const client = new ResourceEditorWorkerClient(
    path.join(extensionRoot, 'src', 'resource-editor-worker.js'),
    bindingPath,
    { appendLine() {} },
  );
  try {
    for (const [index, [name, expectedKind]] of fixtures.entries()) {
      const documentId = `root-acceptance-${index}`;
      const snapshot = await client.request('openDocument', {
        documentId,
        path: path.join(repositoryRoot, name),
      });
      assert.equal(snapshot.kind, expectedKind, name);

      const harness = createRendererHarness();
      assert.equal(harness.posted.length, 1, name);
      assert.equal(harness.posted[0].type, 'ready', name);
      assert.doesNotThrow(
        () => harness.listeners.get('message')({ data: { type: 'snapshot', snapshot } }),
        name,
      );
      assert.match(harness.app.innerHTML, /class="shell"/u, name);
      assert.ok(harness.content.innerHTML.length > 0, name);
      assert.equal(
        harness.posted.some((message) => message.type === 'showError'),
        false,
        name,
      );
      await client.request('closeDocument', { documentId });
    }
  } finally {
    client.dispose();
  }
});
