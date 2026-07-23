'use strict';

declare var CUSTOM_EDITOR_RESOURCE_TYPES: ReadonlySet<string>;

type Mutable<T> = { -readonly [Key in keyof T]: T[Key] };
type NumericView = Uint8Array | Uint32Array | Int32Array | Float32Array;

interface PoseNode extends Omit<
  Mutable<PacketNode>,
  'translation' | 'rotationAxisAngle' | 'scale' | 'color' | 'light'
> {
  translation: Float32Array;
  rotationAxisAngle: Float32Array;
  scale: Float32Array;
  color: Float32Array;
  light?: Mutable<PacketLight>;
}

interface PoseMaterial {
  active: boolean;
  alpha: number | undefined;
  selfIllumColor: Float32Array;
}

interface ModelPose {
  readonly nodes: PoseNode[];
  readonly materials: PoseMaterial[];
  readonly worlds: Float32Array[];
}

interface InstalledAnimationAsset extends AnimationAsset {
  readonly runtime: NonNullable<AnimationAsset['runtime']>;
}

interface MaterialTextureRuntime {
  readonly binding: SceneTextureBinding;
  readonly texture: number;
  handle: WebGLTexture | null | undefined;
  readonly uvTransform: Float32Array;
}

interface MaterialRuntime {
  readonly textures: Map<string, MaterialTextureRuntime>;
}

interface NodeTextureRuntime {
  readonly nodeIndex: number;
  readonly role: string;
  readonly name: string;
  readonly texture: number;
  readonly directives: SceneTextureBinding['directives'];
}

interface ChunkBatch {
  readonly buffer: WebGLBuffer | null;
  values: Float32Array;
  count: number;
  gpuCapacity: number;
}

interface ModelRuntime {
  readonly nodeByName: Map<string, number>;
  readonly bindWorlds: Float32Array[];
  inverseBindWorlds: Float32Array[];
  readonly hiddenNodes: Set<number>;
  readonly materials: MaterialRuntime[];
  readonly materialsByNode: number[][];
  readonly nodeTextures: Map<string, NodeTextureRuntime>;
  readonly attachmentTargets: Map<PacketModel['attachments'][number], number>;
  readonly animationAssets: Map<number, InstalledAnimationAsset>;
  readonly emitterBuffers: Array<Float32Array | undefined>;
  readonly emitterLinkedBuffers: Array<RibbonParticleBuffer | undefined>;
  readonly emitterColors: Array<[Float32Array, Float32Array, Float32Array]>;
  readonly emitterIntervals: Float64Array[];
  readonly emitterTransitionVectors: Float32Array[];
  readonly emitterTransitionIntervals: Float64Array[];
  readonly flareBuffer: Float32Array;
  readonly chunkTranslation: Float32Array;
  readonly chunkRotation: Float32Array;
  readonly chunkScale: Float32Array;
  readonly chunkLocalMatrix: Float32Array;
  readonly chunkWorldMatrix: Float32Array;
  readonly drawWorld: Float32Array;
  readonly drawMvp: Float32Array;
  readonly attachmentWorld: Float32Array;
  readonly emitterWorld: Float32Array;
  readonly effectWorld: Float32Array;
  readonly effectAttachment: Float32Array;
  readonly instancedLocal: Float32Array;
  readonly instancedAttachment: Float32Array;
  readonly lightWorld: Float32Array;
  readonly lightAttachment: Float32Array;
  readonly lightRow: Float32Array;
  readonly localMatrices: Float32Array[];
  readonly worldState: Uint8Array;
  readonly scalarScratch: Float32Array;
  readonly pose: ModelPose;
  readonly poseResult: { asset: InstalledAnimationAsset | undefined; pose: ModelPose };
  poseFrame: number;
  chunkBatch: ChunkBatch;
}

interface RibbonParticleBuffer {
  values: Float32Array;
  vertexCount: number;
}

interface OverlayGpu {
  readonly vao: WebGLVertexArrayObject | null;
  readonly buffer: WebGLBuffer | null;
  readonly count: number;
}

interface SelectionGpu extends OverlayGpu {
  readonly selectionKey: string | number;
}

interface PrimitiveGpu {
  readonly vao: WebGLVertexArrayObject | null;
  readonly buffer: WebGLBuffer | null;
  readonly count: number;
  readonly stride: number;
  readonly vertices: Float32Array;
  dynamicVertices: Float32Array;
  readonly danglyVertices: Float32Array;
  readonly indices: NumericView;
  readonly uvIndices: NumericView;
  readonly sourcePositions: NumericView;
  readonly sourceUvs: NumericView | undefined;
  readonly boneNodes: number[];
  readonly boneTexture: WebGLTexture | null;
  readonly boneMatrices: Float32Array;
  readonly boneScratchA: Float32Array;
  readonly boneScratchB: Float32Array;
  readonly meshInverse: Float32Array;
  readonly vertexConstraints: Float32Array;
  dynamicActive?: boolean;
  animPositions?: Float32Array;
  animUvs?: Float32Array;
  targetAnimPositions?: Float32Array;
  targetAnimUvs?: Float32Array;
  sourceAnimPositions?: Float32Array;
  sourceAnimUvs?: Float32Array;
}

interface AnimationPlayback {
  readonly modelIndex: number;
  readonly animationIndex: number;
  readonly animation: PacketAnimation;
  readonly scope: Map<number, number>;
}

interface AnimationTransition {
  readonly duration: number;
  readonly fromPoses: Map<number, ModelPose>;
  readonly sourceAssets: Map<number, InstalledAnimationAsset>;
  readonly sourceTime: number;
}

interface PointLightCollection {
  storage: Float32Array;
  count: number;
  values: Float32Array;
}

type NwnEnvironment = Exclude<ScenePacketManifest['environment'], 'studio'>['nwn'];

interface Illumination {
  readonly environmentLight: number[];
  readonly fogColor: number[];
  readonly fogEnabled: boolean;
  readonly fogEnd: number;
  readonly background: number[];
}

interface ViewportControls {
  update(deltaSeconds: number): boolean;
  dispose(): void;
}

type MutableVec3 = [number, number, number];
type MutableVec4 = [number, number, number, number];

interface Bounds {
  readonly min: MutableVec3;
  readonly max: MutableVec3;
}

interface BoundsSelection {
  readonly bounds: Bounds;
  readonly vertices: MutableVec3[];
}

interface BoundsCatalog {
  readonly scene: Bounds;
  readonly objects: Map<string, Bounds>;
  readonly objectSelections: Map<string, BoundsSelection>;
  readonly componentSelections: Map<number, BoundsSelection>;
}

interface SceneInstanceRuntime {
  readonly instance: SceneInstance;
  readonly base: Float32Array;
  readonly dynamic: boolean;
  readonly overlay: OverlayGpu | undefined;
}

interface SpriteGpu {
  readonly vao: WebGLVertexArrayObject | null;
  readonly cornerBuffer: WebGLBuffer | null;
  readonly instanceBuffer: WebGLBuffer | null;
  capacity: number;
}

interface RibbonGpu {
  readonly vao: WebGLVertexArrayObject | null;
  readonly buffer: WebGLBuffer | null;
  capacity: number;
}

interface S3tcExtension {
  readonly COMPRESSED_RGBA_S3TC_DXT1_EXT: number;
  readonly COMPRESSED_RGBA_S3TC_DXT5_EXT: number;
}

interface ResourceSnapshotBase<Kind extends string, Data> {
  readonly path: string;
  readonly kind: Kind;
  readonly readOnlyOrigin?: boolean;
  readonly revision?: number;
  readonly data: Data;
}

interface TwoDaData {
  columns: string[];
  default: string | null;
  rows: Array<{ label: string; cells: Array<string | null> }>;
}

interface TlkEntry {
  readonly strRef: number;
  text: string;
  soundResRef: string;
  soundLength: number;
  flags: number;
  volumeVariance: number;
  pitchVariance: number;
}

interface TlkData {
  readonly language: number;
  readonly highest: number;
  readonly total: number;
  readonly offset: number;
  readonly limit: number;
  readonly entries: TlkEntry[];
}

type JsonValue =
  | null
  | boolean
  | number
  | string
  | JsonValue[]
  | { [key: string]: JsonValue };

interface GffStructure {
  id: number;
  fields: GffField[];
}

const gffKinds = [
  'byte', 'char', 'word', 'short', 'dword', 'int', 'float', 'dword64',
  'int64', 'double', 'string', 'resref', 'locstring', 'void', 'struct', 'list',
] as const;
type GffKind = typeof gffKinds[number];

interface GffField {
  label: string;
  kind: GffKind;
  value: JsonValue | GffStructure | GffStructure[];
}

interface GffData {
  readonly fileType: string;
  readonly fileVersion: string;
  root: GffStructure;
}

interface ScriptSourceLocation {
  readonly file: string;
  readonly line: number;
  readonly text?: string | null;
  readonly available: boolean;
}

interface ScriptSuccessor {
  readonly offset: number;
  readonly kind: string;
}

type BuiltinType = string | Readonly<Record<string, string>>;

interface ScriptAction {
  readonly id: number;
  readonly argumentCount: number;
  readonly name: string;
  readonly returnType: BuiltinType;
  readonly parameters: readonly { readonly name: string; readonly ty: BuiltinType }[];
  readonly arityMatches: boolean;
}

interface ScriptInstruction {
  readonly index: number;
  readonly offset: number;
  readonly localOffset: number | null;
  readonly size: number;
  readonly label: string;
  readonly opcode: string;
  readonly opcodeInternal: string;
  readonly auxcode: string;
  readonly auxcodeInternal: string;
  readonly operand: string;
  readonly action: ScriptAction | null;
  readonly rawHex: string;
  readonly jumpTarget: number | null;
  readonly callTarget: number | null;
  readonly successors: readonly ScriptSuccessor[];
  readonly functionIndex: number | null;
  readonly source: ScriptSourceLocation | null;
}

interface ScriptBlock {
  readonly start: number;
  readonly end: number;
  readonly instructionIndices: readonly number[];
  readonly successors: readonly ScriptSuccessor[];
}

interface ScriptFunction {
  readonly index: number;
  readonly name: string;
  readonly start: number;
  readonly end: number;
  readonly returnType: string;
  readonly arguments: readonly string[];
  readonly synthetic: boolean;
  readonly source?: ScriptSourceLocation | null;
  readonly blocks: readonly ScriptBlock[];
}

interface ScriptDebugData {
  readonly primary: 'ncs' | 'ndb';
  readonly hasNcs: boolean;
  readonly hasNdb: boolean;
  readonly hasLangspec: boolean;
  readonly sourceFiles: readonly {
    readonly name: string;
    readonly available: boolean;
    readonly isRoot: boolean;
  }[];
  readonly header?: {
    readonly format: string;
    readonly fileSize: number;
    readonly declaredSize: number;
    readonly codeSize: number;
    readonly instructionCount: number;
  };
  readonly summary: {
    readonly files: number;
    readonly structs: number;
    readonly functions: number;
    readonly variables: number;
    readonly lineMappings: number;
    readonly structEntries?: readonly {
      readonly name: string;
      readonly fields: readonly { readonly name: string; readonly type: string }[];
    }[];
    readonly variableEntries?: readonly {
      readonly name: string;
      readonly type: string;
      readonly start: number;
      readonly end: number;
      readonly stackLocation: number;
    }[];
  };
  readonly functions: readonly ScriptFunction[];
  readonly instructions: readonly ScriptInstruction[];
  readonly diagnostics: readonly string[];
}

interface TextureData {
  readonly width: number;
  readonly height: number;
  readonly rgba: string;
  readonly metadata: Readonly<Record<string, JsonValue>>;
}

interface ArchiveEntry {
  readonly resource: string;
  readonly bif?: string;
  readonly extension: string;
  readonly typeId: number;
  readonly size: number;
  readonly modified: boolean;
}

interface ArchiveData {
  readonly entries: readonly ArchiveEntry[];
  readonly total: number;
  readonly offset: number;
  readonly limit: number;
  readonly query: string;
  readonly bifs?: readonly {
    readonly index: number;
    readonly filename: string;
    readonly drives: number;
    readonly oid: number;
    readonly entryCount: number;
  }[];
}

type ResourceModel =
  | ResourceSnapshotBase<'2da', TwoDaData>
  | ResourceSnapshotBase<'tlk', TlkData>
  | ResourceSnapshotBase<'gff', GffData>
  | ResourceSnapshotBase<'ncs' | 'ndb', ScriptDebugData>
  | ResourceSnapshotBase<'dds' | 'tga' | 'plt', TextureData>
  | ResourceSnapshotBase<'erf' | 'key', ArchiveData>
  | { readonly path: string; readonly kind: 'viewer' };

interface PersistedViewerState {
  readonly scene?: string;
  readonly animationName?: string;
  readonly animationSelection?: {
    readonly modelIndex: number;
    readonly animationIndex: number;
  } | null;
  readonly camera?: unknown;
  readonly selectedObjectKey?: string | null;
  readonly selectedComponentId?: number | null;
  readonly inspector?: {
    readonly width?: number;
    readonly collapsed?: boolean;
    readonly technicalNames?: boolean;
    readonly query?: string;
    readonly routes?: Readonly<Record<string, unknown>>;
    readonly scope?: string;
    readonly scrollPositions?: Readonly<Record<string, unknown>>;
    readonly openSections?: Readonly<Record<string, unknown>>;
    readonly touchedSections?: readonly string[];
  };
}

interface ViewerCamera {
  yaw: number;
  pitch: number;
  distance: number;
  target: [number, number, number];
}

interface WebviewState {
  readonly viewer?: PersistedViewerState;
}

interface ViewerController {
  dispose(): void;
  setAnimation(modelIndex: number | undefined, animationIndex: number | undefined): void;
  selectObject(objectKey: unknown, frame: boolean, notify: boolean): void;
  applyAnimation(packet: DecodedAnimationPacket): void;
  applyTexture(packet: DecodedTexturePacket): void;
  applyInspection(assetKey: unknown, objectKey: unknown, inspection: unknown): void;
  applyInspectionError(assetKey: unknown, objectKey: unknown, message: unknown): void;
}

interface ViewerElements {
  readonly status: WebviewElement;
  animationTime: WebviewElement | null;
  animationEvent: WebviewElement | null;
  readonly workbench: WebviewElement;
  readonly inspector: WebviewElement;
  readonly inspectorContent: WebviewElement;
  readonly inspectorContext: WebviewElement;
  readonly inspectorScope: WebviewElement;
  readonly inspectorSearch: WebviewElement;
  readonly inspectorJump: WebviewElement;
  readonly inspectorTechnical: WebviewElement;
  readonly inspectorCollapse: WebviewElement;
  readonly inspectorReopen: WebviewElement;
  readonly inspectorSash: WebviewElement;
}

interface InspectorRoute {
  readonly page: string;
  readonly root?: 'source' | 'section';
  readonly rootIndex?: number;
  readonly sourceIndex?: number;
  readonly trail?: readonly {
    readonly kind: 'field' | 'entry';
    readonly index: number;
  }[];
}

type InspectionState =
  | { readonly status: 'loading' }
  | { readonly status: 'error'; readonly message: string }
  | { readonly status: 'ready'; readonly data: AreaObjectInspection };

interface InspectionContentOptions {
  readonly query?: string;
  readonly route?: InspectorRoute;
  readonly technicalNames?: boolean;
  readonly openSections?: ReadonlySet<string>;
  readonly useDefaultSections?: boolean;
  readonly scene?: DecodedScenePacket;
  readonly objectKey?: string;
  readonly selectedComponentId?: number;
  readonly componentCount?: number;
}

const EMPTY_OBJECT = Object.freeze({});
const IDENTITY_MATRIX = new Float32Array([1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1]);
const WHITE_COLOR = new Float32Array([1, 1, 1]);
const ZERO_COLOR = new Float32Array(3);
const DEFAULT_DIFFUSE = new Float32Array([0.72, 0.75, 0.8]);
const EMITTER_PROPERTY_CACHE =
  new WeakMap<PacketEmitter, Map<string, readonly PacketPropertyValue[]>>();
const EMITTER_VECTOR_CACHE = new WeakMap<PacketEmitter, Map<string, Float32Array>>();
const EMITTER_TRACK_CACHE =
  new WeakMap<PacketNodeAnimationTrack, Map<string, PreparedEmitterTrack>>();
const DIRECTIVE_CACHE =
  new WeakMap<SceneTextureBinding, Map<string, readonly string[]>>();

interface WebviewElement extends HTMLElement {
  open: boolean;
  type: string;
  value: string;
}

function webviewElement(id: string): WebviewElement | null {
  return document.getElementById(id) as WebviewElement | null;
}

function requiredWebviewElement(id: string): WebviewElement {
  const element = webviewElement(id);
  if (!element) throw new Error(`The resource editor element #${id} is missing.`);
  return element;
}

function requiredCanvas(id: string): HTMLCanvasElement {
  const element = document.getElementById(id);
  if (!(element instanceof HTMLCanvasElement)) {
    throw new Error(`The resource editor canvas #${id} is missing.`);
  }
  return element;
}

const vscode = acquireVsCodeApi<WebviewState>();
const appElement = webviewElement('app');
if (!appElement) throw new Error('The resource editor host element is missing.');
const app = appElement;
const bootTimer = Number(app.dataset.bootTimer);
if (Number.isFinite(bootTimer)) clearTimeout(bootTimer);
delete app.dataset.bootTimer;
let model: ResourceModel | undefined;
let tablePage = 0;
let tlkOffset = 0;
let tlkQuery = '';
const tablePageSize = 200;
let loadingTimer: ReturnType<typeof setTimeout> | undefined;
let fatalErrorReported = false;
let viewer: ViewerController | undefined;
let viewerSession: ViewerSession | undefined;
let scriptDebugState: {
  functionIndex: number;
  selectedOffset: number | undefined;
  query: string;
  page: number;
} = { functionIndex: 0, selectedOffset: undefined, query: '', page: 0 };

window.addEventListener('message', (event: MessageEvent<unknown>) => {
  try {
    const data = event.data;
    if (!isRecord(data) || typeof data.type !== 'string') {
      return;
    }
    if (data.type === 'snapshot') {
      clearTimeout(loadingTimer);
      model = parseResourceModel(data.snapshot);
      render();
    } else if (data.type === 'scene') {
      clearTimeout(loadingTimer);
      model = { kind: 'viewer', path: '3D Scene' };
      viewerSession = createViewerSession(decodeScenePacket(data.packet));
      renderViewer(viewerSession, data.selectedObjectKey);
    } else if (data.type === 'selectAreaObject') {
      viewer?.selectObject(data.objectKey, data.frame !== false, false);
    } else if (data.type === 'animationAsset') {
      viewer?.applyAnimation(decodeAnimationPacket(data.packet));
    } else if (data.type === 'textureAsset') {
      viewer?.applyTexture(decodeTexturePacket(data.packet));
    } else if (data.type === 'areaObjectInspection') {
      viewer?.applyInspection(data.assetKey, data.objectKey, data.inspection);
    } else if (data.type === 'areaObjectInspectionError') {
      viewer?.applyInspectionError(data.assetKey, data.objectKey, data.message);
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

function reportFatalError(error: unknown): void {
  clearTimeout(loadingTimer);
  const message = error instanceof Error ? error.message : String(error || 'Unknown error');
  app.innerHTML = `<div class="empty status-error"><strong>Could not open this resource.</strong><br>${escapeHtml(message)}</div>`;
  if (!fatalErrorReported) {
    fatalErrorReported = true;
    vscode.postMessage({ type: 'showError', message });
  }
}

function render(): void {
  if (!model) return;
  const title = escapeHtml(model.path?.split(/[\\/]/u).pop() || 'NWN resource');
  app.innerHTML = `<section class="shell">
    <header class="titlebar"><h1>${title}</h1><span class="badge">${escapeHtml(model.kind.toUpperCase())}</span></header>
    <div id="toolbar" class="toolbar"></div><div id="content" class="content"></div></section>`;
  const renderers: Readonly<Record<string, () => void>> = {
    gff: renderGff,
    '2da': renderTwoDa,
    tlk: renderTlk,
    dds: renderTexture,
    tga: renderTexture,
    plt: renderTexture,
    erf: renderArchive,
    key: renderArchive,
    ncs: renderScriptDebug,
    ndb: renderScriptDebug,
  };
  (renderers[model.kind] || renderUnsupported)();
}

function decodePacket(packetValue: unknown): DecodedPacket {
  let packet: Uint8Array;
  if (packetValue instanceof Uint8Array) {
    packet = packetValue;
  } else if (packetValue instanceof ArrayBuffer) {
    packet = new Uint8Array(packetValue);
  } else if (ArrayBuffer.isView(packetValue)) {
    packet = new Uint8Array(
      packetValue.buffer,
      packetValue.byteOffset,
      packetValue.byteLength,
    );
  } else {
    throw new Error('The native viewer returned a non-binary scene packet.');
  }
  const expected = [78, 87, 78, 82, 83, 51, 68, 0];
  if (packet.length < 12 || !expected.every((value, index) => packet[index] === value)) {
    throw new Error('The native viewer returned an invalid scene packet.');
  }
  const view = new DataView(packet.buffer, packet.byteOffset, packet.byteLength);
  const manifestLength = view.getUint32(8, true);
  const manifestStart = 12;
  const binaryStart = manifestStart + manifestLength;
  if (binaryStart > packet.length) throw new Error('The scene packet manifest is truncated.');
  const parsedManifest: unknown = JSON.parse(
    new TextDecoder().decode(packet.subarray(manifestStart, binaryStart)),
  );
  if (!isRecord(parsedManifest) || typeof parsedManifest.schema !== 'string') {
    throw new Error('The scene packet manifest has no valid schema identity.');
  }
  const packedBinary = packet.subarray(binaryStart);
  // Current packet encoders align this segment to four bytes. Retain support
  // for packets produced by older native bindings and Uint8Array views whose
  // containing buffer starts at an odd offset by normalizing only when needed.
  const binary = packedBinary.byteOffset % 4 === 0
    ? packedBinary
    : Uint8Array.from(packedBinary);
  if (parsedManifest.schema === 'nwnrs.scene') {
    validateSceneManifest(parsedManifest);
    return { manifest: parsedManifest, binary };
  }
  if (parsedManifest.schema === 'nwnrs.scene.animation') {
    validateAnimationManifest(parsedManifest);
    return { manifest: parsedManifest, binary };
  }
  if (parsedManifest.schema === 'nwnrs.scene.texture') {
    validateTextureManifest(parsedManifest);
    return { manifest: parsedManifest, binary };
  }
  throw new Error(`The native viewer returned unsupported packet schema ${parsedManifest.schema}.`);
}

function decodeScenePacket(packetValue: unknown): DecodedScenePacket {
  const packet = decodePacket(packetValue);
  if (packet.manifest.schema !== 'nwnrs.scene') {
    throw new Error(`Expected a scene packet, received ${packet.manifest.schema}.`);
  }
  return { manifest: packet.manifest, binary: packet.binary };
}

function decodeAnimationPacket(packetValue: unknown): DecodedAnimationPacket {
  const packet = decodePacket(packetValue);
  if (packet.manifest.schema !== 'nwnrs.scene.animation') {
    throw new Error(`Expected an animation packet, received ${packet.manifest.schema}.`);
  }
  return { manifest: packet.manifest, binary: packet.binary };
}

function decodeTexturePacket(packetValue: unknown): DecodedTexturePacket {
  const packet = decodePacket(packetValue);
  if (packet.manifest.schema !== 'nwnrs.scene.texture') {
    throw new Error(`Expected a texture packet, received ${packet.manifest.schema}.`);
  }
  return { manifest: packet.manifest, binary: packet.binary };
}

function validateSceneManifest(value: unknown): asserts value is ScenePacketManifest {
  if (!isRecord(value)
      || value.schema !== 'nwnrs.scene'
      || typeof value.name !== 'string'
      || typeof value.source !== 'string'
      || !Array.isArray(value.instances)
      || !Array.isArray(value.areaObjects)
      || !Array.isArray(value.models)
      || !Array.isArray(value.rootModels)
      || !Array.isArray(value.textures)
      || !Array.isArray(value.shaders)
      || !isRecord(value.dependencies)
      || !Array.isArray(value.diagnostics)) {
    throw new Error('The native viewer returned a malformed scene manifest.');
  }
  for (const [index, modelValue] of value.models.entries()) {
    if (!isRecord(modelValue)
        || typeof modelValue.name !== 'string'
        || !Array.isArray(modelValue.nodes)
        || !Array.isArray(modelValue.meshes)
        || !Array.isArray(modelValue.materials)
        || !Array.isArray(modelValue.animations)) {
      throw new Error(`Scene model ${index} is malformed.`);
    }
  }
}

function validateAnimationManifest(value: unknown): asserts value is SceneAnimationPacketManifest {
  if (!isRecord(value)
      || value.schema !== 'nwnrs.scene.animation'
      || !Number.isInteger(value.modelIndex)
      || !Number.isInteger(value.animationIndex)
      || !isRecord(value.animation)
      || typeof value.animation.name !== 'string'
      || typeof value.animation.length !== 'number'
      || !Array.isArray(value.animation.events)
      || !Array.isArray(value.animation.nodeTracks)) {
    throw new Error('The native viewer returned a malformed animation manifest.');
  }
}

function validateTextureManifest(value: unknown): asserts value is SceneTexturePacketManifest {
  if (!isRecord(value)
      || value.schema !== 'nwnrs.scene.texture'
      || !Number.isInteger(value.textureIndex)
      || typeof value.resource !== 'string'
      || typeof value.kind !== 'string'
      || !Number.isInteger(value.width)
      || !Number.isInteger(value.height)
      || !Array.isArray(value.mipLevels)
      || !(value.rgba8 === null || isRecord(value.rgba8))) {
    throw new Error('The native viewer returned a malformed texture manifest.');
  }
}

function createViewerSession(scene: DecodedScenePacket): ViewerSession {
  return {
    scene,
    animationAssets: new Map(),
    textureAssets: new Map(),
    inspectionAssets: new Map(),
    inspectionErrors: new Map(),
    requestedInspections: new Set(),
  };
}

function renderViewer(session: ViewerSession, initialObjectKey?: unknown): void {
  viewer?.dispose();
  viewerSession = session;
  const { scene } = session;
  const initialMode = ['walkmesh', 'doorWalkmesh', 'placeableWalkmesh'].includes(scene.manifest.source)
    ? 'collision'
    : 'model';
  const animations = viewerAnimations(scene);
  const animationInSelectedData = (scene.manifest.areaObjects || []).length > 0;
  const savedViewer = vscode.getState?.()?.viewer;
  const savedInspector = savedViewer?.scene === viewerStateKey(scene) ? savedViewer.inspector : undefined;
  const inspectorWidth = validInspectorWidth(savedInspector?.width) ? savedInspector.width : 460;
  const inspectorCollapsed = savedInspector?.collapsed === true;
  const savedIndex = savedViewer?.scene === viewerStateKey(scene)
    ? savedAnimationIndex(animations, savedViewer)
    : -1;
  const module = scene.manifest.module;
  app.innerHTML = `<section class="viewer-shell">
    <header class="viewer-toolbar">
      <strong>${escapeHtml(scene.manifest.name)}</strong>
      <span class="spacer"></span>
      ${module ? `<label>Area <select id="viewer-area">${module.areas.map((area) => `<option ${area.toLowerCase() === module.entryArea.toLowerCase() ? 'selected' : ''}>${escapeHtml(area)}</option>`).join('')}</select></label>` : ''}
      ${animationInSelectedData ? '' : animationControl(animations, savedIndex)}
    </header>
    <div id="viewer-workbench" class="viewer-workbench" style="--viewer-inspector-width:${inspectorWidth}px" data-inspector-collapsed="${inspectorCollapsed}">
      <div class="viewer-viewport"><canvas id="viewer-canvas" tabindex="0" aria-label="Interactive nwnrs 3D viewport. Use W A S D to fly and Q E to descend or ascend."></canvas>
        <div id="viewer-status" class="viewer-status" role="status"></div>
        <button id="viewer-inspector-reopen" class="viewer-inspector-reopen secondary" title="Open inspector" aria-label="Open inspector">Inspector</button>
      </div>
      <div id="viewer-inspector-sash" class="viewer-inspector-sash" role="separator" aria-label="Resize inspector" aria-orientation="vertical" aria-valuemin="340" aria-valuemax="720" aria-valuenow="${inspectorWidth}" tabindex="0"></div>
      <aside id="viewer-inspector" class="viewer-inspector" aria-label="Scene inspector">
        <header class="viewer-inspector-header">
          <div class="viewer-inspector-toolbar">
            <label class="viewer-inspector-scope-label"><span class="sr-only">Inspector scope</span><select id="viewer-inspector-scope"><option value="selection">Selected Object</option><option value="scene">Scene</option><option value="dependencies">Dependencies</option></select></label>
            <button id="viewer-inspector-technical" class="secondary icon-button" title="Toggle technical field names" aria-pressed="${savedInspector?.technicalNames === true}">{ }</button>
            <button id="viewer-inspector-collapse" class="secondary icon-button" title="Collapse inspector" aria-label="Collapse inspector">›</button>
          </div>
          <div id="viewer-inspector-context" class="viewer-inspector-context"></div>
          <div class="viewer-inspector-navigation">
            <input id="viewer-inspector-search" type="search" placeholder="Search properties…" aria-label="Search inspector" value="${escapeAttribute(savedInspector?.query || '')}">
            <select id="viewer-inspector-jump" aria-label="Jump to section" hidden><option value="">Jump to section…</option></select>
          </div>
        </header>
        <div id="viewer-inspector-content" class="viewer-inspector-content"></div>
      </aside>
    </div>
  </section>`;
  const canvas = document.getElementById('viewer-canvas');
  if (!(canvas instanceof HTMLCanvasElement)) {
    throw new Error('The resource editor canvas is missing.');
  }
  viewer = createViewer(canvas, scene, {
    status: requiredWebviewElement('viewer-status'),
    animationTime: webviewElement('viewer-animation-time'),
    animationEvent: webviewElement('viewer-animation-event'),
    workbench: requiredWebviewElement('viewer-workbench'),
    inspector: requiredWebviewElement('viewer-inspector'),
    inspectorContent: requiredWebviewElement('viewer-inspector-content'),
    inspectorContext: requiredWebviewElement('viewer-inspector-context'),
    inspectorScope: requiredWebviewElement('viewer-inspector-scope'),
    inspectorSearch: requiredWebviewElement('viewer-inspector-search'),
    inspectorJump: requiredWebviewElement('viewer-inspector-jump'),
    inspectorTechnical: requiredWebviewElement('viewer-inspector-technical'),
    inspectorCollapse: requiredWebviewElement('viewer-inspector-collapse'),
    inspectorReopen: requiredWebviewElement('viewer-inspector-reopen'),
    inspectorSash: requiredWebviewElement('viewer-inspector-sash'),
  }, initialMode, session, typeof initialObjectKey === 'string' ? initialObjectKey : undefined,
  animations, savedIndex, animationInSelectedData);
  const area = webviewElement('viewer-area');
  if (area) area.onchange = () => {
    viewer?.dispose();
    app.innerHTML = '<div class="loading">Loading area…</div>';
    vscode.postMessage({ type: 'selectArea', area: area.value });
  };
}

function viewerAnimations(scene: DecodedScenePacket): ViewerAnimationSelection[] {
  const animatedModels = scene.manifest.models.filter((entry) => (entry.animations || []).length > 0);
  return animatedModels.flatMap((model) => model.animations.map((animation, animationIndex) => ({
    modelIndex: scene.manifest.models.indexOf(model),
    animationIndex,
    name: animation.name,
    label: animatedModels.length > 1 ? `${model.name} — ${animation.name}` : animation.name,
  })));
}

function savedAnimationIndex(
  animations: readonly ViewerAnimationSelection[],
  savedViewer: PersistedViewerState,
): number {
  const selection = savedViewer?.animationSelection;
  if (selection) return animations.findIndex((entry) => entry.modelIndex === selection.modelIndex && entry.animationIndex === selection.animationIndex);
  return savedViewer?.animationName
    ? animations.findIndex((entry) => entry.name.toLowerCase() === savedViewer.animationName)
    : -1;
}

function animationControl(
  animations: readonly ViewerAnimationSelection[],
  selectedIndex = -1,
): string {
  return `<div class="viewer-animation-row"><label class="viewer-animation-control">Animation <select id="viewer-animation"><option value="">None</option>${animations.map((entry, index) => `<option value="${index}" ${index === selectedIndex ? 'selected' : ''}>${escapeHtml(entry.label)}</option>`).join('')}</select></label><span id="viewer-animation-time" class="viewer-animation-time" aria-live="off"></span><span id="viewer-animation-event" class="viewer-animation-time" aria-live="polite"></span></div>`;
}

function animationPlaybackScope(
  scene: DecodedScenePacket,
  modelIndex: number,
  animationIndex: number,
): Map<number, number> {
  const selected = scene.manifest.models[modelIndex]?.animations[animationIndex];
  if (!selected) return new Map();
  const normalizedName = selected.name.toLowerCase(); const scope = new Map([[modelIndex, animationIndex]]); const visited = new Set();
  const visit = (candidateModel: number): void => {
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

function dispatchAnimationEvents(
  animation: PacketAnimation | undefined,
  previousElapsed: number,
  elapsed: number,
  dispatch: (event: PacketAnimationEvent) => void,
): number {
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

function viewerStateKey(scene: DecodedScenePacket): string {
  return `${scene.manifest.source}:${scene.manifest.name || ''}`;
}

function validViewerCamera(camera: unknown): camera is ViewerCamera {
  if (!isRecord(camera)) return false;
  return typeof camera.yaw === 'number' && Number.isFinite(camera.yaw)
    && typeof camera.pitch === 'number' && Number.isFinite(camera.pitch)
    && typeof camera.distance === 'number' && Number.isFinite(camera.distance)
    && camera.distance > 0
    && Array.isArray(camera.target)
    && camera.target.length === 3
    && camera.target.every((value) => typeof value === 'number' && Number.isFinite(value));
}

function validInspectorWidth(width: unknown): width is number {
  return typeof width === 'number' && Number.isFinite(width) && width >= 340 && width <= 720;
}

function sceneInspectorContent(
  scene: DecodedScenePacket,
  query = '',
  openSections: ReadonlySet<string> = new Set(),
  useDefaultSections = true,
): string {
  const environment = typeof scene.manifest.environment === 'object'
    ? scene.manifest.environment.nwn
    : undefined;
  const diagnostics = scene.manifest.diagnostics;
  const collisionCount = scene.manifest.instances.filter((entry) => entry.kind === 'collision').length;
  const matchingModels = scene.manifest.models.filter((entry) => inspectorMatches(query, entry.name, entry.nodes, entry.materials));
  const modelDetails = matchingModels.map((model) => `<details class="viewer-nested-details"><summary>${escapeHtml(model.name)} · ${model.nodes.length} nodes · ${model.meshes.length} meshes · ${model.materials.length} materials</summary>
    <div class="viewer-detail-section"><strong>Nodes</strong>${model.nodes.map((node) => `<div class="node-row" style="padding-left:${Math.max(0, nodeDepth(model, node) * 10)}px"><span>${escapeHtml(node.name)}</span><small>${escapeHtml(node.kind)}</small></div>`).join('') || '<div class="muted">No nodes</div>'}</div>
    <div class="viewer-detail-section"><strong>Materials</strong>${model.resolvedMaterials.map((material) => `<div class="inspector-list-row"><strong>Material ${material.materialIndex}</strong><span>${escapeHtml(material.renderHint || 'default')}</span><small>${material.textures.map((texture) => `${texture.role}: ${texture.name}${texture.texture == null ? ' ⚠' : ''}`).map(escapeHtml).join(' · ')}</small>${material.mtr ? `<small>MTR: ${escapeHtml(material.mtr.resource)}</small>` : ''}</div>`).join('') || '<div class="muted">No materials</div>'}${model.nodeTextures.map((texture) => `<div class="inspector-list-row"><strong>${escapeHtml(texture.role)}</strong><span>${escapeHtml(texture.name)} ${texture.texture == null ? '⚠' : ''}</span></div>`).join('')}</div>
  </details>`).join('');
  const matchingShaders = scene.manifest.shaders.filter((entry) => inspectorMatches(query, entry.resource, entry.stage, entry.source));
  const shaders = matchingShaders.map((shader) => `<details class="viewer-nested-details"><summary>${escapeHtml(shader.resource)} · ${escapeHtml(shader.stage)}</summary><pre>${escapeHtml(shader.source)}</pre></details>`).join('');
  const matchingDiagnostics = diagnostics.filter((entry) => inspectorMatches(query, entry.code, entry.message));
  const overviewEntries = [
    ['Source', scene.manifest.source], ['Models', scene.manifest.models.length],
    ['Textures', scene.manifest.textures.length], ['Collision', collisionCount],
    ['Diagnostics', diagnostics.length],
    ...(environment ? [
      ['Time', environment.isNight ? 'Night' : 'Day'],
      ['Fog clip', environment.fogClipDistance ?? 'unset'],
      ['Skybox', environment.skybox ?? 'unset'],
      ['Weather', `rain ${environment.chanceRain ?? 0}% · snow ${environment.chanceSnow ?? 0}% · lightning ${environment.chanceLightning ?? 0}%`],
    ] : []),
  ].filter(([label, value]) => inspectorMatches(query, label, value));
  const overview = overviewEntries.length ? `<div class="inspection-property-grid">${overviewEntries.map(([label, value]) => compactPropertyRow(label, value)).join('')}</div>` : '';
  const sections = [
    overview ? inspectorSection('scene-overview', 'Overview', overviewEntries.length, overview, openSections.has('scene-overview') || (useDefaultSections && openSections.size === 0)) : '',
    modelDetails ? inspectorSection('scene-models', 'Models', matchingModels.length, modelDetails, openSections.has('scene-models')) : '',
    shaders ? inspectorSection('scene-shaders', 'Shaders', matchingShaders.length, shaders, openSections.has('scene-shaders')) : '',
    matchingDiagnostics.length ? inspectorSection('scene-diagnostics', 'Diagnostics', matchingDiagnostics.length, matchingDiagnostics.map((entry) => `<div class="diagnostic ${escapeAttribute(entry.severity)}"><strong>${escapeHtml(entry.code)}</strong><br>${escapeHtml(entry.message)}</div>`).join(''), openSections.has('scene-diagnostics')) : '',
  ].join('');
  return sections || '<div class="empty inspector-empty">No scene data matches this search.</div>';
}

function dependenciesInspectorContent(scene: DecodedScenePacket, query = ''): string {
  const incoming = new Map<number, string[]>();
  for (const edge of scene.manifest.dependencies.edges) {
    const relationships = incoming.get(edge.to) || [];
    relationships.push(edge.relationship);
    incoming.set(edge.to, relationships);
  }
  const nodes = scene.manifest.dependencies.nodes.filter((node) => inspectorMatches(query, node.resource, node.kind, node.state, node.origin, node.message, incoming.get(node.id)));
  return `<div class="inspector-resource-list">${nodes.map((node) => {
    const relationships = incoming.get(node.id) || [];
    return `<button class="dependency inspector-resource-row ${node.state}" data-resource="${escapeAttribute(node.resource)}" ${node.state === 'resolved' ? '' : 'disabled'}><span>${escapeHtml(node.resource)}</span><small>${escapeHtml(node.kind)} · ${escapeHtml(node.state)}${relationships.length ? ` · ${escapeHtml(relationships.join(', '))}` : ''}</small>${node.origin ? `<small>${escapeHtml(node.origin)}</small>` : ''}${node.message ? `<small>${escapeHtml(node.message)}</small>` : ''}</button>`;
  }).join('') || '<div class="empty inspector-empty">No dependencies match this search.</div>'}</div>`;
}

function inspectorMatches(query: unknown, ...values: readonly unknown[]): boolean {
  const normalized = String(query || '').trim().toLocaleLowerCase();
  if (!normalized) return true;
  return values.some((value) => JSON.stringify(value ?? '').toLocaleLowerCase().includes(normalized));
}

function compactPropertyRow(label: unknown, value: unknown, detail: unknown = ''): string {
  return `<div class="inspection-property-row"><span>${escapeHtml(label)}</span><strong>${escapeHtml(String(value))}</strong>${detail ? `<small>${escapeHtml(detail)}</small>` : ''}</div>`;
}

function inspectorSection(
  id: string,
  label: string,
  count: number,
  content: string,
  open = false,
): string {
  return `<details class="inspector-section" data-section-id="${escapeAttribute(id)}" ${open ? 'open' : ''}><summary><span>${escapeHtml(label)}</span><small>${count}</small></summary><div class="inspector-section-content">${content}</div></details>`;
}

function inspectionContent(
  state: InspectionState | undefined,
  options: InspectionContentOptions = {},
): string {
  if (!state || state.status === 'loading') return '<div class="inspection-status muted">Loading authored data…</div>';
  if (state.status === 'error') return `<div class="diagnostic error"><strong>Authored data could not be loaded</strong><br>${escapeHtml(state.message)}</div>`;
  const inspection = state.data;
  const query = options.query || '';
  const route = options.route || { page: 'object' };
  const technicalNames = options.technicalNames === true;
  const openSections = options.openSections || new Set();
  if (route.page === 'references') return inspectionReferencesPage(inspection, query);
  if (route.page === 'raw-sources') return inspectionRawSourcesPage(inspection, query);
  if (route.page === 'raw-source') return inspectionRawSourcePage(inspection, route, query, technicalNames);
  if (route.page === 'components') return inspectionComponentsPage(options.scene, options.objectKey, options.selectedComponentId, query);
  if (route.page === 'field') return inspectionFieldPage(inspection, route, query, technicalNames, openSections, options.useDefaultSections !== false);
  const sections = (inspection.sections || []).map((section, sectionIndex) => {
    const fields = section.fields.filter((field) => inspectionFieldMatches(field, query));
    if (!fields.length && query) return '';
    const rows = fields.map((field) => inspectionFieldRow(field, {
      page: 'field', root: 'section', rootIndex: sectionIndex, trail: [{ kind: 'field', index: section.fields.indexOf(field) }],
    }, technicalNames)).join('');
    const id = `inspection-${section.id || sectionIndex}`;
    return inspectorSection(id, section.label, fields.length, `<div class="inspection-property-grid">${rows}</div>`, openSections.has(id) || (options.useDefaultSections !== false && openSections.size === 0 && section.defaultOpen));
  }).join('');
  const navigation = `<div class="inspector-navigation-list">
    ${inspectorNavigationRow('Referenced Resources', `${inspection.references?.length || 0} resources`, { page: 'references' })}
    ${inspectorNavigationRow('Raw GFF Sources', `${inspection.sources?.length || 0} sources`, { page: 'raw-sources' })}
    ${inspectorNavigationRow('Rendered Components', `${options.componentCount || 0} components`, { page: 'components' })}
  </div>`;
  const diagnostics = (inspection.diagnostics || []).filter((message) => inspectorMatches(query, message)).map((message) => `<div class="diagnostic warning">${escapeHtml(message)}</div>`).join('');
  return `<div class="inspection-data">${sections || '<div class="empty inspector-empty">No authored properties match this search.</div>'}${navigation}${diagnostics}</div>`;
}

function inspectionFieldMatches(field: InspectionField, query: string): boolean {
  return inspectorMatches(query, field.name, field.label, field.display, field.text, field.value64, field.resource, field.lookup, field.localized)
    || (field.fields || []).some((entry) => inspectionFieldMatches(entry, query))
    || (field.entries || []).some((entry) => (entry.fields || []).some((child) => inspectionFieldMatches(child, query)));
}

function inspectionFieldRow(
  field: InspectionField,
  route: InspectorRoute,
  technicalNames: boolean,
): string {
  const resource = field.resource;
  const lookup = field.lookup;
  const provenance = field.provenance || {};
  const sourceBadge = provenance.layer === 'instance' ? 'GIT' : provenance.resource?.split('.').pop()?.toUpperCase() || 'SOURCE';
  const sourceDetail = [provenance.layer, provenance.resource, provenance.origin].filter(Boolean).join(' · ');
  const display = lookup?.label ? `${lookup.label} · row ${lookup.row}` : field.display || 'unset';
  const value = resource
    ? `<button class="inspection-value-link inspection-resource-open" data-resource="${escapeAttribute(resource.resource)}" ${resource.resolved ? '' : 'disabled'}>${escapeHtml(display)}</button>`
    : longInspectorValue(display);
  return `<div class="inspection-property-row inspection-field-row"><div class="inspection-property-label"><span>${escapeHtml(field.label)}</span>${technicalNames ? `<code>${escapeHtml(field.name)}</code>` : ''}</div><div class="inspection-property-value">${value}</div><span class="inspection-source-badge" tabindex="0" title="${escapeAttribute(sourceDetail || 'Unknown source')}">${escapeHtml(sourceBadge)}</span><button class="inspection-row-more" data-inspector-route="${routeAttribute(route)}" title="Inspect ${escapeAttribute(field.label)}" aria-label="Inspect ${escapeAttribute(field.label)}">›</button></div>`;
}

function longInspectorValue(value: unknown): string {
  const text = String(value ?? 'unset');
  if (text.length <= 180 && !text.includes('\n')) return escapeHtml(text);
  const preview = `${text.replace(/\s+/gu, ' ').slice(0, 150)}…`;
  return `<details class="inspection-long-value"><summary>${escapeHtml(preview)}</summary><div>${escapeHtml(text)}</div></details>`;
}

function inspectorNavigationRow(label: string, detail: string, route: InspectorRoute): string {
  return `<button class="inspector-navigation-row" data-inspector-route="${routeAttribute(route)}"><span>${escapeHtml(label)}</span><small>${escapeHtml(detail)}</small><b aria-hidden="true">›</b></button>`;
}

function routeAttribute(route: InspectorRoute): string {
  return escapeAttribute(JSON.stringify(route));
}

function inspectionReferencesPage(inspection: AreaObjectInspection, query: string): string {
  const references = (inspection.references || []).filter((entry) => inspectorMatches(query, entry.resource, entry.origin, entry.resolved));
  return `<div class="inspector-resource-list">${references.map((reference) => `<button class="dependency inspector-resource-row inspection-resource-open ${reference.resolved ? 'resolved' : 'missing'}" data-resource="${escapeAttribute(reference.resource)}" ${reference.resolved ? '' : 'disabled'}><span>${escapeHtml(reference.resource)}</span><small>${reference.resolved ? escapeHtml(reference.origin || 'resolved') : 'missing'}</small></button>`).join('') || '<div class="empty inspector-empty">No references match this search.</div>'}</div>`;
}

function inspectionRawSourcesPage(inspection: AreaObjectInspection, query: string): string {
  const sources = (inspection.sources || []).map((source, index) => ({ source, index })).filter(({ source }) => inspectorMatches(query, source.layer, source.resource, source.origin, source.data));
  return `<div class="inspector-navigation-list">${sources.map(({ source, index }) => inspectorNavigationRow(`${source.layer} · ${source.resource}`, `${source.data?.fields?.length || 0} fields · ${source.origin || 'unknown origin'}`, { page: 'raw-source', sourceIndex: index })).join('') || '<div class="empty inspector-empty">No raw sources match this search.</div>'}</div>`;
}

function inspectionRawSourcePage(
  inspection: AreaObjectInspection,
  route: InspectorRoute,
  query: string,
  technicalNames: boolean,
): string {
  if (route.sourceIndex === undefined) {
    return '<div class="diagnostic error">The selected raw source is invalid.</div>';
  }
  const source = inspection.sources?.[route.sourceIndex];
  if (!source) return '<div class="diagnostic error">The selected raw source no longer exists.</div>';
  const fields = (source.data?.fields || []).map((field, index) => ({ field, index })).filter(({ field }) => inspectionFieldMatches(field, query));
  return `<div class="inspector-source-origin">${escapeHtml(source.origin || 'unknown origin')} · struct ${source.data?.id ?? 'unknown'}</div><div class="inspection-property-grid">${fields.map(({ field, index }) => inspectionFieldRow(field, { page: 'field', root: 'source', rootIndex: route.sourceIndex, trail: [{ kind: 'field', index }] }, technicalNames)).join('') || '<div class="empty inspector-empty">No raw fields match this search.</div>'}</div>`;
}

function inspectionComponentsPage(
  scene: DecodedScenePacket | undefined,
  objectKey: string | undefined,
  selectedComponentId: number | undefined,
  query: string,
): string {
  const instances = (scene?.manifest.instances || []).map((instance, index) => ({ instance, id: Number.isInteger(instance.id) ? instance.id : index })).filter(({ instance }) => instance.objectKey === objectKey && inspectorMatches(query, instance));
  const vector = (values: readonly number[]) =>
    values.map((value) => Number(value).toFixed(3)).join(', ');
  return `<div class="selected-components">${instances.map(({ instance, id }) => {
    const modelName = scene && instance.model !== null
      ? scene.manifest.models[instance.model]?.name
      : undefined;
    return `<div class="selected-component${id === selectedComponentId ? ' selected' : ''}" data-component-id="${id}"><button class="component-select" data-component-id="${id}" aria-label="Select ${escapeAttribute(instance.label || instance.kind)}"><strong>${escapeHtml(instance.label || instance.kind)}</strong><span>${escapeHtml(instance.kind)}${modelName ? ` · ${escapeHtml(modelName)}` : ''}</span>${instance.resource ? `<small>${escapeHtml(instance.resource)}</small>` : ''}<small>position ${escapeHtml(vector(instance.position))} · scale ${escapeHtml(vector(instance.scale))}</small></button>${instance.resource ? `<button class="component-open" data-resource="${escapeAttribute(instance.resource)}" title="Open ${escapeAttribute(instance.resource)}">Open Resource</button>` : ''}</div>`;
  }).join('') || '<div class="empty inspector-empty">No rendered components match this search.</div>'}</div>`;
}

function inspectionRouteNode(
  inspection: AreaObjectInspection,
  route: InspectorRoute,
): { readonly node: InspectionField | InspectionStructure | undefined; readonly breadcrumbs: string[] } {
  const rootIndex = route.rootIndex;
  let node: InspectionField | InspectionStructure | undefined =
    rootIndex === undefined
      ? undefined
      : route.root === 'source'
        ? inspection.sources[rootIndex]?.data
        : { id: -1, fields: inspection.sections[rootIndex]?.fields || [] };
  const breadcrumbs: string[] = [];
  for (const token of route.trail || []) {
    if (token.kind === 'field') {
      node = node?.fields?.[token.index];
      if (!node) break;
      breadcrumbs.push(node.label || node.name);
    } else if (token.kind === 'entry') {
      node = node && isInspectionField(node) ? node.entries?.[token.index] : undefined;
      if (!node) break;
      breadcrumbs.push(`Entry ${token.index + 1}`);
    }
  }
  return { node, breadcrumbs };
}

function inspectionFieldPage(
  inspection: AreaObjectInspection,
  route: InspectorRoute,
  query: string,
  technicalNames: boolean,
  openSections: ReadonlySet<string> = new Set(),
  useDefaultSections = true,
): string {
  const { node: field } = inspectionRouteNode(inspection, route);
  if (!field) return '<div class="diagnostic error">The selected field no longer exists.</div>';
  const isStructure = !('name' in field);
  const provenance = 'provenance' in field ? field.provenance : undefined;
  const metadata = `<div class="inspection-property-grid inspection-field-metadata">
    ${compactPropertyRow('GFF name', 'name' in field ? field.name : 'structure')}${compactPropertyRow('Type', 'kind' in field ? field.kind : 'struct')}${'structId' in field && field.structId != null ? compactPropertyRow('Struct ID', field.structId) : ''}
    ${'text' in field && field.text != null ? compactPropertyRow('Stored value', field.text) : ''}${compactPropertyRow('Source layer', provenance?.layer || 'unknown')}${compactPropertyRow('Source resource', provenance?.resource || 'unknown')}${compactPropertyRow('Origin', provenance?.origin || 'unknown')}
    ${'lookup' in field && field.lookup ? `${compactPropertyRow('2DA', field.lookup.resource)}${compactPropertyRow('2DA row', field.lookup.row, field.lookup.label || '')}` : ''}
    ${'localized' in field && field.localized ? `${compactPropertyRow('String reference', field.localized.strRef ?? 'none')}${compactPropertyRow('Resolved from', field.localized.source || 'unresolved')}${compactPropertyRow('Language', field.localized.languageId ?? 'unset', field.localized.gender || '')}` : ''}
  </div>`;
  const resourceField = 'resource' in field ? field.resource : undefined;
  const resource = resourceField ? `<button class="inspection-resource-open" data-resource="${escapeAttribute(resourceField.resource)}" ${resourceField.resolved ? '' : 'disabled'}>${resourceField.resolved ? 'Open' : 'Missing'} ${escapeHtml(resourceField.resource)}</button>` : '';
  const localizedField = 'localized' in field ? field.localized : undefined;
  const localized = (localizedField?.entries || []).filter((entry) => inspectorMatches(query, entry.id, entry.text)).map((entry) => `<div class="inspector-list-row"><small>language/gender id ${entry.id}</small><span>${escapeHtml(entry.text)}</span></div>`).join('');
  const trail = route.trail || [];
  const childFieldValues = field.fields || [];
  const childFields = childFieldValues.map((child, index) => ({ child, index })).filter(({ child }) => inspectionFieldMatches(child, query)).map(({ child, index }) => inspectionFieldRow(child, { ...route, trail: [...trail, { kind: 'field', index }] }, technicalNames)).join('');
  const entryValues = 'entries' in field ? field.entries || [] : [];
  const entries = entryValues.map((entry, index) => ({ entry, index })).filter(({ entry }) => inspectorMatches(query, entry)).map(({ entry, index }) => inspectorNavigationRow(`Entry ${index + 1}`, `struct ${entry.id} · ${entry.fields.length} fields`, { ...route, trail: [...trail, { kind: 'entry', index }] })).join('');
  const opaque = 'value64' in field && field.value64 != null ? `<details class="inspection-opaque-value"><summary>Opaque payload · ${escapeHtml(field.display)}</summary><code>${escapeHtml(field.value64)}</code></details>` : '';
  const sectionOpen = (id: string) => openSections.has(id) || (useDefaultSections && openSections.size === 0);
  const display = 'display' in field ? field.display : 'unset';
  return `<div class="inspection-field-page">${isStructure ? '' : `<div class="inspection-field-primary">${longInspectorValue(display)}${resource}</div>`}${metadata}${localized ? inspectorSection('localized-values', 'Inline Localized Values', localizedField?.entries.length || 0, localized, sectionOpen('localized-values')) : ''}${childFields ? inspectorSection('nested-fields', 'Nested Fields', childFieldValues.length, `<div class="inspection-property-grid">${childFields}</div>`, sectionOpen('nested-fields')) : ''}${entries ? inspectorSection('list-entries', 'List Entries', entryValues.length, `<div class="inspector-navigation-list">${entries}</div>`, sectionOpen('list-entries')) : ''}${opaque}</div>`;
}

function blueprintResourceForObject(object: SceneAreaObject | undefined): string | undefined {
  if (!object?.templateResref) return undefined;
  const extensions: Readonly<Record<string, string>> = {
    creature: 'utc', door: 'utd', placeable: 'utp', item: 'uti', store: 'utm',
    encounter: 'ute', sound: 'uts', waypoint: 'utw', trigger: 'utt',
  };
  const extension = extensions[object.kind];
  return extension ? `${object.templateResref}.${extension}` : undefined;
}

function inspectorRouteTitle(
  inspection: AreaObjectInspection,
  route: InspectorRoute | undefined,
): string {
  if (!route || route.page === 'object') return 'Authored Data';
  if (route.page === 'references') return 'Referenced Resources';
  if (route.page === 'raw-sources') return 'Raw GFF Sources';
  if (route.page === 'raw-source') {
    return route.sourceIndex === undefined
      ? 'Raw GFF Source'
      : inspection.sources[route.sourceIndex]?.resource || 'Raw GFF Source';
  }
  if (route.page === 'components') return 'Rendered Components';
  if (route.page === 'field') {
    const resolved = inspectionRouteNode(inspection, route);
    return resolved.breadcrumbs.join(' › ') || 'Field';
  }
  return 'Authored Data';
}

function parentInspectorRoute(route: InspectorRoute | undefined): InspectorRoute {
  if (route?.page === 'raw-source') return { page: 'raw-sources' };
  const trail = route?.trail;
  if (route?.page === 'field' && trail && trail.length > 1) {
    return { ...route, trail: trail.slice(0, -1) };
  }
  if (route?.page === 'field' && route.root === 'source') return { page: 'raw-source', sourceIndex: route.rootIndex };
  return { page: 'object' };
}

function boundedStateEntries<V>(
  entries: Iterable<readonly [string, V]>,
  limit = 32,
): Record<string, V> {
  return Object.fromEntries([...entries].slice(-limit));
}

function createViewer(
  canvas: HTMLCanvasElement,
  scene: DecodedScenePacket,
  elements: ViewerElements,
  initialMode = 'model',
  session: ViewerSession = createViewerSession(scene),
  initialObjectKey?: string,
  animations: readonly ViewerAnimationSelection[] = viewerAnimations(scene),
  initialAnimationIndex = -1,
  animationInSelectedData = false,
): ViewerController {
  const glContext = canvas.getContext('webgl2', { antialias: true, alpha: false });
  if (!glContext) throw new Error('WebGL 2 is required for the nwnrs model viewer.');
  const gl: WebGL2RenderingContext = glContext;
  const nwnEnvironment = typeof scene.manifest.environment === 'object'
    ? scene.manifest.environment.nwn
    : undefined;
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
    layout(location=4) in vec4 aUvRect; layout(location=5) in float aRenderMode;
    uniform mat4 uViewProjection; uniform vec3 uCameraRight; uniform vec3 uCameraUp;
    out vec2 vUv; out vec4 vColor;
    void main(){
      float c=cos(aSizeRotationAlpha.z),s=sin(aSizeRotationAlpha.z);
      vec2 rotated=mat2(c,-s,s,c)*(aCorner*aSizeRotationAlpha.xy);
      vec3 world=aRenderMode>0.5
        ? aCenter+vec3(rotated.x,rotated.y,0.0)
        : aCenter+uCameraRight*rotated.x+uCameraUp*rotated.y;
      gl_Position=uViewProjection*vec4(world,1.0);
      vec2 unit=aCorner*0.5+0.5; vUv=aUvRect.xy+unit*aUvRect.zw;
      vColor=vec4(aColor,aSizeRotationAlpha.w);
    }
  `, `#version 300 es
    precision highp float; in vec2 vUv; in vec4 vColor;
    uniform sampler2D uTexture; uniform bool uHasTexture; out vec4 color;
    void main(){vec4 texel=uHasTexture?texture(uTexture,vUv):vec4(1.0);color=texel*vColor;if(color.a<0.01)discard;}
  `);
  const ribbonProgram = createProgram(gl, `#version 300 es
    precision highp float;
    layout(location=0) in vec3 aPosition; layout(location=1) in vec2 aUv;
    layout(location=2) in vec4 aColor; uniform mat4 uViewProjection;
    out vec2 vUv; out vec4 vColor;
    void main(){gl_Position=uViewProjection*vec4(aPosition,1.0);vUv=aUv;vColor=aColor;}
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
  const ribbonUniforms = uniformLocations(gl, ribbonProgram, ['uViewProjection', 'uTexture', 'uHasTexture']);
  const gpuTextures: Array<WebGLTexture | null | undefined> = new Array(scene.manifest.textures.length);
  const requestedTextures = new Set<number>();
  const requestedAnimations = new Set<string>();
  const maxTextureSize = Number(gl.getParameter(gl.MAX_TEXTURE_SIZE));
  const s3tc = gl.getExtension('WEBGL_compressed_texture_s3tc') || gl.getExtension('WEBKIT_WEBGL_compressed_texture_s3tc');
  const pointLightTexture = gl.createTexture(); gl.bindTexture(gl.TEXTURE_2D, pointLightTexture);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
  const primitiveCache = new Map<string, PrimitiveGpu>();
  const spriteGpu = createSpriteGpu(gl);
  const ribbonGpu = createRibbonGpu(gl);
  const stateKey = viewerStateKey(scene);
  const savedViewer = vscode.getState?.()?.viewer;
  const savedCamera = savedViewer?.scene === stateKey ? savedViewer.camera : undefined;
  const restoredCamera = validViewerCamera(savedCamera);
  const camera: ViewerCamera = restoredCamera
    ? { yaw: savedCamera.yaw, pitch: savedCamera.pitch, distance: savedCamera.distance, target: [...savedCamera.target] }
    : { yaw: -0.8, pitch: 0.65, distance: 20, target: [0, 0, 0] };
  let mode = initialMode;
  let animationFrame: number | undefined;
  let disposed = false;
  let selectedAnimationIndex = Number.isInteger(initialAnimationIndex) && animations[initialAnimationIndex]
    ? initialAnimationIndex
    : -1;
  let activeAnimation: AnimationPlayback | undefined;
  let pendingAnimation: AnimationPlayback | undefined;
  let animationTime = 0;
  let animationElapsed = 0;
  let animationStarted = 0;
  let animationPlaying = false;
  let transition: AnimationTransition | undefined;
  let displayedEventTimer: ReturnType<typeof setTimeout> | undefined;
  let pointLightsCache: PointLightCollection | undefined;
  let pointLightsDirty = true;
  const lightRuntime = { storage: new Float32Array(12 * 16), count: 0, values: new Float32Array(12) };
  let renderScale = 1; let slowFrames = 0; let fastFrames = 0;
  const viewerStarted = performance.now();
  const hasDynamicEffects = scene.manifest.models.some((model) => model.nodes.some((node) => node.emitter || node.dangly)
    || model.resolvedMaterials.some((material) => material.textures.some((texture) => directiveValue(texture, 'proceduretype')?.toLowerCase() === 'cycle')));
  const boundsCatalog = sceneBoundsCatalog(scene);
  const bounds = boundsCatalog.scene;
  const authoredObjects = new Map<string, SceneAreaObject>((scene.manifest.areaObjects || [])
    .map((object) => [object.key, object]));
  const savedObjectKey = savedViewer?.scene === stateKey
    ? savedViewer.selectedObjectKey
    : undefined;
  let selectedObjectKey = initialObjectKey && authoredObjects.has(initialObjectKey)
    ? initialObjectKey
    : savedObjectKey && authoredObjects.has(savedObjectKey) ? savedObjectKey : undefined;
  const componentInstances = new Map<number, SceneInstance>((scene.manifest.instances || []).map((instance, index) => [
    Number.isInteger(instance.id) ? instance.id : index,
    instance,
  ]));
  const savedComponentId = savedViewer?.scene === stateKey && Number.isInteger(savedViewer.selectedComponentId)
    ? savedViewer.selectedComponentId
    : undefined;
  let selectedComponentId = savedComponentId !== null
      && savedComponentId !== undefined
      && componentInstances.get(savedComponentId)?.objectKey === selectedObjectKey
    ? savedComponentId
    : undefined;
  const savedInspector = savedViewer?.scene === stateKey ? savedViewer.inspector || {} : {};
  let inspectorScope: 'selection' | 'scene' | 'dependencies' =
    selectedObjectKey && savedInspector.scope === 'selection'
    ? 'selection'
    : savedInspector.scope === 'scene' || savedInspector.scope === 'dependencies'
      ? savedInspector.scope
      : selectedObjectKey ? 'selection' : 'scene';
  let inspectorQuery = typeof savedInspector.query === 'string' ? savedInspector.query : '';
  let inspectorTechnicalNames = savedInspector.technicalNames === true;
  let inspectorCollapsed = savedInspector.collapsed === true;
  let inspectorWidth = validInspectorWidth(savedInspector.width) ? savedInspector.width : 460;
  const inspectorRoutes = new Map<string, InspectorRoute>(
    Object.entries(savedInspector.routes || {})
      .flatMap(([key, value]) => isInspectorRoute(value) ? [[key, value] as const] : []),
  );
  const inspectorScrollPositions = new Map(Object.entries(savedInspector.scrollPositions || {}).map(([key, value]) => [key, Number(value) || 0]));
  const inspectorOpenSections = new Map<string, Set<string>>(
    Object.entries(savedInspector.openSections || {}).map(([key, value]) => [
      key,
      new Set(Array.isArray(value)
        ? value.filter((entry): entry is string => typeof entry === 'string')
        : []),
    ]),
  );
  const inspectorTouchedSections = new Set(Array.isArray(savedInspector.touchedSections) ? savedInspector.touchedSections : []);
  let inspectorRoute = selectedObjectKey ? inspectorRoutes.get(selectedObjectKey) || { page: 'object' } : { page: 'object' };
  let hoveredComponentId: number | undefined;
  let selectionGpu: SelectionGpu | undefined;
  const modelRuntime = scene.manifest.models.map((entry) => createModelRuntime(entry));
  scene.manifest.models.forEach((entry, modelIndex) => {
    entry.animations.forEach((animation, animationIndex) => {
      const key = animationAssetKey(modelIndex, animationIndex);
      const retained = session.animationAssets.get(key);
      const runtime = modelRuntime[modelIndex];
      if (!runtime) return;
      if (retained) installAnimationAsset(runtime, retained);
      else if (!scene.manifest.assetKey && animation?.tracksLoaded === true) {
        const inline = createAnimationAsset(scene, modelIndex, animationIndex, animation, scene.binary);
        const installed = installAnimationAsset(runtime, inline);
        session.animationAssets.set(key, installed);
      }
    });
  });
  for (const runtime of modelRuntime) runtime.chunkBatch = {
    buffer: gl.createBuffer(), values: new Float32Array(16 * 16), count: 0, gpuCapacity: 0,
  };
  const modelIndexByName = new Map<string, number>(
    scene.manifest.models.map((model, index) => [model.name.toLowerCase(), index]),
  );
  const instanceRuntime: SceneInstanceRuntime[] = scene.manifest.instances.map((instance) => ({
    instance,
    base: composeTransform4(instance.position, instance.rotationAxisAngle, instance.scale),
    dynamic: instance.kind === 'creature' || instance.kind === 'door' || instance.kind === 'placeable' || instance.kind === 'item',
    overlay: createOverlayGpu(gl, instance.polygon),
  }));
  let poseFrame = 0;

  for (const [textureIndex, asset] of session.textureAssets) {
    if (scene.manifest.textures[textureIndex]) gpuTextures[textureIndex] = createTexture(gl, asset.manifest, asset.binary, s3tc);
  }

  function requestTexture(textureIndex: number): void {
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

  function requestAnimation(modelIndex: number, animationIndex: number): void {
    const key = animationAssetKey(modelIndex, animationIndex); const animation = scene.manifest.models[modelIndex]?.animations[animationIndex];
    if (!animation || animationLoaded(modelIndex, animationIndex) || requestedAnimations.has(key) || !scene.manifest.assetKey) return;
    requestedAnimations.add(key);
    vscode.postMessage({ type: 'loadAnimation', assetKey: scene.manifest.assetKey, modelIndex, animationIndex });
  }

  function requestInspection(objectKey: string | undefined): void {
    if (!objectKey || !scene.manifest.assetKey || session.inspectionAssets.has(objectKey)
        || session.requestedInspections.has(objectKey)) return;
    session.inspectionErrors.delete(objectKey);
    session.requestedInspections.add(objectKey);
    vscode.postMessage({
      type: 'inspectAreaObject',
      assetKey: scene.manifest.assetKey,
      objectKey,
    });
  }

  function applyInspection(assetKey: unknown, objectKey: unknown, inspection: unknown): void {
    if (typeof objectKey !== 'string') return;
    if (assetKey !== scene.manifest.assetKey) return;
    if (!isAreaObjectInspection(inspection) || inspection.key !== objectKey) {
      applyInspectionError(assetKey, objectKey, 'The native service returned an invalid authored-data payload.');
      return;
    }
    session.requestedInspections.delete(objectKey);
    session.inspectionErrors.delete(objectKey);
    session.inspectionAssets.set(objectKey, inspection);
    if (selectedObjectKey === objectKey) refreshInspector(true);
  }

  function applyInspectionError(assetKey: unknown, objectKey: unknown, message: unknown): void {
    if (typeof objectKey !== 'string') return;
    if (assetKey !== scene.manifest.assetKey) return;
    session.requestedInspections.delete(objectKey);
    session.inspectionErrors.set(objectKey, String(message || 'Unknown inspection error'));
    if (selectedObjectKey === objectKey) refreshInspector(true);
  }

  function applyTexture(asset: DecodedTexturePacket): void {
    if (asset.manifest.schema !== 'nwnrs.scene.texture') throw new Error(`Unexpected texture asset schema ${asset.manifest.schema}`);
    const index = asset.manifest.textureIndex;
    if (!Number.isInteger(index) || !scene.manifest.textures[index]) throw new Error(`Texture asset index ${index} is not in this scene.`);
    if (asset.manifest.assetKey !== scene.manifest.assetKey) throw new Error(`Texture asset ${index} belongs to a different scene.`);
    if (gpuTextures[index]) gl.deleteTexture(gpuTextures[index]);
    session.textureAssets.set(index, asset);
    gpuTextures[index] = createTexture(gl, asset.manifest, asset.binary, s3tc); requestedTextures.delete(index); draw();
  }

  function applyAnimation(asset: DecodedAnimationPacket): void {
    if (asset.manifest.schema !== 'nwnrs.scene.animation') throw new Error(`Unexpected animation asset schema ${asset.manifest.schema}`);
    const { modelIndex, animationIndex, animation } = asset.manifest; const model = scene.manifest.models[modelIndex];
    const catalog = model?.animations[animationIndex];
    if (!catalog) throw new Error(`Animation asset ${modelIndex}:${animationIndex} is not in this scene.`);
    if (asset.manifest.assetKey !== scene.manifest.assetKey) throw new Error(`Animation asset ${modelIndex}:${animationIndex} belongs to a different scene.`);
    if (animation.name !== catalog.name || animation.length !== catalog.length) throw new Error(`Animation asset ${modelIndex}:${animationIndex} does not match its catalog entry.`);
    const installed = createAnimationAsset(scene, modelIndex, animationIndex, animation, asset.binary);
    const key = animationAssetKey(modelIndex, animationIndex);
    const runtime = modelRuntime[modelIndex];
    if (!runtime) throw new Error(`Animation asset model ${modelIndex} has no renderer runtime.`);
    session.animationAssets.set(key, installAnimationAsset(runtime, installed));
    requestedAnimations.delete(key); poseFrame += 1; pointLightsDirty = true; maybeStartAnimation();
  }

  function animationLoaded(modelIndex: number, animationIndex: number): boolean {
    return modelRuntime[modelIndex]?.animationAssets.has(animationIndex) === true;
  }

  const resizeObserver = new ResizeObserver(() => draw());
  resizeObserver.observe(canvas);
  const persistState = (
    animationSelection?: PersistedViewerState['animationSelection'] | null,
  ): void => {
    if (elements.inspectorContent) {
      const key = inspectorViewKey(); const value = elements.inspectorContent.scrollTop;
      inspectorScrollPositions.delete(key); inspectorScrollPositions.set(key, value);
    }
    const previous = vscode.getState?.() || {};
    vscode.setState?.({ ...previous, viewer: {
      scene: stateKey,
      camera: { yaw: camera.yaw, pitch: camera.pitch, distance: camera.distance, target: [...camera.target] },
      animationSelection: animationSelection === undefined ? previous.viewer?.animationSelection : animationSelection,
      selectedObjectKey: selectedObjectKey || null,
      selectedComponentId: Number.isInteger(selectedComponentId) ? selectedComponentId : null,
      inspector: {
        width: inspectorWidth,
        collapsed: inspectorCollapsed,
        scope: inspectorScope,
        query: inspectorQuery,
        technicalNames: inspectorTechnicalNames,
        routes: boundedStateEntries(inspectorRoutes),
        scrollPositions: boundedStateEntries(inspectorScrollPositions),
        openSections: boundedStateEntries(new Map([...inspectorOpenSections].map(([key, value]) => [key, [...value]]))),
        touchedSections: [...inspectorTouchedSections].slice(-32),
      },
    } });
  };
  const cameraControls = bindViewportControls(
    canvas,
    camera,
    draw,
    () => persistState(),
    (event) => selectObject(pickAreaObject(event), false, true),
  );
  const contextLost = (event: Event): void => {
    event.preventDefault();
    if (elements.status) {
      elements.status.textContent = 'Graphics context lost; waiting for VS Code to restore it…';
    }
  };
  const contextRestored = () => { if (!disposed) renderViewer(session); };
  canvas.addEventListener('webglcontextlost', contextLost); canvas.addEventListener('webglcontextrestored', contextRestored);

  function primitiveGpu(
    modelIndex: number,
    meshIndex: number,
    primitiveIndex: number,
  ): PrimitiveGpu | undefined {
    const key = `${modelIndex}:${meshIndex}:${primitiveIndex}`;
    const cached = primitiveCache.get(key);
    if (cached) return cached;
    const model = scene.manifest.models[modelIndex];
    const runtime = modelRuntime[modelIndex];
    const mesh = model?.meshes[meshIndex];
    const primitive = mesh?.primitives[primitiveIndex];
    if (!model || !runtime || !mesh || !primitive) return undefined;
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
    const boneNodes = primitive.skinBones.map((name) => runtime.nodeByName.get(name.toLowerCase()) ?? -1);
    const vertices: number[] = [];
    const vertexConstraints: number[] = [];
    for (let corner = 0; corner < indices.length; corner += 1) {
      const vertex = Number(indices[corner] ?? 0);
      const px = positions[vertex * 3] ?? 0;
      const py = positions[vertex * 3 + 1] ?? 0;
      const pz = positions[vertex * 3 + 2] ?? 0;
      let nx = normals?.[vertex * 3]; let ny = normals?.[vertex * 3 + 1]; let nz = normals?.[vertex * 3 + 2];
      if (nx == null) {
        const face = Math.floor(corner / 3) * 3;
        [nx, ny, nz] = faceNormal(
          positions,
          Number(indices[face] ?? vertex),
          Number(indices[face + 1] ?? vertex),
          Number(indices[face + 2] ?? vertex),
        );
      }
      const uvIndex = Number(uvIndices[corner] ?? vertex);
      const influences: Array<[number, number]> = [];
      for (
        let influence = Number(skinOffsets[vertex] ?? 0);
        influence < Number(skinOffsets[vertex + 1] ?? 0);
        influence += 1
      ) {
        const localBone = Number(skinIndices[influence] ?? 0);
        const nodeIndex = boneNodes[localBone] ?? -1;
        const weight = Number(skinWeights[influence] ?? 0);
        if (nodeIndex >= 0 && weight > 0) influences.push([localBone, weight]);
      }
      influences.sort((left, right) => right[1] - left[1]);
      while (influences.length < 4) influences.push([0, 0]);
      const selected = influences.slice(0, 4);
      const first = selected[0] ?? [0, 0];
      const second = selected[1] ?? [0, 0];
      const third = selected[2] ?? [0, 0];
      const fourth = selected[3] ?? [0, 0];
      const total = selected.reduce((sum, entry) => sum + entry[1], 0) || 1;
      vertices.push(px, py, pz, nx || 0, ny || 0, nz || 1, uvs?.[uvIndex * 2] || 0, uvs?.[uvIndex * 2 + 1] || 0,
        first[0], second[0], third[0], fourth[0],
        first[1] / total, second[1] / total, third[1] / total, fourth[1] / total);
      const constraintStart = constraintOffsets[vertex]; const constraintEnd = constraintOffsets[vertex + 1];
      vertexConstraints.push(
        constraintStart != null && constraintEnd != null && constraintEnd > constraintStart
          ? Number(constraintValues[constraintStart] ?? 0)
          : 0,
      );
      const colorStart = colorOffsets[vertex];
      const colorEnd = colorOffsets[vertex + 1];
      const authoredColor = colorStart != null && colorEnd != null && colorEnd - colorStart >= 3
        ? Array.from(colorValues.slice(colorStart, colorStart + 3), Number)
        : undefined;
      vertices.push(...(authoredColor || (
        model.nodes[mesh.sourceNode]?.kind === 'aabb'
          ? surfaceColor(Number(faceMaterials[Math.floor(corner / 3)] ?? 0))
          : [1, 1, 1]
      )));
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
    const gpu: PrimitiveGpu = {
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

  function preparePrimitive(
    modelIndex: number,
    meshIndex: number,
    primitiveIndex: number,
    nodeWorld: readonly Float32Array[],
    asset: InstalledAnimationAsset | undefined,
    pose: ModelPose,
  ): PrimitiveGpu | undefined {
    const model = scene.manifest.models[modelIndex]; const runtime = modelRuntime[modelIndex];
    const mesh = model?.meshes[meshIndex];
    const primitive = mesh?.primitives[primitiveIndex];
    const gpu = primitiveGpu(modelIndex, meshIndex, primitiveIndex);
    if (!model || !runtime || !mesh || !primitive || !gpu) return undefined;
    const materialIndex = primitive.material;
    const material = materialIndex === null ? undefined : model.materials[materialIndex];
    if (material?.renderEnabled === false) return undefined;
    const materialRuntime = materialIndex === null ? undefined : runtime.materials[materialIndex];
    const materialPose = materialIndex === null ? undefined : pose.materials[materialIndex];
    const animated = materialPose?.active ? materialPose : undefined;
    const textureFor = (role: string): MaterialTextureRuntime | undefined => {
      const texture = materialRuntime?.textures.get(role);
      if (!texture) return undefined;
      requestTexture(texture.texture);
      texture.handle = gpuTextures[texture.texture];
      return texture;
    };
    const diffuseTexture = textureFor('diffuse');
    bindMaterialTexture(gl, meshUniforms.uTexture, meshUniforms.uHasTexture, diffuseTexture, 0);
    gl.uniform4fv(meshUniforms.uDiffuseUvTransform, textureUvTransform(diffuseTexture?.binding, (performance.now() - viewerStarted) / 1000, diffuseTexture?.uvTransform));
    bindMaterialTexture(gl, meshUniforms.uNormalTexture, meshUniforms.uHasNormalTexture, textureFor('normal'), 1);
    bindMaterialTexture(gl, meshUniforms.uEmissiveTexture, meshUniforms.uHasEmissiveTexture, textureFor('emissive'), 4);
    applyBlendMode(gl, diffuseTexture?.binding);
    const nodeColor = pose.nodes[mesh.sourceNode]?.color || WHITE_COLOR;
    const diffuse = material?.diffuse || DEFAULT_DIFFUSE;
    gl.uniform4f(meshUniforms.uColor, (diffuse[0] ?? 1)*(nodeColor[0] ?? 1), (diffuse[1] ?? 1)*(nodeColor[1] ?? 1), (diffuse[2] ?? 1)*(nodeColor[2] ?? 1), (animated?.alpha ?? material?.alpha ?? 1)*(pose.nodes[mesh.sourceNode]?.alpha ?? 1));
    gl.uniform3fv(meshUniforms.uEmissiveColor, animated?.selfIllumColor || material?.selfIllumColor || ZERO_COLOR);
    gl.uniform3fv(meshUniforms.uMaterialAmbient, material?.ambient || WHITE_COLOR);
    const skinned = gpu.boneNodes.length > 0; gl.uniform1i(meshUniforms.uSkinned, skinned ? 1 : 0);
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
    updateDynamicMesh(gl, gpu, animatedVertices, model.nodes[mesh.sourceNode]?.dangly ?? null, (performance.now()-viewerStarted)/1000, nwnEnvironment?.windPower || 0);
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
    const illumination = globalIllumination(nwnEnvironment);
    const background = illumination.background;
    gl.clearColor(background[0] ?? 0, background[1] ?? 0, background[2] ?? 0, 1); gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);
    const projection = perspective(Math.PI / 4, width / height, Math.max(0.01, camera.distance / 1000), Math.max(1000, camera.distance * 20));
    const eye = orbitEye(camera); const view = lookAt(eye, camera.target, [0, 0, 1]); const viewProjection = multiply4(projection, view);
    gl.useProgram(program); gl.uniform3fv(meshUniforms.uEnvironmentLight, illumination.environmentLight);
    gl.uniform1i(meshUniforms.uInstanced, 0);
    for (const runtime of modelRuntime) runtime.chunkBatch.count = 0;
    gl.uniform3fv(meshUniforms.uCamera, eye);
    gl.uniform1i(meshUniforms.uFogEnabled, illumination.fogEnabled ? 1 : 0);
    gl.uniform3fv(meshUniforms.uFogColor, illumination.fogColor);
    gl.uniform1f(meshUniforms.uFogEnd, illumination.fogEnd);
    if (sceneHasPointLights) {
      if (pointLightsDirty || animationPlaying || !pointLightsCache) {
        const collected = collectSceneLights(scene, poseForModel, modelRuntime, instanceRuntime, lightRuntime);
        pointLightsCache = collected;
        if (collected.count > maxTextureSize) throw new Error(`Scene has ${collected.count} lights, exceeding this GPU's ${maxTextureSize}-light texture capacity.`);
        gl.activeTexture(gl.TEXTURE6); gl.bindTexture(gl.TEXTURE_2D, pointLightTexture);
        gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA32F, 3, Math.max(1, collected.count), 0, gl.RGBA, gl.FLOAT, collected.values);
        pointLightsDirty = false;
      }
      const lights = pointLightsCache;
      if (!lights) throw new Error('Point-light collection was not initialized.');
      gl.uniform1i(meshUniforms.uPointLights, 6); gl.uniform1i(meshUniforms.uPointLightCount, lights.count);
    }
    for (const skyboxPass of [true, false]) for (const entry of instanceRuntime) {
      const { instance } = entry;
      const collision = instance.kind === 'collision'; const skybox = instance.kind === 'skybox';
      if (skybox !== skyboxPass || (mode === 'collision' ? !collision : collision)) continue;
      if (instance.model == null) continue;
      if (skybox) { gl.disable(gl.CULL_FACE); gl.depthMask(false); } else { gl.enable(gl.CULL_FACE); gl.depthMask(true); }
      const base = skybox ? composeTransform4(camera.target, instance.rotationAxisAngle, instance.scale) : entry.base;
      gl.uniform1i(meshUniforms.uDynamicObject, entry.dynamic ? 1 : 0);
      drawModel(instance.model, base, viewProjection, new Set(), illumination.fogEnabled);
    }
    drawChunkBatches(viewProjection, illumination.fogEnabled);
    gl.depthMask(true); gl.enable(gl.CULL_FACE);
    if (mode !== 'collision') drawEffects(viewProjection, view, eye, (performance.now() - viewerStarted) / 1000);
    drawOverlays(viewProjection);
    drawSelection(viewProjection);
    const selected = selectedObjectKey ? authoredObjects.get(selectedObjectKey) : undefined;
    if (elements.status) {
      elements.status.textContent = selected
        ? `${selected.label} · ${selected.kind} #${selected.sourceIndex + 1}`
        : `${scene.manifest.models.length} models · ${scene.manifest.textures.length} textures · ${scene.manifest.instances.length} instances`;
    }
    if (animationPlaying || hasDynamicEffects) {
      const duration = performance.now() - drawStarted;
      slowFrames = duration > 20 ? slowFrames + 1 : 0; fastFrames = duration < 10 ? fastFrames + 1 : 0;
      if (slowFrames >= 8 && renderScale > 0.5) { renderScale = Math.max(0.5, renderScale - 0.1); slowFrames = 0; fastFrames = 0; }
      else if (fastFrames >= 120 && renderScale < 1) { renderScale = Math.min(1, renderScale + 0.1); slowFrames = 0; fastFrames = 0; }
    }
  }

  function drawModel(
    modelIndex: number,
    base: Float32Array,
    viewProjection: Float32Array,
    stack: Set<number>,
    fogEnabled: boolean,
  ): void {
    if (stack.has(modelIndex)) return;
    stack.add(modelIndex);
    const model = scene.manifest.models[modelIndex]; const runtime = modelRuntime[modelIndex]; if (!model || !runtime) return;
    gl.uniform1i(meshUniforms.uFogEnabled, fogEnabled && model.ignoreFog !== 1 ? 1 : 0);
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
      const target = runtime.attachmentTargets.get(attachment) ?? -1;
      multiply4Into(base, nodeWorld[target] || IDENTITY_MATRIX, runtime.attachmentWorld);
      drawModel(attachment.model, runtime.attachmentWorld, viewProjection, new Set(stack), fogEnabled);
    }
  }
  function drawChunkEmitters(
    modelIndex: number,
    model: PacketModel,
    nodeWorld: readonly Float32Array[],
    base: Float32Array,
    viewProjection: Float32Array,
    stack: Set<number>,
    fogEnabled: boolean,
    asset: InstalledAnimationAsset | undefined,
  ): void {
    model.nodes.forEach((node, nodeIndex) => {
      if (!node.emitter || String(emitterProperty(node.emitter, 'update', '')).toLowerCase() !== 'explosion') return;
      const runtime = modelRuntime[modelIndex];
      if (!runtime) return;
      const track = asset?.runtime.tracksByNode[nodeIndex];
      if (animatedEmitterValue(modelIndex, nodeIndex, track, 'detonate', emitterProperty(node.emitter, 'detonate', 0)) <= 0) return;
      const chunkName = String(emitterProperty(node.emitter, 'chunkname', '')).trim(); const chunkModel = modelIndexByName.get(chunkName.toLowerCase()); if (!chunkName || chunkModel == null) return;
      const value = (name: string, fallback: number): number =>
        animatedEmitterValue(modelIndex, nodeIndex, track, name, emitterProperty(node.emitter, name, fallback));
      const life = Math.max(0.001, value('lifeexp', 1)); const count = Math.ceil(Math.max(0, value('birthrate', 1)) * life); if (count > 20000) throw new Error(`Emitter ${node.name} requests ${count} concurrent chunks; the viewer safety limit is 20000.`);
      const nodeBase = multiply4Into(base, nodeWorld[nodeIndex] || IDENTITY_MATRIX, runtime.emitterWorld); const velocity = value('velocity', 0); const randomVelocity = value('randvel', 0); const spread = value('spread', 0); const gravity = value('grav', 0); const drag = Math.max(0, value('drag', 0));
      for (let index = 0; index < count; index += 1) {
        const phase = random01(index, 0); const ageSeconds = (((performance.now() - viewerStarted) / 1000 + phase * life) % life + life) % life; const azimuth = random01(index, 1) * Math.PI * 2; const cone = spread * Math.sqrt(random01(index, 2)); const speed = velocity + (random01(index, 3) * 2 - 1) * randomVelocity; const damping = drag > 0 ? (1 - Math.exp(-drag * ageSeconds)) / drag : ageSeconds;
        const localX=(random01(index,4)-0.5)*value('xsize',node.emitter.xSize)+Math.sin(cone)*Math.cos(azimuth)*speed*damping;
        const localY=(random01(index,5)-0.5)*value('ysize',node.emitter.ySize)+Math.sin(cone)*Math.sin(azimuth)*speed*damping;
        const localZ=Math.cos(cone)*speed*damping-gravity*ageSeconds*ageSeconds*0.5;
        const sizeStart=value('sizestart',1); const size=stagedValue3(ageSeconds/life,Math.max(0.001,Math.min(0.999,value('percentmid',50)/100)),sizeStart,value('sizemid',sizeStart),value('sizeend',1));
        const chunkRuntime = modelRuntime[chunkModel];
        if (!chunkRuntime) continue;
        chunkRuntime.chunkTranslation[0]=localX; chunkRuntime.chunkTranslation[1]=localY; chunkRuntime.chunkTranslation[2]=localZ;
        chunkRuntime.chunkRotation[0]=random01(index,6); chunkRuntime.chunkRotation[1]=random01(index,7); chunkRuntime.chunkRotation[2]=random01(index,8); chunkRuntime.chunkRotation[3]=value('particlerot',0)*ageSeconds*Math.PI/180; chunkRuntime.chunkScale.fill(size);
        composeTransform4Into(chunkRuntime.chunkTranslation, chunkRuntime.chunkRotation, chunkRuntime.chunkScale, chunkRuntime.chunkLocalMatrix);
        multiply4Into(nodeBase, chunkRuntime.chunkLocalMatrix, chunkRuntime.chunkWorldMatrix);
        appendChunkInstance(chunkRuntime.chunkBatch, chunkRuntime.chunkWorldMatrix);
      }
    });
  }

  function drawChunkBatches(viewProjection: Float32Array, fogEnabled: boolean): void {
    gl.uniform1i(meshUniforms.uInstanced, 1); gl.uniformMatrix4fv(meshUniforms.uViewProjection, false, viewProjection);
    for (let modelIndex = 0; modelIndex < modelRuntime.length; modelIndex += 1) {
      const runtime = modelRuntime[modelIndex];
      if (!runtime) continue;
      const batch = runtime.chunkBatch; if (!batch.count) continue;
      gl.bindBuffer(gl.ARRAY_BUFFER, batch.buffer); const byteLength = batch.count * 16 * 4;
      if (byteLength > batch.gpuCapacity) { batch.gpuCapacity = Math.max(byteLength, Math.ceil(batch.gpuCapacity*1.5), 16*16*4); gl.bufferData(gl.ARRAY_BUFFER, batch.gpuCapacity, gl.DYNAMIC_DRAW); }
      gl.bufferSubData(gl.ARRAY_BUFFER, 0, batch.values, 0, batch.count * 16);
      drawInstancedModel(modelIndex, batch, viewProjection, fogEnabled, IDENTITY_MATRIX, new Set());
    }
    gl.uniform1i(meshUniforms.uInstanced, 0);
  }

  function drawInstancedModel(
    modelIndex: number,
    batch: ChunkBatch,
    viewProjection: Float32Array,
    fogEnabled: boolean,
    parentTransform: Float32Array,
    stack: Set<number>,
  ): void {
    if (stack.has(modelIndex)) return; stack.add(modelIndex);
    const model = scene.manifest.models[modelIndex]; const runtime = modelRuntime[modelIndex]; if (!model || !runtime) return;
    gl.uniform1i(meshUniforms.uFogEnabled, fogEnabled && model.ignoreFog !== 1 ? 1 : 0);
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
      const target = runtime.attachmentTargets.get(attachment) ?? -1;
      multiply4Into(parentTransform, worlds[target] || IDENTITY_MATRIX, runtime.instancedAttachment);
      drawInstancedModel(attachment.model, batch, viewProjection, fogEnabled, runtime.instancedAttachment, new Set(stack));
    }
  }
  function poseForModel(
    modelIndex: number,
  ): { asset: InstalledAnimationAsset | undefined; pose: ModelPose } {
    const model = scene.manifest.models[modelIndex]; const runtime = modelRuntime[modelIndex];
    if (!model || !runtime) {
      throw new Error(`Scene model ${modelIndex} has no renderer runtime.`);
    }
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

  function transitionFactor(): number {
    return transition ? Math.max(0, Math.min(1, animationElapsed / Math.max(Number.EPSILON, transition.duration))) : 1;
  }

  function animatedEmitterValue(
    modelIndex: number,
    nodeIndex: number,
    targetTrack: PreparedNodeAnimationTrack | undefined,
    name: string,
    fallback: number,
  ): number {
    const target = samplePreparedEmitterValue(targetTrack?.emitterControllers.get(name.toLowerCase()), animationTime, fallback);
    if (!transition) return target;
    const sourceAsset = transition.sourceAssets.get(modelIndex);
    const sourceTrack = sourceAsset?.runtime.tracksByNode[nodeIndex];
    const source = samplePreparedEmitterValue(sourceTrack?.emitterControllers.get(name.toLowerCase()), transition.sourceTime, fallback);
    return lerpNumber(source, target, transitionFactor());
  }

  function animatedEmitterVectorInto(
    modelIndex: number,
    nodeIndex: number,
    targetTrack: PreparedNodeAnimationTrack | undefined,
    name: string,
    fallback: NumericView | readonly number[],
    result: Float32Array,
    interval: Float64Array,
  ): Float32Array {
    samplePreparedEmitterVectorInto(targetTrack?.emitterControllers.get(name.toLowerCase()), animationTime, fallback, result, interval);
    if (!transition) return result;
    const runtime = modelRuntime[modelIndex];
    const sourceResult = runtime?.emitterTransitionVectors[nodeIndex];
    const sourceInterval = runtime?.emitterTransitionIntervals[nodeIndex];
    if (!runtime || !sourceResult || !sourceInterval) return result;
    const sourceAsset = transition.sourceAssets.get(modelIndex); const sourceTrack = sourceAsset?.runtime.tracksByNode[nodeIndex];
    samplePreparedEmitterVectorInto(
      sourceTrack?.emitterControllers.get(name.toLowerCase()),
      transition.sourceTime,
      fallback,
      sourceResult,
      sourceInterval,
    );
    return lerpArrayInto(sourceResult, result, transitionFactor(), result);
  }

  function drawOverlays(viewProjection: Float32Array): void {
    gl.useProgram(lineProgram); gl.disable(gl.CULL_FACE); gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);
    const colors: Readonly<Partial<Record<string, [number, number, number, number]>>> = {
      trigger: [1, 0.55, 0.1, 1],
      encounter: [0.7, 0.25, 1, 1],
      waypoint: [0.15, 0.85, 1, 1],
      sound: [0.2, 0.9, 0.45, 0.75],
      store: [1, 0.85, 0.15, 1],
    };
    for (const { instance, base, overlay } of instanceRuntime) {
      if (!instance.polygon?.length || (mode === 'collision' && instance.kind !== 'trigger' && instance.kind !== 'encounter')) continue;
      if (!overlay) continue;
      gl.bindVertexArray(overlay.vao);
      gl.uniformMatrix4fv(lineUniforms.uModelViewProjection, false, multiply4(viewProjection, base));
      const color: [number, number, number, number] = instance.objectKey === selectedObjectKey
        ? [1, 0.78, 0.12, 1]
        : colors[instance.kind] || [0.85, 0.85, 0.85, 1];
      gl.uniform4f(lineUniforms.uColor, ...color); gl.drawArrays(gl.LINE_LOOP, 0, overlay.count);
    }
    gl.enable(gl.CULL_FACE);
  }

  function drawSelection(viewProjection: Float32Array): void {
    if (!selectedObjectKey) return;
    if (!selectionGpu) return;
    gl.useProgram(lineProgram); gl.disable(gl.CULL_FACE); gl.disable(gl.DEPTH_TEST);
    gl.bindVertexArray(selectionGpu.vao);
    gl.uniformMatrix4fv(lineUniforms.uModelViewProjection, false, viewProjection);
    const color: [number, number, number, number] = Number.isInteger(hoveredComponentId)
      ? [0.2, 0.82, 1, 1]
      : [1, 0.78, 0.12, 1];
    gl.uniform4f(lineUniforms.uColor, ...color);
    gl.drawArrays(gl.LINES, 0, selectionGpu.count);
    gl.enable(gl.DEPTH_TEST); gl.enable(gl.CULL_FACE);
  }

  function refreshSelectionGpu(): void {
    const componentId = Number.isInteger(hoveredComponentId) ? hoveredComponentId : selectedComponentId;
    const component = componentId === undefined ? undefined : componentInstances.get(componentId);
    if (componentId !== undefined
        && component?.objectKey === selectedObjectKey
        && boundsCatalog.componentSelections.has(componentId)) {
      selectionGpu = replaceSelectionGpu(gl, selectionGpu, componentId, boundsCatalog.componentSelections);
    } else if (selectedObjectKey) {
      selectionGpu = replaceSelectionGpu(gl, selectionGpu, selectedObjectKey, boundsCatalog.objectSelections);
    } else {
      destroyOverlayGpu(gl, selectionGpu);
      selectionGpu = undefined;
    }
  }

  function bindSelectedComponentInteractions() {
    document.querySelectorAll<WebviewElement>('.selected-component').forEach((row) => {
      const componentId = Number(row.dataset.componentId);
      row.onmouseenter = () => {
        if (!componentInstances.has(componentId)) return;
        hoveredComponentId = componentId; refreshSelectionGpu(); draw();
      };
      row.onmouseleave = () => {
        if (hoveredComponentId !== componentId) return;
        hoveredComponentId = undefined; refreshSelectionGpu(); draw();
      };
    });
    document.querySelectorAll<WebviewElement>('.component-select').forEach((button) => {
      const componentId = Number(button.dataset.componentId);
      button.onclick = () => selectComponent(componentId, false);
      button.ondblclick = () => selectComponent(componentId, true);
      button.onfocus = () => button.parentElement?.onmouseenter?.(new MouseEvent('mouseenter'));
      button.onblur = () => button.parentElement?.onmouseleave?.(new MouseEvent('mouseleave'));
      button.onkeydown = (event) => {
        if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); selectComponent(componentId, false); }
        else if (event.key.toLowerCase() === 'f') { event.preventDefault(); selectComponent(componentId, true); }
      };
    });
    document.querySelectorAll<WebviewElement>('.component-open').forEach((button) => {
      button.onclick = (event) => {
        event.stopPropagation();
        vscode.postMessage({ type: 'openDependency', resource: button.dataset.resource });
      };
      button.ondblclick = (event) => event.stopPropagation();
    });
  }

  function bindAnimationControl() {
    const control = webviewElement('viewer-animation');
    if (animationInSelectedData) {
      elements.animationTime = webviewElement('viewer-animation-time');
      elements.animationEvent = webviewElement('viewer-animation-event');
    }
    if (!control) return;
    control.value = selectedAnimationIndex >= 0 ? String(selectedAnimationIndex) : '';
    control.onchange = () => {
      const index = control.value === '' ? -1 : Number(control.value);
      const entry = animations[index];
      setAnimation(entry?.modelIndex, entry?.animationIndex);
    };
  }

  let inspectorSearchTimer: ReturnType<typeof setTimeout> | undefined;

  function inspectorViewKey(): string {
    const routeKey = inspectorScope === 'selection' ? JSON.stringify(inspectorRoute) : 'root';
    return `${inspectorScope}:${selectedObjectKey || 'none'}:${routeKey}`;
  }

  function inspectorSectionStateKey(): string {
    return `${inspectorScope}:${selectedObjectKey || 'none'}:${inspectorRoute.page}`;
  }

  function rememberInspectorScroll(): void {
    const key = inspectorViewKey(); const value = elements.inspectorContent.scrollTop;
    inspectorScrollPositions.delete(key); inspectorScrollPositions.set(key, value);
  }

  function setInspectorRoute(route: InspectorRoute | undefined): void {
    rememberInspectorScroll();
    inspectorRoute = route || { page: 'object' };
    if (selectedObjectKey) { inspectorRoutes.delete(selectedObjectKey); inspectorRoutes.set(selectedObjectKey, inspectorRoute); }
    refreshInspector(false);
    persistState(undefined);
  }

  function currentInspectionState(): InspectionState | undefined {
    if (!selectedObjectKey) return undefined;
    const inspection = session.inspectionAssets.get(selectedObjectKey);
    const inspectionError = session.inspectionErrors.get(selectedObjectKey);
    return inspection
      ? { status: 'ready', data: inspection }
      : inspectionError ? { status: 'error', message: inspectionError }
        : selectedObjectKey ? { status: 'loading' } : undefined;
  }

  function refreshInspector(preserveScroll = true): void {
    const previousScroll = preserveScroll ? elements.inspectorContent.scrollTop : inspectorScrollPositions.get(inspectorViewKey()) || 0;
    const object = selectedObjectKey ? authoredObjects.get(selectedObjectKey) : undefined;
    const inspectionState = currentInspectionState();
    const inspection = inspectionState?.status === 'ready' ? inspectionState.data : undefined;
    const selectionOption = elements.inspectorScope.querySelector<HTMLOptionElement>('option[value="selection"]');
    if (selectionOption) selectionOption.disabled = !object;
    if (!object && inspectorScope === 'selection') inspectorScope = 'scene';
    elements.inspectorScope.value = inspectorScope;
    elements.inspectorSearch.value = inspectorQuery;
    elements.inspectorTechnical.setAttribute('aria-pressed', String(inspectorTechnicalNames));
    elements.workbench.dataset.inspectorCollapsed = String(inspectorCollapsed);
    elements.workbench.style.setProperty('--viewer-inspector-width', `${inspectorWidth}px`);

    if (inspectorScope === 'selection' && object) {
      const pageTitle = inspection ? inspectorRouteTitle(inspection, inspectorRoute) : 'Authored Data';
      const nested = inspectorRoute.page !== 'object';
      const blueprint = blueprintResourceForObject(object);
      elements.inspectorContext.innerHTML = `${nested ? `<div class="viewer-inspector-breadcrumb"><button class="secondary inspector-back" title="Back" aria-label="Back">‹</button><span>${escapeHtml(object.label)} › ${escapeHtml(pageTitle)}</span></div>` : ''}<div class="viewer-inspector-object"><div><strong>${escapeHtml(nested ? pageTitle : object.label)}</strong><small>${escapeHtml(object.kind)} · ${escapeHtml(object.key)}</small>${!nested && blueprint ? `<button class="inspection-value-link inspection-resource-open" data-resource="${escapeAttribute(blueprint)}">${escapeHtml(blueprint)}</button>` : ''}</div><div class="viewer-inspector-object-actions">${!nested ? `<button class="secondary inspector-frame-object" title="Frame selected object">Frame</button>${animationInSelectedData ? animationControl(animations, selectedAnimationIndex) : ''}` : ''}</div></div>`;
      const componentCount = [...componentInstances.values()].filter((entry) => entry.objectKey === selectedObjectKey).length;
      const sectionKey = inspectorSectionStateKey();
      const openSections = inspectorOpenSections.get(sectionKey) || new Set();
      elements.inspectorContent.innerHTML = inspectionContent(inspectionState, {
        route: inspectorRoute,
        query: inspectorQuery,
        technicalNames: inspectorTechnicalNames,
        openSections,
        useDefaultSections: !inspectorTouchedSections.has(sectionKey),
        scene,
        objectKey: selectedObjectKey,
        selectedComponentId,
        componentCount,
      });
    } else if (inspectorScope === 'dependencies') {
      elements.inspectorContext.innerHTML = `<div class="viewer-inspector-object"><div><strong>Dependencies</strong><small>${scene.manifest.dependencies.nodes.length} resources · resolution order preserved</small></div></div>`;
      elements.inspectorContent.innerHTML = dependenciesInspectorContent(scene, inspectorQuery);
    } else {
      const sectionKey = inspectorSectionStateKey();
      elements.inspectorContext.innerHTML = `<div class="viewer-inspector-object"><div><strong>${escapeHtml(scene.manifest.name || 'Scene')}</strong><small>${escapeHtml(scene.manifest.source)} · ${scene.manifest.models.length} models · ${scene.manifest.textures.length} textures</small></div></div>`;
      elements.inspectorContent.innerHTML = sceneInspectorContent(scene, inspectorQuery, inspectorOpenSections.get(sectionKey) || new Set(), !inspectorTouchedSections.has(sectionKey));
    }

    bindInspectorInteractions();
    bindSelectedComponentInteractions();
    if (animationInSelectedData) bindAnimationControl();
    configureInspectorJump();
    requestAnimationFrame(() => { if (!disposed && elements.inspectorContent) elements.inspectorContent.scrollTop = previousScroll; });
    if (object) requestInspection(selectedObjectKey);
  }

  function configureInspectorJump() {
    const sections = [...elements.inspectorContent.querySelectorAll<WebviewElement>('.inspector-section')];
    elements.inspectorJump.hidden = sections.length < 2;
    elements.inspectorJump.innerHTML = `<option value="">Jump to section…</option>${sections.map((section) => `<option value="${escapeAttribute(section.dataset.sectionId)}">${escapeHtml(section.querySelector('summary span')?.textContent || section.dataset.sectionId)}</option>`).join('')}`;
    elements.inspectorJump.value = '';
  }

  function bindInspectorInteractions() {
    document.querySelectorAll<WebviewElement>('.inspection-resource-open:not(:disabled), .dependency:not(:disabled)').forEach((button) => {
      button.onclick = (event) => { event.stopPropagation(); vscode.postMessage({ type: 'openDependency', resource: button.dataset.resource }); };
    });
    document.querySelectorAll<WebviewElement>('[data-inspector-route]').forEach((button) => {
      button.onclick = () => {
        try {
          const route: unknown = JSON.parse(button.dataset.inspectorRoute || '');
          if (isInspectorRoute(route)) setInspectorRoute(route);
        } catch { /* invalid routes are inert */ }
      };
    });
    document.querySelector('.inspector-back')?.addEventListener('click', () => setInspectorRoute(parentInspectorRoute(inspectorRoute)));
    document.querySelector('.inspector-frame-object')?.addEventListener('click', () => {
      const selectedBounds = selectedObjectKey
        ? boundsCatalog.objects.get(selectedObjectKey)
        : undefined;
      if (selectedBounds) { frameBounds(camera, selectedBounds); draw(); persistState(undefined); }
    });
    const sectionKey = inspectorSectionStateKey();
    document.querySelectorAll<WebviewElement>('.inspector-section').forEach((details) => {
      details.ontoggle = () => {
        const open = inspectorOpenSections.get(sectionKey) || new Set();
        const sectionId = details.dataset.sectionId;
        if (!sectionId) return;
        if (details.open) open.add(sectionId); else open.delete(sectionId);
        inspectorOpenSections.set(sectionKey, open); inspectorTouchedSections.add(sectionKey); persistState(undefined);
      };
    });
  }

  function bindInspectorChrome(): void {
    elements.inspectorScope.onchange = () => {
      rememberInspectorScroll();
      const scope = elements.inspectorScope.value;
      inspectorScope = scope === 'selection' || scope === 'dependencies' ? scope : 'scene';
      if (inspectorScope === 'selection' && !selectedObjectKey) inspectorScope = 'scene';
      refreshInspector(false); persistState(undefined);
    };
    elements.inspectorSearch.oninput = () => {
      inspectorQuery = elements.inspectorSearch.value;
      clearTimeout(inspectorSearchTimer);
      inspectorSearchTimer = setTimeout(() => { refreshInspector(false); persistState(undefined); }, 100);
    };
    elements.inspectorJump.onchange = () => {
      const section = [...elements.inspectorContent.querySelectorAll<WebviewElement>('[data-section-id]')]
        .find((entry) => entry.dataset.sectionId === elements.inspectorJump.value);
      if (section) { section.open = true; section.scrollIntoView({ block: 'start', behavior: 'smooth' }); }
      elements.inspectorJump.value = '';
    };
    elements.inspectorTechnical.onclick = () => {
      inspectorTechnicalNames = !inspectorTechnicalNames;
      refreshInspector(true); persistState(undefined);
    };
    elements.inspectorCollapse.onclick = () => {
      inspectorCollapsed = true; elements.workbench.dataset.inspectorCollapsed = 'true'; persistState(undefined); draw();
    };
    elements.inspectorReopen.onclick = () => {
      inspectorCollapsed = false; elements.workbench.dataset.inspectorCollapsed = 'false'; persistState(undefined); draw();
    };
    let resizeStart: { x: number; width: number } | undefined;
    elements.inspectorSash.onpointerdown = (event) => {
      resizeStart = { x: event.clientX, width: inspectorWidth };
      elements.inspectorSash.setPointerCapture?.(event.pointerId);
      elements.workbench.classList.add('resizing-inspector');
      event.preventDefault();
    };
    elements.inspectorSash.onpointermove = (event) => {
      if (!resizeStart) return;
      const maximum = Math.max(340, Math.min(720, elements.workbench.clientWidth - 320));
      inspectorWidth = Math.max(340, Math.min(maximum, resizeStart.width + resizeStart.x - event.clientX));
      elements.workbench.style.setProperty('--viewer-inspector-width', `${inspectorWidth}px`); elements.inspectorSash.setAttribute('aria-valuenow', String(Math.round(inspectorWidth))); draw();
    };
    const finishResize = () => {
      if (!resizeStart) return;
      resizeStart = undefined; elements.workbench.classList.remove('resizing-inspector'); persistState(undefined); draw();
    };
    elements.inspectorSash.onpointerup = finishResize;
    elements.inspectorSash.onpointercancel = finishResize;
    elements.inspectorSash.onkeydown = (event) => {
      if (!['ArrowLeft', 'ArrowRight'].includes(event.key)) return;
      inspectorWidth = Math.max(340, Math.min(720, inspectorWidth + (event.key === 'ArrowLeft' ? 16 : -16)));
      elements.workbench.style.setProperty('--viewer-inspector-width', `${inspectorWidth}px`); elements.inspectorSash.setAttribute('aria-valuenow', String(Math.round(inspectorWidth))); persistState(undefined); draw(); event.preventDefault();
    };
  }

  const inspectorShortcut = (event: KeyboardEvent): void => {
    const tagName = event.target instanceof Element ? event.target.tagName.toLowerCase() : '';
    if (event.key === '/' && !event.metaKey && !event.ctrlKey && !event.altKey && !['input', 'textarea', 'select'].includes(tagName)) {
      inspectorCollapsed = false; elements.workbench.dataset.inspectorCollapsed = 'false'; elements.inspectorSearch.focus(); event.preventDefault();
    }
  };
  if (elements.inspectorSearch && elements.workbench) window.addEventListener('keydown', inspectorShortcut);

  function updateSelectedComponentClasses(): void {
    document.querySelectorAll<WebviewElement>('.selected-component').forEach((row) => {
      row.classList.toggle('selected', Number(row.dataset.componentId) === selectedComponentId);
    });
  }

  function selectComponent(componentId: number, frame: boolean): void {
    const component = componentInstances.get(componentId);
    if (!component || component.objectKey !== selectedObjectKey) return;
    selectedComponentId = componentId; hoveredComponentId = undefined;
    refreshSelectionGpu(); updateSelectedComponentClasses();
    if (frame) {
      const selection = boundsCatalog.componentSelections.get(componentId);
      if (selection) frameBounds(camera, selection.bounds);
    }
    persistState(undefined); draw();
  }

  function selectObject(objectKey: unknown, frame = true, notify = false): void {
    rememberInspectorScroll();
    const nextKey = typeof objectKey === 'string' && authoredObjects.has(objectKey)
      ? objectKey
      : undefined;
    selectedObjectKey = nextKey;
    selectedComponentId = undefined; hoveredComponentId = undefined;
    if (selectedObjectKey) {
      inspectorScope = 'selection';
      inspectorRoute = inspectorRoutes.get(selectedObjectKey) || { page: 'object' };
    } else if (inspectorScope === 'selection') inspectorScope = 'scene';
    refreshSelectionGpu(); refreshInspector(false);
    const selectedBounds = selectedObjectKey
      ? boundsCatalog.objects.get(selectedObjectKey)
      : undefined;
    if (frame && selectedBounds) frameBounds(camera, selectedBounds);
    persistState(undefined);
    draw();
    if (notify) vscode.postMessage({
      type: 'selectAreaObject',
      objectKey: selectedObjectKey || null,
    });
  }

  function pickAreaObject(event: MouseEvent | PointerEvent): string | undefined {
    if (!boundsCatalog.objects.size) return undefined;
    const rect = canvas.getBoundingClientRect();
    const x = ((event.clientX - rect.left) / Math.max(1, rect.width)) * 2 - 1;
    const y = 1 - ((event.clientY - rect.top) / Math.max(1, rect.height)) * 2;
    const eye = orbitEye(camera);
    const projection = perspective(
      Math.PI / 4,
      Math.max(1, canvas.width) / Math.max(1, canvas.height),
      Math.max(0.01, camera.distance / 1000),
      Math.max(1000, camera.distance * 20),
    );
    const viewProjection = multiply4(projection, lookAt(eye, camera.target, [0, 0, 1]));
    const inverse = inverse4(viewProjection);
    const near = transformHomogeneous4(inverse, [x, y, -1, 1]);
    const far = transformHomogeneous4(inverse, [x, y, 1, 1]);
    const direction = normalize3([far[0] - near[0], far[1] - near[1], far[2] - near[2]]);
    let selected: string | undefined; let distance = Infinity;
    for (const [objectKey, objectBounds] of boundsCatalog.objects) {
      const hit = rayBoundsDistance(near, direction, objectBounds);
      if (hit != null && hit < distance) { selected = objectKey; distance = hit; }
    }
    return selected;
  }

  function drawEffects(
    viewProjection: Float32Array,
    view: Float32Array,
    eye: readonly number[],
    effectTime: number,
  ): void {
    gl.useProgram(spriteProgram); gl.disable(gl.CULL_FACE); gl.enable(gl.BLEND); gl.depthMask(false);
    gl.uniformMatrix4fv(spriteUniforms.uViewProjection, false, viewProjection);
    gl.uniform3f(spriteUniforms.uCameraRight, view[0] ?? 0, view[4] ?? 0, view[8] ?? 0);
    gl.uniform3f(spriteUniforms.uCameraUp, view[1] ?? 0, view[5] ?? 0, view[9] ?? 0);
    for (const { instance, base } of instanceRuntime) {
      if (instance.model == null || instance.kind === 'collision' || instance.kind === 'skybox') continue;
      drawModelEffects(instance.model, base, effectTime, eye, viewProjection, new Set());
    }
    gl.depthMask(true); gl.enable(gl.CULL_FACE); gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA); gl.useProgram(program);
  }

  function drawModelEffects(
    modelIndex: number,
    base: Float32Array,
    effectTime: number,
    eye: readonly number[],
    viewProjection: Float32Array,
    stack: Set<number>,
  ): void {
    if (stack.has(modelIndex)) return; stack.add(modelIndex);
    const model = scene.manifest.models[modelIndex]; const runtime = modelRuntime[modelIndex]; if (!model || !runtime) return;
    const { asset, pose } = poseForModel(modelIndex); const worlds = pose.worlds;
    pose.nodes.forEach((node, nodeIndex) => {
      const world = multiply4Into(base, worlds[nodeIndex] || IDENTITY_MATRIX, runtime.effectWorld);
      if (node.emitter) drawEmitter(modelIndex, model, nodeIndex, node, world, asset, effectTime, eye, viewProjection);
      if (node.light?.lensFlares) drawLensFlares(modelIndex, model, nodeIndex, node, world);
    });
    for (const attachment of model.attachments) {
      const target = runtime.attachmentTargets.get(attachment) ?? -1;
      multiply4Into(base, worlds[target] || IDENTITY_MATRIX, runtime.effectAttachment);
      drawModelEffects(attachment.model, runtime.effectAttachment, effectTime, eye, viewProjection, new Set(stack));
    }
  }

  function drawEmitter(
    modelIndex: number,
    _model: PacketModel,
    nodeIndex: number,
    node: PoseNode,
    world: Float32Array,
    asset: InstalledAnimationAsset | undefined,
    effectTime: number,
    eye: readonly number[],
    viewProjection: Float32Array,
  ): void {
    const runtime = modelRuntime[modelIndex];
    const emitter = node.emitter;
    if (!runtime || !emitter) return;
    const track = asset?.runtime.tracksByNode[nodeIndex];
    if (String(emitterProperty(emitter, 'update', '')).toLowerCase() === 'explosion') return;
    const value = (name: string, fallback: number): number =>
      animatedEmitterValue(modelIndex, nodeIndex, track, name, emitterProperty(emitter, name, fallback));
    const life = Math.max(0.001, value('lifeexp', 1)); const birthrate = Math.max(0, value('birthrate', 10));
    const requestedParticles = Math.ceil(life * birthrate); if (requestedParticles > 20000) throw new Error(`Emitter ${node.name} requests ${requestedParticles} concurrent particles; the viewer safety limit is 20000.`); const particleCount = requestedParticles; if (!particleCount) return;
    const velocity = value('velocity', 0); const randomVelocity = value('randvel', 0); const spread = value('spread', 0);
    const mass = value('mass', 0); const drag = Math.max(0, value('drag', 0)); const fps = Math.max(0, value('fps', 0));
    const sizeStart = value('sizestart', 1); const sizeMid = value('sizemid', sizeStart); const sizeEnd = value('sizeend', sizeMid);
    const sizeStartY = value('sizestart_y', 0); const sizeMidY = value('sizemid_y', 0); const sizeEndY = value('sizeend_y', 0);
    const anisotropicSize = Math.abs(sizeStartY) + Math.abs(sizeMidY) + Math.abs(sizeEndY) > 1e-6;
    const colorScratch = runtime.emitterColors[nodeIndex];
    const intervalScratch = runtime.emitterIntervals[nodeIndex];
    if (!colorScratch || !intervalScratch) return;
    emitterVectorInto(emitter, 'colorstart', WHITE_COLOR, colorScratch[0]); animatedEmitterVectorInto(modelIndex, nodeIndex, track, 'colorstart', colorScratch[0], colorScratch[0], intervalScratch);
    emitterVectorInto(emitter, 'colormid', colorScratch[0], colorScratch[1]); animatedEmitterVectorInto(modelIndex, nodeIndex, track, 'colormid', colorScratch[1], colorScratch[1], intervalScratch);
    emitterVectorInto(emitter, 'colorend', colorScratch[1], colorScratch[2]); animatedEmitterVectorInto(modelIndex, nodeIndex, track, 'colorend', colorScratch[2], colorScratch[2], intervalScratch);
    const [colorStart, colorMid, colorEnd] = colorScratch;
    const alphaStart = value('alphastart', 1); const alphaMid = value('alphamid', alphaStart); const alphaEnd = value('alphaend', 0);
    const hasSizeMid = emitterHasValue(emitter, track, 'sizemid');
    const hasSizeMidY = emitterHasValue(emitter, track, 'sizemid_y');
    const hasAlphaMid = emitterHasValue(emitter, track, 'alphamid');
    const hasColorMid = emitterHasValue(emitter, track, 'colormid');
    const percentMid = Math.max(0.001, Math.min(0.999, value('percentmid', 50) / 100));
    const xGrid = Math.max(1, Math.round(emitterProperty(emitter, 'xgrid', 1))); const yGrid = Math.max(1, Math.round(emitterProperty(emitter, 'ygrid', 1)));
    const frameStart = Math.max(0, Math.round(value('framestart', 0))); const frameEnd = Math.max(frameStart, Math.round(value('frameend', xGrid * yGrid - 1)));
    const rotationRate = value('particlerot', 0);
    const opacity = Math.max(0, Math.min(1, value('opacity', 1)));
    const xExtent = value('xsize', emitter.xSize) / 100;
    const yExtent = value('ysize', emitter.ySize) / 100;
    const renderMode = String(emitterProperty(emitter, 'render', 'normal')).toLowerCase();
    const randomFrames = Boolean(value('random', 0));
    let values = runtime.emitterBuffers[nodeIndex];
    if (!values || values.length < particleCount * 15) {
      values = new Float32Array(Math.max(particleCount * 15, Math.ceil((values?.length || 15) * 1.5)));
      runtime.emitterBuffers[nodeIndex] = values;
    }
    const spawnPosition = effectTime * birthrate; const latestSpawn = Math.floor(spawnPosition); const spawnFraction = spawnPosition - latestSpawn;
    let liveParticles = 0;
    for (let ageSlot = 0; ageSlot < particleCount; ageSlot += 1) {
      const ageSeconds = (ageSlot + spawnFraction) / birthrate; if (ageSeconds >= life) continue;
      const age = ageSeconds / life; const seed = latestSpawn - ageSlot;
      const azimuth = random01(seed, 1) * Math.PI * 2;
      const halfAngle = Math.max(0, Math.min(Math.PI, spread * 0.5));
      const cosine = 1 - random01(seed, 2) * (1 - Math.cos(halfAngle)); const sine = Math.sqrt(Math.max(0, 1 - cosine * cosine));
      const speed = velocity + (random01(seed, 3) - 0.5) * randomVelocity;
      let localX = (random01(seed, 4) - 0.5) * xExtent;
      let localY = (random01(seed, 5) - 0.5) * yExtent;
      let localZ = 0;
      const damping = drag > 0 ? (1 - Math.exp(-drag * ageSeconds)) / drag : ageSeconds;
      localX += sine * Math.cos(azimuth) * speed * damping;
      localY += sine * Math.sin(azimuth) * speed * damping;
      localZ += cosine * speed * damping;
      const centerX=(world[0] ?? 0)*localX+(world[4] ?? 0)*localY+(world[8] ?? 0)*localZ+(world[12] ?? 0);
      const centerY=(world[1] ?? 0)*localX+(world[5] ?? 0)*localY+(world[9] ?? 0)*localZ+(world[13] ?? 0);
      const centerZ=(world[2] ?? 0)*localX+(world[6] ?? 0)*localY+(world[10] ?? 0)*localZ+(world[14] ?? 0)-mass*9.81*ageSeconds*ageSeconds*0.5;
      const stage = emitterCurve(age, percentMid, sizeStart, sizeMid, sizeEnd, hasSizeMid);
      const stageY = anisotropicSize ? emitterCurve(age, percentMid, sizeStartY, sizeMidY, sizeEndY, hasSizeMidY) : stage;
      const red=emitterCurve(age,percentMid,colorStart[0] ?? 1,colorMid[0] ?? 1,colorEnd[0] ?? 1,hasColorMid); const green=emitterCurve(age,percentMid,colorStart[1] ?? 1,colorMid[1] ?? 1,colorEnd[1] ?? 1,hasColorMid); const blue=emitterCurve(age,percentMid,colorStart[2] ?? 1,colorMid[2] ?? 1,colorEnd[2] ?? 1,hasColorMid); const alpha = emitterCurve(age, percentMid, alphaStart, alphaMid, alphaEnd, hasAlphaMid) * opacity;
      const frameCount = Math.max(1, frameEnd - frameStart + 1); const randomOffset = randomFrames ? Math.floor(random01(seed, 6) * frameCount) : 0;
      const frame = frameStart + (Math.floor(ageSeconds * fps) + randomOffset) % frameCount; const frameX = frame % xGrid; const frameY = Math.floor(frame / xGrid) % yGrid;
      const offset = liveParticles * 15;
      values[offset]=centerX; values[offset+1]=centerY; values[offset+2]=centerZ; values[offset+3]=Math.max(0.001,stage)*0.5; values[offset+4]=Math.max(0.001,stageY)*0.5; values[offset+5]=rotationRate*ageSeconds; values[offset+6]=alpha;
      values[offset+7]=red; values[offset+8]=green; values[offset+9]=blue; values[offset+10]=frameX/xGrid; values[offset+11]=frameY/yGrid; values[offset+12]=1/xGrid; values[offset+13]=1/yGrid;
      values[offset+14]=renderMode === 'billboard_to_world_z' ? 1 : 0; liveParticles += 1;
    }
    const texture = runtime.nodeTextures.get(`${nodeIndex}:emitter`);
    if (texture) requestTexture(texture.texture); const textureHandle = texture ? gpuTextures[texture.texture] : undefined;
    const blend = String(emitterProperty(emitter, 'blend', 'normal')).toLowerCase(); gl.blendFunc(gl.SRC_ALPHA, blend.includes('lighten') || blend.includes('add') ? gl.ONE : gl.ONE_MINUS_SRC_ALPHA);
    if (renderMode === 'linked') {
      const linked = buildLinkedParticleVertices(values, liveParticles, eye, runtime.emitterLinkedBuffers[nodeIndex]);
      runtime.emitterLinkedBuffers[nodeIndex] = linked;
      gl.useProgram(ribbonProgram); gl.uniformMatrix4fv(ribbonUniforms.uViewProjection, false, viewProjection);
      gl.activeTexture(gl.TEXTURE0); gl.bindTexture(gl.TEXTURE_2D, textureHandle || null); gl.uniform1i(ribbonUniforms.uTexture, 0); gl.uniform1i(ribbonUniforms.uHasTexture, textureHandle ? 1 : 0);
      uploadAndDrawRibbon(gl, ribbonGpu, linked.values, linked.vertexCount);
      gl.useProgram(spriteProgram);
    } else {
      gl.useProgram(spriteProgram); gl.activeTexture(gl.TEXTURE0); gl.bindTexture(gl.TEXTURE_2D, textureHandle || null); gl.uniform1i(spriteUniforms.uTexture, 0); gl.uniform1i(spriteUniforms.uHasTexture, textureHandle ? 1 : 0);
      uploadAndDrawSprites(gl, spriteGpu, values, liveParticles);
    }
  }

  function drawLensFlares(
    modelIndex: number,
    _model: PacketModel,
    nodeIndex: number,
    node: PoseNode,
    world: Float32Array,
  ): void {
    if (!node.light) return;
    const count = Math.min(node.light.flareTextures.length, node.light.flareSizes.length || Infinity); if (!count) return;
    const origin = transformPoint4(world, [0, 0, node.light.verticalDisplacement || 0]);
    for (let index = 0; index < count; index += 1) {
      const runtime = modelRuntime[modelIndex];
      if (!runtime) continue;
      const texture = runtime.nodeTextures.get(`${nodeIndex}:flare:${index}`);
      const shift = node.light.flareColorShifts[index] || node.color || [1, 1, 1];
      const flareSize = node.light.flareSizes[index] ?? 0;
      const size = Math.max(0.001, flareSize * Math.max(0.001, node.light.flareRadius || 1));
      const position = node.light.flarePositions[index] ?? 0;
      const center = origin.map(
        (value, axis) => value + ((camera.target[axis] ?? value) - value) * position,
      );
      const values = runtime.flareBuffer;
      values.set(center, 0); values.set([size, size, 0, Math.max(0, node.alpha ?? 1)], 3); values.set(shift, 7); values.set([0, 0, 1, 1], 10);
      values[14] = 0;
      if (texture) requestTexture(texture.texture); const textureHandle = texture ? gpuTextures[texture.texture] : undefined;
      gl.activeTexture(gl.TEXTURE0); gl.bindTexture(gl.TEXTURE_2D, textureHandle || null); gl.uniform1i(spriteUniforms.uTexture, 0); gl.uniform1i(spriteUniforms.uHasTexture, textureHandle ? 1 : 0); gl.blendFunc(gl.SRC_ALPHA, gl.ONE); uploadAndDrawSprites(gl, spriteGpu, values, 1);
    }
  }

  function frameScene(): void {
    camera.target = [(bounds.min[0] + bounds.max[0]) / 2, (bounds.min[1] + bounds.max[1]) / 2, (bounds.min[2] + bounds.max[2]) / 2];
    camera.distance = Math.max(2, Math.hypot(bounds.max[0] - bounds.min[0], bounds.max[1] - bounds.min[1], bounds.max[2] - bounds.min[2]) * 1.2); draw();
  }
  function setAnimation(modelIndex: number | undefined, animationIndex: number | undefined): void {
    const validSelection = modelIndex !== undefined
      && animationIndex !== undefined
      && Number.isInteger(modelIndex)
      && Number.isInteger(animationIndex);
    const animation = validSelection
      ? scene.manifest.models[modelIndex]?.animations[animationIndex]
      : undefined;
    selectedAnimationIndex = animation
      ? animations.findIndex((entry) => entry.modelIndex === modelIndex && entry.animationIndex === animationIndex)
      : -1;
    const control = webviewElement('viewer-animation');
    if (control) control.value = selectedAnimationIndex >= 0 ? String(selectedAnimationIndex) : '';
    pendingAnimation = animation && modelIndex !== undefined && animationIndex !== undefined ? {
      modelIndex,
      animationIndex,
      animation,
      scope: animationPlaybackScope(scene, modelIndex, animationIndex),
    } : undefined;
    persistState(pendingAnimation
      ? {
        modelIndex: pendingAnimation.modelIndex,
        animationIndex: pendingAnimation.animationIndex,
      }
      : null);
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
  function maybeStartAnimation(): void {
    if (!pendingAnimation) return;
    if ([...pendingAnimation.scope].some(([modelIndex, animationIndex]) => !animationLoaded(modelIndex, animationIndex))) return;
    poseFrame += 1;
    const fromPoses = new Map(modelRuntime.map((_runtime, modelIndex) => [modelIndex, clonePose(poseForModel(modelIndex).pose)]));
    const sourceAssets = new Map<number, InstalledAnimationAsset>();
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
  function tick(now: number): void {
    if (disposed) return;
    const cameraMoved = cameraControls.update(Math.max(0, Math.min(0.1, (now - previousTick) / 1000)));
    previousTick = now;
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
    if (animationPlaying || hasDynamicEffects || cameraMoved) draw(); animationFrame = requestAnimationFrame(tick);
  }
  function emitAnimationEvent(event: PacketAnimationEvent): void {
    if (!elements.animationEvent) return;
    elements.animationEvent.textContent = event.name;
    clearTimeout(displayedEventTimer);
    displayedEventTimer = setTimeout(() => { if (!disposed && elements.animationEvent) elements.animationEvent.textContent = ''; }, 1200);
  }
  bindInspectorChrome();
  refreshInspector(false);
  if (!animationInSelectedData) bindAnimationControl();
  if (selectedAnimationIndex >= 0) {
    const initialAnimation = animations[selectedAnimationIndex];
    if (initialAnimation) setAnimation(initialAnimation.modelIndex, initialAnimation.animationIndex);
  }
  if (selectedObjectKey) {
    refreshSelectionGpu();
    const initialBounds = typeof selectedComponentId === 'number' && Number.isInteger(selectedComponentId)
      ? boundsCatalog.componentSelections.get(selectedComponentId)?.bounds
      : boundsCatalog.objects.get(selectedObjectKey);
    frameBounds(
      camera,
      initialBounds ?? boundsCatalog.objects.get(selectedObjectKey) ?? boundsCatalog.scene,
    );
    draw(); persistState(undefined);
  } else if (restoredCamera) draw(); else { frameScene(); persistState(undefined); }
  let previousTick = performance.now();
  animationFrame = requestAnimationFrame(tick);
  return {
    setAnimation,
    applyAnimation,
    applyTexture,
    applyInspection,
    applyInspectionError,
    selectObject,
    dispose() {
      disposed = true;
      clearTimeout(displayedEventTimer);
      clearTimeout(inspectorSearchTimer);
      window.removeEventListener('keydown', inspectorShortcut);
      if (animationFrame !== undefined) cancelAnimationFrame(animationFrame);
      cameraControls.dispose();
      resizeObserver.disconnect();
      canvas.removeEventListener('webglcontextlost', contextLost);
      canvas.removeEventListener('webglcontextrestored', contextRestored);
      for (const gpu of primitiveCache.values()) {
        gl.deleteBuffer(gpu.buffer); gl.deleteVertexArray(gpu.vao); gl.deleteTexture(gpu.boneTexture);
      }
      for (const runtime of modelRuntime) gl.deleteBuffer(runtime.chunkBatch.buffer);
      for (const entry of instanceRuntime) {
        if (entry.overlay) {
          gl.deleteBuffer(entry.overlay.buffer); gl.deleteVertexArray(entry.overlay.vao);
        }
      }
      destroyOverlayGpu(gl, selectionGpu);
      gl.deleteBuffer(spriteGpu.cornerBuffer); gl.deleteBuffer(spriteGpu.instanceBuffer);
      gl.deleteVertexArray(spriteGpu.vao); gl.deleteBuffer(ribbonGpu.buffer);
      gl.deleteVertexArray(ribbonGpu.vao);
      gpuTextures.forEach((texture) => { if (texture) gl.deleteTexture(texture); });
      gl.deleteTexture(pointLightTexture);
      gl.deleteProgram(program); gl.deleteProgram(lineProgram);
      gl.deleteProgram(spriteProgram); gl.deleteProgram(ribbonProgram);
    },
  };
}

function renderUnsupported(): void {
  requiredWebviewElement('content').innerHTML = '<div class="empty">This resource type has no editor.</div>';
}

function renderTwoDa(): void {
  if (!model || model.kind !== '2da') {
    throw new Error('The 2DA editor received the wrong resource snapshot.');
  }
  const data = model.data;
  const start = tablePage * tablePageSize;
  if (start >= data.rows.length && tablePage > 0) tablePage = Math.max(0, Math.ceil(data.rows.length / tablePageSize) - 1);
  const pageRows = data.rows.slice(tablePage * tablePageSize, (tablePage + 1) * tablePageSize);
  requiredWebviewElement('toolbar').innerHTML = `<button id="add-row">Add row</button><button id="add-column">Add column</button>
    <label>Default <input id="table-default" value="${escapeAttribute(data.default ?? '****')}" title="Use **** for no default"></label>
    <span class="spacer"></span><span class="pager"><button id="prev-page" class="secondary">Previous</button>
    <span>${data.rows.length ? tablePage * tablePageSize + 1 : 0}–${Math.min((tablePage + 1) * tablePageSize, data.rows.length)} of ${data.rows.length}</span>
    <button id="next-page" class="secondary">Next</button></span>`;
  requiredWebviewElement('content').innerHTML = `<div class="table-wrap"><table><thead><tr><th>Row</th>
    ${data.columns.map((column, index) => `<th>${escapeHtml(column)} <button class="secondary remove-column" data-column="${index}" title="Remove column">×</button></th>`).join('')}
    <th>Actions</th></tr></thead><tbody>${pageRows.map((row, pageIndex) => {
      const rowIndex = tablePage * tablePageSize + pageIndex;
      return `<tr><td><input class="row-label" data-row="${rowIndex}" value="${escapeAttribute(row.label)}"></td>
        ${data.columns.map((column, columnIndex) => {
          const value = row.cells[columnIndex];
          return `<td><input class="cell ${value == null ? 'null-cell' : ''}" data-row="${rowIndex}" data-column="${escapeAttribute(column)}" value="${escapeAttribute(value ?? '****')}" title="Use **** for an unset cell"></td>`;
        }).join('')}<td><button class="danger remove-row" data-row="${rowIndex}">Remove</button></td></tr>`;
    }).join('')}</tbody></table></div>`;
  requiredWebviewElement('prev-page').onclick = () => { if (tablePage > 0) { tablePage -= 1; renderTwoDa(); } };
  requiredWebviewElement('next-page').onclick = () => { if ((tablePage + 1) * tablePageSize < data.rows.length) { tablePage += 1; renderTwoDa(); } };
  requiredWebviewElement('add-row').onclick = () => {
    const next = clone(data); const index = next.rows.length;
    next.rows.push({ label: String(index), cells: next.columns.map(() => null) });
    edit({ action: 'replace2da', table: next });
  };
  requiredWebviewElement('add-column').onclick = () => {
    const name = prompt('New column name'); if (!name?.trim()) return;
    const next = clone(data); next.columns.push(name.trim()); next.rows.forEach((row) => row.cells.push(null));
    edit({ action: 'replace2da', table: next });
  };
  requiredWebviewElement('table-default').onchange = (event) => {
    const next = clone(data); next.default = cellValue((event.target as HTMLInputElement).value); edit({ action: 'replace2da', table: next });
  };
  document.querySelectorAll<WebviewElement>('.cell').forEach((input) => input.onchange = () => edit({
    action: 'set2daCell', row: Number(input.dataset.row), column: input.dataset.column, value: cellValue(input.value),
  }));
  document.querySelectorAll<WebviewElement>('.row-label').forEach((input) => input.onchange = () => edit({ action: 'set2daRowLabel', row: Number(input.dataset.row), label: input.value }));
  document.querySelectorAll<WebviewElement>('.remove-row').forEach((button) => button.onclick = () => {
    const next = clone(data); next.rows.splice(Number(button.dataset.row), 1); edit({ action: 'replace2da', table: next });
  });
  document.querySelectorAll<WebviewElement>('.remove-column').forEach((button) => button.onclick = () => {
    const index = Number(button.dataset.column); const next = clone(data); next.columns.splice(index, 1); next.rows.forEach((row) => row.cells.splice(index, 1)); edit({ action: 'replace2da', table: next });
  });
}

function renderTlk(): void {
  if (!model || model.kind !== 'tlk') {
    throw new Error('The TLK editor received the wrong resource snapshot.');
  }
  const data = model.data;
  requiredWebviewElement('toolbar').innerHTML = `<label>Language <select id="tlk-language">${['English', 'French', 'German', 'Italian', 'Spanish', 'Polish'].map((language, index) => `<option value="${index}" ${data.language === index ? 'selected' : ''}>${language}</option>`).join('')}</select></label>
    <input id="tlk-search" type="search" placeholder="Search strref, text, or sound" value="${escapeAttribute(tlkQuery)}">
    <button id="tlk-search-button">Search</button><button id="tlk-add">Add entry</button><span class="spacer"></span>
    <span class="pager"><button id="tlk-prev" class="secondary">Previous</button><span>${data.total ? data.offset + 1 : 0}–${Math.min(data.offset + data.entries.length, data.total)} of ${data.total}</span><button id="tlk-next" class="secondary">Next</button></span>`;
  requiredWebviewElement('content').innerHTML = `<div class="table-wrap"><table><thead><tr><th>StrRef</th><th>Text</th><th>Sound</th><th>Length</th><th>Flags</th></tr></thead><tbody>
    ${data.entries.map((entry) => `<tr data-strref="${entry.strRef}"><td>${entry.strRef}</td>
      <td><textarea class="tlk-field tlk-text" data-field="text">${escapeHtml(entry.text)}</textarea></td>
      <td><input class="tlk-field" data-field="soundResRef" value="${escapeAttribute(entry.soundResRef)}"></td>
      <td><input class="tlk-field" data-field="soundLength" type="number" step="any" value="${entry.soundLength}"></td>
      <td><input class="tlk-field" data-field="flags" type="number" value="${entry.flags}"></td></tr>`).join('')}
    </tbody></table></div>`;
  const search = (): void => { tlkQuery = requiredWebviewElement('tlk-search').value; tlkOffset = 0; refresh({ query: tlkQuery, offset: 0 }); };
  requiredWebviewElement('tlk-search-button').onclick = search;
  requiredWebviewElement('tlk-language').onchange = (event) => edit({ action: 'setTlkLanguage', language: Number((event.target as HTMLInputElement).value) });
  requiredWebviewElement('tlk-search').onkeydown = (event) => { if (event.key === 'Enter') search(); };
  requiredWebviewElement('tlk-prev').onclick = () => { tlkOffset = Math.max(0, data.offset - data.limit); refresh({ query: tlkQuery, offset: tlkOffset }); };
  requiredWebviewElement('tlk-next').onclick = () => { if (data.offset + data.entries.length < data.total) { tlkOffset = data.offset + data.limit; refresh({ query: tlkQuery, offset: tlkOffset }); } };
  requiredWebviewElement('tlk-add').onclick = () => {
    const value = prompt('String reference', String(Math.max(0, data.highest + 1))); if (value == null) return;
    const strRef = Number(value); if (!Number.isInteger(strRef) || strRef < 0 || strRef > 0xffffffff) return showError('String reference must be between 0 and 4294967295.');
    edit({ action: 'setTlkEntry', strRef, entry: { text: '', soundResRef: '', soundLength: 0, flags: 0, volumeVariance: 0, pitchVariance: 0 } });
  };
  document.querySelectorAll<WebviewElement>('.tlk-field').forEach((input) => input.onchange = () => {
    const row = input.closest<HTMLElement>('tr');
    const field = input.dataset.field;
    if (!row || !field) return;
    const strRef = Number(row.dataset.strref);
    const current = data.entries.find((entry) => entry.strRef === strRef);
    if (!current) return;
    const entry = clone(current);
    if (field === 'soundLength' || field === 'flags') entry[field] = Number(input.value);
    else if (field === 'text' || field === 'soundResRef') entry[field] = input.value;
    edit({ action: 'setTlkEntry', strRef, entry });
  });
}

function renderGff(): void {
  if (!model || model.kind !== 'gff') {
    throw new Error('The GFF editor received the wrong resource snapshot.');
  }
  const data = model.data;
  requiredWebviewElement('toolbar').innerHTML = `<span>Type <strong>${escapeHtml(data.fileType)}</strong></span><span>Version <strong>${escapeHtml(data.fileVersion)}</strong></span><button id="gff-add">Add root field</button>`;
  requiredWebviewElement('content').innerHTML = `<div class="gff-root">${renderGffStruct(data.root, ['root'])}</div>`;
  requiredWebviewElement('gff-add').onclick = () => addGffField(['root']);
  bindGffControls();
}

type DataPath = Array<string | number>;

function renderGffStruct(structure: GffStructure, pathParts: DataPath): string {
  return `<details open><summary>Struct ${structure.id} · ${structure.fields.length} fields</summary><div class="gff-node">
    ${structure.fields.map((field, index) => renderGffField(field, [...pathParts, 'fields', index])).join('')}
    <button class="secondary gff-add-field" data-path="${encodePath(pathParts)}">Add field</button></div></details>`;
}

function renderGffField(field: GffField, pathParts: DataPath): string {
  const compound = field.kind === 'struct' && isGffStructure(field.value)
    ? renderGffStruct(field.value, [...pathParts, 'value'])
    : field.kind === 'list'
      ? (() => {
        const entries = Array.isArray(field.value)
          ? field.value.filter(isGffStructure)
          : [];
        return `<details open><summary>List · ${entries.length} structs</summary><div class="gff-node">${entries.map((item, index) => `${renderGffStruct(item, [...pathParts, 'value', index])}<button class="danger gff-remove-list" data-path="${encodePath([...pathParts, 'value'])}" data-index="${index}">Remove struct</button>`).join('')}<button class="secondary gff-add-list" data-path="${encodePath([...pathParts, 'value'])}">Add struct</button></div></details>`;
      })()
      : gffValueControl(field, pathParts);
  return `<div class="gff-field"><input class="gff-label" data-path="${encodePath(pathParts)}" value="${escapeAttribute(field.label)}" maxlength="16">
    <select class="gff-kind" data-path="${encodePath(pathParts)}">${gffKinds.map((kind) => `<option ${kind === field.kind ? 'selected' : ''}>${kind}</option>`).join('')}</select>
    <div>${compound}</div><button class="danger gff-remove" data-path="${encodePath(pathParts)}">Remove</button></div>`;
}

function gffValueControl(field: GffField, pathParts: DataPath): string {
  const valuePath = encodePath([...pathParts, 'value']);
  if (field.kind === 'locstring') return `<textarea class="gff-value" data-kind="locstring" data-path="${valuePath}">${escapeHtml(JSON.stringify(field.value, null, 2))}</textarea>`;
  if (field.kind === 'void') return `<textarea class="gff-value" data-kind="void" data-path="${valuePath}" title="Base64 encoded bytes">${escapeHtml(field.value)}</textarea>`;
  const numeric = ['byte', 'char', 'word', 'short', 'dword', 'int', 'float', 'double'].includes(field.kind);
  return `<input class="gff-value" data-kind="${field.kind}" data-path="${valuePath}" ${numeric ? 'type="number" step="any"' : ''} value="${escapeAttribute(String(field.value))}">`;
}

function bindGffControls(): void {
  document.querySelectorAll<WebviewElement>('.gff-value').forEach((input) => input.onchange = () => {
    let value: JsonValue = input.value;
    if (input.dataset.kind === 'locstring') { try { value = JSON.parse(input.value) as JsonValue; } catch { return showError('Localized string value must be valid JSON.'); } }
    else if (['byte', 'char', 'word', 'short', 'dword', 'int', 'float', 'double'].includes(input.dataset.kind ?? '')) value = Number(value);
    const next = clone(currentGffData()); setAtPath(next, decodePath(input.dataset.path), value); submitGff(next);
  });
  document.querySelectorAll<WebviewElement>('.gff-label').forEach((input) => input.onchange = () => {
    if (!input.value || new TextEncoder().encode(input.value).length > 16) return showError('GFF labels must be 1–16 bytes.');
    const next = clone(currentGffData()); setAtPath(next, [...decodePath(input.dataset.path), 'label'], input.value); submitGff(next);
  });
  document.querySelectorAll<WebviewElement>('.gff-kind').forEach((select) => select.onchange = () => {
    const next = clone(currentGffData());
    const field = getAtPath(next, decodePath(select.dataset.path));
    if (!isGffField(field)) return;
    if (!isGffKind(select.value)) return showError(`Unsupported GFF field kind: ${select.value}`);
    field.kind = select.value; field.value = defaultGffValue(select.value); submitGff(next);
  });
  document.querySelectorAll<WebviewElement>('.gff-remove').forEach((button) => button.onclick = () => {
    const pathParts = decodePath(button.dataset.path); const index = pathParts.pop(); const next = clone(currentGffData());
    const parent = getAtPath(next, pathParts);
    if (Array.isArray(parent) && typeof index === 'number') parent.splice(index, 1);
    submitGff(next);
  });
  document.querySelectorAll<WebviewElement>('.gff-add-field').forEach((button) => button.onclick = () => addGffField(decodePath(button.dataset.path)));
  document.querySelectorAll<WebviewElement>('.gff-add-list').forEach((button) => button.onclick = () => {
    const next = clone(currentGffData());
    const list = getAtPath(next, decodePath(button.dataset.path));
    if (Array.isArray(list)) list.push({ id: 0, fields: [] });
    submitGff(next);
  });
  document.querySelectorAll<WebviewElement>('.gff-remove-list').forEach((button) => button.onclick = () => {
    const next = clone(currentGffData());
    const list = getAtPath(next, decodePath(button.dataset.path));
    if (Array.isArray(list)) list.splice(Number(button.dataset.index), 1);
    submitGff(next);
  });
}

function addGffField(structPath: DataPath): void {
  const label = prompt('Field label (maximum 16 bytes)'); if (!label) return;
  if (new TextEncoder().encode(label).length > 16) return showError('GFF labels cannot exceed 16 bytes.');
  const next = clone(currentGffData()); const structure = getAtPath(next, structPath);
  if (!isGffStructure(structure)) return;
  if (structure.fields.some((field) => field.label === label)) return showError(`Field ${label} already exists in this structure.`);
  structure.fields.push({ label, kind: 'int', value: 0 }); submitGff(next);
}

function submitGff(root: GffData): void { edit({ action: 'replaceGff', root }); }

function currentGffData(): GffData {
  if (!model || model.kind !== 'gff') throw new Error('No GFF document is active.');
  return model.data;
}

function renderScriptDebug(): void {
  const data = currentScriptDebugData();
  requiredWebviewElement('content').classList.add('ncs-content');
  const functions = data.functions || [];
  if (!functions[scriptDebugState.functionIndex]) scriptDebugState.functionIndex = 0;
  const activeFunction = functions[scriptDebugState.functionIndex];
  requiredWebviewElement('toolbar').innerHTML = `<input id="ncs-search" type="search" placeholder="Search instructions, operands, source, or bytes" value="${escapeAttribute(scriptDebugState.query)}" aria-label="Search disassembly"><span class="spacer"></span>${scriptDebugStatusBadge('NCS', data.hasNcs)}${scriptDebugStatusBadge('NDB', data.hasNdb)}${scriptDebugStatusBadge('Sources', (data.sourceFiles || []).some((file) => file.available))}${scriptDebugStatusBadge('nwscript', data.hasLangspec)}`;
  requiredWebviewElement('content').innerHTML = `<div class="ncs-workbench">
    <aside class="ncs-outline" aria-label="NCS outline">${scriptDebugOutline(data)}</aside>
    <section class="ncs-disassembly" aria-label="Disassembly">${scriptDebugSummary(data)}<div id="ncs-table"></div></section>
    <aside class="ncs-context" aria-label="Instruction and control-flow details"><section id="ncs-detail" class="ncs-panel"></section><section class="ncs-panel ncs-cfg"><header><h2>Control Flow</h2><small>${activeFunction ? escapeHtml(activeFunction.name) : 'Unavailable'}</small></header><div id="ncs-graph"></div></section>${scriptDebugDiagnostics(data)}</aside>
  </div>`;
  renderScriptDebugTable();
  renderScriptDebugDetail();
  renderScriptDebugGraph();
  let searchTimer: ReturnType<typeof setTimeout> | undefined;
  requiredWebviewElement('ncs-search').oninput = (event) => {
    clearTimeout(searchTimer);
    searchTimer = setTimeout(() => {
      scriptDebugState.query = (event.target as HTMLInputElement).value;
      scriptDebugState.page = 0;
      renderScriptDebugTable();
    }, 100);
  };
  bindScriptDebugOutline();
}

function scriptDebugStatusBadge(label: string, available: boolean): string {
  return `<span class="ncs-status ${available ? 'available' : 'missing'}"><span aria-hidden="true">${available ? '✓' : '—'}</span>${escapeHtml(label)}</span>`;
}

function scriptDebugSummary(data: ScriptDebugData): string {
  const header = data.header;
  const summary = data.summary || {};
  return `<header class="ncs-summary"><div><strong>${header ? `${header.instructionCount} instructions` : 'Debug information only'}</strong><small>${header ? `${header.fileSize} bytes · ${header.codeSize} byte code section` : 'Matching NCS unavailable'}</small></div><dl><dt>Functions</dt><dd>${(data.functions || []).length}</dd><dt>Variables</dt><dd>${summary.variables || 0}</dd><dt>Source maps</dt><dd>${summary.lineMappings || 0}</dd></dl></header>`;
}

function scriptDebugOutline(data: ScriptDebugData): string {
  const functions = data.functions || [];
  const files = data.sourceFiles || [];
  const structs = data.summary?.structEntries || [];
  const variables = data.summary?.variableEntries || [];
  const functionRows = functions.map((entry, index) => `<button class="ncs-outline-item function ${index === scriptDebugState.functionIndex ? 'selected' : ''}" data-function-index="${index}"><span>${escapeHtml(entry.name)}</span><small>${entry.synthetic ? 'inferred' : `${escapeHtml(entry.returnType)}(${entry.arguments.map(escapeHtml).join(', ')})`} · ${formatNcsOffset(entry.start)}–${formatNcsOffset(entry.end)}</small></button>`).join('');
  const fileRows = files.map((file) => `<button class="ncs-outline-item source ${file.available ? '' : 'unavailable'}" data-source-file="${escapeAttribute(file.name)}" data-source-line="1" ${file.available ? '' : 'disabled'}><span>${escapeHtml(scriptSourceResource(file.name))}</span><small>${file.isRoot ? 'root source' : 'include'} · ${file.available ? 'resolved' : 'unavailable'}</small></button>`).join('');
  const structRows = structs.map((entry) => `<details class="ncs-debug-entry"><summary>${escapeHtml(entry.name)} <small>${entry.fields.length} fields</small></summary>${entry.fields.map((field) => `<div><code>${escapeHtml(field.type)}</code> ${escapeHtml(field.name)}</div>`).join('')}</details>`).join('');
  const variableRows = variables.map((entry) => `<button class="ncs-outline-item variable" data-offset="${entry.start}"><span>${escapeHtml(entry.name)}</span><small>${escapeHtml(entry.type)} · stack ${entry.stackLocation} · ${formatNcsOffset(entry.start)}</small></button>`).join('');
  return `<details open><summary>Functions <small>${functions.length}</small></summary><div>${functionRows || '<div class="muted">No function information</div>'}</div></details>
    <details ${files.length ? 'open' : ''}><summary>Source Files <small>${files.length}</small></summary><div>${fileRows || '<div class="muted">No source table</div>'}</div></details>
    <details><summary>Variables <small>${variables.length}</small></summary><div>${variableRows || '<div class="muted">No variable records</div>'}</div></details>
    <details><summary>Structs <small>${structs.length}</small></summary><div>${structRows || '<div class="muted">No struct records</div>'}</div></details>`;
}

function bindScriptDebugOutline(): void {
  const data = currentScriptDebugData();
  document.querySelectorAll<WebviewElement>('[data-function-index]').forEach((button) => button.onclick = () => {
    scriptDebugState.functionIndex = Number(button.dataset.functionIndex);
    scriptDebugState.selectedOffset = data.functions[scriptDebugState.functionIndex]?.start;
    scriptDebugState.page = 0;
    document.querySelectorAll<WebviewElement>('[data-function-index]').forEach((entry) => entry.classList.toggle('selected', entry === button));
    renderScriptDebugTable(); renderScriptDebugDetail(); renderScriptDebugGraph();
  });
  document.querySelectorAll<WebviewElement>('[data-source-file]').forEach((button) => button.onclick = () => openScriptSource(button.dataset.sourceFile, Number(button.dataset.sourceLine)));
  document.querySelectorAll<WebviewElement>('.ncs-outline-item.variable').forEach((button) => button.onclick = () => selectScriptInstruction(Number(button.dataset.offset), true));
}

function filteredScriptInstructions(): ScriptInstruction[] {
  const data = currentScriptDebugData();
  const active = data.functions?.[scriptDebugState.functionIndex];
  const query = scriptDebugState.query.trim().toLowerCase();
  return (data.instructions || []).filter((instruction) => {
    if (active && (instruction.offset < active.start || instruction.offset >= active.end)) return false;
    if (!query) return true;
    const source = instruction.source ? `${instruction.source.file} ${instruction.source.line} ${instruction.source.text || ''}` : '';
    return `${instruction.offset} ${instruction.label || ''} ${instruction.opcode} ${instruction.opcodeInternal} ${instruction.auxcode || ''} ${instruction.operand || ''} ${instruction.action?.name || ''} ${instruction.rawHex} ${source}`.toLowerCase().includes(query);
  });
}

function renderScriptDebugTable(): void {
  const host = webviewElement('ncs-table'); if (!host) return;
  const rows = filteredScriptInstructions();
  const pageSize = 300;
  const pages = Math.max(1, Math.ceil(rows.length / pageSize));
  scriptDebugState.page = Math.min(scriptDebugState.page, pages - 1);
  const start = scriptDebugState.page * pageSize;
  const visible = rows.slice(start, start + pageSize);
  host.innerHTML = `<div class="ncs-table-wrap"><table class="ncs-table"><thead><tr><th>Offset</th><th>Local</th><th>Label</th><th>Instruction</th><th>Operand</th><th>Source</th></tr></thead><tbody>${visible.map(scriptInstructionRow).join('')}</tbody></table></div><footer class="ncs-pager"><span>${rows.length ? start + 1 : 0}–${Math.min(start + pageSize, rows.length)} of ${rows.length}</span><div><button id="ncs-prev" class="secondary" ${scriptDebugState.page === 0 ? 'disabled' : ''}>Previous</button><span>Page ${scriptDebugState.page + 1} of ${pages}</span><button id="ncs-next" class="secondary" ${scriptDebugState.page + 1 >= pages ? 'disabled' : ''}>Next</button></div></footer>`;
  document.querySelectorAll<WebviewElement>('.ncs-instruction-row').forEach((row) => row.onclick = () => selectScriptInstruction(Number(row.dataset.offset), false));
  document.querySelectorAll<WebviewElement>('.ncs-target').forEach((button) => button.onclick = (event) => { event.stopPropagation(); selectScriptInstruction(Number(button.dataset.target), true); });
  document.querySelectorAll<WebviewElement>('.ncs-source-link').forEach((button) => button.onclick = (event) => { event.stopPropagation(); openScriptSource(button.dataset.sourceFile, Number(button.dataset.sourceLine)); });
  requiredWebviewElement('ncs-prev').onclick = () => { scriptDebugState.page -= 1; renderScriptDebugTable(); };
  requiredWebviewElement('ncs-next').onclick = () => { scriptDebugState.page += 1; renderScriptDebugTable(); };
  highlightSelectedScriptInstruction();
}

function scriptInstructionRow(instruction: ScriptInstruction): string {
  const selected = instruction.offset === scriptDebugState.selectedOffset ? ' selected' : '';
  const operand = instruction.action
    ? `<span class="ncs-action"><strong>${escapeHtml(instruction.action.name)}</strong><small>#${instruction.action.id} · ${instruction.action.argumentCount} args</small></span>`
    : Number.isInteger(instruction.jumpTarget)
    ? `<button class="ncs-target" data-target="${instruction.jumpTarget}" title="Go to ${formatNcsOffset(instruction.jumpTarget)}">${escapeHtml(instruction.operand || formatNcsOffset(instruction.jumpTarget))}</button>`
    : escapeHtml(instruction.operand || '');
  const source = instruction.source
    ? `<button class="ncs-source-link" data-source-file="${escapeAttribute(instruction.source.file)}" data-source-line="${instruction.source.line}" ${instruction.source.available ? '' : 'disabled'}><span>${escapeHtml(scriptSourceResource(instruction.source.file))}:${instruction.source.line}</span><small>${escapeHtml(instruction.source.text || (instruction.source.available ? '' : 'source unavailable'))}</small></button>`
    : '';
  return `<tr id="ncs-offset-${instruction.offset}" class="ncs-instruction-row${selected}" data-offset="${instruction.offset}"><td><code>${formatNcsOffset(instruction.offset)}</code></td><td><code>${Number.isInteger(instruction.localOffset) ? formatNcsOffset(instruction.localOffset) : ''}</code></td><td><code>${escapeHtml(instruction.label || '')}</code></td><td><strong>${escapeHtml(instruction.opcode)}</strong>${instruction.auxcode ? `<small>.${escapeHtml(instruction.auxcode)}</small>` : ''}</td><td><code>${operand}</code></td><td>${source}</td></tr>`;
}

function selectScriptInstruction(offset: number, reveal: boolean): void {
  const instruction = currentScriptDebugData().instructions.find((entry) => entry.offset === offset);
  if (!instruction) return;
  if (instruction.functionIndex !== null
      && Number.isInteger(instruction.functionIndex)
      && instruction.functionIndex !== scriptDebugState.functionIndex) {
    scriptDebugState.functionIndex = instruction.functionIndex;
    scriptDebugState.page = 0;
    document.querySelectorAll<WebviewElement>('[data-function-index]').forEach((entry) => entry.classList.toggle('selected', Number(entry.dataset.functionIndex) === scriptDebugState.functionIndex));
    renderScriptDebugTable(); renderScriptDebugGraph();
  }
  scriptDebugState.selectedOffset = offset;
  highlightSelectedScriptInstruction(); renderScriptDebugDetail(); highlightScriptGraphBlock();
  if (reveal) webviewElement(`ncs-offset-${offset}`)?.scrollIntoView({ block: 'center', behavior: 'smooth' });
}

function highlightSelectedScriptInstruction(): void {
  document.querySelectorAll<WebviewElement>('.ncs-instruction-row').forEach((row) => row.classList.toggle('selected', Number(row.dataset.offset) === scriptDebugState.selectedOffset));
}

function renderScriptDebugDetail(): void {
  const host = webviewElement('ncs-detail'); if (!host) return;
  const instruction = currentScriptDebugData().instructions.find((entry) => entry.offset === scriptDebugState.selectedOffset);
  if (!instruction) { host.innerHTML = '<header><h2>Instruction</h2></header><div class="muted">Select an instruction to inspect its encoding and control flow.</div>'; return; }
  const targets = `${Number.isInteger(instruction.callTarget) ? `<button class="ncs-detail-target" data-target="${instruction.callTarget}">call → ${formatNcsOffset(instruction.callTarget)}</button>` : ''}${(instruction.successors || []).map((successor) => `<button class="ncs-detail-target" data-target="${successor.offset}">${escapeHtml(successor.kind)} → ${formatNcsOffset(successor.offset)}</button>`).join('')}`;
  const action = instruction.action ? `<section class="ncs-action-detail"><strong>${escapeHtml(formatBuiltinType(instruction.action.returnType))} ${escapeHtml(instruction.action.name)}(${instruction.action.parameters.map((parameter) => `${escapeHtml(formatBuiltinType(parameter.ty))} ${escapeHtml(parameter.name)}`).join(', ')})</strong><small>Engine action ${instruction.action.id} · encoded argument count ${instruction.action.argumentCount}${instruction.action.arityMatches ? '' : ' · argument count differs from nwscript.nss'}</small></section>` : '';
  host.innerHTML = `<header><h2>${escapeHtml(instruction.opcode)}${instruction.auxcode ? `.${escapeHtml(instruction.auxcode)}` : ''}</h2><code>${formatNcsOffset(instruction.offset)}</code></header><dl><dt>Internal</dt><dd><code>${escapeHtml(instruction.opcodeInternal)}${instruction.auxcodeInternal ? `.${escapeHtml(instruction.auxcodeInternal)}` : ''}</code></dd><dt>Size</dt><dd>${instruction.size} bytes</dd><dt>Operand</dt><dd><code>${escapeHtml(instruction.operand || 'none')}</code></dd><dt>Encoded bytes</dt><dd><code>${escapeHtml(instruction.rawHex)}</code></dd></dl>${action}${targets ? `<div class="ncs-detail-targets"><strong>Successors</strong>${targets}</div>` : ''}${instruction.source ? `<button id="ncs-detail-source" class="ncs-source-card" ${instruction.source.available ? '' : 'disabled'}><strong>${escapeHtml(scriptSourceResource(instruction.source.file))}:${instruction.source.line}</strong><code>${escapeHtml(instruction.source.text || 'Source unavailable')}</code></button>` : ''}`;
  document.querySelectorAll<WebviewElement>('.ncs-detail-target').forEach((button) => button.onclick = () => selectScriptInstruction(Number(button.dataset.target), true));
  const source = webviewElement('ncs-detail-source');
  const sourceLocation = instruction.source;
  if (source && sourceLocation) source.onclick = () => openScriptSource(sourceLocation.file, sourceLocation.line);
}

function renderScriptDebugGraph(): void {
  const host = webviewElement('ncs-graph'); if (!host) return;
  const data = currentScriptDebugData();
  const fn = data.functions[scriptDebugState.functionIndex];
  if (!fn?.blocks?.length) { host.innerHTML = '<div class="muted">No control-flow blocks are available.</div>'; return; }
  const blocks = fn.blocks;
  const width = 560; const nodeX = 160; const nodeWidth = 240; const nodeHeight = 58; const gap = 42;
  const yFor = (index: number): number => 24 + index * (nodeHeight + gap);
  const indexByStart = new Map(blocks.map((block, index) => [block.start, index]));
  const edges: string[] = [];
  blocks.forEach((block, index) => (block.successors || []).forEach((edge, edgeIndex) => {
    const targetIndex = indexByStart.get(edge.offset); if (targetIndex == null) return;
    const fromY = yFor(index) + nodeHeight; const toY = yFor(targetIndex);
    const lane = targetIndex > index ? 430 + edgeIndex * 18 : 125 - edgeIndex * 18;
    const color = edge.kind === 'branch' ? 'var(--vscode-charts-yellow)' : 'var(--vscode-charts-blue)';
    edges.push(`<path d="M ${nodeX + nodeWidth / 2} ${fromY} C ${lane} ${fromY + 18}, ${lane} ${toY - 18}, ${nodeX + nodeWidth / 2} ${toY}" fill="none" stroke="${color}" marker-end="url(#ncs-arrow)"/>`);
  }));
  const nodes = blocks.map((block, index) => {
    const rows = block.instructionIndices
      .map((instructionIndex) => data.instructions[instructionIndex])
      .filter((instruction): instruction is ScriptInstruction => instruction !== undefined);
    const label = `${formatNcsOffset(block.start)}–${formatNcsOffset(block.end)}`;
    const preview = rows.slice(0, 2).map((row) => `${row.opcode}${row.action ? ` ${row.action.name}` : row.operand ? ` ${row.operand}` : ''}`).join(' · ');
    return `<g class="ncs-graph-block" data-block-start="${block.start}" data-block-end="${block.end}" role="button" tabindex="0"><rect x="${nodeX}" y="${yFor(index)}" width="${nodeWidth}" height="${nodeHeight}" rx="5"/><text x="${nodeX + 10}" y="${yFor(index) + 20}" class="title">${escapeHtml(label)}</text><text x="${nodeX + 10}" y="${yFor(index) + 41}" class="preview">${escapeHtml(preview.slice(0, 52))}</text></g>`;
  }).join('');
  host.innerHTML = `<svg class="ncs-flow-graph" viewBox="0 0 ${width} ${yFor(blocks.length - 1) + nodeHeight + 24}" aria-label="Control-flow graph for ${escapeAttribute(fn.name)}"><defs><marker id="ncs-arrow" markerWidth="7" markerHeight="7" refX="6" refY="3.5" orient="auto"><path d="M0,0 L7,3.5 L0,7 z" fill="context-stroke"/></marker></defs>${edges.join('')}${nodes}</svg>`;
  document.querySelectorAll<WebviewElement>('.ncs-graph-block').forEach((node) => {
    const activate = () => selectScriptInstruction(Number(node.dataset.blockStart), true);
    node.onclick = activate; node.onkeydown = (event) => { if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); activate(); } };
  });
  highlightScriptGraphBlock();
}

function highlightScriptGraphBlock(): void {
  document.querySelectorAll<WebviewElement>('.ncs-graph-block').forEach((node) => {
    const offset = scriptDebugState.selectedOffset;
    node.classList.toggle(
      'selected',
      offset !== undefined
        && Number.isInteger(offset)
        && offset >= Number(node.dataset.blockStart)
        && offset < Number(node.dataset.blockEnd),
    );
  });
}

function scriptDebugDiagnostics(data: ScriptDebugData): string {
  const diagnostics = data.diagnostics || [];
  return diagnostics.length ? `<section class="ncs-panel ncs-diagnostics"><header><h2>Diagnostics</h2><small>${diagnostics.length}</small></header>${diagnostics.map((message) => `<div class="diagnostic warning">${escapeHtml(message)}</div>`).join('')}</section>` : '';
}

function openScriptSource(file: string | undefined, line: number): void {
  if (!file) return;
  vscode.postMessage({ type: 'openScriptSource', file, line });
}

function scriptSourceResource(file: string): string {
  const value = String(file || '');
  return value.toLowerCase().endsWith('.nss') ? value : `${value}.nss`;
}

function formatBuiltinType(value: BuiltinType): string {
  if (typeof value === 'string') return value.toLowerCase();
  if (value.EngineStructure) return String(value.EngineStructure);
  return Object.keys(value)[0]?.toLowerCase() || '?';
}

function formatNcsOffset(offset: number | null | undefined): string {
  return Number(offset || 0).toString(16).toUpperCase().padStart(4, '0');
}

function currentScriptDebugData(): ScriptDebugData {
  if (!model || (model.kind !== 'ncs' && model.kind !== 'ndb')) {
    throw new Error('No NCS/NDB workbench document is active.');
  }
  return model.data;
}

function renderTexture(): void {
  if (!model || (model.kind !== 'dds' && model.kind !== 'tga' && model.kind !== 'plt')) {
    throw new Error('The texture editor received the wrong resource snapshot.');
  }
  const currentModel = model;
  const data = currentModel.data;
  requiredWebviewElement('toolbar').innerHTML = `<span>${data.width} × ${data.height}</span>`;
  requiredWebviewElement('content').innerHTML = `<div class="texture-layout"><div class="canvas-wrap"><canvas id="texture-canvas" width="${data.width}" height="${data.height}"></canvas></div>
    <aside class="inspector"><h2>Texture</h2><dl>${Object.entries(data.metadata || {}).filter(([key]) => key !== 'pixels').map(([key, value]) => `<dt>${escapeHtml(key)}</dt><dd>${escapeHtml(String(value))}</dd>`).join('')}</dl>
    ${currentModel.kind === 'plt' ? '<div id="plt-inspector" class="muted">Click a pixel to edit its value and material layer.</div>' : ''}</aside></div>`;
  const canvas = requiredCanvas('texture-canvas'); drawRgba(canvas, data.rgba);
  if (currentModel.kind === 'plt') canvas.onclick = (event) => showPltPixel(canvas, event, data);
}

function drawRgba(canvas: HTMLCanvasElement, base64: string): void {
  const bytes = Uint8ClampedArray.from(atob(base64), (character) => character.charCodeAt(0));
  const context = canvas.getContext('2d');
  if (!context) throw new Error('The texture editor could not create a 2D canvas context.');
  context.putImageData(new ImageData(bytes, canvas.width, canvas.height), 0, 0);
}

function showPltPixel(
  canvas: HTMLCanvasElement,
  event: MouseEvent,
  data: TextureData,
): void {
  const rect = canvas.getBoundingClientRect();
  const x = Math.min(canvas.width - 1, Math.max(0, Math.floor((event.clientX - rect.left) * canvas.width / rect.width)));
  const y = Math.min(canvas.height - 1, Math.max(0, Math.floor((event.clientY - rect.top) * canvas.height / rect.height)));
  const pixelData = data.metadata.pixelData;
  if (typeof pixelData !== 'string') throw new Error('The PLT snapshot has no pixel metadata.');
  const pixels = Uint8Array.from(atob(pixelData), (character) => character.charCodeAt(0));
  const offset = (y * canvas.width + x) * 2; const pixel = { value: pixels[offset], layer: pixels[offset + 1] }; const inspector = webviewElement('plt-inspector');
  const requiredInspector = requiredWebviewElement('plt-inspector');
  requiredInspector.className = '';
  requiredInspector.innerHTML = `<h3>Pixel ${x}, ${y}</h3><label>Value <input id="plt-value" type="number" min="0" max="255" value="${pixel.value}"></label>
    <label>Layer <select id="plt-layer">${['Skin', 'Hair', 'Metal 1', 'Metal 2', 'Cloth 1', 'Cloth 2', 'Leather 1', 'Leather 2', 'Tattoo 1', 'Tattoo 2'].map((label, index) => `<option value="${index}" ${pixel.layer === index ? 'selected' : ''}>${label}</option>`).join('')}</select></label><button id="plt-apply">Apply pixel</button>`;
  requiredWebviewElement('plt-apply').onclick = () => edit({ action: 'setPltPixel', x, y, value: Number(requiredWebviewElement('plt-value').value), layer: Number(requiredWebviewElement('plt-layer').value) });
}

function renderArchive(): void {
  if (!model || (model.kind !== 'erf' && model.kind !== 'key')) {
    throw new Error('The archive editor received the wrong resource snapshot.');
  }
  const currentModel = model;
  const data = currentModel.data;
  const entries = data.entries;
  requiredWebviewElement('toolbar').innerHTML = `<button id="archive-add">Add resource…</button><input id="archive-search" type="search" placeholder="Filter resources" value="${escapeAttribute(data.query || '')}"><button id="archive-search-button">Search</button><span class="spacer"></span>
    <span class="pager"><button id="archive-prev" class="secondary">Previous</button><span>${data.total ? data.offset + 1 : 0}–${Math.min(data.offset + entries.length, data.total)} of ${data.total}</span><button id="archive-next" class="secondary">Next</button></span>`;
  const renderRows = (): void => {
    requiredWebviewElement('content').innerHTML = `<div class="table-wrap"><table><thead><tr><th>Resource</th>${currentModel.kind === 'key' ? '<th>BIF</th>' : ''}<th>Type</th><th>Size</th><th>State</th><th>Actions</th></tr></thead><tbody>
      ${entries.map((entry) => `<tr><td>${escapeHtml(entry.resource)}</td>${currentModel.kind === 'key' ? `<td>${escapeHtml(entry.bif || '')}</td>` : ''}<td>${escapeHtml(entry.extension || String(entry.typeId))}</td><td>${formatBytes(entry.size)}</td><td>${entry.modified ? 'Modified' : ''}</td><td><div class="archive-actions">
      ${isCustomEditorType(entry.extension) ? `<button class="open-entry" data-resource="${escapeAttribute(entry.resource)}">Open</button>` : ''}
      <button class="secondary export-entry" data-resource="${escapeAttribute(entry.resource)}">Export</button><button class="secondary replace-entry" data-resource="${escapeAttribute(entry.resource)}">Replace</button><button class="secondary rename-entry" data-resource="${escapeAttribute(entry.resource)}">Rename</button><button class="danger remove-entry" data-resource="${escapeAttribute(entry.resource)}">Remove</button></div></td></tr>`).join('')}</tbody></table></div>`;
    bindArchiveRows();
  };
  renderRows();
  requiredWebviewElement('archive-add').onclick = () => {
    let bifIndex: number | undefined;
    const bifs = data.bifs || [];
    if (currentModel.kind === 'key' && bifs.length > 1) {
      const choices = bifs.map((bif) => `${bif.index}: ${bif.filename}`).join('\n');
      const selected = prompt(`BIF index for the new resource:\n${choices}`, '0');
      if (selected == null) return;
      bifIndex = Number(selected);
      if (!Number.isInteger(bifIndex) || !bifs.some((bif) => bif.index === bifIndex)) return showError('Select a valid BIF index.');
    }
    vscode.postMessage({ type: 'addEntry', bifIndex });
  };
  const search = (): void => refresh({ query: requiredWebviewElement('archive-search').value, offset: 0 });
  requiredWebviewElement('archive-search-button').onclick = search;
  requiredWebviewElement('archive-search').onkeydown = (event) => { if (event.key === 'Enter') search(); };
  requiredWebviewElement('archive-prev').onclick = () => refresh({ query: data.query || '', offset: Math.max(0, data.offset - data.limit) });
  requiredWebviewElement('archive-next').onclick = () => { if (data.offset + entries.length < data.total) refresh({ query: data.query || '', offset: data.offset + data.limit }); };
}

function bindArchiveRows(): void {
  document.querySelectorAll<WebviewElement>('.open-entry').forEach((button) => button.onclick = () => vscode.postMessage({ type: 'openEntry', resource: button.dataset.resource }));
  document.querySelectorAll<WebviewElement>('.export-entry').forEach((button) => button.onclick = () => vscode.postMessage({ type: 'exportEntry', resource: button.dataset.resource }));
  document.querySelectorAll<WebviewElement>('.replace-entry').forEach((button) => button.onclick = () => vscode.postMessage({ type: 'replaceEntry', resource: button.dataset.resource }));
  document.querySelectorAll<WebviewElement>('.rename-entry').forEach((button) => button.onclick = () => { const newResource = prompt('New resource name', button.dataset.resource); if (newResource && newResource !== button.dataset.resource) edit({ action: 'renameEntry', resource: button.dataset.resource, newResource }); });
  document.querySelectorAll<WebviewElement>('.remove-entry').forEach((button) => button.onclick = () => { if (confirm(`Remove ${button.dataset.resource}?`)) edit({ action: 'removeEntry', resource: button.dataset.resource }); });
}

function createProgram(
  gl: WebGL2RenderingContext,
  vertexSource: string,
  fragmentSource: string,
): WebGLProgram {
  const compile = (type: number, source: string): WebGLShader => {
    const shader = gl.createShader(type);
    if (!shader) throw new Error('WebGL could not allocate a shader object.');
    gl.shaderSource(shader, source); gl.compileShader(shader);
    if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
      const message = gl.getShaderInfoLog(shader); gl.deleteShader(shader); throw new Error(`WebGL shader compilation failed: ${message}`);
    }
    return shader;
  };
  const vertex = compile(gl.VERTEX_SHADER, vertexSource); const fragment = compile(gl.FRAGMENT_SHADER, fragmentSource);
  const program = gl.createProgram();
  if (!program) throw new Error('WebGL could not allocate a program object.');
  gl.attachShader(program, vertex); gl.attachShader(program, fragment); gl.linkProgram(program);
  gl.deleteShader(vertex); gl.deleteShader(fragment);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
    const message = gl.getProgramInfoLog(program); gl.deleteProgram(program); throw new Error(`WebGL program link failed: ${message}`);
  }
  return program;
}

function uniformLocations<const Names extends readonly string[]>(
  gl: WebGL2RenderingContext,
  program: WebGLProgram,
  names: Names,
): { readonly [Name in Names[number]]: WebGLUniformLocation | null } {
  return Object.fromEntries(
    names.map((name) => [name, gl.getUniformLocation(program, name)]),
  ) as { readonly [Name in Names[number]]: WebGLUniformLocation | null };
}

function numericView(binary: Uint8Array, view: BufferView): NumericView {
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

function createSpriteGpu(gl: WebGL2RenderingContext): SpriteGpu {
  const vao = gl.createVertexArray(); const cornerBuffer = gl.createBuffer(); const instanceBuffer = gl.createBuffer();
  gl.bindVertexArray(vao); gl.bindBuffer(gl.ARRAY_BUFFER, cornerBuffer);
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]), gl.STATIC_DRAW);
  gl.enableVertexAttribArray(0); gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0);
  gl.bindBuffer(gl.ARRAY_BUFFER, instanceBuffer); const stride = 15 * 4;
  const attributes: ReadonlyArray<readonly [number, number, number]> =
    [[1, 3, 0], [2, 4, 3], [3, 3, 7], [4, 4, 10], [5, 1, 14]];
  for (const [location, size, offset] of attributes) {
    gl.enableVertexAttribArray(location); gl.vertexAttribPointer(location, size, gl.FLOAT, false, stride, offset * 4); gl.vertexAttribDivisor(location, 1);
  }
  return { vao, cornerBuffer, instanceBuffer, capacity: 0 };
}

function uploadAndDrawSprites(
  gl: WebGL2RenderingContext,
  gpu: SpriteGpu,
  values: Float32Array,
  count: number,
): void {
  gl.bindVertexArray(gpu.vao); gl.bindBuffer(gl.ARRAY_BUFFER, gpu.instanceBuffer);
  const byteLength = count * 15 * 4;
  if (byteLength > gpu.capacity) {
    gpu.capacity = Math.max(byteLength, Math.ceil(gpu.capacity * 1.5), 15 * 4);
    gl.bufferData(gl.ARRAY_BUFFER, gpu.capacity, gl.DYNAMIC_DRAW);
  }
  gl.bufferSubData(gl.ARRAY_BUFFER, 0, values, 0, count * 15);
  gl.drawArraysInstanced(gl.TRIANGLE_STRIP, 0, 4, count);
}

function createRibbonGpu(gl: WebGL2RenderingContext): RibbonGpu {
  const vao = gl.createVertexArray(); const buffer = gl.createBuffer(); const stride = 9 * 4;
  gl.bindVertexArray(vao); gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
  const attributes: ReadonlyArray<readonly [number, number, number]> =
    [[0, 3, 0], [1, 2, 3], [2, 4, 5]];
  for (const [location, size, offset] of attributes) {
    gl.enableVertexAttribArray(location); gl.vertexAttribPointer(location, size, gl.FLOAT, false, stride, offset * 4);
  }
  return { vao, buffer, capacity: 0 };
}

function uploadAndDrawRibbon(
  gl: WebGL2RenderingContext,
  gpu: RibbonGpu,
  values: Float32Array,
  vertexCount: number,
): void {
  if (!vertexCount) return;
  gl.bindVertexArray(gpu.vao); gl.bindBuffer(gl.ARRAY_BUFFER, gpu.buffer);
  const byteLength = vertexCount * 9 * 4;
  if (byteLength > gpu.capacity) {
    gpu.capacity = Math.max(byteLength, Math.ceil(gpu.capacity * 1.5), 9 * 6 * 4);
    gl.bufferData(gl.ARRAY_BUFFER, gpu.capacity, gl.DYNAMIC_DRAW);
  }
  gl.bufferSubData(gl.ARRAY_BUFFER, 0, values, 0, vertexCount * 9);
  gl.drawArrays(gl.TRIANGLES, 0, vertexCount);
}

function createOverlayGpu(
  gl: WebGL2RenderingContext,
  polygon: readonly (readonly number[])[],
): OverlayGpu | undefined {
  if (!polygon?.length) return undefined;
  const values = new Float32Array(polygon.length * 3);
  for (let index = 0; index < polygon.length; index += 1) {
    const point = polygon[index];
    if (point) values.set(point, index * 3);
  }
  const vao = gl.createVertexArray(); const buffer = gl.createBuffer();
  gl.bindVertexArray(vao); gl.bindBuffer(gl.ARRAY_BUFFER, buffer); gl.bufferData(gl.ARRAY_BUFFER, values, gl.STATIC_DRAW);
  gl.enableVertexAttribArray(0); gl.vertexAttribPointer(0, 3, gl.FLOAT, false, 0, 0);
  return { vao, buffer, count: polygon.length };
}

function appendChunkInstance(batch: ChunkBatch, matrix: Float32Array): void {
  const required = (batch.count + 1) * 16;
  if (required > batch.values.length) {
    const grown = new Float32Array(Math.max(required, Math.ceil(batch.values.length * 1.5)));
    grown.set(batch.values); batch.values = grown;
  }
  batch.values.set(matrix, batch.count * 16); batch.count += 1;
}

function bindInstanceMatrices(
  gl: WebGL2RenderingContext,
  vao: WebGLVertexArrayObject | null,
  buffer: WebGLBuffer | null,
): void {
  gl.bindVertexArray(vao); gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
  for (let column = 0; column < 4; column += 1) {
    const location = 6 + column; gl.enableVertexAttribArray(location);
    gl.vertexAttribPointer(location, 4, gl.FLOAT, false, 16 * 4, column * 4 * 4);
    gl.vertexAttribDivisor(location, 1);
  }
}

function emitterProperty(
  emitter: PacketEmitter | null | undefined,
  name: string,
  fallback: number,
): number;
function emitterProperty(
  emitter: PacketEmitter | null | undefined,
  name: string,
  fallback: string,
): string;
function emitterProperty(
  emitter: PacketEmitter | null | undefined,
  name: string,
  fallback: boolean,
): boolean;
function emitterProperty(
  emitter: PacketEmitter | null | undefined,
  name: string,
  fallback: undefined,
): PacketPropertyValue['value'] | undefined;
function emitterProperty(
  emitter: PacketEmitter | null | undefined,
  name: string,
  fallback: boolean | number | string | undefined,
): boolean | number | string | undefined {
  if (!emitter) return fallback;
  let properties = EMITTER_PROPERTY_CACHE.get(emitter);
  if (!properties) {
    properties = new Map((emitter.properties || []).map((entry) => [entry.name.toLowerCase(), entry.values || []]));
    EMITTER_PROPERTY_CACHE.set(emitter, properties);
  }
  const tagged = properties.get(name.toLowerCase())?.[0];
  if (tagged == null) return fallback;
  const value = tagged.value;
  if (typeof fallback === 'number') {
    const numeric = Number(value);
    return Number.isFinite(numeric) ? numeric : fallback;
  }
  if (typeof fallback === 'string') return String(value);
  if (typeof fallback === 'boolean') return Boolean(value);
  return value;
}

function emitterHasValue(
  emitter: PacketEmitter,
  nodeTrack: PreparedNodeAnimationTrack | undefined,
  name: string,
): boolean {
  emitterProperty(emitter, name, undefined);
  return (EMITTER_PROPERTY_CACHE.get(emitter)?.has(name.toLowerCase()) ?? false)
    || (nodeTrack?.emitterControllers?.has(name.toLowerCase()) ?? false);
}

function emitterVectorInto(
  emitter: PacketEmitter | null | undefined,
  name: string,
  fallback: ArrayLike<number>,
  output: Float32Array,
): Float32Array {
  if (!emitter) { output.set(fallback); return output; }
  emitterProperty(emitter, name, undefined);
  let vectors = EMITTER_VECTOR_CACHE.get(emitter);
  if (!vectors) { vectors = new Map(); EMITTER_VECTOR_CACHE.set(emitter, vectors); }
  const key = name.toLowerCase(); let values = vectors.get(key);
  if (!values) {
    const numericValues = (EMITTER_PROPERTY_CACHE.get(emitter)?.get(key) || [])
      .map((tagged) => Number(tagged.value)).filter(Number.isFinite);
    const packed = Float32Array.from(numericValues);
    vectors.set(key, packed);
    values = packed;
  }
  for (let index = 0; index < 3; index += 1) {
    output[index] = values.length >= 3 ? (values[index] ?? 0) : (fallback[index] ?? 0);
  }
  return output;
}

function samplePreparedEmitterValue(
  track: PreparedEmitterTrack | undefined,
  time: number,
  fallback: number,
): number {
  if (!track) return Number(fallback) || 0;
  const times = track.times; let start = 0; let end = 0; let factor = 0;
  if (times.length > 1 && time > Number(times[0] ?? 0)) {
    if (time >= Number(times[times.length - 1] ?? 0)) start = end = times.length - 1;
    else {
      let low = 1; let high = times.length - 1;
      while (low < high) { const middle = (low + high) >>> 1; if (time <= Number(times[middle] ?? 0)) high = middle; else low = middle + 1; }
      end = low;
      start = end - 1;
      const startTime = Number(times[start] ?? 0);
      const endTime = Number(times[end] ?? startTime);
      factor = Math.max(0, Math.min(1, (time-startTime)/Math.max(Number.EPSILON,endTime-startTime)));
    }
  }
  if (track.bezier && start !== end) factor = cubicBezierFactor(factor);
  const leftOffset = Number(track.offsets[start] ?? 0);
  const rightOffset = Number(track.offsets[end] ?? leftOffset);
  const left = Number(track.values[leftOffset] ?? fallback ?? 0); const right = Number(track.values[rightOffset] ?? left);
  return left + (right - left) * factor;
}

function samplePreparedEmitterVectorInto(
  track: PreparedEmitterTrack | undefined,
  time: number,
  fallback: ArrayLike<number>,
  result: Float32Array,
  interval: Float64Array,
): Float32Array {
  if (!track) { result.set(fallback); return result; }
  sampleIntervalInto(track.times, time, interval, track.bezier);
  const start = interval[0] ?? 0;
  const end = interval[1] ?? start;
  const factor = interval[2] ?? 0;
  const leftOffset = Number(track.offsets[start] ?? 0);
  const rightOffset = Number(track.offsets[end] ?? leftOffset);
  for (let index = 0; index < fallback.length; index += 1) {
    const left = Number(track.values[leftOffset + index] ?? fallback[index]); const right = Number(track.values[rightOffset + index] ?? left);
    result[index] = left + (right - left) * factor;
  }
  return result;
}

function preparedEmitterTrack(
  binary: Uint8Array,
  nodeTrack: PacketNodeAnimationTrack | undefined,
  name: string,
): PreparedEmitterTrack | undefined {
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

function sampleIntervalInto(
  times: ArrayLike<number>,
  time: number,
  result: Float64Array,
  bezier = false,
): Float64Array {
  if (!times.length || times.length === 1 || time <= (times[0] ?? 0)) { result[0]=0; result[1]=0; result[2]=0; return result; }
  const last = times.length - 1; if (time >= (times[last] ?? 0)) { result[0]=last; result[1]=last; result[2]=0; return result; }
  let low = 1; let high = last;
  while (low < high) { const middle = (low + high) >>> 1; if (time <= (times[middle] ?? 0)) high = middle; else low = middle + 1; }
  const end = low; const start = end - 1;
  const startTime = times[start] ?? 0;
  const endTime = times[end] ?? startTime;
  const factor=Math.max(0,Math.min(1,(time-startTime)/Math.max(Number.EPSILON,endTime-startTime)));
  result[0]=start; result[1]=end; result[2]=bezier?cubicBezierFactor(factor):factor; return result;
}

function random01(index: number, stream: number): number {
  const value = Math.sin((index + 1) * 12.9898 + (stream + 1) * 78.233) * 43758.5453123; return value - Math.floor(value);
}

function stagedValue3(age: number, midpoint: number, start: number, middle: number, end: number): number {
  if (age <= midpoint) { const factor = age / midpoint; return start + (middle - start) * factor; }
  const factor = (age - midpoint) / (1 - midpoint); return middle + (end - middle) * factor;
}

function emitterCurve(age: number, midpoint: number, start: number, middle: number, end: number, hasMiddle: boolean): number {
  return hasMiddle
    ? stagedValue3(age, midpoint, start, middle, end)
    : start + (end - start) * age;
}

function buildLinkedParticleVertices(
  particles: Float32Array,
  particleCount: number,
  eye: readonly number[],
  state: RibbonParticleBuffer | undefined,
): RibbonParticleBuffer {
  const segmentCount = Math.max(0, particleCount - 1); const required = segmentCount * 6 * 9;
  const output = state || { values: new Float32Array(Math.max(required, 54)), vertexCount: 0 };
  if (output.values.length < required) {
    output.values = new Float32Array(Math.max(required, Math.ceil(output.values.length * 1.5)));
  }
  let vertex = 0;
  for (let segment = 0; segment < segmentCount; segment += 1) {
    const start = segment * 15; const end = start + 15;
    const value = (offset: number): number => particles[offset] ?? 0;
    const dx=value(end)-value(start),dy=value(end+1)-value(start+1),dz=value(end+2)-value(start+2);
    const mx=(value(start)+value(end))*0.5,my=(value(start+1)+value(end+1))*0.5,mz=(value(start+2)+value(end+2))*0.5;
    const vx=(eye[0] ?? 0)-mx,vy=(eye[1] ?? 0)-my,vz=(eye[2] ?? 0)-mz;
    let sx=dy*vz-dz*vy,sy=dz*vx-dx*vz,sz=dx*vy-dy*vx; let sideLength=Math.hypot(sx,sy,sz);
    if (sideLength < 1e-6) { sx=-dy;sy=dx;sz=0;sideLength=Math.hypot(sx,sy); }
    if (sideLength < 1e-6) { sx=1;sy=0;sz=0;sideLength=1; }
    sx/=sideLength;sy/=sideLength;sz/=sideLength;
    const startWidth=Math.max(0.001,value(start+4)),endWidth=Math.max(0.001,value(end+4));
    const s0x=value(start)-sx*startWidth,s0y=value(start+1)-sy*startWidth,s0z=value(start+2)-sz*startWidth;
    const s1x=value(start)+sx*startWidth,s1y=value(start+1)+sy*startWidth,s1z=value(start+2)+sz*startWidth;
    const e0x=value(end)-sx*endWidth,e0y=value(end+1)-sy*endWidth,e0z=value(end+2)-sz*endWidth;
    const e1x=value(end)+sx*endWidth,e1y=value(end+1)+sy*endWidth,e1z=value(end+2)+sz*endWidth;
    const u0=value(start+10),v0=value(start+11),u1=u0+value(start+12),v1=v0+value(start+13);
    vertex=writeRibbonVertex(output.values,vertex,s0x,s0y,s0z,u0,v0,particles,start);
    vertex=writeRibbonVertex(output.values,vertex,s1x,s1y,s1z,u1,v0,particles,start);
    vertex=writeRibbonVertex(output.values,vertex,e0x,e0y,e0z,u0,v1,particles,end);
    vertex=writeRibbonVertex(output.values,vertex,e0x,e0y,e0z,u0,v1,particles,end);
    vertex=writeRibbonVertex(output.values,vertex,s1x,s1y,s1z,u1,v0,particles,start);
    vertex=writeRibbonVertex(output.values,vertex,e1x,e1y,e1z,u1,v1,particles,end);
  }
  output.vertexCount = vertex;
  return output;
}

function writeRibbonVertex(
  output: Float32Array,
  vertex: number,
  x: number,
  y: number,
  z: number,
  u: number,
  v: number,
  particle: Float32Array,
  particleOffset: number,
): number {
  const offset=vertex*9; output[offset]=x;output[offset+1]=y;output[offset+2]=z;output[offset+3]=u;output[offset+4]=v;
  output[offset+5]=particle[particleOffset+7] ?? 0;output[offset+6]=particle[particleOffset+8] ?? 0;output[offset+7]=particle[particleOffset+9] ?? 0;output[offset+8]=particle[particleOffset+6] ?? 0;
  return vertex+1;
}

function createTexture(
  gl: WebGL2RenderingContext,
  texture: PacketTexture | DecodedTexturePacket['manifest'],
  binary: Uint8Array,
  s3tc: S3tcExtension | null,
): WebGLTexture | null {
  const handle = gl.createTexture(); gl.bindTexture(gl.TEXTURE_2D, handle);
  const compressedLevels = 'mipLevels' in texture ? texture.mipLevels : [];
  if (texture.compression && compressedLevels.length > 0 && s3tc) {
    const format = texture.compression === 'dxt1'
      ? s3tc.COMPRESSED_RGBA_S3TC_DXT1_EXT
      : texture.compression === 'dxt5'
        ? s3tc.COMPRESSED_RGBA_S3TC_DXT5_EXT
        : undefined;
    if (format === undefined) throw new Error(`Unsupported compressed texture format ${texture.compression}.`);
    for (let level = 0; level < compressedLevels.length; level += 1) {
      const mip = compressedLevels[level];
      if (!mip) continue;
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

function bindMaterialTexture(
  gl: WebGL2RenderingContext,
  samplerLocation: WebGLUniformLocation | null,
  enabledLocation: WebGLUniformLocation | null,
  texture: MaterialTextureRuntime | undefined,
  unit: number,
): void {
  gl.uniform1i(enabledLocation, texture?.handle ? 1 : 0);
  if (!texture?.handle) return;
  gl.activeTexture(gl.TEXTURE0 + unit); gl.bindTexture(gl.TEXTURE_2D, texture.handle);
  const clamp = directiveValue(texture.binding, 'clamp') === '1'; const nearest = directiveValue(texture.binding, 'filter')?.toLowerCase() === 'nearest';
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, clamp ? gl.CLAMP_TO_EDGE : gl.REPEAT); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, clamp ? gl.CLAMP_TO_EDGE : gl.REPEAT);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, nearest ? gl.NEAREST : gl.LINEAR); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, nearest ? gl.NEAREST_MIPMAP_NEAREST : gl.LINEAR_MIPMAP_LINEAR);
  gl.uniform1i(samplerLocation, unit);
}

function directiveValue(
  binding: SceneTextureBinding | undefined,
  name: string,
): string | undefined {
  if (!binding) return undefined;
  let directives = DIRECTIVE_CACHE.get(binding);
  if (!directives) {
    directives = new Map((binding.directives || []).map((directive) => [directive.name.toLowerCase(), directive.arguments || []]));
    DIRECTIVE_CACHE.set(binding, directives);
  }
  return directives.get(name.toLowerCase())?.[0];
}

function textureUvTransform(
  binding: SceneTextureBinding | undefined,
  time: number,
  output: Float32Array = new Float32Array(4),
): Float32Array {
  const procedure = directiveValue(binding, 'proceduretype')?.toLowerCase();
  if (procedure !== 'cycle') { output[0]=1; output[1]=1; output[2]=0; output[3]=0; return output; }
  const x = Math.max(1, Number(directiveValue(binding, 'numx')) || 1); const y = Math.max(1, Number(directiveValue(binding, 'numy')) || 1); const fps = Math.max(0, Number(directiveValue(binding, 'fps')) || 1); const frame = Math.floor(time * fps) % (x * y);
  output[0]=1/x; output[1]=1/y; output[2]=(frame%x)/x; output[3]=Math.floor(frame/x)/y; return output;
}

function applyBlendMode(
  gl: WebGL2RenderingContext,
  binding: SceneTextureBinding | undefined,
): void {
  const blending = directiveValue(binding, 'blending')?.toLowerCase(); gl.enable(gl.BLEND);
  if (blending === 'additive') gl.blendFunc(gl.SRC_ALPHA, gl.ONE);
  else gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);
}

function createModelRuntime(model: PacketModel): ModelRuntime {
  const nodeByName = new Map(model.nodes.map((node, index) => [node.name.toLowerCase(), index]));
  const nodes = model.nodes.map(clonePoseNode);
  const materials: MaterialRuntime[] = model.materials.map((_material, materialIndex) => {
    const resolved = model.resolvedMaterials.find((entry) => entry.materialIndex === materialIndex);
    const textures = (resolved?.textures || []).flatMap((entry) => entry.texture === null
      ? []
      : [[entry.role, {
        binding: entry,
        texture: entry.texture,
        handle: undefined,
        uvTransform: new Float32Array([1, 1, 0, 0]),
      }] as const]);
    return {
      textures: new Map<string, MaterialTextureRuntime>(textures),
    };
  });
  const materialsByNode: number[][] = Array.from({ length: model.nodes.length }, () => []);
  model.materials.forEach((material, index) => {
    const nodeMaterials = material.sourceNode == null
      ? undefined
      : materialsByNode[material.sourceNode];
    if (nodeMaterials) nodeMaterials.push(index);
  });
  const pose = {
    nodes,
    materials: model.materials.map(() => ({ active: false, alpha: undefined, selfIllumColor: new Float32Array(3) })),
    worlds: model.nodes.map(() => identity4()),
  };
  const runtime: ModelRuntime = {
    nodeByName,
    bindWorlds: resolveNodeWorlds(model, nodes),
    hiddenNodes: new Set(model.hiddenGeometryNodes
      .map((name) => nodeByName.get(name.toLowerCase()))
      .filter((index): index is number => index !== undefined)),
    materials,
    materialsByNode,
    nodeTextures: new Map<string, NodeTextureRuntime>(
      model.nodeTextures.flatMap((entry) => entry.texture === null
        ? []
        : [[`${entry.nodeIndex}:${entry.role}`, {
          ...entry,
          texture: entry.texture,
        }] as const]),
    ),
    attachmentTargets: new Map(model.attachments.map((attachment) => [attachment, nodeByName.get(attachment.targetNodeName.toLowerCase()) ?? -1])),
    animationAssets: new Map<number, InstalledAnimationAsset>(),
    emitterBuffers: new Array(model.nodes.length),
    emitterLinkedBuffers: new Array(model.nodes.length),
    emitterColors: model.nodes.map(() => [new Float32Array(3), new Float32Array(3), new Float32Array(3)]),
    emitterIntervals: model.nodes.map(() => new Float64Array(3)),
    emitterTransitionVectors: model.nodes.map(() => new Float32Array(3)),
    emitterTransitionIntervals: model.nodes.map(() => new Float64Array(3)),
    flareBuffer: new Float32Array(15),
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
    inverseBindWorlds: [],
    chunkBatch: {
      buffer: null,
      values: new Float32Array(0),
      count: 0,
      gpuCapacity: 0,
    },
  };
  runtime.inverseBindWorlds = runtime.bindWorlds.map((world) => inverse4(world));
  return runtime;
}

function clonePoseNode(node: PacketNode | PoseNode): PoseNode {
  return {
    ...node,
    translation: Float32Array.from(node.translation),
    rotationAxisAngle: Float32Array.from(node.rotationAxisAngle),
    scale: Float32Array.from(node.scale),
    color: Float32Array.from(node.color || [1, 1, 1]),
    light: node.light ? { ...node.light } : undefined,
  };
}

function clonePose(pose: ModelPose): ModelPose {
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

function animationAssetKey(modelIndex: number, animationIndex: number): string {
  return `${modelIndex}:${animationIndex}`;
}

function createAnimationAsset(
  scene: DecodedScenePacket,
  modelIndex: number,
  animationIndex: number,
  animation: PacketAnimation,
  binary: Uint8Array,
): AnimationAsset {
  if (!(binary instanceof Uint8Array)) throw new Error(`Animation asset ${modelIndex}:${animationIndex} has no binary payload.`);
  return { sceneKey: scene.manifest.assetKey, modelIndex, animationIndex, animation, binary };
}

function installAnimationAsset(
  runtime: ModelRuntime,
  asset: AnimationAsset,
): InstalledAnimationAsset {
  const installed: InstalledAnimationAsset = {
    ...asset,
    runtime: indexAnimationRuntime(runtime, asset.animation, asset.binary),
  };
  runtime.animationAssets.set(asset.animationIndex, installed);
  return installed;
}

function indexAnimationRuntime(
  runtime: ModelRuntime,
  animation: PacketAnimation,
  binary: Uint8Array,
): NonNullable<AnimationAsset['runtime']> {
  const tracksByNode: Array<PreparedNodeAnimationTrack | undefined> =
    new Array(runtime.pose.nodes.length);
  const tracks: PreparedNodeAnimationTrack[] = [];
  for (const track of animation.nodeTracks || []) {
    const nodeIndex = track.targetNode ?? runtime.nodeByName.get(String(track.targetName || '').toLowerCase());
    if (typeof nodeIndex !== 'number' || !Number.isInteger(nodeIndex) || nodeIndex < 0) continue;
    const bezier = new Set((track.bezierControllers || []).map((name) => String(name).toLowerCase()));
    const prepared: PreparedNodeAnimationTrack = {
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

function preparePackedTrack(
  binary: Uint8Array,
  track: PacketNodeAnimationTrack['translation'] | undefined,
  bezier = false,
) {
  return track && binary ? {
    times: numericView(binary, track.times),
    values: numericView(binary, track.values),
    width: track.values.componentsPerElement,
    bezier,
  } : undefined;
}

function prepareEmitterControllers(
  binary: Uint8Array,
  entries: PacketNodeAnimationTrack['emitterControllers'] = [],
) {
  return new Map(entries.map((entry) => [entry.controller.toLowerCase(), {
    times: numericView(binary, entry.times),
    values: numericView(binary, entry.values.values),
    offsets: numericView(binary, entry.values.rowOffsets),
    bezier: entry.bezierKeyed === true,
  }]));
}

function prepareAnimMeshTrack(
  binary: Uint8Array,
  track: PacketNodeAnimationTrack['animmesh'],
) {
  return track ? {
    ...track,
    vertexValues: numericView(binary, track.vertexSamples),
    uvValues: numericView(binary, track.uvSamples),
  } : undefined;
}

function nodeDepth(model: PacketModel, node: PacketNode | PoseNode): number {
  let depth = 0; let parent = node.parent; const visited = new Set<number>();
  while (parent != null && model.nodes[parent] && !visited.has(parent)) {
    visited.add(parent); depth += 1; parent = model.nodes[parent]?.parent ?? null;
  }
  return depth;
}

function resolveNodeWorlds(
  model: PacketModel,
  nodes: readonly (PacketNode | PoseNode)[],
): Float32Array[] {
  const result: Array<Float32Array | undefined> = new Array(nodes.length);
  const visiting = new Set<number>();
  const resolve = (index: number): Float32Array => {
    if (result[index]) return result[index];
    if (visiting.has(index)) throw new Error(`Model ${model.name} contains a node parent cycle at ${nodes[index]?.name || index}.`);
    const node = nodes[index]; if (!node) return identity4(); visiting.add(index);
    const local = multiply4(translation4(node.translation), multiply4(axisAngle4(node.rotationAxisAngle), scale4(node.scale)));
    const world = node.parent == null ? local : multiply4(resolve(node.parent), local);
    result[index] = world; visiting.delete(index); return world;
  };
  nodes.forEach((_node, index) => resolve(index));
  return result.map((world) => world || identity4());
}

function resolveNodeWorldsInto(
  runtime: ModelRuntime,
  model: PacketModel,
  nodes: readonly PoseNode[],
  worlds: Float32Array[],
): Float32Array[] {
  runtime.worldState.fill(0);
  const resolve = (index: number): Float32Array => {
    const world = worlds[index];
    if (!world) throw new Error(`Model ${model.name} has no world matrix for node ${index}.`);
    if (runtime.worldState[index] === 2) return world;
    if (runtime.worldState[index] === 1) throw new Error(`Model ${model.name} contains a node parent cycle at ${nodes[index]?.name || index}.`);
    runtime.worldState[index] = 1;
    const node = nodes[index]; const local = runtime.localMatrices[index];
    if (!node || !local) throw new Error(`Model ${model.name} has no node runtime for index ${index}.`);
    composeTransform4Into(node.translation, node.rotationAxisAngle, node.scale, local);
    if (node.parent == null) world.set(local);
    else multiply4Into(resolve(node.parent), local, world);
    runtime.worldState[index] = 2;
    return world;
  };
  for (let index = 0; index < nodes.length; index += 1) resolve(index);
  return worlds;
}

function sampleModelPoseInto(
  runtime: ModelRuntime,
  model: PacketModel,
  asset: InstalledAnimationAsset | undefined,
  time: number,
): ModelPose {
  const { nodes, materials, worlds } = runtime.pose;
  for (let index = 0; index < nodes.length; index += 1) {
    const source = model.nodes[index]; const target = nodes[index];
    if (!source || !target) continue;
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
    if (!node || !source) continue;
    samplePreparedTrackInto(track.translation, sampledTime, source.translation, node.translation);
    samplePreparedTrackInto(track.rotationAxisAngle, sampledTime, source.rotationAxisAngle, node.rotationAxisAngle, true);
    samplePreparedTrackInto(track.scale, sampledTime, source.scale, node.scale);
    samplePreparedTrackInto(track.color, sampledTime, source.color || [1, 1, 1], node.color);
    runtime.scalarScratch[0] = source.alpha ?? 1; samplePreparedTrackInto(track.alpha, sampledTime, runtime.scalarScratch, runtime.scalarScratch); node.alpha = runtime.scalarScratch[0];
    runtime.scalarScratch[0] = source.radius ?? 0; samplePreparedTrackInto(track.radius, sampledTime, runtime.scalarScratch, runtime.scalarScratch); node.radius = runtime.scalarScratch[0];
    const sourceLight = source.light;
    if (node.light && sourceLight) {
      runtime.scalarScratch[0] = sourceLight.multiplier; samplePreparedTrackInto(track.multiplier, sampledTime, runtime.scalarScratch, runtime.scalarScratch); node.light.multiplier = runtime.scalarScratch[0] ?? sourceLight.multiplier;
      runtime.scalarScratch[0] = sourceLight.shadowRadius; samplePreparedTrackInto(track.shadowRadius, sampledTime, runtime.scalarScratch, runtime.scalarScratch); node.light.shadowRadius = runtime.scalarScratch[0] ?? sourceLight.shadowRadius;
      runtime.scalarScratch[0] = sourceLight.verticalDisplacement; samplePreparedTrackInto(track.verticalDisplacement, sampledTime, runtime.scalarScratch, runtime.scalarScratch); node.light.verticalDisplacement = runtime.scalarScratch[0] ?? sourceLight.verticalDisplacement;
    }
    for (const materialIndex of runtime.materialsByNode[nodeIndex] || []) {
      const material = model.materials[materialIndex]; const state = materials[materialIndex];
      if (!material || !state) continue;
      state.active = true;
      runtime.scalarScratch[0] = material.alpha ?? 1; samplePreparedTrackInto(track.alpha, sampledTime, runtime.scalarScratch, runtime.scalarScratch); state.alpha = runtime.scalarScratch[0];
      samplePreparedTrackInto(track.selfIllumColor, sampledTime, material.selfIllumColor || ZERO_COLOR, state.selfIllumColor);
    }
  }
  resolveNodeWorldsInto(runtime, model, nodes, worlds);
  return runtime.pose;
}

function blendPoseInto(
  target: ModelPose,
  source: ModelPose,
  factor: number,
  model: PacketModel,
): ModelPose {
  const amount = Math.max(0, Math.min(1, factor));
  for (let nodeIndex = 0; nodeIndex < target.nodes.length; nodeIndex += 1) {
    const to = target.nodes[nodeIndex]; const from = source.nodes[nodeIndex]; if (!to || !from) continue;
    lerpArrayInto(from.translation, to.translation, amount, to.translation);
    slerpAxisAngleValuesInto(from.rotationAxisAngle, to.rotationAxisAngle, amount, to.rotationAxisAngle);
    lerpArrayInto(from.scale, to.scale, amount, to.scale);
    lerpArrayInto(from.color, to.color, amount, to.color);
    to.alpha = lerpOptionalNumber(from.alpha, to.alpha, amount) ?? null;
    to.radius = lerpOptionalNumber(from.radius, to.radius, amount) ?? null;
    if (to.light && from.light) {
      to.light.multiplier = lerpNumber(from.light.multiplier, to.light.multiplier, amount);
      to.light.shadowRadius = lerpNumber(from.light.shadowRadius, to.light.shadowRadius, amount);
      to.light.verticalDisplacement = lerpNumber(from.light.verticalDisplacement, to.light.verticalDisplacement, amount);
    }
  }
  for (let materialIndex = 0; materialIndex < target.materials.length; materialIndex += 1) {
    const to = target.materials[materialIndex]; const from = source.materials[materialIndex]; if (!to || !from) continue;
    const material = model.materials[materialIndex]; const baseAlpha = material?.alpha ?? 1; const baseSelfIllum = material?.selfIllumColor || ZERO_COLOR;
    const fromAlpha = from.active ? from.alpha : baseAlpha; const toAlpha = to.active ? to.alpha : baseAlpha;
    const fromSelfIllum = from.active ? from.selfIllumColor : baseSelfIllum; const toSelfIllum = to.active ? to.selfIllumColor : baseSelfIllum;
    to.active = true; to.alpha = lerpNumber(fromAlpha, toAlpha, amount);
    lerpArrayInto(fromSelfIllum, toSelfIllum, amount, to.selfIllumColor);
  }
  return target;
}

function lerpNumber(from: number | undefined, to: number | undefined, factor: number): number {
  const left = typeof from === 'number' && Number.isFinite(from)
    ? from
    : typeof to === 'number' && Number.isFinite(to)
      ? to
      : 0;
  const right = typeof to === 'number' && Number.isFinite(to) ? to : left;
  return left + (right - left) * factor;
}

function lerpOptionalNumber(
  from: number | null | undefined,
  to: number | null | undefined,
  factor: number,
): number | undefined {
  const left = typeof from === 'number' && Number.isFinite(from) ? from : undefined;
  const right = typeof to === 'number' && Number.isFinite(to) ? to : undefined;
  if (left === undefined && right === undefined) return undefined;
  return lerpNumber(left, right, factor);
}

function lerpArrayInto(
  from: ArrayLike<number> | undefined,
  to: ArrayLike<number> | undefined,
  factor: number,
  output: Float32Array,
): Float32Array {
  for (let index = 0; index < output.length; index += 1) output[index] = lerpNumber(from?.[index], to?.[index], factor);
  return output;
}

function collectSceneLights(
  scene: DecodedScenePacket,
  poseForModel: (modelIndex: number) => { asset: InstalledAnimationAsset | undefined; pose: ModelPose },
  modelRuntime: readonly ModelRuntime[],
  instanceRuntime: readonly SceneInstanceRuntime[],
  target: PointLightCollection,
): PointLightCollection {
  target.count = 0;
  const append = (values: Float32Array): void => {
    const required = (target.count + 1) * 12;
    if (required > target.storage.length) {
      const grown = new Float32Array(Math.max(required, Math.ceil(target.storage.length * 1.5)));
      grown.set(target.storage); target.storage = grown;
    }
    target.storage.set(values, target.count * 12); target.count += 1;
  };
  const collectModel = (
    modelIndex: number,
    base: Float32Array,
    stack: Set<number>,
    lightOverrides: readonly (Vec3 | null)[] = [],
  ): void => {
    if (stack.has(modelIndex)) return; const model = scene.manifest.models[modelIndex]; const runtime = modelRuntime[modelIndex]; if (!model || !runtime) return; stack.add(modelIndex);
    const pose = poseForModel(modelIndex).pose; const worlds = pose.worlds; let lightIndex = 0;
    pose.nodes.forEach((node, nodeIndex) => {
      if (!node.light) return; const world = multiply4Into(base, worlds[nodeIndex] || IDENTITY_MATRIX, runtime.lightWorld); const z=node.light.verticalDisplacement||0; const multiplier = node.light.negativeLight ? -Math.abs(node.light.multiplier) : node.light.multiplier;
      const override = lightOverrideForNode(node.name, lightOverrides, lightIndex); lightIndex += 1; const color = override || node.color;
      const row=runtime.lightRow; row[0]=(world[8] ?? 0)*z+(world[12] ?? 0); row[1]=(world[9] ?? 0)*z+(world[13] ?? 0); row[2]=(world[10] ?? 0)*z+(world[14] ?? 0); row[3]=Math.max(0.01,node.radius||node.light.flareRadius||10); row[4]=color[0] ?? 1; row[5]=color[1] ?? 1; row[6]=color[2] ?? 1; row[7]=multiplier; row[8]=node.light.ambientOnly?1:0; row[9]=node.light.affectDynamic?1:0; row[10]=node.light.lightPriority||0; row[11]=0; append(row);
    });
    for (const attachment of model.attachments) { const nodeIndex=runtime.attachmentTargets.get(attachment) ?? -1; multiply4Into(base,worlds[nodeIndex]||IDENTITY_MATRIX,runtime.lightAttachment); collectModel(attachment.model,runtime.lightAttachment,new Set(stack)); }
  };
  for (const { instance, base } of instanceRuntime) if (instance.model != null && instance.kind !== 'collision' && instance.kind !== 'skybox') collectModel(instance.model,base,new Set(),instance.lightColorOverrides);
  if (target.count === 0) target.values.fill(0);
  else if (target.values.buffer !== target.storage.buffer || target.values.length !== target.count*12) target.values=target.storage.subarray(0,target.count*12);
  return target;
}

function lightOverrideForNode(
  name: string,
  overrides: readonly (Vec3 | null)[],
  fallbackIndex: number,
): Vec3 | null | undefined {
  const normalized = String(name || '').toLowerCase().replaceAll('_', '');
  const named = normalized.includes('mainlight1') ? 0 : normalized.includes('mainlight2') ? 1 : normalized.includes('sourcelight1') ? 2 : normalized.includes('sourcelight2') ? 3 : fallbackIndex;
  return overrides?.[named] || undefined;
}

function samplePreparedTrackInto(
  track: PreparedTrack | undefined,
  time: number,
  fallback: ArrayLike<number>,
  output: Float32Array,
  rotation = false,
): Float32Array {
  if (!track?.times.length || !track.values.length) { output.set(fallback); return output; }
  const { times, values, width } = track;
  let start = 0; let end = 0; let factor = 0;
  if (times.length === 1 || time <= Number(times[0] ?? 0)) end = 0;
  else if (time >= Number(times[times.length - 1] ?? 0)) { start = times.length - 1; end = start; }
  else {
    let low = 1; let high = times.length - 1;
    while (low < high) { const middle = (low + high) >>> 1; if (time <= Number(times[middle] ?? 0)) high = middle; else low = middle + 1; }
    end = low; start = end - 1;
    const startTime = Number(times[start] ?? 0);
    const endTime = Number(times[end] ?? startTime);
    factor = Math.max(0, Math.min(1, (time - startTime) / Math.max(Number.EPSILON, endTime - startTime)));
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

function cubicBezierFactor(factor: number): number {
  const clamped = Math.max(0, Math.min(1, factor));
  return clamped * clamped * (3 - 2 * clamped);
}

function slerpAxisAngleInto(
  values: ArrayLike<number>,
  leftOffset: number,
  rightOffset: number,
  factor: number,
  output: Float32Array,
): Float32Array {
  return slerpAxisAngleRawInto(
    values[leftOffset], values[leftOffset+1], values[leftOffset+2], values[leftOffset+3],
    values[rightOffset], values[rightOffset+1], values[rightOffset+2], values[rightOffset+3],
    factor, output,
  );
}

function slerpAxisAngleValuesInto(
  left: ArrayLike<number>,
  right: ArrayLike<number>,
  factor: number,
  output: Float32Array,
): Float32Array {
  return slerpAxisAngleRawInto(
    left[0], left[1], left[2], left[3], right[0], right[1], right[2], right[3], factor, output,
  );
}

function slerpAxisAngleRawInto(
  lax: number | undefined,
  lay: number | undefined,
  laz: number | undefined,
  la: number | undefined,
  rax: number | undefined,
  ray: number | undefined,
  raz: number | undefined,
  ra: number | undefined,
  factor: number,
  output: Float32Array,
): Float32Array {
  lax ??= 0; lay ??= 1; laz ??= 0; la ??= 0;
  rax ??= 0; ray ??= 1; raz ??= 0; ra ??= 0;
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

function updateBoneTexture(
  gl: WebGL2RenderingContext,
  gpu: PrimitiveGpu,
  inverseBindWorlds: readonly Float32Array[],
  posedWorlds: readonly Float32Array[],
  meshBindWorld: Float32Array,
  meshWorld: Float32Array,
): void {
  inverse4Into(meshWorld, gpu.meshInverse); const matrices = gpu.boneMatrices;
  for (let index = 0; index < Math.max(1, gpu.boneNodes.length); index += 1) {
    const node = gpu.boneNodes[index] ?? -1;
    const inverseBind = node >= 0 ? inverseBindWorlds[node] : undefined;
    const posedWorld = node >= 0 ? posedWorlds[node] : undefined;
    if (inverseBind && posedWorld) {
      multiply4Into(inverseBind, meshBindWorld, gpu.boneScratchA);
      multiply4Into(posedWorld, gpu.boneScratchA, gpu.boneScratchB);
      multiply4Into(gpu.meshInverse, gpu.boneScratchB, gpu.boneScratchA);
      matrices.set(gpu.boneScratchA, index * 16);
    } else matrices.set(IDENTITY_MATRIX, index * 16);
  }
  gl.activeTexture(gl.TEXTURE5); gl.bindTexture(gl.TEXTURE_2D, gpu.boneTexture);
  gl.texSubImage2D(gl.TEXTURE_2D, 0, 0, 0, 4, Math.max(1, gpu.boneNodes.length), gl.RGBA, gl.FLOAT, matrices);
}

function updatePreparedAnimMesh(
  gpu: PrimitiveGpu,
  targetTrack: PreparedAnimMeshTrack | undefined,
  targetTime: number,
  targetLength: number,
  sourceTrack: PreparedAnimMeshTrack | undefined,
  sourceTime: number,
  sourceLength: number,
  factor: number,
): Float32Array {
  if (!targetTrack && !sourceTrack) return gpu.vertices;
  const targetPositions = samplePreparedAnimMeshValues(gpu, targetTrack, targetTime, targetLength, 'target', 'position');
  const targetUvs = samplePreparedAnimMeshValues(gpu, targetTrack, targetTime, targetLength, 'target', 'uv');
  const sourcePositions = samplePreparedAnimMeshValues(gpu, sourceTrack, sourceTime, sourceLength, 'source', 'position');
  const sourceUvs = samplePreparedAnimMeshValues(gpu, sourceTrack, sourceTime, sourceLength, 'source', 'uv');
  gpu.dynamicVertices = ensureFloatCapacity(gpu.dynamicVertices, gpu.vertices.length);
  const output = gpu.dynamicVertices; output.set(gpu.vertices); const amount = Math.max(0, Math.min(1, factor));
  for (let corner = 0; corner < gpu.indices.length; corner += 1) {
    const vertex = Number(gpu.indices[corner] ?? 0);
    const uv = Number(gpu.uvIndices[corner] ?? vertex);
    const base = corner * gpu.stride;
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

function samplePreparedAnimMeshValues(
  gpu: PrimitiveGpu,
  track: PreparedAnimMeshTrack | undefined,
  time: number,
  animationLength: number,
  side: 'target' | 'source',
  channel: 'position' | 'uv',
): Float32Array | undefined {
  if (!track) return undefined;
  const positions = channel === 'position'; const width = positions ? 3 : 2;
  const frameCount = positions ? track.vertexFrameCount : track.uvFrameCount;
  const perFrame = positions ? track.verticesPerFrame : track.uvsPerFrame;
  const values = positions ? track.vertexValues : track.uvValues;
  let output: Float32Array;
  if (side === 'target' && positions) {
    gpu.targetAnimPositions = ensureFloatCapacity(gpu.targetAnimPositions, perFrame * width);
    output = gpu.targetAnimPositions;
  } else if (side === 'target') {
    gpu.targetAnimUvs = ensureFloatCapacity(gpu.targetAnimUvs, perFrame * width);
    output = gpu.targetAnimUvs;
  } else if (positions) {
    gpu.sourceAnimPositions = ensureFloatCapacity(gpu.sourceAnimPositions, perFrame * width);
    output = gpu.sourceAnimPositions;
  } else {
    gpu.sourceAnimUvs = ensureFloatCapacity(gpu.sourceAnimUvs, perFrame * width);
    output = gpu.sourceAnimUvs;
  }
  return sampleAnimMeshValuesInto(frameCount, perFrame, values, track.samplePeriod, animationLength, time, width, output);
}

function updateDynamicMesh(
  gl: WebGL2RenderingContext,
  gpu: PrimitiveGpu,
  input: Float32Array,
  dangly: PacketNode['dangly'],
  time: number,
  windPower: number,
): void {
  let output = input;
  if (dangly && gpu.vertexConstraints.some((value) => value > 0)) {
    output = gpu.danglyVertices; output.set(input); const period = Math.max(0.01, dangly.period || 1); const tightness = Math.max(0, Math.min(1, dangly.tightness || 0)); const wind = 1 + Math.max(0, windPower) / 10;
    for (let vertex = 0; vertex < gpu.count; vertex += 1) {
      const constraint = Math.max(0, gpu.vertexConstraints[vertex] || 0); if (!constraint) continue;
      const phase = time * Math.PI * 2 / period + vertex * 0.173; const amplitude = dangly.displacement * constraint * (1 - tightness) * wind;
      const base = vertex * gpu.stride;
      output[base] = (output[base] ?? 0) + Math.sin(phase) * amplitude;
      output[base + 1] = (output[base + 1] ?? 0) + Math.cos(phase * 0.73) * amplitude * 0.5;
      output[base + 2] = (output[base + 2] ?? 0) - Math.abs(Math.sin(phase * 0.5)) * amplitude * 0.25;
    }
  }
  if (output === gpu.vertices && !gpu.dynamicActive) return;
  gl.bindBuffer(gl.ARRAY_BUFFER, gpu.buffer); gl.bufferSubData(gl.ARRAY_BUFFER, 0, output); gpu.dynamicActive = output !== gpu.vertices;
}

function ensureFloatCapacity(value: Float32Array | undefined, length: number): Float32Array {
  return value !== undefined && value.length >= length ? value : new Float32Array(length);
}

function sampleAnimMeshValuesInto(
  frameCount: number,
  perFrame: number,
  values: NumericArray,
  period: number | null,
  length: number,
  time: number,
  width: number,
  result: Float32Array,
): Float32Array | undefined {
  if (!frameCount || !perFrame || !values.length) return undefined;
  if (frameCount === 1) {
    const valueCount = perFrame * width;
    for (let index = 0; index < valueCount; index += 1) result[index] = values[index] ?? 0;
    return result;
  }
  const samplePeriod = period !== null && period > Number.EPSILON ? period : Math.max(Number.EPSILON, length / frameCount); const phase = length > 0 ? ((time % length) + length) % length : Math.max(0, time); const cycle = samplePeriod * frameCount; const position = (phase % cycle) / samplePeriod; const current = Math.min(frameCount - 1, Math.floor(position)); const next = (current + 1) % frameCount; const factor = position - current;
  const valueCount = perFrame * width; const startOffset = current * valueCount; const endOffset = next * valueCount; for (let index = 0; index < valueCount; index += 1) {
    const startValue = Number(values[startOffset + index] ?? 0);
    const endValue = Number(values[endOffset + index] ?? startValue);
    result[index] = startValue + (endValue - startValue) * factor;
  }
  return result;
}

function globalIllumination(environment: NwnEnvironment | undefined): Illumination {
  const source = environment || {}; const night = source.isNight === true;
  const ambient = packedColor(night ? source.moonAmbientColor : source.sunAmbientColor, [1, 1, 1]);
  const diffuse = packedColor(night ? source.moonDiffuseColor : source.sunDiffuseColor, [1, 1, 1]);
  const fog = packedColor(night ? source.moonFogColor : source.sunFogColor, ambient);
  const mixed = ambient.map((value, index) => value * 0.4 + (diffuse[index] ?? 1) * 0.6);
  const luminance = (mixed[0] ?? 0) * 0.2126 + (mixed[1] ?? 0) * 0.7152 + (mixed[2] ?? 0) * 0.0722;
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

function faceNormal(positions: NumericView, ai: number, bi: number, ci: number): [number, number, number] {
  const a: [number, number, number] = [positions[ai * 3] || 0, positions[ai * 3 + 1] || 0, positions[ai * 3 + 2] || 0];
  const b: [number, number, number] = [positions[bi * 3] || 0, positions[bi * 3 + 1] || 0, positions[bi * 3 + 2] || 0];
  const c: [number, number, number] = [positions[ci * 3] || 0, positions[ci * 3 + 1] || 0, positions[ci * 3 + 2] || 0];
  const ab: MutableVec3 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
  const ac: MutableVec3 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
  const normal: MutableVec3 = [
    ab[1] * ac[2] - ab[2] * ac[1],
    ab[2] * ac[0] - ab[0] * ac[2],
    ab[0] * ac[1] - ab[1] * ac[0],
  ];
  const length = Math.hypot(...normal) || 1; return [normal[0] / length, normal[1] / length, normal[2] / length];
}

function surfaceColor(materialIndex: number): [number, number, number] {
  // The engine's walkmesh material ids are stable even though WOK/DWK/PWK files do not
  // carry display colors. Keep the palette here so collision views remain deterministic
  // and distinct without inventing data in the renderer-neutral scene representation.
  const palette: ReadonlyArray<[number, number, number]> = [
    [0.52, 0.36, 0.20], [0.35, 0.35, 0.35], [0.24, 0.62, 0.24], [0.58, 0.58, 0.58],
    [0.56, 0.38, 0.19], [0.18, 0.45, 0.82], [0.88, 0.20, 0.20], [0.78, 0.78, 0.86],
    [0.62, 0.22, 0.62], [0.62, 0.68, 0.74], [0.18, 0.66, 0.70], [0.23, 0.44, 0.25],
    [0.42, 0.28, 0.17], [0.46, 0.62, 0.20], [0.94, 0.36, 0.08], [0.12, 0.12, 0.16],
  ];
  const index = Number.isInteger(materialIndex) ? Math.abs(materialIndex) % palette.length : 0;
  return palette[index] || [0.52, 0.36, 0.20];
}

function bindViewportControls(
  canvas: HTMLCanvasElement,
  camera: ViewerCamera,
  draw: () => void,
  changed: () => void = () => {},
  clicked: (event: PointerEvent) => void = () => {},
): ViewportControls {
  let drag: {
    x: number;
    y: number;
    startX: number;
    startY: number;
    button: number;
    moved: boolean;
  } | undefined;
  const pressed = new Set<string>();
  let fastMovement = false;
  const pointerdown = (event: PointerEvent): void => {
    canvas.focus?.();
    drag = {
      x: event.clientX,
      y: event.clientY,
      startX: event.clientX,
      startY: event.clientY,
      button: event.button,
      moved: false,
    };
    canvas.setPointerCapture(event.pointerId);
  };
  const pointermove = (event: PointerEvent): void => {
    if (!drag) return; const dx = event.clientX - drag.x; const dy = event.clientY - drag.y; drag.x = event.clientX; drag.y = event.clientY;
    if (Math.hypot(event.clientX - drag.startX, event.clientY - drag.startY) > 3) drag.moved = true;
    if (drag.button === 0) { camera.yaw -= dx * 0.008; camera.pitch = Math.max(-1.5, Math.min(1.5, camera.pitch + dy * 0.008)); }
    else { const scale = camera.distance * 0.002; camera.target[0] = (camera.target[0] ?? 0) - dx * scale; camera.target[2] = (camera.target[2] ?? 0) + dy * scale; }
    changed(); draw();
  };
  const pointerup = (event: PointerEvent): void => {
    if (drag?.button === 0 && !drag.moved) clicked(event);
    drag = undefined;
  };
  const pointercancel = (): void => { drag = undefined; };
  const contextmenu = (event: MouseEvent): void => event.preventDefault();
  const wheel = (event: WheelEvent): void => { event.preventDefault(); camera.distance = Math.max(0.1, camera.distance * Math.exp(event.deltaY * 0.001)); changed(); draw(); };
  const keydown = (event: KeyboardEvent): void => {
    const key = String(event.key).toLowerCase();
    if (key === 'shift') { fastMovement = true; return; }
    if (['w', 'a', 's', 'd', 'q', 'e'].includes(key)) {
      event.preventDefault(); pressed.add(key); return;
    }
    const step = event.shiftKey ? 0.2 : 0.06; let handled = true;
    if (event.key === 'ArrowLeft') camera.yaw += step; else if (event.key === 'ArrowRight') camera.yaw -= step; else if (event.key === 'ArrowUp') camera.pitch = Math.min(1.5, camera.pitch + step); else if (event.key === 'ArrowDown') camera.pitch = Math.max(-1.5, camera.pitch - step); else if (event.key === '+' || event.key === '=') camera.distance = Math.max(0.1, camera.distance * 0.9); else if (event.key === '-') camera.distance *= 1.1; else handled = false;
    if (handled) { event.preventDefault(); changed(); draw(); }
  };
  const keyup = (event: KeyboardEvent): void => {
    const key = String(event.key).toLowerCase();
    if (key === 'shift') fastMovement = false;
    if (pressed.delete(key)) { event.preventDefault(); changed(); }
  };
  const blur = (): void => {
    if (pressed.size) changed();
    pressed.clear(); fastMovement = false; drag = undefined;
  };
  canvas.addEventListener('pointerdown', pointerdown);
  canvas.addEventListener('pointermove', pointermove);
  canvas.addEventListener('pointerup', pointerup);
  canvas.addEventListener('pointercancel', pointercancel);
  canvas.addEventListener('contextmenu', contextmenu);
  canvas.addEventListener('wheel', wheel, { passive: false });
  canvas.addEventListener('keydown', keydown);
  canvas.addEventListener('keyup', keyup);
  canvas.addEventListener('blur', blur);
  return {
    update(deltaSeconds: number): boolean {
      if (!pressed.size || !(deltaSeconds > 0)) return false;
      const eye = orbitEye(camera);
      const forward = normalize3([
        camera.target[0] - eye[0],
        camera.target[1] - eye[1],
        camera.target[2] - eye[2],
      ]);
      let right = normalize3([forward[1], -forward[0], 0]);
      if (Math.hypot(right[0], right[1]) < 1e-6) right = [-Math.sin(camera.yaw), Math.cos(camera.yaw), 0];
      const direction: [number, number, number] = [0, 0, 0];
      const apply = (vector: readonly number[], amount: number): void => {
        for (let axis = 0; axis < 3; axis += 1) {
          direction[axis] = (direction[axis] ?? 0) + (vector[axis] ?? 0) * amount;
        }
      };
      if (pressed.has('w')) apply(forward, 1);
      if (pressed.has('s')) apply(forward, -1);
      if (pressed.has('d')) apply(right, 1);
      if (pressed.has('a')) apply(right, -1);
      if (pressed.has('e')) direction[2] += 1;
      if (pressed.has('q')) direction[2] -= 1;
      const magnitude = Math.hypot(...direction);
      if (magnitude < 1e-6) return false;
      const speed = Math.max(1, camera.distance * 0.75) * (fastMovement ? 3 : 1);
      for (let axis = 0; axis < 3; axis += 1) {
        camera.target[axis] = (camera.target[axis] ?? 0)
          + (direction[axis] ?? 0) / magnitude * speed * deltaSeconds;
      }
      return true;
    },
    dispose(): void {
      blur();
      canvas.removeEventListener('pointerdown', pointerdown);
      canvas.removeEventListener('pointermove', pointermove);
      canvas.removeEventListener('pointerup', pointerup);
      canvas.removeEventListener('pointercancel', pointercancel);
      canvas.removeEventListener('contextmenu', contextmenu);
      canvas.removeEventListener('wheel', wheel);
      canvas.removeEventListener('keydown', keydown);
      canvas.removeEventListener('keyup', keyup);
      canvas.removeEventListener('blur', blur);
    },
  };
}

function sceneBounds(scene: DecodedScenePacket): Bounds {
  return sceneBoundsCatalog(scene).scene;
}

function sceneBoundsCatalog(scene: DecodedScenePacket): BoundsCatalog {
  const sceneAccumulator = newBoundsAccumulator();
  const objectLocalAccumulators = new Map<string, Bounds>();
  const objectBases = new Map<string, Float32Array>((scene.manifest.areaObjects || []).map((object) => [
    object.key,
    composeTransform4(object.position || [0, 0, 0], object.rotationAxisAngle || [0, 0, 1, 0], [1, 1, 1]),
  ]));
  const componentLocalAccumulators = new Map<number, Bounds>();
  const componentBases = new Map<number, Float32Array>();
  const includeModel = (
    modelIndex: number,
    base: Float32Array,
    stack: Set<number>,
    include: (point: readonly number[]) => void,
  ): void => {
    if (stack.has(modelIndex)) return; const model = scene.manifest.models[modelIndex]; if (!model) return; stack.add(modelIndex);
    const worlds = resolveNodeWorlds(model, model.nodes);
    for (const mesh of model.meshes) for (const primitive of mesh.primitives) {
      const world = multiply4(base, worlds[mesh.sourceNode] || identity4()); const positions = numericView(scene.binary, primitive.positions);
      for (let index = 0; index < positions.length; index += 3) include(transformPoint4(world, [Number(positions[index] ?? 0), Number(positions[index + 1] ?? 0), Number(positions[index + 2] ?? 0)]));
    }
    model.nodes.forEach((node, nodeIndex) => {
      if (!node.emitter) return;
      const world = multiply4(base, worlds[nodeIndex] || identity4()); const extent = emitterSpatialExtent(node.emitter);
      // Particle travel affects whole-scene framing, but it is not part of the
      // authored object's selectable geometry. Keeping it out of the logical
      // object accumulator prevents effects from inflating selection boxes and
      // changing the camera distance used when an object is selected.
      for (const x of [-extent[0], extent[0]]) for (const y of [-extent[1], extent[1]]) for (const z of [-extent[2], extent[2]]) includeBoundsPoint(sceneAccumulator, transformPoint4(world, [x, y, z]));
    });
    for (const attachment of model.attachments) { const target = model.nodes.findIndex((node) => node.name.toLowerCase() === attachment.targetNodeName.toLowerCase()); includeModel(attachment.model, multiply4(base, worlds[target] || identity4()), new Set(stack), include); }
  };
  scene.manifest.instances.forEach((instance, instanceIndex) => {
    if (instance.kind === 'skybox') return;
    const componentId = Number.isInteger(instance.id) ? instance.id : instanceIndex;
    const base = multiply4(translation4(instance.position), multiply4(axisAngle4(instance.rotationAxisAngle), scale4(instance.scale)));
    const inverseBase = inverse4(base);
    const componentAccumulator = newBoundsAccumulator();
    componentLocalAccumulators.set(componentId, componentAccumulator);
    componentBases.set(componentId, base);
    const objectBase = instance.objectKey
      ? mapGetOrInsert(objectBases, instance.objectKey, () => base)
      : undefined;
    const inverseObjectBase = objectBase ? inverse4(objectBase) : undefined;
    const objectLocalAccumulator = instance.objectKey
      ? mapGetOrInsert(objectLocalAccumulators, instance.objectKey, newBoundsAccumulator)
      : undefined;
    const include = (point: readonly number[]): void => {
      includeBoundsPoint(sceneAccumulator, point);
      includeBoundsPoint(componentAccumulator, transformPoint4(inverseBase, point));
      if (objectLocalAccumulator && inverseObjectBase) {
        includeBoundsPoint(objectLocalAccumulator, transformPoint4(inverseObjectBase, point));
      }
    };
    include(instance.position);
    instance.polygon?.forEach((point) => include(transformPoint4(base, point))); if (instance.model != null) includeModel(instance.model, base, new Set(), include);
  });
  for (const object of scene.manifest.areaObjects || []) {
    const local = mapGetOrInsert(objectLocalAccumulators, object.key, newBoundsAccumulator);
    includeBoundsPoint(local, [0, 0, 0]);
  }
  const selectionFromLocalBounds = (localAccumulator: Bounds, base: Float32Array): BoundsSelection => {
    const localBounds = paddedBounds(finalizeBounds(localAccumulator), 0.25);
    const vertices = boxLineVertices(localBounds)
      .map((point) => transformPoint4(base, point));
    const worldAccumulator = newBoundsAccumulator();
    vertices.forEach((point) => includeBoundsPoint(worldAccumulator, point));
    return { bounds: finalizeBounds(worldAccumulator), vertices };
  };
  const objectSelections = new Map([...objectLocalAccumulators].map(([key, accumulator]) => [
    key,
    selectionFromLocalBounds(accumulator, objectBases.get(key) || identity4()),
  ]));
  const componentSelections = new Map([...componentLocalAccumulators].map(([id, accumulator]) => [
    id,
    selectionFromLocalBounds(accumulator, componentBases.get(id) || identity4()),
  ]));
  const objects = new Map([...objectSelections].map(([key, selection]) => [key, selection.bounds]));
  return { scene: finalizeBounds(sceneAccumulator), objects, objectSelections, componentSelections };
}

function emitterSpatialExtent(emitter: PacketEmitter): MutableVec3 {
  const life=Math.max(0,Number(emitterProperty(emitter,'lifeexp',0))||0);
  const velocity=Math.abs(Number(emitterProperty(emitter,'velocity',0))||0)+Math.abs(Number(emitterProperty(emitter,'randvel',0))||0)*0.5;
  const mass=Math.abs(Number(emitterProperty(emitter,'mass',0))||0);
  const particleSize=Math.max(0,
    Math.abs(Number(emitterProperty(emitter,'sizestart',0))||0),
    Math.abs(Number(emitterProperty(emitter,'sizemid',0))||0),
    Math.abs(Number(emitterProperty(emitter,'sizeend',0))||0),
    Math.abs(Number(emitterProperty(emitter,'sizestart_y',0))||0),
    Math.abs(Number(emitterProperty(emitter,'sizemid_y',0))||0),
    Math.abs(Number(emitterProperty(emitter,'sizeend_y',0))||0),
  )*0.5;
  const travel=velocity*life+mass*9.81*life*life*0.5;
  return [Math.abs(emitter.xSize||0)/200+travel+particleSize,Math.abs(emitter.ySize||0)/200+travel+particleSize,travel+particleSize];
}

function newBoundsAccumulator(): Bounds {
  return { min: [Infinity, Infinity, Infinity], max: [-Infinity, -Infinity, -Infinity] };
}

function includeBoundsPoint(bounds: Bounds, point: readonly number[]): void {
  for (let index = 0; index < 3; index += 1) {
    const value = point[index] ?? 0;
    bounds.min[index] = Math.min(bounds.min[index] ?? Infinity, value);
    bounds.max[index] = Math.max(bounds.max[index] ?? -Infinity, value);
  }
}

function finalizeBounds(bounds: Bounds): Bounds {
  return Number.isFinite(bounds.min[0])
    ? { min: [...bounds.min], max: [...bounds.max] }
    : { min: [-1, -1, -1], max: [1, 1, 1] };
}

function paddedBounds(bounds: Bounds, minimumExtent: number): Bounds {
  const result: Bounds = { min: [...bounds.min], max: [...bounds.max] };
  for (let axis = 0; axis < 3; axis += 1) {
    const minimum = result.min[axis] ?? 0;
    const maximum = result.max[axis] ?? 0;
    const missing = Math.max(0, minimumExtent - (maximum - minimum));
    result.min[axis] = minimum - missing / 2;
    result.max[axis] = maximum + missing / 2;
  }
  return result;
}

function mapGetOrInsert<Key, Value>(
  map: Map<Key, Value>,
  key: Key,
  create: () => Value,
): Value {
  let value = map.get(key);
  if (!value) { value = create(); map.set(key, value); }
  return value;
}

function boxLineVertices(bounds: Bounds): MutableVec3[] {
  const [x0, y0, z0] = bounds.min; const [x1, y1, z1] = bounds.max;
  const corners: MutableVec3[] = [
    [x0, y0, z0], [x1, y0, z0], [x1, y1, z0], [x0, y1, z0],
    [x0, y0, z1], [x1, y0, z1], [x1, y1, z1], [x0, y1, z1],
  ];
  const edges: ReadonlyArray<readonly [number, number]> =
    [[0, 1], [1, 2], [2, 3], [3, 0], [4, 5], [5, 6], [6, 7], [7, 4], [0, 4], [1, 5], [2, 6], [3, 7]];
  return edges.flatMap(([start, end]) => {
    const startPoint = corners[start];
    const endPoint = corners[end];
    return startPoint && endPoint ? [startPoint, endPoint] : [];
  });
}

function replaceSelectionGpu<Key extends string | number>(
  gl: WebGL2RenderingContext,
  previous: SelectionGpu | undefined,
  selectionKey: Key,
  selections: ReadonlyMap<Key, BoundsSelection>,
): SelectionGpu | undefined {
  destroyOverlayGpu(gl, previous);
  const selection = selections.get(selectionKey);
  if (!selection) return undefined;
  const overlay = createOverlayGpu(gl, selection.vertices);
  return overlay ? { ...overlay, selectionKey } : undefined;
}

function destroyOverlayGpu(
  gl: WebGL2RenderingContext,
  gpu: OverlayGpu | undefined,
): void {
  if (!gpu) return;
  gl.deleteBuffer(gpu.buffer);
  gl.deleteVertexArray(gpu.vao);
}

function frameBounds(camera: ViewerCamera, bounds: Bounds): void {
  camera.target = [
    (bounds.min[0] + bounds.max[0]) / 2,
    (bounds.min[1] + bounds.max[1]) / 2,
    (bounds.min[2] + bounds.max[2]) / 2,
  ];
  camera.distance = Math.max(1.5, Math.hypot(
    bounds.max[0] - bounds.min[0],
    bounds.max[1] - bounds.min[1],
    bounds.max[2] - bounds.min[2],
  ) * 1.6);
}

function transformHomogeneous4(
  matrix: Float32Array,
  vector: readonly number[],
): MutableVec3 {
  const [x = 0, y = 0, z = 0, w = 1] = vector;
  const result: MutableVec4 = [
    (matrix[0] ?? 0)*x+(matrix[4] ?? 0)*y+(matrix[8] ?? 0)*z+(matrix[12] ?? 0)*w,
    (matrix[1] ?? 0)*x+(matrix[5] ?? 0)*y+(matrix[9] ?? 0)*z+(matrix[13] ?? 0)*w,
    (matrix[2] ?? 0)*x+(matrix[6] ?? 0)*y+(matrix[10] ?? 0)*z+(matrix[14] ?? 0)*w,
    (matrix[3] ?? 0)*x+(matrix[7] ?? 0)*y+(matrix[11] ?? 0)*z+(matrix[15] ?? 0)*w,
  ];
  const divisor = Math.abs(result[3]) > 1e-12 ? result[3] : 1;
  return [result[0] / divisor, result[1] / divisor, result[2] / divisor];
}

function normalize3(vector: readonly number[]): MutableVec3 {
  const length = Math.hypot(...vector) || 1;
  return [(vector[0] ?? 0) / length, (vector[1] ?? 0) / length, (vector[2] ?? 0) / length];
}

function rayBoundsDistance(
  origin: readonly number[],
  direction: readonly number[],
  bounds: Bounds,
): number | undefined {
  let near = -Infinity; let far = Infinity;
  for (let axis = 0; axis < 3; axis += 1) {
    const directionValue = direction[axis] ?? 0;
    const originValue = origin[axis] ?? 0;
    const minimum = bounds.min[axis] ?? 0;
    const maximum = bounds.max[axis] ?? 0;
    if (Math.abs(directionValue) < 1e-12) {
      if (originValue < minimum || originValue > maximum) return undefined;
      continue;
    }
    const first = (minimum - originValue) / directionValue;
    const second = (maximum - originValue) / directionValue;
    near = Math.max(near, Math.min(first, second));
    far = Math.min(far, Math.max(first, second));
    if (far < near) return undefined;
  }
  return far < 0 ? undefined : Math.max(0, near);
}

function transformPoint4(matrix: Float32Array, vector: readonly number[]): MutableVec3 {
  const [x = 0, y = 0, z = 0] = vector;
  return [
    (matrix[0] ?? 0)*x+(matrix[4] ?? 0)*y+(matrix[8] ?? 0)*z+(matrix[12] ?? 0),
    (matrix[1] ?? 0)*x+(matrix[5] ?? 0)*y+(matrix[9] ?? 0)*z+(matrix[13] ?? 0),
    (matrix[2] ?? 0)*x+(matrix[6] ?? 0)*y+(matrix[10] ?? 0)*z+(matrix[14] ?? 0),
  ];
}

function packedColor(value: number | null | undefined, fallback: readonly number[]): number[] {
  if (!Number.isInteger(value)) return [...fallback];
  const packed = value ?? 0;
  return [(packed & 255) / 255, ((packed >>> 8) & 255) / 255, ((packed >>> 16) & 255) / 255];
}

function identity4(): Float32Array { return new Float32Array([1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1]); }
function composeTransform4(
  translation: ArrayLike<number>,
  rotationAxisAngle: ArrayLike<number>,
  scale: ArrayLike<number>,
): Float32Array {
  return composeTransform4Into(translation, rotationAxisAngle, scale, new Float32Array(16));
}
function composeTransform4Into(
  translation: ArrayLike<number>,
  rotationAxisAngle: ArrayLike<number>,
  scale: ArrayLike<number>,
  output: Float32Array,
): Float32Array {
  const tx = translation[0] ?? 0; const ty = translation[1] ?? 0; const tz = translation[2] ?? 0;
  const x0 = rotationAxisAngle[0] ?? 0; const y0 = rotationAxisAngle[1] ?? 1; const z0 = rotationAxisAngle[2] ?? 0; const angle = rotationAxisAngle[3] ?? 0;
  const sx = scale[0] ?? 1; const sy = scale[1] ?? 1; const sz = scale[2] ?? 1;
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
function translation4(vector: ArrayLike<number>): Float32Array { const result = identity4(); result[12] = vector[0] ?? 0; result[13] = vector[1] ?? 0; result[14] = vector[2] ?? 0; return result; }
function scale4(vector: ArrayLike<number>): Float32Array { const result = identity4(); result[0] = vector[0] ?? 1; result[5] = vector[1] ?? 1; result[10] = vector[2] ?? 1; return result; }
function axisAngle4(vector: ArrayLike<number>): Float32Array {
  let x = vector[0] ?? 0; let y = vector[1] ?? 1; let z = vector[2] ?? 0; const angle = vector[3] ?? 0;
  const length = Math.hypot(x, y, z); if (!length || !angle) return identity4(); x /= length; y /= length; z /= length;
  const c = Math.cos(angle); const s = Math.sin(angle); const t = 1 - c;
  return new Float32Array([t*x*x+c, t*x*y+s*z, t*x*z-s*y, 0, t*x*y-s*z, t*y*y+c, t*y*z+s*x, 0, t*x*z+s*y, t*y*z-s*x, t*z*z+c, 0, 0, 0, 0, 1]);
}
function multiply4(a: Float32Array, b: Float32Array): Float32Array {
  const out = new Float32Array(16);
  return multiply4Into(a, b, out);
}
function multiply4Into(a: Float32Array, b: Float32Array, out: Float32Array): Float32Array {
  for (let column = 0; column < 4; column += 1) for (let row = 0; row < 4; row += 1) {
    out[column * 4 + row] = (a[row] ?? 0) * (b[column * 4] ?? 0) + (a[4 + row] ?? 0) * (b[column * 4 + 1] ?? 0) + (a[8 + row] ?? 0) * (b[column * 4 + 2] ?? 0) + (a[12 + row] ?? 0) * (b[column * 4 + 3] ?? 0);
  }
  return out;
}
function inverse4(matrix: Float32Array): Float32Array {
  const output = new Float32Array(16);
  return inverse4Into(matrix, output);
}
function inverse4Into(matrix: Float32Array, output: Float32Array): Float32Array {
  const a00=matrix[0] ?? 0,a01=matrix[1] ?? 0,a02=matrix[2] ?? 0,a03=matrix[3] ?? 0,a10=matrix[4] ?? 0,a11=matrix[5] ?? 0,a12=matrix[6] ?? 0,a13=matrix[7] ?? 0,a20=matrix[8] ?? 0,a21=matrix[9] ?? 0,a22=matrix[10] ?? 0,a23=matrix[11] ?? 0,a30=matrix[12] ?? 0,a31=matrix[13] ?? 0,a32=matrix[14] ?? 0,a33=matrix[15] ?? 0;
  const b00=a00*a11-a01*a10,b01=a00*a12-a02*a10,b02=a00*a13-a03*a10,b03=a01*a12-a02*a11,b04=a01*a13-a03*a11,b05=a02*a13-a03*a12,b06=a20*a31-a21*a30,b07=a20*a32-a22*a30,b08=a20*a33-a23*a30,b09=a21*a32-a22*a31,b10=a21*a33-a23*a31,b11=a22*a33-a23*a32;
  const determinant=b00*b11-b01*b10+b02*b09+b03*b08-b04*b07+b05*b06;
  if (Math.abs(determinant) < 1e-12) { output.set(IDENTITY_MATRIX); return output; } const inverse=1/determinant;
  output[0]=(a11*b11-a12*b10+a13*b09)*inverse; output[1]=(a02*b10-a01*b11-a03*b09)*inverse; output[2]=(a31*b05-a32*b04+a33*b03)*inverse; output[3]=(a22*b04-a21*b05-a23*b03)*inverse;
  output[4]=(a12*b08-a10*b11-a13*b07)*inverse; output[5]=(a00*b11-a02*b08+a03*b07)*inverse; output[6]=(a32*b02-a30*b05-a33*b01)*inverse; output[7]=(a20*b05-a22*b02+a23*b01)*inverse;
  output[8]=(a10*b10-a11*b08+a13*b06)*inverse; output[9]=(a01*b08-a00*b10-a03*b06)*inverse; output[10]=(a30*b04-a31*b02+a33*b00)*inverse; output[11]=(a21*b02-a20*b04-a23*b00)*inverse;
  output[12]=(a11*b07-a10*b09-a12*b06)*inverse; output[13]=(a00*b09-a01*b07+a02*b06)*inverse; output[14]=(a31*b01-a30*b03-a32*b00)*inverse; output[15]=(a20*b03-a21*b01+a22*b00)*inverse; return output;
}
function perspective(fovy: number, aspect: number, near: number, far: number): Float32Array {
  const f = 1 / Math.tan(fovy / 2); const range = 1 / (near - far);
  return new Float32Array([f / aspect, 0, 0, 0, 0, f, 0, 0, 0, 0, (near + far) * range, -1, 0, 0, 2 * near * far * range, 0]);
}
function orbitEye(camera: ViewerCamera): MutableVec3 {
  const cp = Math.cos(camera.pitch); return [camera.target[0] + camera.distance * cp * Math.cos(camera.yaw), camera.target[1] + camera.distance * cp * Math.sin(camera.yaw), camera.target[2] + camera.distance * Math.sin(camera.pitch)];
}
function lookAt(
  eye: readonly [number, number, number],
  target: readonly [number, number, number],
  up: readonly [number, number, number],
): Float32Array {
  let z: MutableVec3 = [eye[0] - target[0], eye[1] - target[1], eye[2] - target[2]];
  let length = Math.hypot(...z) || 1;
  z = [z[0] / length, z[1] / length, z[2] / length];
  let x: MutableVec3 = [
    up[1] * z[2] - up[2] * z[1],
    up[2] * z[0] - up[0] * z[2],
    up[0] * z[1] - up[1] * z[0],
  ];
  length = Math.hypot(...x) || 1;
  x = [x[0] / length, x[1] / length, x[2] / length];
  const y: MutableVec3 = [
    z[1] * x[2] - z[2] * x[1],
    z[2] * x[0] - z[0] * x[2],
    z[0] * x[1] - z[1] * x[0],
  ];
  return new Float32Array([x[0], y[0], z[0], 0, x[1], y[1], z[1], 0, x[2], y[2], z[2], 0, -x[0]*eye[0]-x[1]*eye[1]-x[2]*eye[2], -y[0]*eye[0]-y[1]*eye[1]-y[2]*eye[2], -z[0]*eye[0]-z[1]*eye[1]-z[2]*eye[2], 1]);
}

function edit(payload: Readonly<Record<string, unknown>>): void { vscode.postMessage({ type: 'edit', edit: payload }); }
function refresh(options: Readonly<Record<string, unknown>>): void { vscode.postMessage({ type: 'refresh', options }); }
function showError(message: string): void { vscode.postMessage({ type: 'showError', message }); }
function editorToolbar(): WebviewElement | null { return webviewElement('toolbar'); }
function content(): WebviewElement | null { return webviewElement('content'); }
function clone<Value>(value: Value): Value { return structuredClone(value); }
function cellValue(value: string): string | null { return value === '****' ? null : value; }
function encodePath(value: DataPath): string { return encodeURIComponent(JSON.stringify(value)); }
function decodePath(value: string | undefined): DataPath {
  if (value === undefined) throw new Error('A GFF editor control is missing its data path.');
  const decoded: unknown = JSON.parse(decodeURIComponent(value));
  if (!Array.isArray(decoded) || !decoded.every(
    (part) => typeof part === 'string' || (typeof part === 'number' && Number.isInteger(part)),
  )) {
    throw new Error('A GFF editor control contains an invalid data path.');
  }
  return decoded;
}
function getAtPath(value: unknown, pathParts: DataPath): unknown {
  let current = value;
  for (const part of pathParts) {
    if (Array.isArray(current) && typeof part === 'number') {
      current = current[part];
    } else if (isRecord(current) && typeof part === 'string') {
      current = current[part];
    } else {
      return undefined;
    }
  }
  return current;
}
function setAtPath(value: unknown, pathParts: DataPath, replacement: unknown): void {
  const last = pathParts.at(-1);
  if (last === undefined) throw new Error('Cannot replace an empty GFF data path.');
  const parent = getAtPath(value, pathParts.slice(0, -1));
  if (Array.isArray(parent) && typeof last === 'number') {
    if (last < 0 || last >= parent.length) throw new Error('GFF array path is out of bounds.');
    parent[last] = replacement;
    return;
  }
  if (isRecord(parent) && typeof last === 'string') {
    parent[last] = replacement;
    return;
  }
  throw new Error('GFF data path does not identify a writable value.');
}
function bytesToBase64(bytes: Uint8Array): string { let binary = ''; const chunk = 0x8000; for (let index = 0; index < bytes.length; index += chunk) binary += String.fromCharCode(...bytes.subarray(index, index + chunk)); return btoa(binary); }
function formatBytes(value: number): string { if (value < 1024) return `${value} B`; if (value < 1048576) return `${(value / 1024).toFixed(1)} KiB`; return `${(value / 1048576).toFixed(1)} MiB`; }
function escapeHtml(value: unknown): string { return String(value).replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;').replaceAll('"', '&quot;').replaceAll("'", '&#39;'); }
function escapeAttribute(value: unknown): string { return escapeHtml(value).replaceAll('\n', '&#10;').replaceAll('\r', '&#13;'); }
function isCustomEditorType(extension: unknown): boolean { return CUSTOM_EDITOR_RESOURCE_TYPES.has(String(extension).toLowerCase()); }

function isGffKind(value: string): value is GffKind {
  return gffKinds.some((kind) => kind === value);
}

function defaultGffValue(kind: GffKind): GffField['value'] {
  if (['string', 'resref', 'void', 'dword64', 'int64'].includes(kind)) return kind.endsWith('64') ? '0' : '';
  if (kind === 'locstring') return { strRef: 4294967295, entries: [] };
  if (kind === 'struct') return { id: 0, fields: [] };
  if (kind === 'list') return [];
  return 0;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function parseResourceModel(value: unknown): ResourceModel {
  if (!isResourceModel(value)) {
    throw new Error('The native resource editor returned a malformed snapshot.');
  }
  return value;
}

function isResourceModel(value: unknown): value is ResourceModel {
  if (!isSnapshotEnvelope(value)) return false;
  switch (value.kind) {
    case '2da':
      return isTwoDaData(value.data);
    case 'tlk':
      return isTlkData(value.data);
    case 'gff':
      return isGffData(value.data);
    case 'ncs':
    case 'ndb':
      return isScriptDebugData(value.data);
    case 'dds':
    case 'tga':
    case 'plt':
      return isTextureData(value.data);
    case 'erf':
    case 'key':
      return isArchiveData(value.data);
    default:
      return false;
  }
}

function isSnapshotEnvelope(value: unknown): value is {
  readonly path: string;
  readonly kind: string;
  readonly readOnlyOrigin?: boolean;
  readonly revision?: number;
  readonly data: unknown;
} {
  return isRecord(value)
    && typeof value.path === 'string'
    && typeof value.kind === 'string'
    && 'data' in value
    && (value.readOnlyOrigin === undefined || typeof value.readOnlyOrigin === 'boolean')
    && (value.revision === undefined || isNonNegativeInteger(value.revision));
}

function isTwoDaData(value: unknown): value is TwoDaData {
  return isRecord(value)
    && Array.isArray(value.columns)
    && value.columns.every(isString)
    && (value.default === null || typeof value.default === 'string')
    && Array.isArray(value.rows)
    && value.rows.every((row) => isRecord(row)
      && typeof row.label === 'string'
      && Array.isArray(row.cells)
      && row.cells.every((cell) => cell === null || typeof cell === 'string'));
}

function isTlkData(value: unknown): value is TlkData {
  return isRecord(value)
    && isNonNegativeInteger(value.language)
    && Number.isInteger(value.highest)
    && isNonNegativeInteger(value.total)
    && isNonNegativeInteger(value.offset)
    && isPositiveInteger(value.limit)
    && Array.isArray(value.entries)
    && value.entries.every(isTlkEntry);
}

function isTlkEntry(value: unknown): value is TlkEntry {
  return isRecord(value)
    && isNonNegativeInteger(value.strRef)
    && typeof value.text === 'string'
    && typeof value.soundResRef === 'string'
    && isFiniteNumber(value.soundLength)
    && Number.isInteger(value.flags)
    && Number.isInteger(value.volumeVariance)
    && Number.isInteger(value.pitchVariance);
}

function isGffData(value: unknown): value is GffData {
  return isRecord(value)
    && typeof value.fileType === 'string'
    && typeof value.fileVersion === 'string'
    && isGffStructure(value.root);
}

function isTextureData(value: unknown): value is TextureData {
  return isRecord(value)
    && isPositiveInteger(value.width)
    && isPositiveInteger(value.height)
    && typeof value.rgba === 'string'
    && isRecord(value.metadata)
    && Object.values(value.metadata).every(isJsonValue);
}

function isArchiveData(value: unknown): value is ArchiveData {
  return isRecord(value)
    && Array.isArray(value.entries)
    && value.entries.every(isArchiveEntry)
    && isNonNegativeInteger(value.total)
    && isNonNegativeInteger(value.offset)
    && isPositiveInteger(value.limit)
    && typeof value.query === 'string'
    && (value.bifs === undefined || (
      Array.isArray(value.bifs)
      && value.bifs.every((bif) => isRecord(bif)
        && isNonNegativeInteger(bif.index)
        && typeof bif.filename === 'string'
        && isNonNegativeInteger(bif.drives)
        && isNonNegativeInteger(bif.oid)
        && isNonNegativeInteger(bif.entryCount))
    ));
}

function isArchiveEntry(value: unknown): value is ArchiveEntry {
  return isRecord(value)
    && typeof value.resource === 'string'
    && (value.bif === undefined || typeof value.bif === 'string')
    && typeof value.extension === 'string'
    && isNonNegativeInteger(value.typeId)
    && isNonNegativeInteger(value.size)
    && typeof value.modified === 'boolean';
}

function isScriptDebugData(value: unknown): value is ScriptDebugData {
  return isRecord(value)
    && (value.primary === 'ncs' || value.primary === 'ndb')
    && typeof value.hasNcs === 'boolean'
    && typeof value.hasNdb === 'boolean'
    && typeof value.hasLangspec === 'boolean'
    && Array.isArray(value.sourceFiles)
    && value.sourceFiles.every((file) => isRecord(file)
      && typeof file.name === 'string'
      && typeof file.available === 'boolean'
      && typeof file.isRoot === 'boolean')
    && (value.header === undefined || isScriptHeader(value.header))
    && isScriptSummary(value.summary)
    && Array.isArray(value.functions)
    && value.functions.every(isScriptFunction)
    && Array.isArray(value.instructions)
    && value.instructions.every(isScriptInstruction)
    && Array.isArray(value.diagnostics)
    && value.diagnostics.every(isString);
}

function isScriptHeader(value: unknown): boolean {
  return isRecord(value)
    && typeof value.format === 'string'
    && isNonNegativeInteger(value.fileSize)
    && isNonNegativeInteger(value.declaredSize)
    && isNonNegativeInteger(value.codeSize)
    && isNonNegativeInteger(value.instructionCount);
}

function isScriptSummary(value: unknown): boolean {
  return isRecord(value)
    && isNonNegativeInteger(value.files)
    && isNonNegativeInteger(value.structs)
    && isNonNegativeInteger(value.functions)
    && isNonNegativeInteger(value.variables)
    && isNonNegativeInteger(value.lineMappings)
    && (value.structEntries === undefined || (
      Array.isArray(value.structEntries)
      && value.structEntries.every((entry) => isRecord(entry)
        && typeof entry.name === 'string'
        && Array.isArray(entry.fields)
        && entry.fields.every((field) => isRecord(field)
          && typeof field.name === 'string'
          && typeof field.type === 'string'))
    ))
    && (value.variableEntries === undefined || (
      Array.isArray(value.variableEntries)
      && value.variableEntries.every((entry) => isRecord(entry)
        && typeof entry.name === 'string'
        && typeof entry.type === 'string'
        && isNonNegativeInteger(entry.start)
        && isNonNegativeInteger(entry.end)
        && Number.isInteger(entry.stackLocation))
    ));
}

function isScriptFunction(value: unknown): value is ScriptFunction {
  return isRecord(value)
    && isNonNegativeInteger(value.index)
    && typeof value.name === 'string'
    && isNonNegativeInteger(value.start)
    && isNonNegativeInteger(value.end)
    && typeof value.returnType === 'string'
    && Array.isArray(value.arguments)
    && value.arguments.every(isString)
    && typeof value.synthetic === 'boolean'
    && (value.source === undefined || value.source === null || isScriptSourceLocation(value.source))
    && Array.isArray(value.blocks)
    && value.blocks.every(isScriptBlock);
}

function isScriptBlock(value: unknown): value is ScriptBlock {
  return isRecord(value)
    && isNonNegativeInteger(value.start)
    && isNonNegativeInteger(value.end)
    && Array.isArray(value.instructionIndices)
    && value.instructionIndices.every(isNonNegativeInteger)
    && Array.isArray(value.successors)
    && value.successors.every(isScriptSuccessor);
}

function isScriptInstruction(value: unknown): value is ScriptInstruction {
  return isRecord(value)
    && isNonNegativeInteger(value.index)
    && isNonNegativeInteger(value.offset)
    && (value.localOffset === null || isNonNegativeInteger(value.localOffset))
    && isPositiveInteger(value.size)
    && typeof value.label === 'string'
    && typeof value.opcode === 'string'
    && typeof value.opcodeInternal === 'string'
    && typeof value.auxcode === 'string'
    && typeof value.auxcodeInternal === 'string'
    && typeof value.operand === 'string'
    && (value.action === null || isScriptAction(value.action))
    && typeof value.rawHex === 'string'
    && (value.jumpTarget === null || isNonNegativeInteger(value.jumpTarget))
    && (value.callTarget === null || isNonNegativeInteger(value.callTarget))
    && Array.isArray(value.successors)
    && value.successors.every(isScriptSuccessor)
    && (value.functionIndex === null || isNonNegativeInteger(value.functionIndex))
    && (value.source === null || isScriptSourceLocation(value.source));
}

function isScriptAction(value: unknown): value is ScriptAction {
  return isRecord(value)
    && isNonNegativeInteger(value.id)
    && isNonNegativeInteger(value.argumentCount)
    && typeof value.name === 'string'
    && isBuiltinType(value.returnType)
    && Array.isArray(value.parameters)
    && value.parameters.every((parameter) => isRecord(parameter)
      && typeof parameter.name === 'string'
      && isBuiltinType(parameter.ty))
    && typeof value.arityMatches === 'boolean';
}

function isBuiltinType(value: unknown): value is BuiltinType {
  return typeof value === 'string'
    || (isRecord(value) && Object.values(value).every(isString));
}

function isScriptSourceLocation(value: unknown): value is ScriptSourceLocation {
  return isRecord(value)
    && typeof value.file === 'string'
    && isPositiveInteger(value.line)
    && (value.text === undefined || value.text === null || typeof value.text === 'string')
    && typeof value.available === 'boolean';
}

function isScriptSuccessor(value: unknown): value is ScriptSuccessor {
  return isRecord(value)
    && isNonNegativeInteger(value.offset)
    && typeof value.kind === 'string';
}

function isJsonValue(value: unknown): value is JsonValue {
  return value === null
    || typeof value === 'boolean'
    || typeof value === 'number'
    || typeof value === 'string'
    || (Array.isArray(value) && value.every(isJsonValue))
    || (isRecord(value) && Object.values(value).every(isJsonValue));
}

function isString(value: unknown): value is string {
  return typeof value === 'string';
}

function isFiniteNumber(value: unknown): value is number {
  return typeof value === 'number' && Number.isFinite(value);
}

function isNonNegativeInteger(value: unknown): value is number {
  return typeof value === 'number' && Number.isInteger(value) && value >= 0;
}

function isPositiveInteger(value: unknown): value is number {
  return isNonNegativeInteger(value) && value > 0;
}

function isInspectorRoute(value: unknown): value is InspectorRoute {
  if (!isRecord(value) || typeof value.page !== 'string') return false;
  if (value.root !== undefined && value.root !== 'source' && value.root !== 'section') return false;
  if (value.rootIndex !== undefined && !Number.isInteger(value.rootIndex)) return false;
  if (value.sourceIndex !== undefined && !Number.isInteger(value.sourceIndex)) return false;
  return value.trail === undefined || (
    Array.isArray(value.trail)
    && value.trail.every((entry) => isRecord(entry)
      && (entry.kind === 'field' || entry.kind === 'entry')
      && Number.isInteger(entry.index))
  );
}

function isAreaObjectInspection(value: unknown): value is AreaObjectInspection {
  return isRecord(value)
    && value.schema === 'nwnrs.area-object-inspection'
    && typeof value.key === 'string'
    && typeof value.label === 'string'
    && typeof value.kind === 'string'
    && Array.isArray(value.sections)
    && Array.isArray(value.sources)
    && Array.isArray(value.references)
    && Array.isArray(value.diagnostics);
}

function isInspectionField(
  value: InspectionField | InspectionStructure,
): value is InspectionField {
  return 'label' in value && typeof value.label === 'string';
}

function isGffStructure(value: unknown): value is GffStructure {
  return isRecord(value)
    && typeof value.id === 'number'
    && Array.isArray(value.fields)
    && value.fields.every(isGffField);
}

function isGffField(value: unknown): value is GffField {
  return isRecord(value)
    && typeof value.label === 'string'
    && typeof value.kind === 'string'
    && isGffKind(value.kind)
    && isGffFieldValue(value.kind, value.value);
}

function isGffFieldValue(kind: GffKind, value: unknown): boolean {
  if (kind === 'struct') return isGffStructure(value);
  if (kind === 'list') return Array.isArray(value) && value.every(isGffStructure);
  if (kind === 'locstring') {
    return isRecord(value)
      && isNonNegativeInteger(value.strRef)
      && Array.isArray(value.entries)
      && value.entries.every((entry) => isRecord(entry)
        && isNonNegativeInteger(entry.language)
        && typeof entry.text === 'string');
  }
  if (kind === 'string' || kind === 'resref' || kind === 'void'
      || kind === 'dword64' || kind === 'int64') {
    return typeof value === 'string';
  }
  return isFiniteNumber(value);
}
