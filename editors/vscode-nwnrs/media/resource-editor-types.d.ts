/**
 * Webview-only runtime state.
 *
 * Rust-owned scene, packet, and inspection contracts are generated into
 * `src/generated/webview.d.ts`; they are intentionally not repeated here.
 */

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

interface DecodedScenePacket {
  readonly manifest: ScenePacketManifest;
  readonly binary: Uint8Array;
}

interface DecodedAnimationPacket {
  readonly manifest: SceneAnimationPacketManifest;
  readonly binary: Uint8Array;
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
