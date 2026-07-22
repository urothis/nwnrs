'use strict';

const EMPTY_OBJECT = Object.freeze({});
const IDENTITY_MATRIX = new Float32Array([1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1]);
const WHITE_COLOR = new Float32Array([1, 1, 1]);
const ZERO_COLOR = new Float32Array(3);
const DEFAULT_DIFFUSE = new Float32Array([0.72, 0.75, 0.8]);
const EMITTER_PROPERTY_CACHE = new WeakMap();
const EMITTER_VECTOR_CACHE = new WeakMap();
const EMITTER_TRACK_CACHE = new WeakMap();
const DIRECTIVE_CACHE = new WeakMap();

const vscode = acquireVsCodeApi();
const app = document.getElementById('app');
let model;
let tablePage = 0;
let tlkOffset = 0;
let tlkQuery = '';
const tablePageSize = 200;
let loadingTimer;
let fatalErrorReported = false;
let viewer;
let viewerSession;

window.addEventListener('message', (event) => {
  try {
    if (event.data?.type === 'snapshot') {
      clearTimeout(loadingTimer);
      model = event.data.snapshot;
      render();
    } else if (event.data?.type === 'scene') {
      clearTimeout(loadingTimer);
      model = { kind: 'viewer', path: '3D Scene' };
      viewerSession = createViewerSession(decodeScenePacket(event.data.packet));
      renderViewer(viewerSession);
    } else if (event.data?.type === 'animationAsset') {
      viewer?.applyAnimation(decodeScenePacket(event.data.packet));
    } else if (event.data?.type === 'textureAsset') {
      viewer?.applyTexture(decodeScenePacket(event.data.packet));
    } else if (event.data?.type === 'textureFile') {
      void importTexture(event.data.contents).catch(reportFatalError);
    }
  } catch (error) {
    reportFatalError(error);
  }
});
window.addEventListener('error', (event) => reportFatalError(event.error || event.message));
window.addEventListener('unhandledrejection', (event) => reportFatalError(event.reason));
vscode.postMessage({ type: 'ready' });
loadingTimer = setTimeout(
  () => reportFatalError(new Error('Timed out waiting for the resource snapshot.')),
  10_000,
);

function reportFatalError(error) {
  clearTimeout(loadingTimer);
  const message = error instanceof Error ? error.message : String(error || 'Unknown error');
  app.innerHTML = `<div class="empty status-error"><strong>Could not open this resource.</strong><br>${escapeHtml(message)}</div>`;
  if (!fatalErrorReported) {
    fatalErrorReported = true;
    vscode.postMessage({ type: 'showError', message });
  }
}

function render() {
  if (!model) return;
  const title = escapeHtml(model.path?.split(/[\\/]/u).pop() || 'NWN resource');
  app.innerHTML = `<section class="shell">
    <header class="titlebar"><h1>${title}</h1><span class="badge">${escapeHtml(model.kind.toUpperCase())}</span>
    ${model.readOnlyOrigin ? '<span class="badge">Save as Override</span>' : ''}</header>
    <div id="toolbar" class="toolbar"></div><div id="content" class="content"></div></section>`;
  const renderers = {
    gff: renderGff,
    '2da': renderTwoDa,
    tlk: renderTlk,
    dds: renderTexture,
    tga: renderTexture,
    plt: renderTexture,
    erf: renderArchive,
    key: renderArchive,
  };
  (renderers[model.kind] || renderUnsupported)();
}

function decodeScenePacket(packetValue) {
  const packet = packetValue instanceof Uint8Array
    ? packetValue
    : new Uint8Array(packetValue);
  const expected = [78, 87, 78, 82, 83, 51, 68, 0];
  if (packet.length < 12 || !expected.every((value, index) => packet[index] === value)) {
    throw new Error('The native viewer returned an invalid scene packet.');
  }
  const view = new DataView(packet.buffer, packet.byteOffset, packet.byteLength);
  const manifestLength = view.getUint32(8, true);
  const manifestStart = 12;
  const binaryStart = manifestStart + manifestLength;
  if (binaryStart > packet.length) throw new Error('The scene packet manifest is truncated.');
  const manifest = JSON.parse(new TextDecoder().decode(packet.subarray(manifestStart, binaryStart)));
  const packedBinary = packet.subarray(binaryStart);
  // Current packet encoders align this segment to four bytes. Retain support
  // for packets produced by older native bindings and Uint8Array views whose
  // containing buffer starts at an odd offset by normalizing only when needed.
  const binary = packedBinary.byteOffset % 4 === 0
    ? packedBinary
    : Uint8Array.from(packedBinary);
  return { manifest, binary };
}

function createViewerSession(scene) {
  return { scene, animationAssets: new Map(), textureAssets: new Map() };
}

function renderViewer(session) {
  viewer?.dispose();
  viewerSession = session;
  const { scene } = session;
  const initialMode = ['walkmesh', 'doorWalkmesh', 'placeableWalkmesh'].includes(scene.manifest.source)
    ? 'collision'
    : 'model';
  const animations = viewerAnimations(scene);
  app.innerHTML = `<section class="viewer-shell">
    <header class="viewer-toolbar">
      <strong>${escapeHtml(scene.manifest.name)}</strong>
      <span class="spacer"></span>
      ${scene.manifest.module ? `<label>Area <select id="viewer-area">${scene.manifest.module.areas.map((area) => `<option ${area.toLowerCase() === scene.manifest.module.entryArea.toLowerCase() ? 'selected' : ''}>${escapeHtml(area)}</option>`).join('')}</select></label>` : ''}
      <label class="viewer-animation-control">Animation <select id="viewer-animation"><option value="">None</option>${animations.map((entry, index) => `<option value="${index}">${escapeHtml(entry.label)}</option>`).join('')}</select></label>
      <span id="viewer-animation-time" class="viewer-animation-time" aria-live="off"></span>
      <span id="viewer-animation-event" class="viewer-animation-time" aria-live="polite"></span>
    </header>
    <div class="viewer-body"><div class="viewer-viewport"><canvas id="viewer-canvas" tabindex="0" aria-label="Interactive nwnrs 3D viewport"></canvas>
      <aside class="viewer-overlay-stack" aria-label="Scene information">${sceneDisclosure(scene)}${dependenciesDisclosure(scene)}</aside>
      <div id="viewer-status" class="viewer-status" role="status"></div>
    </div></div>
  </section>`;
  viewer = createViewer(document.getElementById('viewer-canvas'), scene, {
    status: document.getElementById('viewer-status'),
    animationTime: document.getElementById('viewer-animation-time'),
    animationEvent: document.getElementById('viewer-animation-event'),
  }, initialMode, session);
  document.getElementById('viewer-animation').onchange = (event) => {
    const entry = event.target.value === '' ? undefined : animations[Number(event.target.value)];
    viewer.setAnimation(entry?.modelIndex, entry?.animationIndex);
  };
  const savedViewer = vscode.getState?.()?.viewer;
  if (savedViewer?.scene === viewerStateKey(scene)) {
    const selection = savedViewer.animationSelection;
    const savedIndex = selection
      ? animations.findIndex((entry) => entry.modelIndex === selection.modelIndex && entry.animationIndex === selection.animationIndex)
      : savedViewer.animationName
        ? animations.findIndex((entry) => entry.name.toLowerCase() === savedViewer.animationName)
        : -1;
    if (savedIndex >= 0) {
      document.getElementById('viewer-animation').value = String(savedIndex);
      viewer.setAnimation(animations[savedIndex].modelIndex, animations[savedIndex].animationIndex);
    }
  }
  bindLazyDisclosure('viewer-scene-data', () => sceneDisclosureContent(scene));
  bindLazyDisclosure('viewer-dependencies', () => dependenciesDisclosureContent(scene), () => {
    document.querySelectorAll('.dependency:not(:disabled)').forEach((button) => {
      button.onclick = () => vscode.postMessage({ type: 'openDependency', resource: button.dataset.resource });
    });
  });
  const area = document.getElementById('viewer-area');
  if (area) area.onchange = () => {
    viewer.dispose();
    app.innerHTML = '<div class="loading">Loading area…</div>';
    vscode.postMessage({ type: 'selectArea', area: area.value });
  };
}

function bindLazyDisclosure(id, renderContent, afterRender) {
  const disclosure = document.getElementById(id);
  disclosure.ontoggle = () => {
    if (!disclosure.open || disclosure.dataset.loaded === 'true') return;
    disclosure.querySelector('.viewer-disclosure-content').innerHTML = renderContent();
    disclosure.dataset.loaded = 'true'; afterRender?.();
  };
}

function viewerAnimations(scene) {
  const animatedModels = scene.manifest.models.filter((entry) => entry.animations.length > 0);
  return animatedModels.flatMap((model) => model.animations.map((animation, animationIndex) => ({
    modelIndex: scene.manifest.models.indexOf(model),
    animationIndex,
    name: animation.name,
    label: animatedModels.length > 1 ? `${model.name} — ${animation.name}` : animation.name,
  })));
}

function animationPlaybackScope(scene, modelIndex, animationIndex) {
  const selected = scene.manifest.models[modelIndex]?.animations[animationIndex];
  if (!selected) return new Map();
  const normalizedName = selected.name.toLowerCase(); const scope = new Map([[modelIndex, animationIndex]]); const visited = new Set();
  const visit = (candidateModel) => {
    if (visited.has(candidateModel)) return; visited.add(candidateModel);
    const model = scene.manifest.models[candidateModel]; if (!model) return;
    if (candidateModel !== modelIndex) {
      const match = model.animations.findIndex((animation) => animation.name.toLowerCase() === normalizedName);
      if (match >= 0) scope.set(candidateModel, match);
    }
    for (const attachment of model.attachments || []) visit(attachment.model);
  };
  visit(modelIndex);
  return scope;
}

function dispatchAnimationEvents(animation, previousElapsed, elapsed, dispatch) {
  if (!animation?.events?.length || elapsed < previousElapsed) return 0;
  const events = [...animation.events].sort((left, right) => left.time - right.time); let count = 0;
  if (!(animation.length > 0)) {
    for (const event of events) if (event.time > previousElapsed && event.time <= elapsed) { dispatch(event); count += 1; }
    return count;
  }
  let firstCycle = Math.max(0, Math.floor(Math.max(0, previousElapsed) / animation.length));
  const lastCycle = Math.max(firstCycle, Math.floor(Math.max(0, elapsed) / animation.length));
  // A suspended webview must resume at the current state instead of replaying
  // an unbounded backlog of historical sound/effect cues.
  firstCycle = Math.max(firstCycle, lastCycle - 31);
  for (let cycle = firstCycle; cycle <= lastCycle; cycle += 1) for (const event of events) {
    const absoluteTime = cycle * animation.length + event.time;
    if (absoluteTime > previousElapsed && absoluteTime <= elapsed) {
      dispatch({ ...event, cycle, absoluteTime }); count += 1;
    }
  }
  return count;
}

function viewerStateKey(scene) {
  return `${scene.manifest.source}:${scene.manifest.name || ''}`;
}

function validViewerCamera(camera) {
  return camera && Number.isFinite(camera.yaw) && Number.isFinite(camera.pitch)
    && Number.isFinite(camera.distance) && camera.distance > 0
    && Array.isArray(camera.target) && camera.target.length === 3 && camera.target.every(Number.isFinite);
}

function sceneDisclosure(scene) {
  return `<details id="viewer-scene-data" class="viewer-disclosure"><summary><span>Scene Data</span><small>${scene.manifest.models.length} models · ${scene.manifest.textures.length} textures</small></summary><div class="viewer-disclosure-content"></div></details>`;
}

function sceneDisclosureContent(scene) {
  const environment = scene.manifest.environment?.nwn; const diagnostics = scene.manifest.diagnostics;
  const collisionCount = scene.manifest.instances.filter((entry) => entry.kind === 'collision').length;
  const modelDetails = scene.manifest.models.map((model) => `<details class="viewer-nested-details"><summary>${escapeHtml(model.name)} · ${model.nodes.length} nodes · ${model.meshes.length} meshes · ${model.materials.length} materials</summary>
    <div class="viewer-detail-section"><strong>Nodes</strong>${model.nodes.map((node) => `<div class="node-row" style="padding-left:${Math.max(0, nodeDepth(model, node) * 10)}px"><span>${escapeHtml(node.name)}</span><small>${escapeHtml(node.kind)}</small></div>`).join('') || '<div class="muted">No nodes</div>'}</div>
    <div class="viewer-detail-section"><strong>Materials</strong>${model.resolvedMaterials.map((material) => `<div class="inspector-card"><strong>Material ${material.materialIndex}</strong><div>${escapeHtml(material.renderHint || 'default')}</div>${material.textures.map((texture) => `<div>${escapeHtml(texture.role)}: ${escapeHtml(texture.name)} ${texture.texture == null ? '⚠' : ''}</div>`).join('')}${material.mtr ? `<div>MTR: ${escapeHtml(material.mtr.resource)}</div>` : ''}</div>`).join('') || '<div class="muted">No materials</div>'}${model.nodeTextures.map((texture) => `<div class="inspector-card"><strong>${escapeHtml(texture.role)}</strong><div>${escapeHtml(texture.name)} ${texture.texture == null ? '⚠' : ''}</div></div>`).join('')}</div>
  </details>`).join('');
  const shaders = scene.manifest.shaders.map((shader) => `<details class="viewer-nested-details"><summary>${escapeHtml(shader.resource)} · ${escapeHtml(shader.stage)}</summary><pre>${escapeHtml(shader.source)}</pre></details>`).join('');
  return `<dl><dt>Source</dt><dd>${escapeHtml(scene.manifest.source)}</dd><dt>Models</dt><dd>${scene.manifest.models.length}</dd><dt>Textures</dt><dd>${scene.manifest.textures.length}</dd><dt>Collision</dt><dd>${collisionCount}</dd><dt>Diagnostics</dt><dd>${diagnostics.length}</dd>${environment ? `<dt>Time</dt><dd>${environment.isNight ? 'Night' : 'Day'}</dd><dt>Fog clip</dt><dd>${environment.fogClipDistance ?? 'unset'}</dd><dt>Skybox</dt><dd>${escapeHtml(environment.skybox ?? 'unset')}</dd><dt>Weather</dt><dd>rain ${environment.chanceRain ?? 0}% · snow ${environment.chanceSnow ?? 0}% · lightning ${environment.chanceLightning ?? 0}%</dd>` : ''}</dl>
    ${modelDetails}${shaders}${diagnostics.map((entry) => `<div class="diagnostic ${escapeAttribute(entry.severity)}"><strong>${escapeHtml(entry.code)}</strong><br>${escapeHtml(entry.message)}</div>`).join('')}
  `;
}

function dependenciesDisclosure(scene) {
  const count = scene.manifest.dependencies.nodes.length;
  return `<details id="viewer-dependencies" class="viewer-disclosure"><summary><span>Dependencies</span><small>${count}</small></summary><div class="viewer-disclosure-content"></div></details>`;
}

function dependenciesDisclosureContent(scene) {
  const incoming = new Map();
  for (const edge of scene.manifest.dependencies.edges) { const relationships = incoming.get(edge.to) || []; relationships.push(edge.relationship); incoming.set(edge.to, relationships); }
  const nodes = scene.manifest.dependencies.nodes;
  return nodes.map((node) => `<button class="dependency ${node.state}" data-resource="${escapeAttribute(node.resource)}" ${node.state === 'resolved' ? '' : 'disabled'}><span>${escapeHtml(node.resource)}</span><small>${escapeHtml(node.kind)} · ${escapeHtml(node.state)}${incoming.get(node.id)?.length ? ` · ${escapeHtml(incoming.get(node.id).join(', '))}` : ''}</small>${node.origin ? `<small>${escapeHtml(node.origin)}</small>` : ''}${node.message ? `<small>${escapeHtml(node.message)}</small>` : ''}</button>`).join('') || '<div class="muted">No dependencies</div>';
}

function createViewer(canvas, scene, elements, initialMode = 'model', session = createViewerSession(scene)) {
  const gl = canvas.getContext('webgl2', { antialias: true, alpha: false });
  if (!gl) throw new Error('WebGL 2 is required for the nwnrs model viewer.');
  const sceneHasSkinning = scene.manifest.models.some((model) => model.meshes.some((mesh) => mesh.primitives.some((primitive) => (primitive.skinBones || []).length > 0)));
  const sceneHasPointLights = scene.manifest.models.some((model) => model.nodes.some((node) => node.light));
  const program = createProgram(gl, sceneHasSkinning ? `#version 300 es
    precision highp float;
    layout(location=0) in vec3 aPosition;
    layout(location=1) in vec3 aNormal;
    layout(location=2) in vec2 aUv;
    layout(location=3) in vec4 aBoneIndices;
    layout(location=4) in vec4 aBoneWeights;
    layout(location=5) in vec3 aVertexColor;
    layout(location=6) in mat4 aInstanceModel;
    uniform mat4 uModelViewProjection;
    uniform mat4 uModel;
    uniform mat4 uViewProjection;
    uniform bool uInstanced;
    uniform sampler2D uBoneMatrices;
    uniform bool uSkinned;
    out vec3 vNormal; out vec2 vUv; out vec3 vWorldPosition; out vec3 vVertexColor;
    mat4 boneMatrix(int index) {
      return mat4(texelFetch(uBoneMatrices,ivec2(0,index),0),texelFetch(uBoneMatrices,ivec2(1,index),0),texelFetch(uBoneMatrices,ivec2(2,index),0),texelFetch(uBoneMatrices,ivec2(3,index),0));
    }
    void main(){
      mat4 skin=mat4(1.0);
      if(uSkinned){
        skin=mat4(0.0);
        float total=0.0;
        for(int i=0;i<4;i++){if(aBoneWeights[i]>0.0){skin+=boneMatrix(int(aBoneIndices[i]))*aBoneWeights[i];total+=aBoneWeights[i];}}
        if(total>0.0)skin/=total;else skin=mat4(1.0);
      }
      mat4 model=uInstanced?aInstanceModel*uModel:uModel;
      vec4 world=model*skin*vec4(aPosition,1.0);
      gl_Position=(uInstanced?uViewProjection*model:uModelViewProjection)*skin*vec4(aPosition,1.0); vNormal=transpose(inverse(mat3(model*skin)))*aNormal; vUv=aUv; vWorldPosition=world.xyz; vVertexColor=aVertexColor;
    }
  ` : `#version 300 es
    precision highp float;
    layout(location=0) in vec3 aPosition;
    layout(location=1) in vec3 aNormal;
    layout(location=2) in vec2 aUv;
    layout(location=5) in vec3 aVertexColor;
    layout(location=6) in mat4 aInstanceModel;
    uniform mat4 uModelViewProjection;
    uniform mat4 uModel;
    uniform mat4 uViewProjection;
    uniform bool uInstanced;
    out vec3 vNormal; out vec2 vUv; out vec3 vWorldPosition; out vec3 vVertexColor;
    void main(){
      mat4 model=uInstanced?aInstanceModel*uModel:uModel;
      vec4 world=model*vec4(aPosition,1.0);
      gl_Position=(uInstanced?uViewProjection*model:uModelViewProjection)*vec4(aPosition,1.0);
      vNormal=transpose(inverse(mat3(model)))*aNormal; vUv=aUv; vWorldPosition=world.xyz; vVertexColor=aVertexColor;
    }
  `, `#version 300 es
    #define HAS_POINT_LIGHTS ${sceneHasPointLights ? 1 : 0}
    precision highp float;
    in vec3 vNormal; in vec2 vUv; in vec3 vWorldPosition; in vec3 vVertexColor;
    uniform vec4 uColor; uniform vec3 uEnvironmentLight; uniform vec3 uCamera;
    uniform vec3 uMaterialAmbient; uniform vec3 uEmissiveColor;
    uniform bool uFogEnabled; uniform vec3 uFogColor; uniform float uFogEnd;
    #if HAS_POINT_LIGHTS
    uniform sampler2D uPointLights; uniform int uPointLightCount; uniform bool uDynamicObject;
    #endif
    uniform sampler2D uTexture; uniform sampler2D uNormalTexture; uniform sampler2D uEmissiveTexture; uniform vec4 uDiffuseUvTransform;
    uniform bool uHasTexture; uniform bool uHasNormalTexture; uniform bool uHasEmissiveTexture;
    out vec4 color;
    vec3 safeNormalize(vec3 value,vec3 fallback){float lengthSquared=dot(value,value);return lengthSquared>1e-12?value*inversesqrt(lengthSquared):fallback;}
    vec3 mappedNormal(){
      vec3 n=safeNormalize(vNormal,vec3(0.0,0.0,1.0)); if(!uHasNormalTexture)return n;
      vec3 q1=dFdx(vWorldPosition),q2=dFdy(vWorldPosition); vec2 st1=dFdx(vUv),st2=dFdy(vUv);
      vec3 tangentValue=q1*st2.t-q2*st1.t; vec3 bitangentValue=-q1*st2.s+q2*st1.s;
      if(dot(tangentValue,tangentValue)<=1e-12||dot(bitangentValue,bitangentValue)<=1e-12)return n;
      vec3 tangent=safeNormalize(tangentValue,vec3(1.0,0.0,0.0)); vec3 bitangent=safeNormalize(bitangentValue,vec3(0.0,1.0,0.0));
      vec3 sampled=texture(uNormalTexture,vUv).xyz*2.0-1.0; return safeNormalize(mat3(tangent,bitangent,n)*sampled,n);
    }
    void main(){
      vec2 diffuseUv=vUv*uDiffuseUvTransform.xy+uDiffuseUvTransform.zw; vec4 texel=uHasTexture?texture(uTexture,diffuseUv):vec4(1.0); vec4 base=vec4(texel.rgb*uColor.rgb*vVertexColor,texel.a*uColor.a);
      if(base.a<0.01)discard; vec3 normal=mappedNormal();
      vec3 emissive=uEmissiveColor+(uHasEmissiveTexture?texture(uEmissiveTexture,vUv).rgb:vec3(0.0));
      // The inspection light is an omnidirectional irradiance value. It must not
      // depend on the surface normal, camera, or an invented directional source.
      vec3 lit=base.rgb*uEnvironmentLight*uMaterialAmbient+emissive;
      #if HAS_POINT_LIGHTS
      for(int i=0;i<uPointLightCount;i++){
        vec4 positionRadius=texelFetch(uPointLights,ivec2(0,i),0); vec4 colorMultiplier=texelFetch(uPointLights,ivec2(1,i),0); vec4 options=texelFetch(uPointLights,ivec2(2,i),0);
        if(uDynamicObject&&options.y<0.5)continue;
        vec3 delta=positionRadius.xyz-vWorldPosition; float distanceToLight=length(delta); float attenuation=clamp(1.0-distanceToLight/max(positionRadius.w,0.01),0.0,1.0); attenuation*=attenuation;
        float incidence=options.x>0.5?1.0:max(dot(normal,safeNormalize(delta,normal)),0.0); lit+=base.rgb*colorMultiplier.rgb*colorMultiplier.a*incidence*attenuation;
      }
      #endif
      if(uFogEnabled){float fog=clamp(length(uCamera-vWorldPosition)/max(0.01,uFogEnd),0.0,1.0);lit=mix(lit,uFogColor,fog*fog);}
      color=vec4(lit,base.a);
    }
  `);
  const lineProgram = createProgram(gl, `#version 300 es
    precision highp float; layout(location=0) in vec3 aPosition; uniform mat4 uModelViewProjection;
    void main(){gl_Position=uModelViewProjection*vec4(aPosition,1.0);}
  `, `#version 300 es
    precision highp float; uniform vec4 uColor; out vec4 color; void main(){color=uColor;}
  `);
  const spriteProgram = createProgram(gl, `#version 300 es
    precision highp float;
    layout(location=0) in vec2 aCorner; layout(location=1) in vec3 aCenter;
    layout(location=2) in vec4 aSizeRotationAlpha; layout(location=3) in vec3 aColor;
    layout(location=4) in vec4 aUvRect;
    uniform mat4 uViewProjection; uniform vec3 uCameraRight; uniform vec3 uCameraUp;
    out vec2 vUv; out vec4 vColor;
    void main(){
      float c=cos(aSizeRotationAlpha.z),s=sin(aSizeRotationAlpha.z);
      vec2 rotated=mat2(c,-s,s,c)*(aCorner*aSizeRotationAlpha.xy);
      vec3 world=aCenter+uCameraRight*rotated.x+uCameraUp*rotated.y;
      gl_Position=uViewProjection*vec4(world,1.0);
      vec2 unit=aCorner*0.5+0.5; vUv=aUvRect.xy+unit*aUvRect.zw;
      vColor=vec4(aColor,aSizeRotationAlpha.w);
    }
  `, `#version 300 es
    precision highp float; in vec2 vUv; in vec4 vColor;
    uniform sampler2D uTexture; uniform bool uHasTexture; out vec4 color;
    void main(){vec4 texel=uHasTexture?texture(uTexture,vUv):vec4(1.0);color=texel*vColor;if(color.a<0.01)discard;}
  `);
  const meshUniforms = uniformLocations(gl, program, [
    'uEnvironmentLight', 'uCamera', 'uFogEnabled', 'uFogColor', 'uFogEnd', 'uPointLights',
    'uPointLightCount', 'uDynamicObject', 'uModel', 'uModelViewProjection', 'uTexture',
    'uHasTexture', 'uDiffuseUvTransform', 'uNormalTexture', 'uHasNormalTexture',
    'uEmissiveTexture', 'uHasEmissiveTexture', 'uColor', 'uEmissiveColor', 'uMaterialAmbient',
    'uSkinned', 'uBoneMatrices', 'uViewProjection', 'uInstanced',
  ]);
  const lineUniforms = uniformLocations(gl, lineProgram, ['uModelViewProjection', 'uColor']);
  const spriteUniforms = uniformLocations(gl, spriteProgram, ['uViewProjection', 'uCameraRight', 'uCameraUp', 'uTexture', 'uHasTexture']);
  const gpuTextures = new Array(scene.manifest.textures.length);
  const requestedTextures = new Set(); const requestedAnimations = new Set();
  const maxTextureSize = gl.getParameter(gl.MAX_TEXTURE_SIZE);
  const s3tc = gl.getExtension('WEBGL_compressed_texture_s3tc') || gl.getExtension('WEBKIT_WEBGL_compressed_texture_s3tc');
  const pointLightTexture = gl.createTexture(); gl.bindTexture(gl.TEXTURE_2D, pointLightTexture);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
  const primitiveCache = new Map();
  const spriteGpu = createSpriteGpu(gl);
  const stateKey = viewerStateKey(scene);
  const savedViewer = vscode.getState?.()?.viewer;
  const savedCamera = savedViewer?.scene === stateKey ? savedViewer.camera : undefined;
  const restoredCamera = validViewerCamera(savedCamera);
  const camera = restoredCamera
    ? { yaw: savedCamera.yaw, pitch: savedCamera.pitch, distance: savedCamera.distance, target: [...savedCamera.target] }
    : { yaw: -0.8, pitch: 0.65, distance: 20, target: [0, 0, 0] };
  let mode = initialMode; let animationFrame; let disposed = false;
  let activeAnimation; let pendingAnimation; let animationTime = 0; let animationElapsed = 0;
  let animationStarted = 0; let animationPlaying = false; let transition;
  let displayedEventTimer;
  let pointLightsCache; let pointLightsDirty = true;
  const lightRuntime = { storage: new Float32Array(12 * 16), count: 0, values: new Float32Array(12) };
  let renderScale = 1; let slowFrames = 0; let fastFrames = 0;
  const viewerStarted = performance.now();
  const hasDynamicEffects = scene.manifest.models.some((model) => model.nodes.some((node) => node.emitter || node.dangly)
    || model.resolvedMaterials.some((material) => material.textures.some((texture) => directiveValue(texture, 'proceduretype')?.toLowerCase() === 'cycle')));
  const bounds = sceneBounds(scene);
  const modelRuntime = scene.manifest.models.map((entry) => createModelRuntime(entry));
  scene.manifest.models.forEach((entry, modelIndex) => {
    entry.animations.forEach((animation, animationIndex) => {
      const key = animationAssetKey(modelIndex, animationIndex);
      const retained = session.animationAssets.get(key);
      if (retained) installAnimationAsset(modelRuntime[modelIndex], retained);
      else if (!scene.manifest.assetKey && animation?.tracksLoaded === true) {
        const inline = createAnimationAsset(scene, modelIndex, animationIndex, animation, scene.binary);
        session.animationAssets.set(key, inline); installAnimationAsset(modelRuntime[modelIndex], inline);
      }
    });
  });
  for (const runtime of modelRuntime) runtime.chunkBatch = {
    buffer: gl.createBuffer(), values: new Float32Array(16 * 16), count: 0, gpuCapacity: 0,
  };
  const modelIndexByName = new Map(scene.manifest.models.map((model, index) => [model.name.toLowerCase(), index]));
  const instanceRuntime = scene.manifest.instances.map((instance) => ({
    instance,
    base: composeTransform4(instance.position, instance.rotationAxisAngle, instance.scale),
    dynamic: instance.kind === 'creature' || instance.kind === 'door' || instance.kind === 'placeable' || instance.kind === 'item',
    overlay: createOverlayGpu(gl, instance.polygon),
  }));
  let poseFrame = 0;

  for (const [textureIndex, asset] of session.textureAssets) {
    if (scene.manifest.textures[textureIndex]) gpuTextures[textureIndex] = createTexture(gl, asset.manifest, asset.binary, s3tc);
  }

  function requestTexture(textureIndex) {
    if (!Number.isInteger(textureIndex) || textureIndex < 0 || textureIndex >= gpuTextures.length || gpuTextures[textureIndex] || requestedTextures.has(textureIndex)) return;
    const catalog = scene.manifest.textures[textureIndex];
    if (catalog?.rgba8) {
      gpuTextures[textureIndex] = createTexture(gl, catalog, scene.binary, s3tc);
      return;
    }
    if (!scene.manifest.assetKey) return;
    requestedTextures.add(textureIndex);
    vscode.postMessage({ type: 'loadTexture', assetKey: scene.manifest.assetKey, textureIndex, preferCompressed: Boolean(s3tc) });
  }

  function requestAnimation(modelIndex, animationIndex) {
    const key = animationAssetKey(modelIndex, animationIndex); const animation = scene.manifest.models[modelIndex]?.animations[animationIndex];
    if (!animation || animationLoaded(modelIndex, animationIndex) || requestedAnimations.has(key) || !scene.manifest.assetKey) return;
    requestedAnimations.add(key);
    vscode.postMessage({ type: 'loadAnimation', assetKey: scene.manifest.assetKey, modelIndex, animationIndex });
  }

  function applyTexture(asset) {
    if (asset.manifest.schema !== 'nwnrs.scene.texture') throw new Error(`Unexpected texture asset schema ${asset.manifest.schema}`);
    const index = asset.manifest.textureIndex;
    if (!Number.isInteger(index) || !scene.manifest.textures[index]) throw new Error(`Texture asset index ${index} is not in this scene.`);
    if (asset.manifest.assetKey !== scene.manifest.assetKey) throw new Error(`Texture asset ${index} belongs to a different scene.`);
    if (gpuTextures[index]) gl.deleteTexture(gpuTextures[index]);
    session.textureAssets.set(index, asset);
    gpuTextures[index] = createTexture(gl, asset.manifest, asset.binary, s3tc); requestedTextures.delete(index); draw();
  }

  function applyAnimation(asset) {
    if (asset.manifest.schema !== 'nwnrs.scene.animation') throw new Error(`Unexpected animation asset schema ${asset.manifest.schema}`);
    const { modelIndex, animationIndex, animation } = asset.manifest; const model = scene.manifest.models[modelIndex];
    const catalog = model?.animations[animationIndex];
    if (!catalog) throw new Error(`Animation asset ${modelIndex}:${animationIndex} is not in this scene.`);
    if (asset.manifest.assetKey !== scene.manifest.assetKey) throw new Error(`Animation asset ${modelIndex}:${animationIndex} belongs to a different scene.`);
    if (animation.name !== catalog.name || animation.length !== catalog.length) throw new Error(`Animation asset ${modelIndex}:${animationIndex} does not match its catalog entry.`);
    const installed = createAnimationAsset(scene, modelIndex, animationIndex, animation, asset.binary);
    const key = animationAssetKey(modelIndex, animationIndex);
    session.animationAssets.set(key, installed); installAnimationAsset(modelRuntime[modelIndex], installed);
    requestedAnimations.delete(key); poseFrame += 1; pointLightsDirty = true; maybeStartAnimation();
  }

  function animationLoaded(modelIndex, animationIndex) {
    return modelRuntime[modelIndex]?.animationAssets.has(animationIndex) === true;
  }

  const resizeObserver = new ResizeObserver(() => draw());
  resizeObserver.observe(canvas);
  const persistState = (animationSelection) => {
    const previous = vscode.getState?.() || {};
    vscode.setState?.({ ...previous, viewer: {
      scene: stateKey,
      camera: { yaw: camera.yaw, pitch: camera.pitch, distance: camera.distance, target: [...camera.target] },
      animationSelection: animationSelection === undefined ? previous.viewer?.animationSelection : animationSelection,
    } });
  };
  bindViewportControls(canvas, camera, draw, () => persistState());
  const contextLost = (event) => { event.preventDefault(); elements.status.textContent = 'Graphics context lost; waiting for VS Code to restore it…'; };
  const contextRestored = () => { if (!disposed) renderViewer(session); };
  canvas.addEventListener('webglcontextlost', contextLost); canvas.addEventListener('webglcontextrestored', contextRestored);

  function primitiveGpu(modelIndex, meshIndex, primitiveIndex) {
    const key = `${modelIndex}:${meshIndex}:${primitiveIndex}`;
    if (primitiveCache.has(key)) return primitiveCache.get(key);
    const model = scene.manifest.models[modelIndex];
    const primitive = model.meshes[meshIndex].primitives[primitiveIndex];
    const positions = numericView(scene.binary, primitive.positions);
    const indices = numericView(scene.binary, primitive.indices);
    const normals = primitive.normals ? numericView(scene.binary, primitive.normals) : undefined;
    const uvSet = primitive.uvSets[0];
    const uvs = uvSet ? numericView(scene.binary, uvSet.coordinates) : undefined;
    const uvIndices = numericView(scene.binary, primitive.uvIndices);
    const skinIndices = numericView(scene.binary, primitive.skinBoneIndices);
    const skinWeights = numericView(scene.binary, primitive.skinWeights);
    const skinOffsets = numericView(scene.binary, primitive.skinRowOffsets);
    const colorValues = numericView(scene.binary, primitive.colors.values);
    const colorOffsets = numericView(scene.binary, primitive.colors.rowOffsets);
    const faceMaterials = numericView(scene.binary, primitive.faceMaterialIndices);
    const constraintValues = numericView(scene.binary, primitive.constraints.values);
    const constraintOffsets = numericView(scene.binary, primitive.constraints.rowOffsets);
    const boneNodes = primitive.skinBones.map((name) => modelRuntime[modelIndex].nodeByName.get(name.toLowerCase()) ?? -1);
    const vertices = []; const vertexConstraints = [];
    for (let corner = 0; corner < indices.length; corner += 1) {
      const vertex = indices[corner];
      const px = positions[vertex * 3] || 0; const py = positions[vertex * 3 + 1] || 0; const pz = positions[vertex * 3 + 2] || 0;
      let nx = normals?.[vertex * 3]; let ny = normals?.[vertex * 3 + 1]; let nz = normals?.[vertex * 3 + 2];
      if (nx == null) {
        const face = Math.floor(corner / 3) * 3;
        [nx, ny, nz] = faceNormal(positions, indices[face], indices[face + 1], indices[face + 2]);
      }
      const uvIndex = uvIndices[corner] ?? vertex;
      const influences = [];
      for (let influence = skinOffsets[vertex] || 0; influence < (skinOffsets[vertex + 1] || 0); influence += 1) {
        const localBone = skinIndices[influence]; const nodeIndex = boneNodes[localBone]; const weight = skinWeights[influence] || 0;
        if (nodeIndex >= 0 && weight > 0) influences.push([localBone, weight]);
      }
      influences.sort((left, right) => right[1] - left[1]);
      while (influences.length < 4) influences.push([0, 0]);
      const selected = influences.slice(0, 4); const total = selected.reduce((sum, entry) => sum + entry[1], 0) || 1;
      vertices.push(px, py, pz, nx || 0, ny || 0, nz || 1, uvs?.[uvIndex * 2] || 0, uvs?.[uvIndex * 2 + 1] || 0,
        selected[0][0], selected[1][0], selected[2][0], selected[3][0], selected[0][1] / total, selected[1][1] / total, selected[2][1] / total, selected[3][1] / total);
      const constraintStart = constraintOffsets[vertex]; const constraintEnd = constraintOffsets[vertex + 1];
      vertexConstraints.push(constraintStart != null && constraintEnd > constraintStart ? (constraintValues[constraintStart] || 0) : 0);
      const colorStart = colorOffsets[vertex]; const colorEnd = colorOffsets[vertex + 1]; const authoredColor = colorStart != null && colorEnd - colorStart >= 3 ? Array.from(colorValues.slice(colorStart, colorStart + 3)) : undefined;
      vertices.push(...(authoredColor || (model.nodes[model.meshes[meshIndex].sourceNode]?.kind === 'aabb' ? surfaceColor(faceMaterials[Math.floor(corner / 3)] || 0) : [1, 1, 1])));
    }
    const vao = gl.createVertexArray(); const buffer = gl.createBuffer();
    gl.bindVertexArray(vao); gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array(vertices), gl.STATIC_DRAW);
    const stride = 19 * 4;
    gl.enableVertexAttribArray(0); gl.vertexAttribPointer(0, 3, gl.FLOAT, false, stride, 0);
    gl.enableVertexAttribArray(1); gl.vertexAttribPointer(1, 3, gl.FLOAT, false, stride, 3 * 4);
    gl.enableVertexAttribArray(2); gl.vertexAttribPointer(2, 2, gl.FLOAT, false, stride, 6 * 4);
    gl.enableVertexAttribArray(3); gl.vertexAttribPointer(3, 4, gl.FLOAT, false, stride, 8 * 4);
    gl.enableVertexAttribArray(4); gl.vertexAttribPointer(4, 4, gl.FLOAT, false, stride, 12 * 4);
    gl.enableVertexAttribArray(5); gl.vertexAttribPointer(5, 3, gl.FLOAT, false, stride, 16 * 4);
    const boneTexture = gl.createTexture(); gl.bindTexture(gl.TEXTURE_2D, boneTexture);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    const staticVertices = new Float32Array(vertices);
    const boneCount = Math.max(1, boneNodes.length);
    const gpu = {
      vao, buffer, count: vertices.length / 19, stride: 19, vertices: staticVertices,
      dynamicVertices: new Float32Array(staticVertices.length), danglyVertices: new Float32Array(staticVertices.length),
      indices, uvIndices, sourcePositions: positions, sourceUvs: uvs, boneNodes, boneTexture,
      boneMatrices: new Float32Array(boneCount * 16), boneScratchA: identity4(), boneScratchB: identity4(), meshInverse: identity4(),
      vertexConstraints: new Float32Array(vertexConstraints),
    };
    gl.bindTexture(gl.TEXTURE_2D, boneTexture);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA32F, 4, boneCount, 0, gl.RGBA, gl.FLOAT, gpu.boneMatrices);
    primitiveCache.set(key, gpu); return gpu;
  }

  function preparePrimitive(modelIndex, meshIndex, primitiveIndex, nodeWorld, asset, pose) {
    const model = scene.manifest.models[modelIndex]; const runtime = modelRuntime[modelIndex];
    const mesh = model.meshes[meshIndex]; const primitive = mesh.primitives[primitiveIndex];
    const gpu = primitiveGpu(modelIndex, meshIndex, primitiveIndex); const material = model.materials[primitive.material] || EMPTY_OBJECT;
    if (material.renderEnabled === false) return undefined;
    const materialRuntime = runtime.materials[primitive.material]; const materialPose = pose.materials[primitive.material]; const animated = materialPose?.active ? materialPose : EMPTY_OBJECT;
    const textureFor = (role) => { const texture = materialRuntime?.textures.get(role); if (!texture) return undefined; requestTexture(texture.texture); texture.handle = gpuTextures[texture.texture]; return texture; };
    const diffuseTexture = textureFor('diffuse');
    bindMaterialTexture(gl, meshUniforms.uTexture, meshUniforms.uHasTexture, diffuseTexture, 0);
    gl.uniform4fv(meshUniforms.uDiffuseUvTransform, textureUvTransform(diffuseTexture?.binding, (performance.now() - viewerStarted) / 1000, diffuseTexture?.uvTransform));
    bindMaterialTexture(gl, meshUniforms.uNormalTexture, meshUniforms.uHasNormalTexture, textureFor('normal'), 1);
    bindMaterialTexture(gl, meshUniforms.uEmissiveTexture, meshUniforms.uHasEmissiveTexture, textureFor('emissive'), 4);
    applyBlendMode(gl, diffuseTexture?.binding);
    const nodeColor = pose.nodes[mesh.sourceNode]?.color || WHITE_COLOR; const diffuse = material.diffuse || DEFAULT_DIFFUSE;
    gl.uniform4f(meshUniforms.uColor, diffuse[0]*nodeColor[0], diffuse[1]*nodeColor[1], diffuse[2]*nodeColor[2], (animated.alpha ?? material.alpha ?? 1)*(pose.nodes[mesh.sourceNode]?.alpha ?? 1));
    gl.uniform3fv(meshUniforms.uEmissiveColor, animated.selfIllumColor || material.selfIllumColor || ZERO_COLOR);
    gl.uniform3fv(meshUniforms.uMaterialAmbient, material.ambient || WHITE_COLOR);
    const skinned = gpu.boneNodes.length > 0; gl.uniform1i(meshUniforms.uSkinned, skinned);
    if (skinned) updateBoneTexture(gl, gpu, runtime.inverseBindWorlds, nodeWorld, runtime.bindWorlds[mesh.sourceNode] || IDENTITY_MATRIX, nodeWorld[mesh.sourceNode] || IDENTITY_MATRIX);
    gl.activeTexture(gl.TEXTURE5); gl.bindTexture(gl.TEXTURE_2D, gpu.boneTexture); gl.uniform1i(meshUniforms.uBoneMatrices, 5);
    const animmesh = asset?.runtime.tracksByNode[mesh.sourceNode]?.animmesh;
    const sourceAsset = transition?.sourceAssets.get(modelIndex);
    const sourceAnimmesh = sourceAsset?.runtime.tracksByNode[mesh.sourceNode]?.animmesh;
    const animatedVertices = updatePreparedAnimMesh(
      gpu,
      animmesh,
      animationTime,
      asset?.animation.length || 0,
      sourceAnimmesh,
      transition?.sourceTime || 0,
      sourceAsset?.animation.length || 0,
      transitionFactor(),
    );
    updateDynamicMesh(gl, gpu, animatedVertices, model.nodes[mesh.sourceNode]?.dangly, (performance.now()-viewerStarted)/1000, scene.manifest.environment?.nwn?.windPower || 0);
    return gpu;
  }

  function draw() {
    if (disposed) return;
    const drawStarted = performance.now();
    const pixelRatio = Math.min(devicePixelRatio, 2) * renderScale; const width = Math.max(1, Math.floor(canvas.clientWidth * pixelRatio));
    const height = Math.max(1, Math.floor(canvas.clientHeight * pixelRatio));
    if (canvas.width !== width || canvas.height !== height) { canvas.width = width; canvas.height = height; }
    gl.viewport(0, 0, width, height); gl.enable(gl.DEPTH_TEST); gl.enable(gl.CULL_FACE); gl.enable(gl.BLEND);
    gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);
    const illumination = globalIllumination(scene.manifest.environment?.nwn);
    const background = illumination.background;
    gl.clearColor(background[0], background[1], background[2], 1); gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);
    const projection = perspective(Math.PI / 4, width / height, Math.max(0.01, camera.distance / 1000), Math.max(1000, camera.distance * 20));
    const eye = orbitEye(camera); const view = lookAt(eye, camera.target, [0, 0, 1]); const viewProjection = multiply4(projection, view);
    gl.useProgram(program); gl.uniform3fv(meshUniforms.uEnvironmentLight, illumination.environmentLight);
    gl.uniform1i(meshUniforms.uInstanced, false);
    for (const runtime of modelRuntime) runtime.chunkBatch.count = 0;
    gl.uniform3fv(meshUniforms.uCamera, eye);
    gl.uniform1i(meshUniforms.uFogEnabled, illumination.fogEnabled);
    gl.uniform3fv(meshUniforms.uFogColor, illumination.fogColor);
    gl.uniform1f(meshUniforms.uFogEnd, illumination.fogEnd);
    if (sceneHasPointLights) {
      if (pointLightsDirty || animationPlaying || !pointLightsCache) {
        pointLightsCache = collectSceneLights(scene, poseForModel, modelRuntime, instanceRuntime, lightRuntime);
        if (pointLightsCache.count > maxTextureSize) throw new Error(`Scene has ${pointLightsCache.count} lights, exceeding this GPU's ${maxTextureSize}-light texture capacity.`);
        gl.activeTexture(gl.TEXTURE6); gl.bindTexture(gl.TEXTURE_2D, pointLightTexture);
        gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA32F, 3, Math.max(1, pointLightsCache.count), 0, gl.RGBA, gl.FLOAT, pointLightsCache.values);
        pointLightsDirty = false;
      }
      gl.uniform1i(meshUniforms.uPointLights, 6); gl.uniform1i(meshUniforms.uPointLightCount, pointLightsCache.count);
    }
    for (const skyboxPass of [true, false]) for (const entry of instanceRuntime) {
      const { instance } = entry;
      const collision = instance.kind === 'collision'; const skybox = instance.kind === 'skybox';
      if (skybox !== skyboxPass || (mode === 'collision' ? !collision : collision)) continue;
      if (instance.model == null) continue;
      if (skybox) { gl.disable(gl.CULL_FACE); gl.depthMask(false); } else { gl.enable(gl.CULL_FACE); gl.depthMask(true); }
      const base = skybox ? composeTransform4(camera.target, instance.rotationAxisAngle, instance.scale) : entry.base;
      gl.uniform1i(meshUniforms.uDynamicObject, entry.dynamic);
      drawModel(instance.model, base, viewProjection, new Set(), illumination.fogEnabled);
    }
    drawChunkBatches(viewProjection, illumination.fogEnabled);
    gl.depthMask(true); gl.enable(gl.CULL_FACE);
    if (mode !== 'collision') drawEffects(viewProjection, view, (performance.now() - viewerStarted) / 1000);
    drawOverlays(viewProjection);
    elements.status.textContent = `${scene.manifest.models.length} models · ${scene.manifest.textures.length} textures · ${scene.manifest.instances.length} instances`;
    if (animationPlaying || hasDynamicEffects) {
      const duration = performance.now() - drawStarted;
      slowFrames = duration > 20 ? slowFrames + 1 : 0; fastFrames = duration < 10 ? fastFrames + 1 : 0;
      if (slowFrames >= 8 && renderScale > 0.5) { renderScale = Math.max(0.5, renderScale - 0.1); slowFrames = 0; fastFrames = 0; }
      else if (fastFrames >= 120 && renderScale < 1) { renderScale = Math.min(1, renderScale + 0.1); slowFrames = 0; fastFrames = 0; }
    }
  }

  function drawModel(modelIndex, base, viewProjection, stack, fogEnabled) {
    if (stack.has(modelIndex)) return;
    stack.add(modelIndex);
    const model = scene.manifest.models[modelIndex]; const runtime = modelRuntime[modelIndex]; if (!model || !runtime) return;
    gl.uniform1i(meshUniforms.uFogEnabled, fogEnabled && model.ignoreFog !== 1);
    const { asset, pose } = poseForModel(modelIndex);
    const nodeWorld = pose.worlds;
    model.meshes.forEach((mesh, meshIndex) => {
      if (runtime.hiddenNodes.has(mesh.sourceNode)) return;
      const world = multiply4Into(base, nodeWorld[mesh.sourceNode] || IDENTITY_MATRIX, runtime.drawWorld);
      const mvp = multiply4Into(viewProjection, world, runtime.drawMvp);
      gl.uniformMatrix4fv(meshUniforms.uModel, false, world);
      gl.uniformMatrix4fv(meshUniforms.uModelViewProjection, false, mvp);
      mesh.primitives.forEach((primitive, primitiveIndex) => {
        const gpu = preparePrimitive(modelIndex, meshIndex, primitiveIndex, nodeWorld, asset, pose);
        if (!gpu) return;
        gl.bindVertexArray(gpu.vao); gl.drawArrays(gl.TRIANGLES, 0, gpu.count);
      });
    });
    drawChunkEmitters(modelIndex, model, nodeWorld, base, viewProjection, stack, fogEnabled, asset);
    for (const attachment of model.attachments) {
      const target = runtime.attachmentTargets.get(attachment);
      multiply4Into(base, nodeWorld[target] || IDENTITY_MATRIX, runtime.attachmentWorld);
      drawModel(attachment.model, runtime.attachmentWorld, viewProjection, new Set(stack), fogEnabled);
    }
  }
  function drawChunkEmitters(modelIndex, model, nodeWorld, base, viewProjection, stack, fogEnabled, asset) {
    model.nodes.forEach((node, nodeIndex) => {
      if (!node.emitter || String(emitterProperty(node.emitter, 'update', '')).toLowerCase() !== 'explosion') return;
      const runtime = modelRuntime[modelIndex];
      const track = asset?.runtime.tracksByNode[nodeIndex];
      if (animatedEmitterValue(modelIndex, nodeIndex, track, 'detonate', emitterProperty(node.emitter, 'detonate', 0)) <= 0) return;
      const chunkName = String(emitterProperty(node.emitter, 'chunkname', '')).trim(); const chunkModel = modelIndexByName.get(chunkName.toLowerCase()); if (!chunkName || chunkModel == null) return;
      const value = (name, fallback) => animatedEmitterValue(modelIndex, nodeIndex, track, name, emitterProperty(node.emitter, name, fallback));
      const life = Math.max(0.001, value('lifeexp', 1)); const count = Math.ceil(Math.max(0, value('birthrate', 1)) * life); if (count > 20000) throw new Error(`Emitter ${node.name} requests ${count} concurrent chunks; the viewer safety limit is 20000.`);
      const nodeBase = multiply4Into(base, nodeWorld[nodeIndex] || IDENTITY_MATRIX, runtime.emitterWorld); const velocity = value('velocity', 0); const randomVelocity = value('randvel', 0); const spread = value('spread', 0); const gravity = value('grav', 0); const drag = Math.max(0, value('drag', 0));
      for (let index = 0; index < count; index += 1) {
        const phase = random01(index, 0); const ageSeconds = (((performance.now() - viewerStarted) / 1000 + phase * life) % life + life) % life; const azimuth = random01(index, 1) * Math.PI * 2; const cone = spread * Math.sqrt(random01(index, 2)); const speed = velocity + (random01(index, 3) * 2 - 1) * randomVelocity; const damping = drag > 0 ? (1 - Math.exp(-drag * ageSeconds)) / drag : ageSeconds;
        const localX=(random01(index,4)-0.5)*value('xsize',node.emitter.xSize)+Math.sin(cone)*Math.cos(azimuth)*speed*damping;
        const localY=(random01(index,5)-0.5)*value('ysize',node.emitter.ySize)+Math.sin(cone)*Math.sin(azimuth)*speed*damping;
        const localZ=Math.cos(cone)*speed*damping-gravity*ageSeconds*ageSeconds*0.5;
        const sizeStart=value('sizestart',1); const size=stagedValue3(ageSeconds/life,Math.max(0.001,Math.min(0.999,value('percentmid',50)/100)),sizeStart,value('sizemid',sizeStart),value('sizeend',1));
        const chunkRuntime = modelRuntime[chunkModel];
        chunkRuntime.chunkTranslation[0]=localX; chunkRuntime.chunkTranslation[1]=localY; chunkRuntime.chunkTranslation[2]=localZ;
        chunkRuntime.chunkRotation[0]=random01(index,6); chunkRuntime.chunkRotation[1]=random01(index,7); chunkRuntime.chunkRotation[2]=random01(index,8); chunkRuntime.chunkRotation[3]=value('particlerot',0)*ageSeconds*Math.PI/180; chunkRuntime.chunkScale.fill(size);
        composeTransform4Into(chunkRuntime.chunkTranslation, chunkRuntime.chunkRotation, chunkRuntime.chunkScale, chunkRuntime.chunkLocalMatrix);
        multiply4Into(nodeBase, chunkRuntime.chunkLocalMatrix, chunkRuntime.chunkWorldMatrix);
        appendChunkInstance(chunkRuntime.chunkBatch, chunkRuntime.chunkWorldMatrix);
      }
    });
  }

  function drawChunkBatches(viewProjection, fogEnabled) {
    gl.uniform1i(meshUniforms.uInstanced, true); gl.uniformMatrix4fv(meshUniforms.uViewProjection, false, viewProjection);
    for (let modelIndex = 0; modelIndex < modelRuntime.length; modelIndex += 1) {
      const batch = modelRuntime[modelIndex].chunkBatch; if (!batch.count) continue;
      gl.bindBuffer(gl.ARRAY_BUFFER, batch.buffer); const byteLength = batch.count * 16 * 4;
      if (byteLength > batch.gpuCapacity) { batch.gpuCapacity = Math.max(byteLength, Math.ceil(batch.gpuCapacity*1.5), 16*16*4); gl.bufferData(gl.ARRAY_BUFFER, batch.gpuCapacity, gl.DYNAMIC_DRAW); }
      gl.bufferSubData(gl.ARRAY_BUFFER, 0, batch.values, 0, batch.count * 16);
      drawInstancedModel(modelIndex, batch, viewProjection, fogEnabled, IDENTITY_MATRIX, new Set());
    }
    gl.uniform1i(meshUniforms.uInstanced, false);
  }

  function drawInstancedModel(modelIndex, batch, viewProjection, fogEnabled, parentTransform, stack) {
    if (stack.has(modelIndex)) return; stack.add(modelIndex);
    const model = scene.manifest.models[modelIndex]; const runtime = modelRuntime[modelIndex]; if (!model || !runtime) return;
    gl.uniform1i(meshUniforms.uFogEnabled, fogEnabled && model.ignoreFog !== 1);
    const { asset, pose } = poseForModel(modelIndex); const worlds = pose.worlds;
    model.meshes.forEach((mesh, meshIndex) => {
      if (runtime.hiddenNodes.has(mesh.sourceNode)) return;
      const local = multiply4Into(parentTransform, worlds[mesh.sourceNode] || IDENTITY_MATRIX, runtime.instancedLocal);
      gl.uniformMatrix4fv(meshUniforms.uModel, false, local);
      mesh.primitives.forEach((_primitive, primitiveIndex) => {
        const gpu = preparePrimitive(modelIndex, meshIndex, primitiveIndex, worlds, asset, pose); if (!gpu) return;
        bindInstanceMatrices(gl, gpu.vao, batch.buffer); gl.drawArraysInstanced(gl.TRIANGLES, 0, gpu.count, batch.count);
      });
    });
    for (const attachment of model.attachments) {
      const target = runtime.attachmentTargets.get(attachment);
      multiply4Into(parentTransform, worlds[target] || IDENTITY_MATRIX, runtime.instancedAttachment);
      drawInstancedModel(attachment.model, batch, viewProjection, fogEnabled, runtime.instancedAttachment, new Set(stack));
    }
  }
  function poseForModel(modelIndex) {
    const model = scene.manifest.models[modelIndex]; const runtime = modelRuntime[modelIndex];
    if (runtime.poseFrame === poseFrame) return runtime.poseResult;
    const animationIndex = activeAnimation?.scope.get(modelIndex);
    const asset = animationIndex == null ? undefined : runtime.animationAssets.get(animationIndex);
    sampleModelPoseInto(runtime, model, asset, animationTime);
    const from = transition?.fromPoses.get(modelIndex);
    if (from) {
      blendPoseInto(runtime.pose, from, transitionFactor(), model);
      resolveNodeWorldsInto(runtime, model, runtime.pose.nodes, runtime.pose.worlds);
    }
    runtime.poseResult.asset = asset; runtime.poseFrame = poseFrame;
    return runtime.poseResult;
  }

  function transitionFactor() {
    return transition ? Math.max(0, Math.min(1, animationElapsed / Math.max(Number.EPSILON, transition.duration))) : 1;
  }

  function animatedEmitterValue(modelIndex, nodeIndex, targetTrack, name, fallback) {
    const target = samplePreparedEmitterValue(targetTrack?.emitterControllers.get(name.toLowerCase()), animationTime, fallback);
    if (!transition) return target;
    const sourceAsset = transition.sourceAssets.get(modelIndex);
    const sourceTrack = sourceAsset?.runtime.tracksByNode[nodeIndex];
    const source = samplePreparedEmitterValue(sourceTrack?.emitterControllers.get(name.toLowerCase()), transition.sourceTime, fallback);
    return lerpNumber(source, target, transitionFactor());
  }

  function animatedEmitterVectorInto(modelIndex, nodeIndex, targetTrack, name, fallback, result, interval) {
    samplePreparedEmitterVectorInto(targetTrack?.emitterControllers.get(name.toLowerCase()), animationTime, fallback, result, interval);
    if (!transition) return result;
    const runtime = modelRuntime[modelIndex]; const sourceResult = runtime.emitterTransitionVectors[nodeIndex];
    const sourceAsset = transition.sourceAssets.get(modelIndex); const sourceTrack = sourceAsset?.runtime.tracksByNode[nodeIndex];
    samplePreparedEmitterVectorInto(
      sourceTrack?.emitterControllers.get(name.toLowerCase()),
      transition.sourceTime,
      fallback,
      sourceResult,
      runtime.emitterTransitionIntervals[nodeIndex],
    );
    return lerpArrayInto(sourceResult, result, transitionFactor(), result);
  }

  function drawOverlays(viewProjection) {
    gl.useProgram(lineProgram); gl.disable(gl.CULL_FACE); gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);
    for (const { instance, base, overlay } of instanceRuntime) {
      if (!instance.polygon?.length || (mode === 'collision' && instance.kind !== 'trigger' && instance.kind !== 'encounter')) continue;
      if (!overlay) continue;
      gl.bindVertexArray(overlay.vao);
      gl.uniformMatrix4fv(lineUniforms.uModelViewProjection, false, multiply4(viewProjection, base));
      const color = ({ trigger: [1, 0.55, 0.1, 1], encounter: [0.7, 0.25, 1, 1], waypoint: [0.15, 0.85, 1, 1], sound: [0.2, 0.9, 0.45, 0.75], store: [1, 0.85, 0.15, 1] })[instance.kind] || [0.85, 0.85, 0.85, 1];
      gl.uniform4f(lineUniforms.uColor, ...color); gl.drawArrays(gl.LINE_LOOP, 0, overlay.count);
    }
    gl.enable(gl.CULL_FACE);
  }

  function drawEffects(viewProjection, view, effectTime) {
    gl.useProgram(spriteProgram); gl.disable(gl.CULL_FACE); gl.enable(gl.BLEND); gl.depthMask(false);
    gl.uniformMatrix4fv(spriteUniforms.uViewProjection, false, viewProjection);
    gl.uniform3f(spriteUniforms.uCameraRight, view[0], view[4], view[8]);
    gl.uniform3f(spriteUniforms.uCameraUp, view[1], view[5], view[9]);
    for (const { instance, base } of instanceRuntime) {
      if (instance.model == null || instance.kind === 'collision' || instance.kind === 'skybox') continue;
      drawModelEffects(instance.model, base, effectTime, new Set());
    }
    gl.depthMask(true); gl.enable(gl.CULL_FACE); gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA); gl.useProgram(program);
  }

  function drawModelEffects(modelIndex, base, effectTime, stack) {
    if (stack.has(modelIndex)) return; stack.add(modelIndex);
    const model = scene.manifest.models[modelIndex]; const runtime = modelRuntime[modelIndex]; if (!model || !runtime) return;
    const { asset, pose } = poseForModel(modelIndex); const worlds = pose.worlds;
    pose.nodes.forEach((node, nodeIndex) => {
      const world = multiply4Into(base, worlds[nodeIndex] || IDENTITY_MATRIX, runtime.effectWorld);
      if (node.emitter) drawEmitter(modelIndex, model, nodeIndex, node, world, asset, effectTime);
      if (node.light?.lensFlares) drawLensFlares(modelIndex, model, nodeIndex, node, world);
    });
    for (const attachment of model.attachments) {
      const target = runtime.attachmentTargets.get(attachment);
      multiply4Into(base, worlds[target] || IDENTITY_MATRIX, runtime.effectAttachment);
      drawModelEffects(attachment.model, runtime.effectAttachment, effectTime, new Set(stack));
    }
  }

  function drawEmitter(modelIndex, model, nodeIndex, node, world, asset, effectTime) {
    const runtime = modelRuntime[modelIndex];
    const emitter = node.emitter; const track = asset?.runtime.tracksByNode[nodeIndex];
    if (String(emitterProperty(emitter, 'update', '')).toLowerCase() === 'explosion') return;
    const value = (name, fallback) => animatedEmitterValue(modelIndex, nodeIndex, track, name, emitterProperty(emitter, name, fallback));
    const life = Math.max(0.001, value('lifeexp', 1)); const birthrate = Math.max(0, value('birthrate', 10));
    const requestedParticles = Math.ceil(life * birthrate); if (requestedParticles > 20000) throw new Error(`Emitter ${node.name} requests ${requestedParticles} concurrent particles; the viewer safety limit is 20000.`); const particleCount = requestedParticles; if (!particleCount) return;
    const velocity = value('velocity', 0); const randomVelocity = value('randvel', 0); const spread = value('spread', 0);
    const gravity = value('grav', 0); const drag = Math.max(0, value('drag', 0)); const fps = Math.max(0, value('fps', 0));
    const sizeStart = value('sizestart', 1); const sizeMid = value('sizemid', sizeStart); const sizeEnd = value('sizeend', sizeMid);
    const sizeStartY = value('sizestart_y', sizeStart); const sizeMidY = value('sizemid_y', sizeMid); const sizeEndY = value('sizeend_y', sizeEnd);
    const colorScratch = runtime.emitterColors[nodeIndex];
    const intervalScratch = runtime.emitterIntervals[nodeIndex];
    emitterVectorInto(emitter, 'colorstart', WHITE_COLOR, colorScratch[0]); animatedEmitterVectorInto(modelIndex, nodeIndex, track, 'colorstart', colorScratch[0], colorScratch[0], intervalScratch);
    emitterVectorInto(emitter, 'colormid', colorScratch[0], colorScratch[1]); animatedEmitterVectorInto(modelIndex, nodeIndex, track, 'colormid', colorScratch[1], colorScratch[1], intervalScratch);
    emitterVectorInto(emitter, 'colorend', colorScratch[1], colorScratch[2]); animatedEmitterVectorInto(modelIndex, nodeIndex, track, 'colorend', colorScratch[2], colorScratch[2], intervalScratch);
    const [colorStart, colorMid, colorEnd] = colorScratch;
    const alphaStart = value('alphastart', 1); const alphaMid = value('alphamid', alphaStart); const alphaEnd = value('alphaend', 0);
    const percentMid = Math.max(0.001, Math.min(0.999, value('percentmid', 50) / 100));
    const xGrid = Math.max(1, Math.round(emitterProperty(emitter, 'xgrid', 1))); const yGrid = Math.max(1, Math.round(emitterProperty(emitter, 'ygrid', 1)));
    const frameStart = Math.max(0, Math.round(value('framestart', 0))); const frameEnd = Math.max(frameStart, Math.round(value('frameend', xGrid * yGrid - 1)));
    const rotationRate = value('particlerot', 0) * Math.PI / 180;
    let values = runtime.emitterBuffers[nodeIndex];
    if (!values || values.length < particleCount * 14) {
      values = new Float32Array(Math.max(particleCount * 14, Math.ceil((values?.length || 14) * 1.5)));
      runtime.emitterBuffers[nodeIndex] = values;
    }
    for (let index = 0; index < particleCount; index += 1) {
      const phase = random01(index, 0); const ageSeconds = ((effectTime + phase * life) % life + life) % life; const age = ageSeconds / life;
      const azimuth = random01(index, 1) * Math.PI * 2; const cone = spread * Math.sqrt(random01(index, 2)); const speed = velocity + (random01(index, 3) * 2 - 1) * randomVelocity;
      let localX = (random01(index, 4) - 0.5) * value('xsize', emitter.xSize);
      let localY = (random01(index, 5) - 0.5) * value('ysize', emitter.ySize);
      let localZ = 0;
      const damping = drag > 0 ? (1 - Math.exp(-drag * ageSeconds)) / drag : ageSeconds;
      localX += Math.sin(cone) * Math.cos(azimuth) * speed * damping;
      localY += Math.sin(cone) * Math.sin(azimuth) * speed * damping;
      localZ += Math.cos(cone) * speed * damping - gravity * ageSeconds * ageSeconds * 0.5;
      const centerX=world[0]*localX+world[4]*localY+world[8]*localZ+world[12];
      const centerY=world[1]*localX+world[5]*localY+world[9]*localZ+world[13];
      const centerZ=world[2]*localX+world[6]*localY+world[10]*localZ+world[14];
      const stage = stagedValue3(age, percentMid, sizeStart, sizeMid, sizeEnd); const stageY = stagedValue3(age, percentMid, sizeStartY, sizeMidY, sizeEndY);
      const red=stagedValue3(age,percentMid,colorStart[0],colorMid[0],colorEnd[0]); const green=stagedValue3(age,percentMid,colorStart[1],colorMid[1],colorEnd[1]); const blue=stagedValue3(age,percentMid,colorStart[2],colorMid[2],colorEnd[2]); const alpha = stagedValue3(age, percentMid, alphaStart, alphaMid, alphaEnd);
      const frame = frameStart + Math.floor(ageSeconds * fps) % Math.max(1, frameEnd - frameStart + 1); const frameX = frame % xGrid; const frameY = Math.floor(frame / xGrid) % yGrid;
      const offset = index * 14;
      values[offset]=centerX; values[offset+1]=centerY; values[offset+2]=centerZ; values[offset+3]=stage*0.5; values[offset+4]=stageY*0.5; values[offset+5]=rotationRate*ageSeconds; values[offset+6]=alpha;
      values[offset+7]=red; values[offset+8]=green; values[offset+9]=blue; values[offset+10]=frameX/xGrid; values[offset+11]=frameY/yGrid; values[offset+12]=1/xGrid; values[offset+13]=1/yGrid;
    }
    const texture = runtime?.nodeTextures.get(`${nodeIndex}:emitter`);
    if (texture) requestTexture(texture.texture); const textureHandle = texture ? gpuTextures[texture.texture] : undefined;
    gl.activeTexture(gl.TEXTURE0); gl.bindTexture(gl.TEXTURE_2D, textureHandle || null); gl.uniform1i(spriteUniforms.uTexture, 0); gl.uniform1i(spriteUniforms.uHasTexture, Boolean(textureHandle));
    const blend = String(emitterProperty(emitter, 'blend', 'normal')).toLowerCase(); gl.blendFunc(gl.SRC_ALPHA, blend.includes('lighten') || blend.includes('add') ? gl.ONE : gl.ONE_MINUS_SRC_ALPHA);
    uploadAndDrawSprites(gl, spriteGpu, values, particleCount);
  }

  function drawLensFlares(modelIndex, model, nodeIndex, node, world) {
    const count = Math.min(node.light.flareTextures.length, node.light.flareSizes.length || Infinity); if (!count) return;
    const origin = transformPoint4(world, [0, 0, node.light.verticalDisplacement || 0]);
    for (let index = 0; index < count; index += 1) {
      const runtime = modelRuntime[modelIndex];
      const texture = runtime?.nodeTextures.get(`${nodeIndex}:flare:${index}`);
      const shift = node.light.flareColorShifts[index] || node.color || [1, 1, 1]; const size = Math.max(0.001, node.light.flareSizes[index] * Math.max(0.001, node.light.flareRadius || 1));
      const position = node.light.flarePositions[index] ?? 0; const center = origin.map((value, axis) => value + (camera.target[axis] - value) * position);
      const values = runtime.flareBuffer;
      values.set(center, 0); values.set([size, size, 0, Math.max(0, node.alpha ?? 1)], 3); values.set(shift, 7); values.set([0, 0, 1, 1], 10);
      if (texture) requestTexture(texture.texture); const textureHandle = texture ? gpuTextures[texture.texture] : undefined;
      gl.activeTexture(gl.TEXTURE0); gl.bindTexture(gl.TEXTURE_2D, textureHandle || null); gl.uniform1i(spriteUniforms.uTexture, 0); gl.uniform1i(spriteUniforms.uHasTexture, Boolean(textureHandle)); gl.blendFunc(gl.SRC_ALPHA, gl.ONE); uploadAndDrawSprites(gl, spriteGpu, values, 1);
    }
  }

  function frameScene() {
    camera.target = [(bounds.min[0] + bounds.max[0]) / 2, (bounds.min[1] + bounds.max[1]) / 2, (bounds.min[2] + bounds.max[2]) / 2];
    camera.distance = Math.max(2, Math.hypot(bounds.max[0] - bounds.min[0], bounds.max[1] - bounds.min[1], bounds.max[2] - bounds.min[2]) * 1.2); draw();
  }
  function setAnimation(modelIndex, animationIndex) {
    const animation = Number.isInteger(modelIndex) && Number.isInteger(animationIndex)
      ? scene.manifest.models[modelIndex]?.animations[animationIndex]
      : undefined;
    pendingAnimation = animation ? {
      modelIndex,
      animationIndex,
      animation,
      scope: animationPlaybackScope(scene, modelIndex, animationIndex),
    } : undefined;
    persistState(pendingAnimation ? { modelIndex, animationIndex } : null);
    if (!pendingAnimation) {
      activeAnimation = undefined; animationPlaying = false; animationTime = 0; animationElapsed = 0;
      transition = undefined; poseFrame += 1; pointLightsDirty = true;
      if (elements.animationTime) elements.animationTime.textContent = '';
      if (elements.animationEvent) elements.animationEvent.textContent = '';
      draw(); return;
    }
    if (elements.animationTime) elements.animationTime.textContent = 'Loading…';
    for (const [candidateModel, candidateAnimation] of pendingAnimation.scope) requestAnimation(candidateModel, candidateAnimation);
    maybeStartAnimation();
  }
  function maybeStartAnimation() {
    if (!pendingAnimation) return;
    if ([...pendingAnimation.scope].some(([modelIndex, animationIndex]) => !animationLoaded(modelIndex, animationIndex))) return;
    poseFrame += 1;
    const fromPoses = new Map(modelRuntime.map((_runtime, modelIndex) => [modelIndex, clonePose(poseForModel(modelIndex).pose)]));
    const sourceAssets = new Map();
    if (activeAnimation) for (const [modelIndex, animationIndex] of activeAnimation.scope) {
      const asset = modelRuntime[modelIndex]?.animationAssets.get(animationIndex);
      if (asset) sourceAssets.set(modelIndex, asset);
    }
    const sourceTime = animationTime;
    activeAnimation = pendingAnimation; pendingAnimation = undefined;
    animationTime = 0; animationElapsed = 0;
    animationPlaying = true; animationStarted = performance.now();
    const duration = Math.max(0, activeAnimation.animation.transitionTime || 0);
    transition = duration > 0 ? { duration, fromPoses, sourceAssets, sourceTime } : undefined;
    poseFrame += 1; pointLightsDirty = true;
    if (elements.animationTime) elements.animationTime.textContent = '0.00s';
    dispatchAnimationEvents(activeAnimation.animation, -Number.EPSILON, 0, emitAnimationEvent);
    draw();
  }
  function tick(now) {
    if (disposed) return;
    if (animationPlaying && activeAnimation) {
      const previousElapsed = animationElapsed;
      animationElapsed = Math.max(0, (now - animationStarted) / 1000);
      const selected = activeAnimation.animation;
      animationTime = selected.length > 0 ? animationElapsed % selected.length : animationElapsed;
      dispatchAnimationEvents(selected, previousElapsed, animationElapsed, emitAnimationEvent);
      if (transition && animationElapsed >= transition.duration) transition = undefined;
      poseFrame += 1;
      if (elements.animationTime) elements.animationTime.textContent = `${animationTime.toFixed(2)}s`;
    }
    if (animationPlaying || hasDynamicEffects) draw(); animationFrame = requestAnimationFrame(tick);
  }
  function emitAnimationEvent(event) {
    if (!elements.animationEvent) return;
    elements.animationEvent.textContent = event.name;
    clearTimeout(displayedEventTimer);
    displayedEventTimer = setTimeout(() => { if (!disposed && elements.animationEvent) elements.animationEvent.textContent = ''; }, 1200);
  }
  if (restoredCamera) draw(); else { frameScene(); persistState(undefined); }
  animationFrame = requestAnimationFrame(tick);
  return {
    setAnimation,
    applyAnimation,
    applyTexture,
    dispose() { disposed = true; clearTimeout(displayedEventTimer); cancelAnimationFrame(animationFrame); resizeObserver.disconnect(); canvas.removeEventListener('webglcontextlost', contextLost); canvas.removeEventListener('webglcontextrestored', contextRestored); for (const gpu of primitiveCache.values()) { gl.deleteBuffer(gpu.buffer); gl.deleteVertexArray(gpu.vao); gl.deleteTexture(gpu.boneTexture); } for (const runtime of modelRuntime) gl.deleteBuffer(runtime.chunkBatch.buffer); for (const entry of instanceRuntime) { if (entry.overlay) { gl.deleteBuffer(entry.overlay.buffer); gl.deleteVertexArray(entry.overlay.vao); } } gl.deleteBuffer(spriteGpu.cornerBuffer); gl.deleteBuffer(spriteGpu.instanceBuffer); gl.deleteVertexArray(spriteGpu.vao); gpuTextures.forEach((texture) => gl.deleteTexture(texture)); gl.deleteTexture(pointLightTexture); gl.deleteProgram(program); gl.deleteProgram(lineProgram); gl.deleteProgram(spriteProgram); },
  };
}

function renderUnsupported() {
  content().innerHTML = '<div class="empty">This resource type has no editor.</div>';
}

function renderTwoDa() {
  const data = model.data;
  const start = tablePage * tablePageSize;
  if (start >= data.rows.length && tablePage > 0) tablePage = Math.max(0, Math.ceil(data.rows.length / tablePageSize) - 1);
  const pageRows = data.rows.slice(tablePage * tablePageSize, (tablePage + 1) * tablePageSize);
  toolbar().innerHTML = `<button id="add-row">Add row</button><button id="add-column">Add column</button>
    <label>Default <input id="table-default" value="${escapeAttribute(data.default ?? '****')}" title="Use **** for no default"></label>
    <span class="spacer"></span><span class="pager"><button id="prev-page" class="secondary">Previous</button>
    <span>${data.rows.length ? tablePage * tablePageSize + 1 : 0}–${Math.min((tablePage + 1) * tablePageSize, data.rows.length)} of ${data.rows.length}</span>
    <button id="next-page" class="secondary">Next</button></span>`;
  content().innerHTML = `<div class="table-wrap"><table><thead><tr><th>Row</th>
    ${data.columns.map((column, index) => `<th>${escapeHtml(column)} <button class="secondary remove-column" data-column="${index}" title="Remove column">×</button></th>`).join('')}
    <th>Actions</th></tr></thead><tbody>${pageRows.map((row, pageIndex) => {
      const rowIndex = tablePage * tablePageSize + pageIndex;
      return `<tr><td><input class="row-label" data-row="${rowIndex}" value="${escapeAttribute(row.label)}"></td>
        ${data.columns.map((column, columnIndex) => {
          const value = row.cells[columnIndex];
          return `<td><input class="cell ${value == null ? 'null-cell' : ''}" data-row="${rowIndex}" data-column="${escapeAttribute(column)}" value="${escapeAttribute(value ?? '****')}" title="Use **** for an unset cell"></td>`;
        }).join('')}<td><button class="danger remove-row" data-row="${rowIndex}">Remove</button></td></tr>`;
    }).join('')}</tbody></table></div>`;
  document.getElementById('prev-page').onclick = () => { if (tablePage > 0) { tablePage -= 1; renderTwoDa(); } };
  document.getElementById('next-page').onclick = () => { if ((tablePage + 1) * tablePageSize < data.rows.length) { tablePage += 1; renderTwoDa(); } };
  document.getElementById('add-row').onclick = () => {
    const next = clone(data); const index = next.rows.length;
    next.rows.push({ label: String(index), cells: next.columns.map(() => null) });
    edit({ action: 'replace2da', table: next });
  };
  document.getElementById('add-column').onclick = () => {
    const name = prompt('New column name'); if (!name?.trim()) return;
    const next = clone(data); next.columns.push(name.trim()); next.rows.forEach((row) => row.cells.push(null));
    edit({ action: 'replace2da', table: next });
  };
  document.getElementById('table-default').onchange = (event) => {
    const next = clone(data); next.default = cellValue(event.target.value); edit({ action: 'replace2da', table: next });
  };
  document.querySelectorAll('.cell').forEach((input) => input.onchange = () => edit({
    action: 'set2daCell', row: Number(input.dataset.row), column: input.dataset.column, value: cellValue(input.value),
  }));
  document.querySelectorAll('.row-label').forEach((input) => input.onchange = () => edit({ action: 'set2daRowLabel', row: Number(input.dataset.row), label: input.value }));
  document.querySelectorAll('.remove-row').forEach((button) => button.onclick = () => {
    const next = clone(data); next.rows.splice(Number(button.dataset.row), 1); edit({ action: 'replace2da', table: next });
  });
  document.querySelectorAll('.remove-column').forEach((button) => button.onclick = () => {
    const index = Number(button.dataset.column); const next = clone(data); next.columns.splice(index, 1); next.rows.forEach((row) => row.cells.splice(index, 1)); edit({ action: 'replace2da', table: next });
  });
}

function renderTlk() {
  const data = model.data;
  toolbar().innerHTML = `<label>Language <select id="tlk-language">${['English', 'French', 'German', 'Italian', 'Spanish', 'Polish'].map((language, index) => `<option value="${index}" ${data.language === index ? 'selected' : ''}>${language}</option>`).join('')}</select></label>
    <input id="tlk-search" type="search" placeholder="Search strref, text, or sound" value="${escapeAttribute(tlkQuery)}">
    <button id="tlk-search-button">Search</button><button id="tlk-add">Add entry</button><span class="spacer"></span>
    <span class="pager"><button id="tlk-prev" class="secondary">Previous</button><span>${data.total ? data.offset + 1 : 0}–${Math.min(data.offset + data.entries.length, data.total)} of ${data.total}</span><button id="tlk-next" class="secondary">Next</button></span>`;
  content().innerHTML = `<div class="table-wrap"><table><thead><tr><th>StrRef</th><th>Text</th><th>Sound</th><th>Length</th><th>Flags</th></tr></thead><tbody>
    ${data.entries.map((entry) => `<tr data-strref="${entry.strRef}"><td>${entry.strRef}</td>
      <td><textarea class="tlk-field tlk-text" data-field="text">${escapeHtml(entry.text)}</textarea></td>
      <td><input class="tlk-field" data-field="soundResRef" value="${escapeAttribute(entry.soundResRef)}"></td>
      <td><input class="tlk-field" data-field="soundLength" type="number" step="any" value="${entry.soundLength}"></td>
      <td><input class="tlk-field" data-field="flags" type="number" value="${entry.flags}"></td></tr>`).join('')}
    </tbody></table></div>`;
  const search = () => { tlkQuery = document.getElementById('tlk-search').value; tlkOffset = 0; refresh({ query: tlkQuery, offset: 0 }); };
  document.getElementById('tlk-search-button').onclick = search;
  document.getElementById('tlk-language').onchange = (event) => edit({ action: 'setTlkLanguage', language: Number(event.target.value) });
  document.getElementById('tlk-search').onkeydown = (event) => { if (event.key === 'Enter') search(); };
  document.getElementById('tlk-prev').onclick = () => { tlkOffset = Math.max(0, data.offset - data.limit); refresh({ query: tlkQuery, offset: tlkOffset }); };
  document.getElementById('tlk-next').onclick = () => { if (data.offset + data.entries.length < data.total) { tlkOffset = data.offset + data.limit; refresh({ query: tlkQuery, offset: tlkOffset }); } };
  document.getElementById('tlk-add').onclick = () => {
    const value = prompt('String reference', String(Math.max(0, data.highest + 1))); if (value == null) return;
    const strRef = Number(value); if (!Number.isInteger(strRef) || strRef < 0 || strRef > 0xffffffff) return showError('String reference must be between 0 and 4294967295.');
    edit({ action: 'setTlkEntry', strRef, entry: { text: '', soundResRef: '', soundLength: 0, flags: 0, volumeVariance: 0, pitchVariance: 0 } });
  };
  document.querySelectorAll('.tlk-field').forEach((input) => input.onchange = () => {
    const row = input.closest('tr'); const strRef = Number(row.dataset.strref);
    const current = data.entries.find((entry) => entry.strRef === strRef); const entry = clone(current);
    entry[input.dataset.field] = input.type === 'number' ? Number(input.value) : input.value;
    edit({ action: 'setTlkEntry', strRef, entry });
  });
}

function renderGff() {
  const data = model.data;
  toolbar().innerHTML = `<span>Type <strong>${escapeHtml(data.fileType)}</strong></span><span>Version <strong>${escapeHtml(data.fileVersion)}</strong></span><button id="gff-add">Add root field</button>`;
  content().innerHTML = `<div class="gff-root">${renderGffStruct(data.root, ['root'])}</div>`;
  document.getElementById('gff-add').onclick = () => addGffField(['root']);
  bindGffControls();
}

function renderGffStruct(structure, pathParts) {
  return `<details open><summary>Struct ${structure.id} · ${structure.fields.length} fields</summary><div class="gff-node">
    ${structure.fields.map((field, index) => renderGffField(field, [...pathParts, 'fields', index])).join('')}
    <button class="secondary gff-add-field" data-path="${encodePath(pathParts)}">Add field</button></div></details>`;
}

function renderGffField(field, pathParts) {
  const compound = field.kind === 'struct'
    ? renderGffStruct(field.value, [...pathParts, 'value'])
    : field.kind === 'list'
      ? `<details open><summary>List · ${field.value.length} structs</summary><div class="gff-node">${field.value.map((item, index) => `${renderGffStruct(item, [...pathParts, 'value', index])}<button class="danger gff-remove-list" data-path="${encodePath([...pathParts, 'value'])}" data-index="${index}">Remove struct</button>`).join('')}<button class="secondary gff-add-list" data-path="${encodePath([...pathParts, 'value'])}">Add struct</button></div></details>`
      : gffValueControl(field, pathParts);
  return `<div class="gff-field"><input class="gff-label" data-path="${encodePath(pathParts)}" value="${escapeAttribute(field.label)}" maxlength="16">
    <select class="gff-kind" data-path="${encodePath(pathParts)}">${gffKinds.map((kind) => `<option ${kind === field.kind ? 'selected' : ''}>${kind}</option>`).join('')}</select>
    <div>${compound}</div><button class="danger gff-remove" data-path="${encodePath(pathParts)}">Remove</button></div>`;
}

function gffValueControl(field, pathParts) {
  const valuePath = encodePath([...pathParts, 'value']);
  if (field.kind === 'locstring') return `<textarea class="gff-value" data-kind="locstring" data-path="${valuePath}">${escapeHtml(JSON.stringify(field.value, null, 2))}</textarea>`;
  if (field.kind === 'void') return `<textarea class="gff-value" data-kind="void" data-path="${valuePath}" title="Base64 encoded bytes">${escapeHtml(field.value)}</textarea>`;
  const numeric = ['byte', 'char', 'word', 'short', 'dword', 'int', 'float', 'double'].includes(field.kind);
  return `<input class="gff-value" data-kind="${field.kind}" data-path="${valuePath}" ${numeric ? 'type="number" step="any"' : ''} value="${escapeAttribute(String(field.value))}">`;
}

function bindGffControls() {
  document.querySelectorAll('.gff-value').forEach((input) => input.onchange = () => {
    let value = input.value;
    if (input.dataset.kind === 'locstring') { try { value = JSON.parse(value); } catch { return showError('Localized string value must be valid JSON.'); } }
    else if (['byte', 'char', 'word', 'short', 'dword', 'int', 'float', 'double'].includes(input.dataset.kind)) value = Number(value);
    const next = clone(model.data); setAtPath(next, decodePath(input.dataset.path), value); submitGff(next);
  });
  document.querySelectorAll('.gff-label').forEach((input) => input.onchange = () => {
    if (!input.value || new TextEncoder().encode(input.value).length > 16) return showError('GFF labels must be 1–16 bytes.');
    const next = clone(model.data); setAtPath(next, [...decodePath(input.dataset.path), 'label'], input.value); submitGff(next);
  });
  document.querySelectorAll('.gff-kind').forEach((select) => select.onchange = () => {
    const next = clone(model.data); const field = getAtPath(next, decodePath(select.dataset.path)); field.kind = select.value; field.value = defaultGffValue(select.value); submitGff(next);
  });
  document.querySelectorAll('.gff-remove').forEach((button) => button.onclick = () => {
    const pathParts = decodePath(button.dataset.path); const index = pathParts.pop(); const next = clone(model.data); getAtPath(next, pathParts).splice(index, 1); submitGff(next);
  });
  document.querySelectorAll('.gff-add-field').forEach((button) => button.onclick = () => addGffField(decodePath(button.dataset.path)));
  document.querySelectorAll('.gff-add-list').forEach((button) => button.onclick = () => {
    const next = clone(model.data); getAtPath(next, decodePath(button.dataset.path)).push({ id: 0, fields: [] }); submitGff(next);
  });
  document.querySelectorAll('.gff-remove-list').forEach((button) => button.onclick = () => {
    const next = clone(model.data); getAtPath(next, decodePath(button.dataset.path)).splice(Number(button.dataset.index), 1); submitGff(next);
  });
}

function addGffField(structPath) {
  const label = prompt('Field label (maximum 16 bytes)'); if (!label) return;
  if (new TextEncoder().encode(label).length > 16) return showError('GFF labels cannot exceed 16 bytes.');
  const next = clone(model.data); const structure = getAtPath(next, structPath);
  if (structure.fields.some((field) => field.label === label)) return showError(`Field ${label} already exists in this structure.`);
  structure.fields.push({ label, kind: 'int', value: 0 }); submitGff(next);
}

function submitGff(root) { edit({ action: 'replaceGff', root }); }

function renderTexture() {
  const data = model.data;
  toolbar().innerHTML = `${model.kind === 'plt' ? '' : '<button id="texture-import">Import image pixels…</button>'}<span>${data.width} × ${data.height}</span>`;
  content().innerHTML = `<div class="texture-layout"><div class="canvas-wrap"><canvas id="texture-canvas" width="${data.width}" height="${data.height}"></canvas></div>
    <aside class="inspector"><h2>Texture</h2><dl>${Object.entries(data.metadata || {}).filter(([key]) => key !== 'pixels').map(([key, value]) => `<dt>${escapeHtml(key)}</dt><dd>${escapeHtml(String(value))}</dd>`).join('')}</dl>
    ${model.kind === 'plt' ? '<div id="plt-inspector" class="muted">Click a pixel to edit its value and material layer.</div>' : ''}</aside></div>`;
  const canvas = document.getElementById('texture-canvas'); drawRgba(canvas, data.rgba);
  if (model.kind !== 'plt') document.getElementById('texture-import').onclick = () => vscode.postMessage({ type: 'importTexture' });
  if (model.kind === 'plt') canvas.onclick = (event) => showPltPixel(canvas, event);
}

function drawRgba(canvas, base64) {
  const bytes = Uint8ClampedArray.from(atob(base64), (character) => character.charCodeAt(0));
  const context = canvas.getContext('2d'); context.putImageData(new ImageData(bytes, canvas.width, canvas.height), 0, 0);
}

async function importTexture(base64) {
  const bytes = Uint8Array.from(atob(base64), (character) => character.charCodeAt(0));
  const blob = new Blob([bytes]); const bitmap = await createImageBitmap(blob);
  const canvas = document.createElement('canvas'); canvas.width = bitmap.width; canvas.height = bitmap.height;
  const context = canvas.getContext('2d'); context.drawImage(bitmap, 0, 0); bitmap.close();
  const rgba = context.getImageData(0, 0, canvas.width, canvas.height).data;
  edit({ action: 'replaceTexture', width: canvas.width, height: canvas.height, rgba: bytesToBase64(rgba) });
}

function showPltPixel(canvas, event) {
  const rect = canvas.getBoundingClientRect();
  const x = Math.min(canvas.width - 1, Math.max(0, Math.floor((event.clientX - rect.left) * canvas.width / rect.width)));
  const y = Math.min(canvas.height - 1, Math.max(0, Math.floor((event.clientY - rect.top) * canvas.height / rect.height)));
  const pixels = Uint8Array.from(atob(model.data.metadata.pixelData), (character) => character.charCodeAt(0));
  const offset = (y * canvas.width + x) * 2; const pixel = { value: pixels[offset], layer: pixels[offset + 1] }; const inspector = document.getElementById('plt-inspector');
  inspector.className = '';
  inspector.innerHTML = `<h3>Pixel ${x}, ${y}</h3><label>Value <input id="plt-value" type="number" min="0" max="255" value="${pixel.value}"></label>
    <label>Layer <select id="plt-layer">${['Skin', 'Hair', 'Metal 1', 'Metal 2', 'Cloth 1', 'Cloth 2', 'Leather 1', 'Leather 2', 'Tattoo 1', 'Tattoo 2'].map((label, index) => `<option value="${index}" ${pixel.layer === index ? 'selected' : ''}>${label}</option>`).join('')}</select></label><button id="plt-apply">Apply pixel</button>`;
  document.getElementById('plt-apply').onclick = () => edit({ action: 'setPltPixel', x, y, value: Number(document.getElementById('plt-value').value), layer: Number(document.getElementById('plt-layer').value) });
}

function renderArchive() {
  const data = model.data;
  const entries = data.entries;
  toolbar().innerHTML = `<button id="archive-add">Add resource…</button><input id="archive-search" type="search" placeholder="Filter resources" value="${escapeAttribute(data.query || '')}"><button id="archive-search-button">Search</button><span class="spacer"></span>
    <span class="pager"><button id="archive-prev" class="secondary">Previous</button><span>${data.total ? data.offset + 1 : 0}–${Math.min(data.offset + entries.length, data.total)} of ${data.total}</span><button id="archive-next" class="secondary">Next</button></span>`;
  const renderRows = () => {
    content().innerHTML = `<div class="table-wrap"><table><thead><tr><th>Resource</th>${model.kind === 'key' ? '<th>BIF</th>' : ''}<th>Type</th><th>Size</th><th>State</th><th>Actions</th></tr></thead><tbody>
      ${entries.map((entry) => `<tr><td>${escapeHtml(entry.resource)}</td>${model.kind === 'key' ? `<td>${escapeHtml(entry.bif || '')}</td>` : ''}<td>${escapeHtml(entry.extension || String(entry.typeId))}</td><td>${formatBytes(entry.size)}</td><td>${entry.modified ? 'Modified' : ''}</td><td><div class="archive-actions">
      ${isEditableType(entry.extension) ? `<button class="open-entry" data-resource="${escapeAttribute(entry.resource)}">Open</button>` : ''}
      <button class="secondary export-entry" data-resource="${escapeAttribute(entry.resource)}">Export</button><button class="secondary replace-entry" data-resource="${escapeAttribute(entry.resource)}">Replace</button><button class="secondary rename-entry" data-resource="${escapeAttribute(entry.resource)}">Rename</button><button class="danger remove-entry" data-resource="${escapeAttribute(entry.resource)}">Remove</button></div></td></tr>`).join('')}</tbody></table></div>`;
    bindArchiveRows();
  };
  renderRows();
  document.getElementById('archive-add').onclick = () => {
    let bifIndex;
    if (model.kind === 'key' && data.bifs.length > 1) {
      const choices = data.bifs.map((bif) => `${bif.index}: ${bif.filename}`).join('\n');
      const selected = prompt(`BIF index for the new resource:\n${choices}`, '0');
      if (selected == null) return;
      bifIndex = Number(selected);
      if (!Number.isInteger(bifIndex) || !data.bifs.some((bif) => bif.index === bifIndex)) return showError('Select a valid BIF index.');
    }
    vscode.postMessage({ type: 'addEntry', bifIndex });
  };
  const search = () => refresh({ query: document.getElementById('archive-search').value, offset: 0 });
  document.getElementById('archive-search-button').onclick = search;
  document.getElementById('archive-search').onkeydown = (event) => { if (event.key === 'Enter') search(); };
  document.getElementById('archive-prev').onclick = () => refresh({ query: data.query || '', offset: Math.max(0, data.offset - data.limit) });
  document.getElementById('archive-next').onclick = () => { if (data.offset + entries.length < data.total) refresh({ query: data.query || '', offset: data.offset + data.limit }); };
}

function bindArchiveRows() {
  document.querySelectorAll('.open-entry').forEach((button) => button.onclick = () => vscode.postMessage({ type: 'openEntry', resource: button.dataset.resource }));
  document.querySelectorAll('.export-entry').forEach((button) => button.onclick = () => vscode.postMessage({ type: 'exportEntry', resource: button.dataset.resource }));
  document.querySelectorAll('.replace-entry').forEach((button) => button.onclick = () => vscode.postMessage({ type: 'replaceEntry', resource: button.dataset.resource }));
  document.querySelectorAll('.rename-entry').forEach((button) => button.onclick = () => { const newResource = prompt('New resource name', button.dataset.resource); if (newResource && newResource !== button.dataset.resource) edit({ action: 'renameEntry', resource: button.dataset.resource, newResource }); });
  document.querySelectorAll('.remove-entry').forEach((button) => button.onclick = () => { if (confirm(`Remove ${button.dataset.resource}?`)) edit({ action: 'removeEntry', resource: button.dataset.resource }); });
}

function createProgram(gl, vertexSource, fragmentSource) {
  const compile = (type, source) => {
    const shader = gl.createShader(type); gl.shaderSource(shader, source); gl.compileShader(shader);
    if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
      const message = gl.getShaderInfoLog(shader); gl.deleteShader(shader); throw new Error(`WebGL shader compilation failed: ${message}`);
    }
    return shader;
  };
  const vertex = compile(gl.VERTEX_SHADER, vertexSource); const fragment = compile(gl.FRAGMENT_SHADER, fragmentSource);
  const program = gl.createProgram(); gl.attachShader(program, vertex); gl.attachShader(program, fragment); gl.linkProgram(program);
  gl.deleteShader(vertex); gl.deleteShader(fragment);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
    const message = gl.getProgramInfoLog(program); gl.deleteProgram(program); throw new Error(`WebGL program link failed: ${message}`);
  }
  return program;
}

function uniformLocations(gl, program, names) {
  return Object.fromEntries(names.map((name) => [name, gl.getUniformLocation(program, name)]));
}

function numericView(binary, view) {
  if (!(binary instanceof Uint8Array)) throw new Error('A packed scene payload is not binary data.');
  if (!view || !Number.isSafeInteger(view.byteOffset) || view.byteOffset < 0
      || !Number.isSafeInteger(view.byteLength) || view.byteLength < 0) {
    throw new Error('A packed scene buffer view has an invalid byte range.');
  }
  const bytes = binary.buffer;
  const offset = binary.byteOffset + view.byteOffset;
  if (offset + view.byteLength > binary.byteOffset + binary.byteLength) throw new Error('A packed scene buffer view is out of range.');
  if (view.component === 'u8') return new Uint8Array(bytes, offset, view.byteLength);
  if (view.byteOffset % 4 !== 0 || offset % 4 !== 0 || view.byteLength % 4 !== 0) {
    throw new Error(`A packed ${view.component} buffer view is not aligned to four bytes.`);
  }
  const count = view.byteLength / 4;
  if (view.component === 'u32') return new Uint32Array(bytes, offset, count);
  if (view.component === 'i32') return new Int32Array(bytes, offset, count);
  if (view.component === 'f32') return new Float32Array(bytes, offset, count);
  throw new Error(`Unsupported packed component ${view.component}.`);
}

function createSpriteGpu(gl) {
  const vao = gl.createVertexArray(); const cornerBuffer = gl.createBuffer(); const instanceBuffer = gl.createBuffer();
  gl.bindVertexArray(vao); gl.bindBuffer(gl.ARRAY_BUFFER, cornerBuffer);
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]), gl.STATIC_DRAW);
  gl.enableVertexAttribArray(0); gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0);
  gl.bindBuffer(gl.ARRAY_BUFFER, instanceBuffer); const stride = 14 * 4;
  for (const [location, size, offset] of [[1, 3, 0], [2, 4, 3], [3, 3, 7], [4, 4, 10]]) {
    gl.enableVertexAttribArray(location); gl.vertexAttribPointer(location, size, gl.FLOAT, false, stride, offset * 4); gl.vertexAttribDivisor(location, 1);
  }
  return { vao, cornerBuffer, instanceBuffer, capacity: 0 };
}

function uploadAndDrawSprites(gl, gpu, values, count) {
  gl.bindVertexArray(gpu.vao); gl.bindBuffer(gl.ARRAY_BUFFER, gpu.instanceBuffer);
  const byteLength = count * 14 * 4;
  if (byteLength > gpu.capacity) {
    gpu.capacity = Math.max(byteLength, Math.ceil(gpu.capacity * 1.5), 14 * 4);
    gl.bufferData(gl.ARRAY_BUFFER, gpu.capacity, gl.DYNAMIC_DRAW);
  }
  gl.bufferSubData(gl.ARRAY_BUFFER, 0, values, 0, count * 14);
  gl.drawArraysInstanced(gl.TRIANGLE_STRIP, 0, 4, count);
}

function createOverlayGpu(gl, polygon) {
  if (!polygon?.length) return undefined;
  const values = new Float32Array(polygon.length * 3);
  for (let index = 0; index < polygon.length; index += 1) values.set(polygon[index], index * 3);
  const vao = gl.createVertexArray(); const buffer = gl.createBuffer();
  gl.bindVertexArray(vao); gl.bindBuffer(gl.ARRAY_BUFFER, buffer); gl.bufferData(gl.ARRAY_BUFFER, values, gl.STATIC_DRAW);
  gl.enableVertexAttribArray(0); gl.vertexAttribPointer(0, 3, gl.FLOAT, false, 0, 0);
  return { vao, buffer, count: polygon.length };
}

function appendChunkInstance(batch, matrix) {
  const required = (batch.count + 1) * 16;
  if (required > batch.values.length) {
    const grown = new Float32Array(Math.max(required, Math.ceil(batch.values.length * 1.5)));
    grown.set(batch.values); batch.values = grown;
  }
  batch.values.set(matrix, batch.count * 16); batch.count += 1;
}

function bindInstanceMatrices(gl, vao, buffer) {
  gl.bindVertexArray(vao); gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
  for (let column = 0; column < 4; column += 1) {
    const location = 6 + column; gl.enableVertexAttribArray(location);
    gl.vertexAttribPointer(location, 4, gl.FLOAT, false, 16 * 4, column * 4 * 4);
    gl.vertexAttribDivisor(location, 1);
  }
}

function emitterProperty(emitter, name, fallback) {
  if (!emitter) return fallback;
  let properties = EMITTER_PROPERTY_CACHE.get(emitter);
  if (!properties) {
    properties = new Map((emitter.properties || []).map((entry) => [entry.name.toLowerCase(), entry.values || []]));
    EMITTER_PROPERTY_CACHE.set(emitter, properties);
  }
  const tagged = properties.get(name.toLowerCase())?.[0];
  if (tagged == null) return fallback; const value = typeof tagged === 'object' && 'value' in tagged ? tagged.value : tagged;
  return value == null ? fallback : value;
}

function emitterVector(emitter, name, fallback) {
  return Array.from(emitterVectorInto(emitter, name, fallback, new Float32Array(3)));
}

function emitterVectorInto(emitter, name, fallback, output) {
  if (!emitter) { output.set(fallback); return output; }
  emitterProperty(emitter, name, undefined);
  let vectors = EMITTER_VECTOR_CACHE.get(emitter);
  if (!vectors) { vectors = new Map(); EMITTER_VECTOR_CACHE.set(emitter, vectors); }
  const key = name.toLowerCase(); let values = vectors.get(key);
  if (!values) {
    values = (EMITTER_PROPERTY_CACHE.get(emitter)?.get(key) || [])
      .map((tagged) => typeof tagged === 'object' && 'value' in tagged ? Number(tagged.value) : Number(tagged)).filter(Number.isFinite);
    vectors.set(key, values);
  }
  for (let index = 0; index < 3; index += 1) output[index] = values?.length >= 3 ? values[index] : fallback[index];
  return output;
}

function sampleEmitterValue(binary, nodeTrack, name, time, fallback) {
  return samplePreparedEmitterValue(preparedEmitterTrack(binary, nodeTrack, name), time, fallback);
}

function samplePreparedEmitterValue(track, time, fallback) {
  if (!track) return Number(fallback) || 0;
  const times = track.times; let start = 0; let end = 0; let factor = 0;
  if (times.length > 1 && time > times[0]) {
    if (time >= times[times.length - 1]) start = end = times.length - 1;
    else {
      let low = 1; let high = times.length - 1;
      while (low < high) { const middle = (low + high) >>> 1; if (time <= times[middle]) high = middle; else low = middle + 1; }
      end = low; start = end - 1; factor = Math.max(0, Math.min(1, (time-times[start])/Math.max(Number.EPSILON,times[end]-times[start])));
    }
  }
  if (track.bezier && start !== end) factor = cubicBezierFactor(factor);
  const leftOffset = track.offsets[start]; const rightOffset = track.offsets[end];
  const left = Number(track.values[leftOffset] ?? fallback ?? 0); const right = Number(track.values[rightOffset] ?? left);
  return left + (right - left) * factor;
}

function sampleEmitterVector(binary, nodeTrack, name, time, fallback) {
  return sampleEmitterVectorInto(binary, nodeTrack, name, time, fallback, new Float32Array(fallback.length), new Float64Array(3));
}

function sampleEmitterVectorInto(binary, nodeTrack, name, time, fallback, result, interval) {
  return samplePreparedEmitterVectorInto(preparedEmitterTrack(binary, nodeTrack, name), time, fallback, result, interval);
}

function samplePreparedEmitterVectorInto(track, time, fallback, result, interval) {
  if (!track) { result.set(fallback); return result; }
  sampleIntervalInto(track.times, time, interval, track.bezier); const start=interval[0],end=interval[1],factor=interval[2];
  const leftOffset = track.offsets[start]; const rightOffset = track.offsets[end];
  for (let index = 0; index < fallback.length; index += 1) {
    const left = Number(track.values[leftOffset + index] ?? fallback[index]); const right = Number(track.values[rightOffset + index] ?? left);
    result[index] = left + (right - left) * factor;
  }
  return result;
}

function preparedEmitterTrack(binary, nodeTrack, name) {
  if (!nodeTrack) return undefined;
  let controllers = EMITTER_TRACK_CACHE.get(nodeTrack);
  if (!controllers) {
    controllers = new Map((nodeTrack.emitterControllers || []).map((entry) => [entry.controller.toLowerCase(), {
      times: numericView(binary, entry.times),
      values: numericView(binary, entry.values.values),
      offsets: numericView(binary, entry.values.rowOffsets),
      bezier: entry.bezierKeyed === true,
    }]));
    EMITTER_TRACK_CACHE.set(nodeTrack, controllers);
  }
  return controllers.get(name.toLowerCase());
}

function sampleIntervalInto(times, time, result, bezier = false) {
  if (!times.length || times.length === 1 || time <= times[0]) { result[0]=0; result[1]=0; result[2]=0; return result; }
  const last = times.length - 1; if (time >= times[last]) { result[0]=last; result[1]=last; result[2]=0; return result; }
  let low = 1; let high = last;
  while (low < high) { const middle = (low + high) >>> 1; if (time <= times[middle]) high = middle; else low = middle + 1; }
  const end = low; const start = end - 1;
  const factor=Math.max(0,Math.min(1,(time-times[start])/Math.max(Number.EPSILON,times[end]-times[start])));
  result[0]=start; result[1]=end; result[2]=bezier?cubicBezierFactor(factor):factor; return result;
}

function random01(index, stream) {
  const value = Math.sin((index + 1) * 12.9898 + (stream + 1) * 78.233) * 43758.5453123; return value - Math.floor(value);
}

function stagedValue3(age, midpoint, start, middle, end) {
  if (age <= midpoint) { const factor = age / midpoint; return start + (middle - start) * factor; }
  const factor = (age - midpoint) / (1 - midpoint); return middle + (end - middle) * factor;
}

function createTexture(gl, texture, binary, s3tc) {
  const handle = gl.createTexture(); gl.bindTexture(gl.TEXTURE_2D, handle);
  const compressedLevels = Array.isArray(texture.mipLevels) ? texture.mipLevels : [];
  if (texture.compression && compressedLevels.length > 0 && s3tc) {
    const format = texture.compression === 'dxt1'
      ? s3tc.COMPRESSED_RGBA_S3TC_DXT1_EXT
      : texture.compression === 'dxt5'
        ? s3tc.COMPRESSED_RGBA_S3TC_DXT5_EXT
        : undefined;
    if (format === undefined) throw new Error(`Unsupported compressed texture format ${texture.compression}.`);
    for (let level = 0; level < compressedLevels.length; level += 1) {
      const mip = compressedLevels[level];
      gl.compressedTexImage2D(gl.TEXTURE_2D, level, format, mip.width, mip.height, 0, numericView(binary, mip.data));
    }
  } else {
    if (!texture.rgba8) throw new Error('Texture asset has neither a supported compressed payload nor RGBA pixels.');
    gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, true);
    try {
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA8, texture.width, texture.height, 0, gl.RGBA, gl.UNSIGNED_BYTE, numericView(binary, texture.rgba8));
    } finally {
      // Pixel-store flags are global WebGL state. Leaving this enabled reverses
      // the rows of later bone-matrix and point-light data textures.
      gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, false);
    }
    gl.generateMipmap(gl.TEXTURE_2D);
  }
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.REPEAT); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.REPEAT);
  const hasMipmaps = compressedLevels.length > 1 || !texture.compression;
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, hasMipmaps ? gl.LINEAR_MIPMAP_LINEAR : gl.LINEAR); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
  return handle;
}

function bindMaterialTexture(gl, samplerLocation, enabledLocation, texture, unit) {
  gl.uniform1i(enabledLocation, Boolean(texture?.handle));
  if (!texture?.handle) return;
  gl.activeTexture(gl.TEXTURE0 + unit); gl.bindTexture(gl.TEXTURE_2D, texture.handle);
  const clamp = directiveValue(texture.binding, 'clamp') === '1'; const nearest = directiveValue(texture.binding, 'filter')?.toLowerCase() === 'nearest';
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, clamp ? gl.CLAMP_TO_EDGE : gl.REPEAT); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, clamp ? gl.CLAMP_TO_EDGE : gl.REPEAT);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, nearest ? gl.NEAREST : gl.LINEAR); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, nearest ? gl.NEAREST_MIPMAP_NEAREST : gl.LINEAR_MIPMAP_LINEAR);
  gl.uniform1i(samplerLocation, unit);
}

function directiveValue(binding, name) {
  if (!binding) return undefined;
  let directives = DIRECTIVE_CACHE.get(binding);
  if (!directives) {
    directives = new Map((binding.directives || []).map((directive) => [directive.name.toLowerCase(), directive.arguments || []]));
    DIRECTIVE_CACHE.set(binding, directives);
  }
  return directives.get(name.toLowerCase())?.[0];
}

function textureUvTransform(binding, time, output = new Float32Array(4)) {
  const procedure = directiveValue(binding, 'proceduretype')?.toLowerCase();
  if (procedure !== 'cycle') { output[0]=1; output[1]=1; output[2]=0; output[3]=0; return output; }
  const x = Math.max(1, Number(directiveValue(binding, 'numx')) || 1); const y = Math.max(1, Number(directiveValue(binding, 'numy')) || 1); const fps = Math.max(0, Number(directiveValue(binding, 'fps')) || 1); const frame = Math.floor(time * fps) % (x * y);
  output[0]=1/x; output[1]=1/y; output[2]=(frame%x)/x; output[3]=Math.floor(frame/x)/y; return output;
}

function applyBlendMode(gl, binding) {
  const blending = directiveValue(binding, 'blending')?.toLowerCase(); gl.enable(gl.BLEND);
  if (blending === 'additive') gl.blendFunc(gl.SRC_ALPHA, gl.ONE);
  else gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);
}

function createModelRuntime(model) {
  const nodeByName = new Map(model.nodes.map((node, index) => [node.name.toLowerCase(), index]));
  const nodes = model.nodes.map(clonePoseNode);
  const materials = model.materials.map((material, materialIndex) => {
    const resolved = model.resolvedMaterials.find((entry) => entry.materialIndex === materialIndex);
    return {
      textures: new Map((resolved?.textures || [])
        .filter((entry) => entry.texture != null)
        .map((entry) => [entry.role, { binding: entry, texture: entry.texture, handle: undefined, uvTransform: new Float32Array([1, 1, 0, 0]) }])),
    };
  });
  const materialsByNode = Array.from({ length: model.nodes.length }, () => []);
  model.materials.forEach((material, index) => {
    if (material.sourceNode != null && materialsByNode[material.sourceNode]) materialsByNode[material.sourceNode].push(index);
  });
  const pose = {
    nodes,
    materials: model.materials.map(() => ({ active: false, alpha: undefined, selfIllumColor: new Float32Array(3) })),
    worlds: model.nodes.map(() => identity4()),
  };
  const runtime = {
    nodeByName,
    bindWorlds: resolveNodeWorlds(model, nodes),
    hiddenNodes: new Set(model.hiddenGeometryNodes.map((name) => nodeByName.get(name.toLowerCase())).filter(Number.isInteger)),
    materials,
    materialsByNode,
    nodeTextures: new Map(model.nodeTextures.filter((entry) => entry.texture != null).map((entry) => [`${entry.nodeIndex}:${entry.role}`, entry])),
    attachmentTargets: new Map(model.attachments.map((attachment) => [attachment, nodeByName.get(attachment.targetNodeName.toLowerCase()) ?? -1])),
    animationAssets: new Map(),
    emitterBuffers: new Array(model.nodes.length),
    emitterColors: model.nodes.map(() => [new Float32Array(3), new Float32Array(3), new Float32Array(3)]),
    emitterIntervals: model.nodes.map(() => new Float64Array(3)),
    emitterTransitionVectors: model.nodes.map(() => new Float32Array(3)),
    emitterTransitionIntervals: model.nodes.map(() => new Float64Array(3)),
    flareBuffer: new Float32Array(14),
    chunkTranslation: new Float32Array(3),
    chunkRotation: new Float32Array(4),
    chunkScale: new Float32Array(3),
    chunkLocalMatrix: identity4(),
    chunkWorldMatrix: identity4(),
    drawWorld: identity4(),
    drawMvp: identity4(),
    attachmentWorld: identity4(),
    emitterWorld: identity4(),
    effectWorld: identity4(),
    effectAttachment: identity4(),
    instancedLocal: identity4(),
    instancedAttachment: identity4(),
    lightWorld: identity4(),
    lightAttachment: identity4(),
    lightRow: new Float32Array(12),
    localMatrices: model.nodes.map(() => identity4()),
    worldState: new Uint8Array(model.nodes.length),
    scalarScratch: new Float32Array(1),
    pose,
    poseResult: { asset: undefined, pose },
    poseFrame: -1,
  };
  runtime.inverseBindWorlds = runtime.bindWorlds.map((world) => inverse4(world));
  return runtime;
}

function clonePoseNode(node) {
  return {
    ...node,
    translation: Float32Array.from(node.translation),
    rotationAxisAngle: Float32Array.from(node.rotationAxisAngle),
    scale: Float32Array.from(node.scale),
    color: Float32Array.from(node.color || [1, 1, 1]),
    light: node.light ? { ...node.light } : undefined,
  };
}

function clonePose(pose) {
  return {
    nodes: pose.nodes.map(clonePoseNode),
    materials: pose.materials.map((material) => ({
      active: material.active,
      alpha: material.alpha,
      selfIllumColor: Float32Array.from(material.selfIllumColor),
    })),
    worlds: pose.worlds.map((world) => Float32Array.from(world)),
  };
}

function animationAssetKey(modelIndex, animationIndex) {
  return `${modelIndex}:${animationIndex}`;
}

function createAnimationAsset(scene, modelIndex, animationIndex, animation, binary) {
  if (!(binary instanceof Uint8Array)) throw new Error(`Animation asset ${modelIndex}:${animationIndex} has no binary payload.`);
  return { sceneKey: scene.manifest.assetKey, modelIndex, animationIndex, animation, binary };
}

function installAnimationAsset(runtime, asset) {
  const installed = { ...asset, runtime: indexAnimationRuntime(runtime, asset.animation, asset.binary) };
  runtime.animationAssets.set(asset.animationIndex, installed);
  return installed;
}

function indexAnimationRuntime(runtime, animation, binary) {
  const tracksByNode = new Array(runtime.pose.nodes.length);
  const tracks = [];
  for (const track of animation.nodeTracks || []) {
    const nodeIndex = track.targetNode ?? runtime.nodeByName.get(String(track.targetName || '').toLowerCase());
    if (!Number.isInteger(nodeIndex) || nodeIndex < 0) continue;
    const bezier = new Set((track.bezierControllers || []).map((name) => String(name).toLowerCase()));
    const prepared = {
      source: track,
      nodeIndex,
      translation: preparePackedTrack(binary, track.translation, bezier.has('position')),
      rotationAxisAngle: preparePackedTrack(binary, track.rotationAxisAngle, bezier.has('orientation')),
      scale: preparePackedTrack(binary, track.scale, bezier.has('scale')),
      color: preparePackedTrack(binary, track.color, bezier.has('color')),
      alpha: preparePackedTrack(binary, track.alpha, bezier.has('alpha')),
      radius: preparePackedTrack(binary, track.radius, bezier.has('radius')),
      multiplier: preparePackedTrack(binary, track.multiplier, bezier.has('multiplier')),
      shadowRadius: preparePackedTrack(binary, track.shadowRadius, bezier.has('shadowradius')),
      verticalDisplacement: preparePackedTrack(binary, track.verticalDisplacement, bezier.has('verticaldisplacement')),
      selfIllumColor: preparePackedTrack(binary, track.selfIllumColor, bezier.has('selfillumcolor')),
      emitterControllers: prepareEmitterControllers(binary, track.emitterControllers),
      animmesh: prepareAnimMeshTrack(binary, track.animmesh),
    };
    tracksByNode[nodeIndex] = prepared; tracks.push(prepared);
  }
  return { tracksByNode, tracks };
}

function preparePackedTrack(binary, track, bezier = false) {
  return track && binary ? {
    times: numericView(binary, track.times),
    values: numericView(binary, track.values),
    width: track.values.componentsPerElement,
    bezier,
  } : undefined;
}

function prepareEmitterControllers(binary, entries = []) {
  return new Map(entries.map((entry) => [entry.controller.toLowerCase(), {
    times: numericView(binary, entry.times),
    values: numericView(binary, entry.values.values),
    offsets: numericView(binary, entry.values.rowOffsets),
    bezier: entry.bezierKeyed === true,
  }]));
}

function prepareAnimMeshTrack(binary, track) {
  return track ? {
    ...track,
    vertexValues: numericView(binary, track.vertexSamples),
    uvValues: numericView(binary, track.uvSamples),
  } : undefined;
}

function nodeDepth(model, node) {
  let depth = 0; let parent = node.parent; const visited = new Set();
  while (parent != null && model.nodes[parent] && !visited.has(parent)) { visited.add(parent); depth += 1; parent = model.nodes[parent].parent; }
  return depth;
}

function resolveNodeWorlds(model, nodes) {
  const result = new Array(nodes.length); const visiting = new Set();
  const resolve = (index) => {
    if (result[index]) return result[index];
    if (visiting.has(index)) throw new Error(`Model ${model.name} contains a node parent cycle at ${nodes[index]?.name || index}.`);
    const node = nodes[index]; if (!node) return identity4(); visiting.add(index);
    const local = multiply4(translation4(node.translation), multiply4(axisAngle4(node.rotationAxisAngle), scale4(node.scale)));
    result[index] = node.parent == null ? local : multiply4(resolve(node.parent), local); visiting.delete(index); return result[index];
  };
  nodes.forEach((_node, index) => resolve(index)); return result;
}

function resolveNodeWorldsInto(runtime, model, nodes, worlds) {
  runtime.worldState.fill(0);
  const resolve = (index) => {
    if (runtime.worldState[index] === 2) return worlds[index];
    if (runtime.worldState[index] === 1) throw new Error(`Model ${model.name} contains a node parent cycle at ${nodes[index]?.name || index}.`);
    runtime.worldState[index] = 1;
    const node = nodes[index]; const local = runtime.localMatrices[index];
    composeTransform4Into(node.translation, node.rotationAxisAngle, node.scale, local);
    if (node.parent == null) worlds[index].set(local);
    else multiply4Into(resolve(node.parent), local, worlds[index]);
    runtime.worldState[index] = 2;
    return worlds[index];
  };
  for (let index = 0; index < nodes.length; index += 1) resolve(index);
  return worlds;
}

function sampleModelPoseInto(runtime, model, asset, time) {
  const { nodes, materials, worlds } = runtime.pose;
  for (let index = 0; index < nodes.length; index += 1) {
    const source = model.nodes[index]; const target = nodes[index];
    target.translation.set(source.translation); target.rotationAxisAngle.set(source.rotationAxisAngle); target.scale.set(source.scale);
    target.color.set(source.color || [1, 1, 1]); target.alpha = source.alpha; target.radius = source.radius;
    if (target.light && source.light) Object.assign(target.light, source.light);
  }
  for (const state of materials) { state.active = false; state.alpha = undefined; state.selfIllumColor.fill(0); }
  if (!asset) { resolveNodeWorldsInto(runtime, model, nodes, worlds); return runtime.pose; }
  const { animation } = asset;
  const sampledTime = animation.length > 0 ? ((time % animation.length) + animation.length) % animation.length : Math.max(0, time);
  const animationRuntime = asset.runtime;
  for (const track of animationRuntime.tracks) {
    const nodeIndex = track.nodeIndex; const node = nodes[nodeIndex]; const source = model.nodes[nodeIndex];
    samplePreparedTrackInto(track.translation, sampledTime, source.translation, node.translation);
    samplePreparedTrackInto(track.rotationAxisAngle, sampledTime, source.rotationAxisAngle, node.rotationAxisAngle, true);
    samplePreparedTrackInto(track.scale, sampledTime, source.scale, node.scale);
    samplePreparedTrackInto(track.color, sampledTime, source.color || [1, 1, 1], node.color);
    runtime.scalarScratch[0] = source.alpha ?? 1; samplePreparedTrackInto(track.alpha, sampledTime, runtime.scalarScratch, runtime.scalarScratch); node.alpha = runtime.scalarScratch[0];
    runtime.scalarScratch[0] = source.radius ?? 0; samplePreparedTrackInto(track.radius, sampledTime, runtime.scalarScratch, runtime.scalarScratch); node.radius = runtime.scalarScratch[0];
    if (node.light) {
      runtime.scalarScratch[0] = source.light.multiplier; samplePreparedTrackInto(track.multiplier, sampledTime, runtime.scalarScratch, runtime.scalarScratch); node.light.multiplier = runtime.scalarScratch[0];
      runtime.scalarScratch[0] = source.light.shadowRadius; samplePreparedTrackInto(track.shadowRadius, sampledTime, runtime.scalarScratch, runtime.scalarScratch); node.light.shadowRadius = runtime.scalarScratch[0];
      runtime.scalarScratch[0] = source.light.verticalDisplacement; samplePreparedTrackInto(track.verticalDisplacement, sampledTime, runtime.scalarScratch, runtime.scalarScratch); node.light.verticalDisplacement = runtime.scalarScratch[0];
    }
    for (const materialIndex of runtime.materialsByNode[nodeIndex]) {
      const material = model.materials[materialIndex]; const state = materials[materialIndex];
      state.active = true;
      runtime.scalarScratch[0] = material.alpha ?? 1; samplePreparedTrackInto(track.alpha, sampledTime, runtime.scalarScratch, runtime.scalarScratch); state.alpha = runtime.scalarScratch[0];
      samplePreparedTrackInto(track.selfIllumColor, sampledTime, material.selfIllumColor || ZERO_COLOR, state.selfIllumColor);
    }
  }
  resolveNodeWorldsInto(runtime, model, nodes, worlds);
  return runtime.pose;
}

function blendPoseInto(target, source, factor, model) {
  const amount = Math.max(0, Math.min(1, factor));
  for (let nodeIndex = 0; nodeIndex < target.nodes.length; nodeIndex += 1) {
    const to = target.nodes[nodeIndex]; const from = source.nodes[nodeIndex]; if (!from) continue;
    lerpArrayInto(from.translation, to.translation, amount, to.translation);
    slerpAxisAngleValuesInto(from.rotationAxisAngle, to.rotationAxisAngle, amount, to.rotationAxisAngle);
    lerpArrayInto(from.scale, to.scale, amount, to.scale);
    lerpArrayInto(from.color, to.color, amount, to.color);
    to.alpha = lerpOptionalNumber(from.alpha, to.alpha, amount);
    to.radius = lerpOptionalNumber(from.radius, to.radius, amount);
    if (to.light && from.light) {
      to.light.multiplier = lerpNumber(from.light.multiplier, to.light.multiplier, amount);
      to.light.shadowRadius = lerpNumber(from.light.shadowRadius, to.light.shadowRadius, amount);
      to.light.verticalDisplacement = lerpNumber(from.light.verticalDisplacement, to.light.verticalDisplacement, amount);
    }
  }
  for (let materialIndex = 0; materialIndex < target.materials.length; materialIndex += 1) {
    const to = target.materials[materialIndex]; const from = source.materials[materialIndex]; if (!from) continue;
    const material = model.materials[materialIndex]; const baseAlpha = material?.alpha ?? 1; const baseSelfIllum = material?.selfIllumColor || ZERO_COLOR;
    const fromAlpha = from.active ? from.alpha : baseAlpha; const toAlpha = to.active ? to.alpha : baseAlpha;
    const fromSelfIllum = from.active ? from.selfIllumColor : baseSelfIllum; const toSelfIllum = to.active ? to.selfIllumColor : baseSelfIllum;
    to.active = true; to.alpha = lerpNumber(fromAlpha, toAlpha, amount);
    lerpArrayInto(fromSelfIllum, toSelfIllum, amount, to.selfIllumColor);
  }
  return target;
}

function lerpNumber(from, to, factor) {
  const left = Number.isFinite(from) ? from : Number.isFinite(to) ? to : 0;
  const right = Number.isFinite(to) ? to : left;
  return left + (right - left) * factor;
}

function lerpOptionalNumber(from, to, factor) {
  if (!Number.isFinite(from) && !Number.isFinite(to)) return undefined;
  return lerpNumber(from, to, factor);
}

function lerpArrayInto(from, to, factor, output) {
  for (let index = 0; index < output.length; index += 1) output[index] = lerpNumber(from?.[index], to?.[index], factor);
  return output;
}

function collectSceneLights(scene, poseForModel, modelRuntime, instanceRuntime, target) {
  target.count = 0;
  const append = (values) => {
    const required = (target.count + 1) * 12;
    if (required > target.storage.length) {
      const grown = new Float32Array(Math.max(required, Math.ceil(target.storage.length * 1.5)));
      grown.set(target.storage); target.storage = grown;
    }
    target.storage.set(values, target.count * 12); target.count += 1;
  };
  const collectModel = (modelIndex, base, stack, lightOverrides = []) => {
    if (stack.has(modelIndex)) return; const model = scene.manifest.models[modelIndex]; const runtime = modelRuntime[modelIndex]; if (!model || !runtime) return; stack.add(modelIndex);
    const pose = poseForModel(modelIndex).pose; const worlds = pose.worlds; let lightIndex = 0;
    pose.nodes.forEach((node, nodeIndex) => {
      if (!node.light) return; const world = multiply4Into(base, worlds[nodeIndex] || IDENTITY_MATRIX, runtime.lightWorld); const z=node.light.verticalDisplacement||0; const multiplier = node.light.negativeLight ? -Math.abs(node.light.multiplier) : node.light.multiplier;
      const override = lightOverrideForNode(node.name, lightOverrides, lightIndex); lightIndex += 1; const color = override || node.color;
      const row=runtime.lightRow; row[0]=world[8]*z+world[12]; row[1]=world[9]*z+world[13]; row[2]=world[10]*z+world[14]; row[3]=Math.max(0.01,node.radius||node.light.flareRadius||10); row[4]=color[0]; row[5]=color[1]; row[6]=color[2]; row[7]=multiplier; row[8]=node.light.ambientOnly?1:0; row[9]=node.light.affectDynamic?1:0; row[10]=node.light.lightPriority||0; row[11]=0; append(row);
    });
    for (const attachment of model.attachments) { const nodeIndex=runtime.attachmentTargets.get(attachment); multiply4Into(base,worlds[nodeIndex]||IDENTITY_MATRIX,runtime.lightAttachment); collectModel(attachment.model,runtime.lightAttachment,new Set(stack)); }
  };
  for (const { instance, base } of instanceRuntime) if (instance.model != null && instance.kind !== 'collision' && instance.kind !== 'skybox') collectModel(instance.model,base,new Set(),instance.lightColorOverrides);
  if (target.count === 0) target.values.fill(0);
  else if (target.values.buffer !== target.storage.buffer || target.values.length !== target.count*12) target.values=target.storage.subarray(0,target.count*12);
  return target;
}

function lightOverrideForNode(name, overrides, fallbackIndex) {
  const normalized = String(name || '').toLowerCase().replaceAll('_', '');
  const named = normalized.includes('mainlight1') ? 0 : normalized.includes('mainlight2') ? 1 : normalized.includes('sourcelight1') ? 2 : normalized.includes('sourcelight2') ? 3 : fallbackIndex;
  return overrides?.[named] || undefined;
}

function samplePackedTrack(binary, track, time, fallback, rotation = false) {
  const output = new Float32Array(fallback.length);
  samplePreparedTrackInto(preparePackedTrack(binary, track), time, fallback, output, rotation);
  return output;
}

function samplePreparedTrackInto(track, time, fallback, output, rotation = false) {
  if (!track?.times.length || !track.values.length) { output.set(fallback); return output; }
  const { times, values, width } = track;
  let start = 0; let end = 0; let factor = 0;
  if (times.length === 1 || time <= times[0]) end = 0;
  else if (time >= times[times.length - 1]) { start = times.length - 1; end = start; }
  else {
    let low = 1; let high = times.length - 1;
    while (low < high) { const middle = (low + high) >>> 1; if (time <= times[middle]) high = middle; else low = middle + 1; }
    end = low; start = end - 1;
    factor = Math.max(0, Math.min(1, (time - times[start]) / Math.max(Number.EPSILON, times[end] - times[start])));
  }
  if (track.bezier && start !== end) factor = cubicBezierFactor(factor);
  if (rotation && width >= 4 && start !== end) return slerpAxisAngleInto(values, start * width, end * width, factor, output);
  for (let index = 0; index < output.length; index += 1) {
    const left = Number(values[start * width + index] ?? fallback[index] ?? 0);
    const right = Number(values[end * width + index] ?? left);
    output[index] = left + (right - left) * factor;
  }
  return output;
}

function cubicBezierFactor(factor) {
  const clamped = Math.max(0, Math.min(1, factor));
  return clamped * clamped * (3 - 2 * clamped);
}

function slerpAxisAngleInto(values, leftOffset, rightOffset, factor, output) {
  return slerpAxisAngleRawInto(
    values[leftOffset], values[leftOffset+1], values[leftOffset+2], values[leftOffset+3],
    values[rightOffset], values[rightOffset+1], values[rightOffset+2], values[rightOffset+3],
    factor, output,
  );
}

function slerpAxisAngleValuesInto(left, right, factor, output) {
  return slerpAxisAngleRawInto(
    left[0], left[1], left[2], left[3], right[0], right[1], right[2], right[3], factor, output,
  );
}

function slerpAxisAngleRawInto(lax, lay, laz, la, rax, ray, raz, ra, factor, output) {
  const leftLength=Math.hypot(lax,lay,laz),rightLength=Math.hypot(rax,ray,raz);
  const leftSine=leftLength&&la?Math.sin(la/2)/leftLength:0,rightSine=rightLength&&ra?Math.sin(ra/2)/rightLength:0;
  const ax=lax*leftSine,ay=lay*leftSine,az=laz*leftSine,aw=leftLength&&la?Math.cos(la/2):1;
  const bx=rax*rightSine,by=ray*rightSine,bz=raz*rightSine,bw=rightLength&&ra?Math.cos(ra/2):1;
  let cosine = ax*bx + ay*by + az*bz + aw*bw; const sign = cosine < 0 ? -1 : 1; cosine = Math.abs(cosine);
  let first; let second;
  if (cosine > 0.9995) { first = 1 - factor; second = factor; }
  else { const angle = Math.acos(Math.max(-1, Math.min(1, cosine))); const sine = Math.sin(angle); first = Math.sin((1-factor)*angle)/sine; second = Math.sin(factor*angle)/sine; }
  let x = ax*first + bx*second*sign; let y = ay*first + by*second*sign; let z = az*first + bz*second*sign; let w = aw*first + bw*second*sign;
  const length = Math.hypot(x, y, z, w) || 1; x/=length; y/=length; z/=length; w/=length;
  const half = Math.acos(Math.max(-1, Math.min(1, w))); const sine = Math.sin(half);
  if (sine < 1e-6) output.set([0, 1, 0, 0]); else output.set([x/sine, y/sine, z/sine, half*2]);
  return output;
}

function slerpAxisAngle(left, right, factor) {
  const a = quaternionFromAxisAngle(left); let b = quaternionFromAxisAngle(right); let cosine = a.reduce((sum, value, index) => sum + value * b[index], 0);
  if (cosine < 0) { b = b.map((value) => -value); cosine = -cosine; }
  let result;
  if (cosine > 0.9995) result = a.map((value, index) => value + (b[index] - value) * factor);
  else { const angle = Math.acos(Math.max(-1, Math.min(1, cosine))); const sine = Math.sin(angle); const first = Math.sin((1 - factor) * angle) / sine; const second = Math.sin(factor * angle) / sine; result = a.map((value, index) => value * first + b[index] * second); }
  const length = Math.hypot(...result) || 1; result = result.map((value) => value / length); const half = Math.acos(Math.max(-1, Math.min(1, result[3]))); const sine = Math.sin(half);
  return sine < 1e-6 ? [0, 1, 0, 0] : [result[0] / sine, result[1] / sine, result[2] / sine, half * 2];
}

function quaternionFromAxisAngle([x, y, z, angle]) {
  const length = Math.hypot(x, y, z); if (!length || !angle) return [0, 0, 0, 1]; const sine = Math.sin(angle / 2) / length; return [x * sine, y * sine, z * sine, Math.cos(angle / 2)];
}

function updateBoneTexture(gl, gpu, inverseBindWorlds, posedWorlds, meshBindWorld, meshWorld) {
  inverse4Into(meshWorld, gpu.meshInverse); const matrices = gpu.boneMatrices;
  for (let index = 0; index < Math.max(1, gpu.boneNodes.length); index += 1) {
    const node = gpu.boneNodes[index];
    if (node >= 0 && inverseBindWorlds[node] && posedWorlds[node]) {
      multiply4Into(inverseBindWorlds[node], meshBindWorld, gpu.boneScratchA);
      multiply4Into(posedWorlds[node], gpu.boneScratchA, gpu.boneScratchB);
      multiply4Into(gpu.meshInverse, gpu.boneScratchB, gpu.boneScratchA);
      matrices.set(gpu.boneScratchA, index * 16);
    } else matrices.set(IDENTITY_MATRIX, index * 16);
  }
  gl.activeTexture(gl.TEXTURE5); gl.bindTexture(gl.TEXTURE_2D, gpu.boneTexture);
  gl.texSubImage2D(gl.TEXTURE_2D, 0, 0, 0, 4, Math.max(1, gpu.boneNodes.length), gl.RGBA, gl.FLOAT, matrices);
}

function updateAnimMesh(gpu, track, time, animationLength, binary) {
  if (!track) return gpu.vertices;
  gpu.animPositions = ensureFloatCapacity(gpu.animPositions, track.verticesPerFrame * 3);
  gpu.animUvs = ensureFloatCapacity(gpu.animUvs, track.uvsPerFrame * 2);
  const positions = sampleAnimMeshValuesInto(track.vertexFrameCount, track.verticesPerFrame, numericView(binary, track.vertexSamples), track.samplePeriod, animationLength, time, 3, gpu.animPositions);
  const uvs = sampleAnimMeshValuesInto(track.uvFrameCount, track.uvsPerFrame, numericView(binary, track.uvSamples), track.samplePeriod, animationLength, time, 2, gpu.animUvs);
  gpu.dynamicVertices = ensureFloatCapacity(gpu.dynamicVertices, gpu.vertices.length);
  const output = gpu.dynamicVertices; output.set(gpu.vertices);
  for (let corner = 0; corner < gpu.indices.length; corner += 1) { const vertex = gpu.indices[corner]; const uv = gpu.uvIndices[corner] ?? vertex; if (positions) output.set(positions.subarray(vertex * 3, vertex * 3 + 3), corner * gpu.stride); if (uvs) output.set(uvs.subarray(uv * 2, uv * 2 + 2), corner * gpu.stride + 6); }
  return output;
}

function updatePreparedAnimMesh(gpu, targetTrack, targetTime, targetLength, sourceTrack, sourceTime, sourceLength, factor) {
  if (!targetTrack && !sourceTrack) return gpu.vertices;
  const targetPositions = samplePreparedAnimMeshValues(gpu, targetTrack, targetTime, targetLength, 'target', 'position');
  const targetUvs = samplePreparedAnimMeshValues(gpu, targetTrack, targetTime, targetLength, 'target', 'uv');
  const sourcePositions = samplePreparedAnimMeshValues(gpu, sourceTrack, sourceTime, sourceLength, 'source', 'position');
  const sourceUvs = samplePreparedAnimMeshValues(gpu, sourceTrack, sourceTime, sourceLength, 'source', 'uv');
  gpu.dynamicVertices = ensureFloatCapacity(gpu.dynamicVertices, gpu.vertices.length);
  const output = gpu.dynamicVertices; output.set(gpu.vertices); const amount = Math.max(0, Math.min(1, factor));
  for (let corner = 0; corner < gpu.indices.length; corner += 1) {
    const vertex = gpu.indices[corner]; const uv = gpu.uvIndices[corner] ?? vertex; const base = corner * gpu.stride;
    for (let axis = 0; axis < 3; axis += 1) {
      const fallback = gpu.vertices[base + axis]; const from = sourcePositions?.[vertex * 3 + axis] ?? fallback; const to = targetPositions?.[vertex * 3 + axis] ?? fallback;
      output[base + axis] = lerpNumber(from, to, amount);
    }
    for (let axis = 0; axis < 2; axis += 1) {
      const fallback = gpu.vertices[base + 6 + axis]; const from = sourceUvs?.[uv * 2 + axis] ?? fallback; const to = targetUvs?.[uv * 2 + axis] ?? fallback;
      output[base + 6 + axis] = lerpNumber(from, to, amount);
    }
  }
  return output;
}

function samplePreparedAnimMeshValues(gpu, track, time, animationLength, side, channel) {
  if (!track) return undefined;
  const positions = channel === 'position'; const width = positions ? 3 : 2;
  const frameCount = positions ? track.vertexFrameCount : track.uvFrameCount;
  const perFrame = positions ? track.verticesPerFrame : track.uvsPerFrame;
  const values = positions ? track.vertexValues : track.uvValues;
  const property = `${side}${positions ? 'AnimPositions' : 'AnimUvs'}`;
  gpu[property] = ensureFloatCapacity(gpu[property], perFrame * width);
  return sampleAnimMeshValuesInto(frameCount, perFrame, values, track.samplePeriod, animationLength, time, width, gpu[property]);
}

function updateDynamicMesh(gl, gpu, input, dangly, time, windPower) {
  let output = input;
  if (dangly && gpu.vertexConstraints.some((value) => value > 0)) {
    output = gpu.danglyVertices; output.set(input); const period = Math.max(0.01, dangly.period || 1); const tightness = Math.max(0, Math.min(1, dangly.tightness || 0)); const wind = 1 + Math.max(0, windPower) / 10;
    for (let vertex = 0; vertex < gpu.count; vertex += 1) {
      const constraint = Math.max(0, gpu.vertexConstraints[vertex] || 0); if (!constraint) continue;
      const phase = time * Math.PI * 2 / period + vertex * 0.173; const amplitude = dangly.displacement * constraint * (1 - tightness) * wind;
      output[vertex * gpu.stride] += Math.sin(phase) * amplitude; output[vertex * gpu.stride + 1] += Math.cos(phase * 0.73) * amplitude * 0.5; output[vertex * gpu.stride + 2] -= Math.abs(Math.sin(phase * 0.5)) * amplitude * 0.25;
    }
  }
  if (output === gpu.vertices && !gpu.dynamicActive) return;
  gl.bindBuffer(gl.ARRAY_BUFFER, gpu.buffer); gl.bufferSubData(gl.ARRAY_BUFFER, 0, output); gpu.dynamicActive = output !== gpu.vertices;
}

function ensureFloatCapacity(value, length) {
  return value?.length >= length ? value : new Float32Array(length);
}

function sampleAnimMeshValuesInto(frameCount, perFrame, values, period, length, time, width, result) {
  if (!frameCount || !perFrame || !values.length) return undefined; if (frameCount === 1) return values.subarray(0, perFrame * width);
  const samplePeriod = period > Number.EPSILON ? period : Math.max(Number.EPSILON, length / frameCount); const phase = length > 0 ? ((time % length) + length) % length : Math.max(0, time); const cycle = samplePeriod * frameCount; const position = (phase % cycle) / samplePeriod; const current = Math.min(frameCount - 1, Math.floor(position)); const next = (current + 1) % frameCount; const factor = position - current;
  const valueCount = perFrame * width; const startOffset = current * valueCount; const endOffset = next * valueCount; for (let index = 0; index < valueCount; index += 1) result[index] = values[startOffset + index] + (values[endOffset + index] - values[startOffset + index]) * factor; return result;
}

function globalIllumination(environment) {
  const source = environment || {}; const night = source.isNight === true;
  const ambient = packedColor(night ? source.moonAmbientColor : source.sunAmbientColor, [1, 1, 1]);
  const diffuse = packedColor(night ? source.moonDiffuseColor : source.sunDiffuseColor, [1, 1, 1]);
  const fog = packedColor(night ? source.moonFogColor : source.sunFogColor, ambient);
  const mixed = ambient.map((value, index) => value * 0.4 + diffuse[index] * 0.6);
  const luminance = mixed[0] * 0.2126 + mixed[1] * 0.7152 + mixed[2] * 0.0722;
  const targetLuminance = night ? 0.72 : 0.95; const exposure = targetLuminance / Math.max(luminance, 0.001);
  const environmentLight = environment ? mixed.map((value) => Math.max(0.35, Math.min(1.15, value * exposure))) : [1, 1, 1];
  const fogDistance = Number(source.fogClipDistance);
  return {
    environmentLight,
    fogColor: fog,
    fogEnabled: Boolean(environment && Number.isFinite(fogDistance) && fogDistance > 0),
    fogEnd: Number.isFinite(fogDistance) && fogDistance > 0 ? fogDistance : 100,
    background: environment ? fog.map((value) => value * (night ? 0.35 : 0.65)) : [0.035, 0.045, 0.06],
  };
}

function faceNormal(positions, ai, bi, ci) {
  const a = [positions[ai * 3] || 0, positions[ai * 3 + 1] || 0, positions[ai * 3 + 2] || 0];
  const b = [positions[bi * 3] || 0, positions[bi * 3 + 1] || 0, positions[bi * 3 + 2] || 0];
  const c = [positions[ci * 3] || 0, positions[ci * 3 + 1] || 0, positions[ci * 3 + 2] || 0];
  const ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]]; const ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
  const normal = [ab[1] * ac[2] - ab[2] * ac[1], ab[2] * ac[0] - ab[0] * ac[2], ab[0] * ac[1] - ab[1] * ac[0]];
  const length = Math.hypot(...normal) || 1; return normal.map((value) => value / length);
}

function surfaceColor(materialIndex) {
  // The engine's walkmesh material ids are stable even though WOK/DWK/PWK files do not
  // carry display colors. Keep the palette here so collision views remain deterministic
  // and distinct without inventing data in the renderer-neutral scene representation.
  const palette = [
    [0.52, 0.36, 0.20], [0.35, 0.35, 0.35], [0.24, 0.62, 0.24], [0.58, 0.58, 0.58],
    [0.56, 0.38, 0.19], [0.18, 0.45, 0.82], [0.88, 0.20, 0.20], [0.78, 0.78, 0.86],
    [0.62, 0.22, 0.62], [0.62, 0.68, 0.74], [0.18, 0.66, 0.70], [0.23, 0.44, 0.25],
    [0.42, 0.28, 0.17], [0.46, 0.62, 0.20], [0.94, 0.36, 0.08], [0.12, 0.12, 0.16],
  ];
  const index = Number.isInteger(materialIndex) ? Math.abs(materialIndex) % palette.length : 0;
  return palette[index];
}

function bindViewportControls(canvas, camera, draw, changed = () => {}) {
  let drag;
  canvas.addEventListener('pointerdown', (event) => { drag = { x: event.clientX, y: event.clientY, button: event.button }; canvas.setPointerCapture(event.pointerId); });
  canvas.addEventListener('pointermove', (event) => {
    if (!drag) return; const dx = event.clientX - drag.x; const dy = event.clientY - drag.y; drag.x = event.clientX; drag.y = event.clientY;
    if (drag.button === 0) { camera.yaw -= dx * 0.008; camera.pitch = Math.max(-1.5, Math.min(1.5, camera.pitch + dy * 0.008)); }
    else { const scale = camera.distance * 0.002; camera.target[0] -= dx * scale; camera.target[2] += dy * scale; }
    changed(); draw();
  });
  canvas.addEventListener('pointerup', () => { drag = undefined; });
  canvas.addEventListener('pointercancel', () => { drag = undefined; });
  canvas.addEventListener('contextmenu', (event) => event.preventDefault());
  canvas.addEventListener('wheel', (event) => { event.preventDefault(); camera.distance = Math.max(0.1, camera.distance * Math.exp(event.deltaY * 0.001)); changed(); draw(); }, { passive: false });
  canvas.addEventListener('keydown', (event) => {
    const step = event.shiftKey ? 0.2 : 0.06; let handled = true;
    if (event.key === 'ArrowLeft') camera.yaw += step; else if (event.key === 'ArrowRight') camera.yaw -= step; else if (event.key === 'ArrowUp') camera.pitch = Math.min(1.5, camera.pitch + step); else if (event.key === 'ArrowDown') camera.pitch = Math.max(-1.5, camera.pitch - step); else if (event.key === '+' || event.key === '=') camera.distance = Math.max(0.1, camera.distance * 0.9); else if (event.key === '-') camera.distance *= 1.1; else handled = false;
    if (handled) { event.preventDefault(); changed(); draw(); }
  });
}

function sceneBounds(scene) {
  const min = [Infinity, Infinity, Infinity]; const max = [-Infinity, -Infinity, -Infinity];
  const include = (point) => point.forEach((value, index) => { min[index] = Math.min(min[index], value); max[index] = Math.max(max[index], value); });
  const includeModel = (modelIndex, base, stack) => {
    if (stack.has(modelIndex)) return; const model = scene.manifest.models[modelIndex]; if (!model) return; stack.add(modelIndex);
    const worlds = resolveNodeWorlds(model, model.nodes);
    for (const mesh of model.meshes) for (const primitive of mesh.primitives) {
      const world = multiply4(base, worlds[mesh.sourceNode] || identity4()); const positions = numericView(scene.binary, primitive.positions);
      for (let index = 0; index < positions.length; index += 3) include(transformPoint4(world, [positions[index], positions[index + 1], positions[index + 2]]));
    }
    for (const attachment of model.attachments) { const target = model.nodes.findIndex((node) => node.name.toLowerCase() === attachment.targetNodeName.toLowerCase()); includeModel(attachment.model, multiply4(base, worlds[target] || identity4()), new Set(stack)); }
  };
  scene.manifest.instances.forEach((instance) => {
    if (instance.kind === 'skybox') return;
    const base = multiply4(translation4(instance.position), multiply4(axisAngle4(instance.rotationAxisAngle), scale4(instance.scale))); include(instance.position);
    instance.polygon?.forEach((point) => include(transformPoint4(base, point))); if (instance.model != null) includeModel(instance.model, base, new Set());
  });
  if (!Number.isFinite(min[0])) return { min: [-1, -1, -1], max: [1, 1, 1] };
  return { min, max };
}

function transformPoint4(matrix, [x, y, z]) {
  return [matrix[0]*x+matrix[4]*y+matrix[8]*z+matrix[12], matrix[1]*x+matrix[5]*y+matrix[9]*z+matrix[13], matrix[2]*x+matrix[6]*y+matrix[10]*z+matrix[14]];
}

function packedColor(value, fallback) {
  if (!Number.isInteger(value)) return fallback;
  return [(value & 255) / 255, ((value >>> 8) & 255) / 255, ((value >>> 16) & 255) / 255];
}

function identity4() { return new Float32Array([1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1]); }
function composeTransform4(translation, rotationAxisAngle, scale) {
  return composeTransform4Into(translation, rotationAxisAngle, scale, new Float32Array(16));
}
function composeTransform4Into([tx, ty, tz], [x0, y0, z0, angle], [sx, sy, sz], output) {
  let x = x0; let y = y0; let z = z0; const length = Math.hypot(x, y, z);
  if (!length || !angle) {
    output.set([sx, 0, 0, 0, 0, sy, 0, 0, 0, 0, sz, 0, tx, ty, tz, 1]);
    return output;
  }
  x /= length; y /= length; z /= length;
  const c = Math.cos(angle); const s = Math.sin(angle); const t = 1 - c;
  output.set([
    (t*x*x+c)*sx, (t*x*y+s*z)*sx, (t*x*z-s*y)*sx, 0,
    (t*x*y-s*z)*sy, (t*y*y+c)*sy, (t*y*z+s*x)*sy, 0,
    (t*x*z+s*y)*sz, (t*y*z-s*x)*sz, (t*z*z+c)*sz, 0,
    tx, ty, tz, 1,
  ]);
  return output;
}
function translation4([x, y, z]) { const result = identity4(); result[12] = x; result[13] = y; result[14] = z; return result; }
function scale4([x, y, z]) { const result = identity4(); result[0] = x; result[5] = y; result[10] = z; return result; }
function axisAngle4([x, y, z, angle]) {
  const length = Math.hypot(x, y, z); if (!length || !angle) return identity4(); x /= length; y /= length; z /= length;
  const c = Math.cos(angle); const s = Math.sin(angle); const t = 1 - c;
  return new Float32Array([t*x*x+c, t*x*y+s*z, t*x*z-s*y, 0, t*x*y-s*z, t*y*y+c, t*y*z+s*x, 0, t*x*z+s*y, t*y*z-s*x, t*z*z+c, 0, 0, 0, 0, 1]);
}
function multiply4(a, b) {
  const out = new Float32Array(16);
  return multiply4Into(a, b, out);
}
function multiply4Into(a, b, out) {
  for (let column = 0; column < 4; column += 1) for (let row = 0; row < 4; row += 1) {
    out[column * 4 + row] = a[row] * b[column * 4] + a[4 + row] * b[column * 4 + 1] + a[8 + row] * b[column * 4 + 2] + a[12 + row] * b[column * 4 + 3];
  }
  return out;
}
function inverse4(matrix) {
  const output = new Float32Array(16);
  return inverse4Into(matrix, output);
}
function inverse4Into(matrix, output) {
  const a00=matrix[0],a01=matrix[1],a02=matrix[2],a03=matrix[3],a10=matrix[4],a11=matrix[5],a12=matrix[6],a13=matrix[7],a20=matrix[8],a21=matrix[9],a22=matrix[10],a23=matrix[11],a30=matrix[12],a31=matrix[13],a32=matrix[14],a33=matrix[15];
  const b00=a00*a11-a01*a10,b01=a00*a12-a02*a10,b02=a00*a13-a03*a10,b03=a01*a12-a02*a11,b04=a01*a13-a03*a11,b05=a02*a13-a03*a12,b06=a20*a31-a21*a30,b07=a20*a32-a22*a30,b08=a20*a33-a23*a30,b09=a21*a32-a22*a31,b10=a21*a33-a23*a31,b11=a22*a33-a23*a32;
  const determinant=b00*b11-b01*b10+b02*b09+b03*b08-b04*b07+b05*b06;
  if (Math.abs(determinant) < 1e-12) { output.set(IDENTITY_MATRIX); return output; } const inverse=1/determinant;
  output[0]=(a11*b11-a12*b10+a13*b09)*inverse; output[1]=(a02*b10-a01*b11-a03*b09)*inverse; output[2]=(a31*b05-a32*b04+a33*b03)*inverse; output[3]=(a22*b04-a21*b05-a23*b03)*inverse;
  output[4]=(a12*b08-a10*b11-a13*b07)*inverse; output[5]=(a00*b11-a02*b08+a03*b07)*inverse; output[6]=(a32*b02-a30*b05-a33*b01)*inverse; output[7]=(a20*b05-a22*b02+a23*b01)*inverse;
  output[8]=(a10*b10-a11*b08+a13*b06)*inverse; output[9]=(a01*b08-a00*b10-a03*b06)*inverse; output[10]=(a30*b04-a31*b02+a33*b00)*inverse; output[11]=(a21*b02-a20*b04-a23*b00)*inverse;
  output[12]=(a11*b07-a10*b09-a12*b06)*inverse; output[13]=(a00*b09-a01*b07+a02*b06)*inverse; output[14]=(a31*b01-a30*b03-a32*b00)*inverse; output[15]=(a20*b03-a21*b01+a22*b00)*inverse; return output;
}
function perspective(fovy, aspect, near, far) {
  const f = 1 / Math.tan(fovy / 2); const range = 1 / (near - far);
  return new Float32Array([f / aspect, 0, 0, 0, 0, f, 0, 0, 0, 0, (near + far) * range, -1, 0, 0, 2 * near * far * range, 0]);
}
function orbitEye(camera) {
  const cp = Math.cos(camera.pitch); return [camera.target[0] + camera.distance * cp * Math.cos(camera.yaw), camera.target[1] + camera.distance * cp * Math.sin(camera.yaw), camera.target[2] + camera.distance * Math.sin(camera.pitch)];
}
function lookAt(eye, target, up) {
  let z = [eye[0] - target[0], eye[1] - target[1], eye[2] - target[2]]; let length = Math.hypot(...z) || 1; z = z.map((value) => value / length);
  let x = [up[1] * z[2] - up[2] * z[1], up[2] * z[0] - up[0] * z[2], up[0] * z[1] - up[1] * z[0]]; length = Math.hypot(...x) || 1; x = x.map((value) => value / length);
  const y = [z[1] * x[2] - z[2] * x[1], z[2] * x[0] - z[0] * x[2], z[0] * x[1] - z[1] * x[0]];
  return new Float32Array([x[0], y[0], z[0], 0, x[1], y[1], z[1], 0, x[2], y[2], z[2], 0, -x[0]*eye[0]-x[1]*eye[1]-x[2]*eye[2], -y[0]*eye[0]-y[1]*eye[1]-y[2]*eye[2], -z[0]*eye[0]-z[1]*eye[1]-z[2]*eye[2], 1]);
}

function edit(payload) { vscode.postMessage({ type: 'edit', edit: payload }); }
function refresh(options) { vscode.postMessage({ type: 'refresh', options }); }
function showError(message) { vscode.postMessage({ type: 'showError', message }); }
function toolbar() { return document.getElementById('toolbar'); }
function content() { return document.getElementById('content'); }
function clone(value) { return structuredClone(value); }
function cellValue(value) { return value === '****' ? null : value; }
function encodePath(value) { return encodeURIComponent(JSON.stringify(value)); }
function decodePath(value) { return JSON.parse(decodeURIComponent(value)); }
function getAtPath(value, pathParts) { return pathParts.reduce((current, part) => current[part], value); }
function setAtPath(value, pathParts, replacement) { const last = pathParts[pathParts.length - 1]; getAtPath(value, pathParts.slice(0, -1))[last] = replacement; }
function bytesToBase64(bytes) { let binary = ''; const chunk = 0x8000; for (let index = 0; index < bytes.length; index += chunk) binary += String.fromCharCode(...bytes.subarray(index, index + chunk)); return btoa(binary); }
function formatBytes(value) { if (value < 1024) return `${value} B`; if (value < 1048576) return `${(value / 1024).toFixed(1)} KiB`; return `${(value / 1048576).toFixed(1)} MiB`; }
function escapeHtml(value) { return String(value).replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;').replaceAll('"', '&quot;').replaceAll("'", '&#39;'); }
function escapeAttribute(value) { return escapeHtml(value).replaceAll('\n', '&#10;').replaceAll('\r', '&#13;'); }
function isEditableType(extension) { return ['2da', 'tlk', 'dds', 'tga', 'plt', 'gff', 'utc', 'utd', 'ute', 'uti', 'utm', 'utp', 'uts', 'utt', 'utw', 'git', 'are', 'gic', 'ifo', 'fac', 'dlg', 'itp', 'bic', 'jrl', 'gui', 'erf', 'hak', 'mod', 'nwm', 'mdl', 'wok', 'dwk', 'pwk'].includes(String(extension).toLowerCase()); }

const gffKinds = ['byte', 'char', 'word', 'short', 'dword', 'int', 'float', 'dword64', 'int64', 'double', 'string', 'resref', 'locstring', 'void', 'struct', 'list'];
function defaultGffValue(kind) {
  if (['string', 'resref', 'void', 'dword64', 'int64'].includes(kind)) return kind.endsWith('64') ? '0' : '';
  if (kind === 'locstring') return { strRef: 4294967295, entries: [] };
  if (kind === 'struct') return { id: 0, fields: [] };
  if (kind === 'list') return [];
  return 0;
}
