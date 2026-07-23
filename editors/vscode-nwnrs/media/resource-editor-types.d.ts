type Vec2 = readonly [number, number];
type Vec3 = readonly [number, number, number];
type Vec4 = readonly [number, number, number, number];
type Matrix4 = Float32Array;
type NumericArray =
  | readonly number[]
  | Float32Array
  | Float64Array
  | Int32Array
  | Uint32Array
  | Uint8Array;

interface BufferView {
  readonly byteOffset: number;
  readonly byteLength: number;
  readonly component: 'u8' | 'u32' | 'i32' | 'f32';
  readonly componentsPerElement: number;
  readonly elementCount: number;
}

interface RaggedBuffer {
  readonly values: BufferView;
  readonly rowOffsets: BufferView;
}

interface PacketKeyTrack {
  readonly times: BufferView;
  readonly values: BufferView;
}

interface PacketEmitterTrack {
  readonly controller: string;
  readonly bezierKeyed: boolean;
  readonly times: BufferView;
  readonly values: RaggedBuffer;
}

interface PacketAnimMeshTrack {
  readonly samplePeriod: number | null;
  readonly vertexFrameCount: number;
  readonly verticesPerFrame: number;
  readonly vertexSamples: BufferView;
  readonly uvFrameCount: number;
  readonly uvsPerFrame: number;
  readonly uvSamples: BufferView;
}

interface PacketNodeAnimationTrack {
  readonly targetName: string;
  readonly targetNode: number | null;
  readonly translation: PacketKeyTrack;
  readonly rotationAxisAngle: PacketKeyTrack;
  readonly scale: PacketKeyTrack;
  readonly color: PacketKeyTrack;
  readonly radius: PacketKeyTrack;
  readonly alpha: PacketKeyTrack;
  readonly selfIllumColor: PacketKeyTrack;
  readonly multiplier: PacketKeyTrack;
  readonly shadowRadius: PacketKeyTrack;
  readonly verticalDisplacement: PacketKeyTrack;
  readonly emitterControllers: readonly PacketEmitterTrack[];
  readonly animmesh: PacketAnimMeshTrack | null;
  readonly bezierControllers: readonly string[];
  readonly opaqueControllerCount: number;
}

interface PacketAnimationEvent {
  readonly time: number;
  readonly name: string;
  readonly cycle?: number;
  readonly absoluteTime?: number;
}

interface PacketAnimation {
  readonly name: string;
  readonly length: number;
  readonly transitionTime: number;
  readonly rootName: string | null;
  readonly rootNode: number | null;
  readonly events: readonly PacketAnimationEvent[];
  readonly tracksLoaded: boolean;
  readonly nodeTracks: readonly PacketNodeAnimationTrack[];
}

interface PacketPropertyValue {
  readonly kind: 'bool' | 'int' | 'float' | 'text';
  readonly value: boolean | number | string;
}

interface PacketEmitterProperty {
  readonly name: string;
  readonly values: readonly PacketPropertyValue[];
}

interface PacketEmitter {
  readonly xSize: number;
  readonly ySize: number;
  readonly properties: readonly PacketEmitterProperty[];
}

interface PacketLight {
  readonly multiplier: number;
  readonly ambientOnly: number;
  readonly nDynamicType: number | null;
  readonly isDynamic: number;
  readonly affectDynamic: number;
  readonly negativeLight: number;
  readonly lightPriority: number;
  readonly fadingLight: number;
  readonly lensFlares: number;
  readonly flareRadius: number;
  readonly shadowRadius: number;
  readonly verticalDisplacement: number;
  readonly flareTextures: readonly string[];
  readonly flareSizes: readonly number[];
  readonly flarePositions: readonly number[];
  readonly flareColorShifts: readonly Vec3[];
}

interface PacketNode {
  readonly kind: string;
  readonly nodeType: string;
  readonly name: string;
  readonly parent: number | null;
  readonly partNumber: number | null;
  readonly translation: Vec3;
  readonly rotationAxisAngle: Vec4;
  readonly scale: Vec3;
  readonly center: Vec3 | null;
  readonly color: Vec3 | null;
  readonly radius: number | null;
  readonly alpha: number | null;
  readonly wirecolor: Vec3 | null;
  readonly mesh: number | null;
  readonly light: PacketLight | null;
  readonly emitter: PacketEmitter | null;
  readonly dangly: {
    readonly displacement: number;
    readonly tightness: number;
    readonly period: number;
  } | null;
  readonly referenceModel: string | null;
  readonly referenceReattachable: number | null;
  readonly opaqueControllerCount: number;
}

interface PacketPrimitive {
  readonly samplePeriod: number | null;
  readonly positions: BufferView;
  readonly indices: BufferView;
  readonly faceGroups: BufferView;
  readonly uvIndices: BufferView;
  readonly faceMaterialIndices: BufferView;
  readonly uvSets: readonly { readonly index: number; readonly coordinates: BufferView }[];
  readonly normals: BufferView | null;
  readonly tangents: RaggedBuffer;
  readonly colors: RaggedBuffer;
  readonly constraints: RaggedBuffer;
  readonly skinBones: readonly string[];
  readonly skinBoneIndices: BufferView;
  readonly skinWeights: BufferView;
  readonly skinRowOffsets: BufferView;
  readonly surfaceLabels: readonly string[];
  readonly textureNames: readonly string[];
  readonly material: number | null;
}

interface PacketMesh {
  readonly name: string;
  readonly sourceNode: number;
  readonly primitives: readonly PacketPrimitive[];
}

interface PacketMaterial {
  readonly sourceNode: number;
  readonly renderEnabled: boolean;
  readonly shadowEnabled: boolean;
  readonly beaming: number;
  readonly inheritColor: number;
  readonly tilefade: number;
  readonly rotateTexture: number;
  readonly lightMapped: number;
  readonly transparencyHint: number;
  readonly shininess: number;
  readonly alpha: number;
  readonly ambient: Vec3;
  readonly diffuse: Vec3;
  readonly specular: Vec3;
  readonly selfIllumColor: Vec3;
  readonly materialName: string | null;
  readonly renderHint: string | null;
  readonly helperBitmap: string | null;
  readonly textures: readonly { readonly slot: string; readonly name: string }[];
}

interface SceneDirective {
  readonly name: string;
  readonly arguments: readonly string[];
  readonly continuations: readonly string[];
}

interface SceneTextureBinding {
  readonly role: string;
  readonly source: string;
  readonly name: string;
  readonly texture: number | null;
  readonly directives: readonly SceneDirective[];
}

interface SceneMaterialAssets {
  readonly materialIndex: number;
  readonly sourceNode: number;
  readonly renderHint: string | null;
  readonly mtr: { readonly resource: string } | null;
  readonly textures: readonly SceneTextureBinding[];
}

interface PacketModel {
  readonly name: string;
  readonly supermodel: string | null;
  readonly classification: string | null;
  readonly animationScale: number | null;
  readonly ignoreFog: number | null;
  readonly bounds?: {
    readonly min: readonly [number, number, number];
    readonly max: readonly [number, number, number];
  } | null;
  readonly nodes: readonly PacketNode[];
  readonly meshes: readonly PacketMesh[];
  readonly materials: readonly PacketMaterial[];
  readonly resolvedMaterials: readonly SceneMaterialAssets[];
  readonly nodeTextures: readonly {
    readonly nodeIndex: number;
    readonly role: string;
    readonly name: string;
    readonly texture: number | null;
    readonly directives: readonly SceneDirective[];
  }[];
  readonly animations: readonly PacketAnimation[];
  readonly hiddenGeometryNodes: readonly string[];
  readonly attachments: readonly { readonly targetNodeName: string; readonly model: number }[];
}

interface SceneInstance {
  readonly id: number;
  readonly objectKey: string | null;
  readonly label: string;
  readonly kind: string;
  readonly model: number | null;
  readonly resource: string | null;
  readonly position: Vec3;
  readonly rotationAxisAngle: Vec4;
  readonly scale: Vec3;
  readonly polygon: readonly Vec3[];
  readonly lightColorOverrides: readonly (Vec3 | null)[];
}

interface SceneAreaObject {
  readonly key: string;
  readonly label: string;
  readonly kind: string;
  readonly sourceIndex: number;
  readonly tag: string | null;
  readonly templateResref: string | null;
  readonly position: Vec3;
  readonly rotationAxisAngle: Vec4;
}

interface ScenePacketManifest {
  readonly schema: 'nwnrs.scene';
  readonly assetKey?: string | null;
  readonly name: string;
  readonly source:
    | 'model'
    | 'walkmesh'
    | 'doorWalkmesh'
    | 'placeableWalkmesh'
    | 'creature'
    | 'door'
    | 'placeable'
    | 'item'
    | 'area'
    | 'module';
  readonly environment: {
    readonly nwn?: {
      readonly dayNightCycle?: boolean | null;
      readonly isNight?: boolean | null;
      readonly lightingScheme?: number | null;
      readonly fogClipDistance?: number | null;
      readonly skybox?: number | null;
      readonly windPower?: number | null;
      readonly chanceRain?: number | null;
      readonly chanceSnow?: number | null;
      readonly chanceLightning?: number | null;
      readonly sunAmbientColor?: number | null;
      readonly sunDiffuseColor?: number | null;
      readonly sunFogColor?: number | null;
      readonly moonAmbientColor?: number | null;
      readonly moonDiffuseColor?: number | null;
      readonly moonFogColor?: number | null;
      readonly sunFogAmount?: number | null;
      readonly sunShadows?: boolean | null;
      readonly moonFogAmount?: number | null;
      readonly moonShadows?: boolean | null;
      readonly shadowOpacity?: number | null;
    };
  } | 'studio';
  readonly module: {
    readonly areas: readonly string[];
    readonly entryArea: string;
    readonly entryPosition: readonly [number | null, number | null, number | null];
    readonly entryDirection: readonly [number | null, number | null];
    readonly customTlk: string | null;
    readonly haks: readonly string[];
  } | null;
  readonly instances: readonly SceneInstance[];
  readonly areaObjects: readonly SceneAreaObject[];
  readonly models: readonly PacketModel[];
  readonly rootModels: readonly number[];
  readonly textures: readonly PacketTexture[];
  readonly shaders: readonly {
    readonly resource: string;
    readonly origin: string;
    readonly stage: string;
    readonly source: string;
  }[];
  readonly dependencies: {
    readonly nodes: readonly {
      readonly id: number;
      readonly resource: string;
      readonly kind: string;
      readonly state: string;
      readonly origin: string | null;
      readonly message: string | null;
    }[];
    readonly edges: readonly {
      readonly from: number;
      readonly to: number;
      readonly relationship: string;
    }[];
  };
  readonly diagnostics: readonly {
    readonly severity: string;
    readonly code: string;
    readonly message: string;
    readonly resource: string | null;
  }[];
}

interface PacketTexture {
  readonly resource: string;
  readonly origin: string;
  readonly kind: string;
  readonly width: number;
  readonly height: number;
  readonly compression: string | null;
  readonly mipCount: number;
  readonly rgba8: BufferView | null;
}

interface DecodedScenePacket {
  readonly manifest: ScenePacketManifest;
  readonly binary: Uint8Array;
}

interface SceneAnimationPacketManifest {
  readonly schema: 'nwnrs.scene.animation';
  readonly assetKey?: string | null;
  readonly modelIndex: number;
  readonly animationIndex: number;
  readonly animation: PacketAnimation;
}

interface DecodedAnimationPacket {
  readonly manifest: SceneAnimationPacketManifest;
  readonly binary: Uint8Array;
}

interface SceneTexturePacketManifest {
  readonly schema: 'nwnrs.scene.texture';
  readonly assetKey?: string | null;
  readonly textureIndex: number;
  readonly resource: string;
  readonly kind: string;
  readonly width: number;
  readonly height: number;
  readonly compression: string | null;
  readonly mipLevels: readonly {
    readonly width: number;
    readonly height: number;
    readonly data: BufferView;
  }[];
  readonly rgba8: BufferView | null;
}

interface DecodedTexturePacket {
  readonly manifest: SceneTexturePacketManifest;
  readonly binary: Uint8Array;
}

type DecodedPacket =
  | DecodedScenePacket
  | DecodedAnimationPacket
  | DecodedTexturePacket;

interface ViewerAnimationSelection {
  readonly modelIndex: number;
  readonly animationIndex: number;
  readonly name: string;
  readonly label: string;
}

interface ViewerSession {
  readonly scene: DecodedScenePacket;
  readonly animationAssets: Map<string, AnimationAsset>;
  readonly textureAssets: Map<number, DecodedTexturePacket>;
  readonly inspectionAssets: Map<string, AreaObjectInspection>;
  readonly inspectionErrors: Map<string, string>;
  readonly requestedInspections: Set<string>;
}

interface PreparedTrack {
  readonly times: NumericArray;
  readonly values: NumericArray;
  readonly width: number;
  readonly bezier: boolean;
}

interface PreparedEmitterTrack {
  readonly times: NumericArray;
  readonly values: NumericArray;
  readonly offsets: NumericArray;
  readonly bezier: boolean;
}

interface PreparedAnimMeshTrack extends PacketAnimMeshTrack {
  readonly vertexValues: NumericArray;
  readonly uvValues: NumericArray;
}

interface PreparedNodeAnimationTrack {
  readonly source: PacketNodeAnimationTrack;
  readonly nodeIndex: number;
  readonly translation?: PreparedTrack;
  readonly rotationAxisAngle?: PreparedTrack;
  readonly scale?: PreparedTrack;
  readonly color?: PreparedTrack;
  readonly alpha?: PreparedTrack;
  readonly radius?: PreparedTrack;
  readonly multiplier?: PreparedTrack;
  readonly shadowRadius?: PreparedTrack;
  readonly verticalDisplacement?: PreparedTrack;
  readonly selfIllumColor?: PreparedTrack;
  readonly emitterControllers: Map<string, PreparedEmitterTrack>;
  readonly animmesh?: PreparedAnimMeshTrack;
}

interface AnimationRuntime {
  readonly tracksByNode: readonly (PreparedNodeAnimationTrack | undefined)[];
  readonly tracks: readonly PreparedNodeAnimationTrack[];
}

interface AnimationAsset {
  readonly sceneKey?: string | null;
  readonly modelIndex: number;
  readonly animationIndex: number;
  readonly animation: PacketAnimation;
  readonly binary: Uint8Array;
  readonly runtime?: AnimationRuntime;
}

interface InspectionProvenance {
  readonly layer: string;
  readonly resource: string;
  readonly origin?: string | null;
}

interface InspectionResource {
  readonly resource: string;
  readonly origin?: string | null;
  readonly resolved: boolean;
}

interface InspectionStructure {
  readonly id: number;
  readonly fields: readonly InspectionField[];
}

interface InspectionField {
  readonly name: string;
  readonly label: string;
  readonly kind: string;
  readonly display: string;
  readonly text?: string | null;
  readonly structId?: number | null;
  readonly fields?: readonly InspectionField[];
  readonly entries?: readonly InspectionStructure[];
  readonly value64?: string | null;
  readonly localized?: {
    readonly strRef?: number | null;
    readonly source?: string | null;
    readonly languageId?: number | null;
    readonly gender?: string | null;
    readonly entries: readonly { readonly id: number; readonly text: string }[];
  } | null;
  readonly resource?: InspectionResource | null;
  readonly lookup?: { readonly resource: string; readonly row: number; readonly label?: string | null } | null;
  readonly provenance: InspectionProvenance;
}

interface AreaObjectInspection {
  readonly schema: string;
  readonly key: string;
  readonly label: string;
  readonly kind: string;
  readonly sections: readonly {
    readonly id: string;
    readonly label: string;
    readonly defaultOpen: boolean;
    readonly fields: readonly InspectionField[];
  }[];
  readonly sources: readonly {
    readonly layer: string;
    readonly resource: string;
    readonly origin?: string | null;
    readonly data: InspectionStructure;
  }[];
  readonly references: readonly InspectionResource[];
  readonly diagnostics: readonly string[];
}
