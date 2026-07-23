use std::collections::BTreeMap;

use nwnrs_types::{
    dds::{DdsFormat, DdsMipLevel},
    mdl::{
        NodeKind, NwnAnimMeshTrack, NwnComposedScene, NwnEmitterControllerTrack, NwnMaterial,
        NwnNodeAnimationTrack, NwnPrimitive, NwnPropertyValue, NwnScene, NwnSceneAttachment,
        NwnSceneNode, NwnTextureSlot, ScalarKey, Vec3Key, Vec4Key,
    },
};
use serde::{Deserialize, Serialize};

use crate::{
    DependencyGraph, ModelScene, RenderDiagnostic, RenderEnvironment, RenderInstance,
    RenderMaterialAssets, RenderModelAssets, RenderModule, RenderNodeTexture, RenderScene,
    RenderShaderSource, RenderTexture, RenderTextureCompression, RenderTextureKind, RendererError,
    RendererResult, SceneSource,
};

const PACKET_MAGIC: &[u8; 8] = b"NWNRS3D\0";

/// Scalar representation stored in a packed buffer view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BufferComponent {
    /// Unsigned 8-bit integer.
    U8,
    /// Unsigned 32-bit integer.
    U32,
    /// Signed 32-bit integer.
    I32,
    /// IEEE 754 32-bit float.
    F32,
}

/// A typed slice of the binary payload accompanying a scene manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BufferView {
    /// Byte offset from the beginning of the packed payload.
    pub byte_offset:            usize,
    /// Byte length.
    pub byte_length:            usize,
    /// Scalar representation.
    pub component:              BufferComponent,
    /// Scalars per logical element.
    pub components_per_element: usize,
    /// Number of logical elements.
    pub element_count:          usize,
}

/// Packed representation of nested numeric rows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RaggedBuffer {
    /// Flattened row values.
    pub values:      BufferView,
    /// One offset per row plus the final end offset.
    pub row_offsets: BufferView,
}

/// One UV stream in a packed primitive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PacketUvSet {
    /// Authored UV-set index.
    pub index:       usize,
    /// Packed float2 coordinates.
    pub coordinates: BufferView,
}

/// One renderer-ready primitive and its source-preserving side streams.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PacketPrimitive {
    /// Geometry animmesh sample period.
    pub sample_period:         Option<f32>,
    /// Packed float3 positions.
    pub positions:             BufferView,
    /// Packed u32 triangle vertex indices.
    pub indices:               BufferView,
    /// Packed i32 smoothing/group values, one per face.
    pub face_groups:           BufferView,
    /// Packed u32 triangle UV indices.
    pub uv_indices:            BufferView,
    /// Packed i32 material/surface indices, one per face.
    pub face_material_indices: BufferView,
    /// Authored UV streams.
    pub uv_sets:               Vec<PacketUvSet>,
    /// Packed float3 normals, when present.
    pub normals:               Option<BufferView>,
    /// Packed tangent rows.
    pub tangents:              RaggedBuffer,
    /// Packed vertex-color rows.
    pub colors:                RaggedBuffer,
    /// Packed dangly constraint rows.
    pub constraints:           RaggedBuffer,
    /// Bone names referenced by packed skin rows.
    pub skin_bones:            Vec<String>,
    /// Packed bone-table indices for every influence.
    pub skin_bone_indices:     BufferView,
    /// Packed influence weights.
    pub skin_weights:          BufferView,
    /// Packed offsets delimiting each vertex influence row.
    pub skin_row_offsets:      BufferView,
    /// Surface labels preserved from `multimaterial`.
    pub surface_labels:        Vec<String>,
    /// Additional texture names preserved from `texturenames`.
    pub texture_names:         Vec<String>,
    /// Material index in the containing model.
    pub material:              Option<usize>,
}

/// One renderer-ready mesh.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PacketMesh {
    /// Mesh name.
    pub name:        String,
    /// Owning node index.
    pub source_node: usize,
    /// Mesh primitives.
    pub primitives:  Vec<PacketPrimitive>,
}

/// One material texture binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PacketTextureBinding {
    /// Authored texture slot.
    pub slot: String,
    /// Texture resource token.
    pub name: String,
}

/// Complete static material parameters preserved from an MDL node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PacketMaterial {
    /// Owning node index.
    pub source_node:       usize,
    /// Whether rendering is enabled.
    pub render_enabled:    bool,
    /// Whether shadows are enabled.
    pub shadow_enabled:    bool,
    /// Beaming mode.
    pub beaming:           i32,
    /// Color inheritance mode.
    pub inherit_color:     i32,
    /// Tile-fade mode.
    pub tilefade:          i32,
    /// Texture-rotation mode.
    pub rotate_texture:    i32,
    /// Light-map mode.
    pub light_mapped:      i32,
    /// Transparency hint.
    pub transparency_hint: i32,
    /// Shininess.
    pub shininess:         f32,
    /// Alpha.
    pub alpha:             f32,
    /// Ambient color.
    pub ambient:           [f32; 3],
    /// Diffuse color.
    pub diffuse:           [f32; 3],
    /// Specular color.
    pub specular:          [f32; 3],
    /// Self-illumination color.
    pub self_illum_color:  [f32; 3],
    /// MTR resource name.
    pub material_name:     Option<String>,
    /// Renderer hint.
    pub render_hint:       Option<String>,
    /// Helper bitmap used by collision geometry.
    pub helper_bitmap:     Option<String>,
    /// Authored texture bindings.
    pub textures:          Vec<PacketTextureBinding>,
}

/// Scene-graph node data needed for rendering and inspection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PacketNode {
    /// Typed node-kind name.
    pub kind:                    String,
    /// Authored node type token.
    pub node_type:               String,
    /// Node name.
    pub name:                    String,
    /// Parent node index.
    pub parent:                  Option<usize>,
    /// Part-number metadata.
    pub part_number:             Option<i32>,
    /// Local translation.
    pub translation:             [f32; 3],
    /// Local axis-angle rotation.
    pub rotation_axis_angle:     [f32; 4],
    /// Local per-axis scale.
    pub scale:                   [f32; 3],
    /// Node center metadata.
    pub center:                  Option<[f32; 3]>,
    /// Static node color.
    pub color:                   Option<[f32; 3]>,
    /// Static node radius.
    pub radius:                  Option<f32>,
    /// Static node alpha.
    pub alpha:                   Option<f32>,
    /// Wireframe color.
    pub wirecolor:               Option<[f32; 3]>,
    /// Mesh index.
    pub mesh:                    Option<usize>,
    /// Complete light payload.
    pub light:                   Option<PacketLight>,
    /// Complete emitter payload.
    pub emitter:                 Option<PacketEmitter>,
    /// Complete danglymesh physics payload.
    pub dangly:                  Option<PacketDangly>,
    /// Referenced model name.
    pub reference_model:         Option<String>,
    /// Reference reattachment behavior.
    pub reference_reattachable:  Option<i32>,
    /// Number of opaque compiled controllers retained by the Rust session.
    pub opaque_controller_count: usize,
}

/// Complete static light data authored by one model node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PacketLight {
    /// Light intensity multiplier.
    pub multiplier:            f32,
    /// Ambient-only mode.
    pub ambient_only:          i32,
    /// Compiled dynamic-light type.
    pub n_dynamic_type:        Option<i32>,
    /// Dynamic-light toggle.
    pub is_dynamic:            i32,
    /// Whether the light affects dynamic objects.
    pub affect_dynamic:        i32,
    /// Negative-light toggle.
    pub negative_light:        i32,
    /// Engine light priority.
    pub light_priority:        i32,
    /// Fading-light toggle.
    pub fading_light:          i32,
    /// Lens-flare toggle.
    pub lens_flares:           i32,
    /// Lens-flare radius.
    pub flare_radius:          f32,
    /// Shadow radius.
    pub shadow_radius:         f32,
    /// Vertical displacement.
    pub vertical_displacement: f32,
    /// Lens-flare texture names.
    pub flare_textures:        Vec<String>,
    /// Lens-flare element sizes.
    pub flare_sizes:           Vec<f32>,
    /// Lens-flare element positions.
    pub flare_positions:       Vec<f32>,
    /// Lens-flare color shifts.
    pub flare_color_shifts:    Vec<[f32; 3]>,
}

/// One typed emitter-property value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum PacketPropertyValue {
    /// Boolean value.
    Bool(bool),
    /// Signed integer value.
    Int(i32),
    /// Floating-point value.
    Float(f32),
    /// Text value.
    Text(String),
}

/// One ordered emitter property.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PacketEmitterProperty {
    /// Authored property name.
    pub name:   String,
    /// Ordered typed property values.
    pub values: Vec<PacketPropertyValue>,
}

/// Complete static emitter payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PacketEmitter {
    /// Emitter width.
    pub x_size:     f32,
    /// Emitter height.
    pub y_size:     f32,
    /// Remaining authored properties.
    pub properties: Vec<PacketEmitterProperty>,
}

/// Complete danglymesh physics payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PacketDangly {
    /// Maximum displacement.
    pub displacement: f32,
    /// Return-force/tightness.
    pub tightness:    f32,
    /// Oscillation period.
    pub period:       f32,
}

/// One animation event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PacketAnimationEvent {
    /// Event time.
    pub time: f32,
    /// Event name.
    pub name: String,
}

/// Packed time/value samples for one animation channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PacketKeyTrack {
    /// Packed scalar key times.
    pub times:  BufferView,
    /// Packed scalar/vector key values.
    pub values: BufferView,
}

/// One packed emitter-controller curve.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PacketEmitterTrack {
    /// Canonical NWN controller property name.
    pub controller:   String,
    /// Whether source interpolation uses Bezier keys.
    pub bezier_keyed: bool,
    /// Packed sample times.
    pub times:        BufferView,
    /// Packed variable-width controller values.
    pub values:       RaggedBuffer,
}

/// Packed animmesh samples for one animated node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PacketAnimMeshTrack {
    /// Authored sample period.
    pub sample_period:      Option<f32>,
    /// Number of complete vertex frames.
    pub vertex_frame_count: usize,
    /// Vertices per complete frame.
    pub vertices_per_frame: usize,
    /// Packed float3 vertex samples.
    pub vertex_samples:     BufferView,
    /// Number of complete UV frames.
    pub uv_frame_count:     usize,
    /// UV values per complete frame.
    pub uvs_per_frame:      usize,
    /// Packed float2 UV samples.
    pub uv_samples:         BufferView,
}

/// Complete animation channels for one target node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PacketNodeAnimationTrack {
    /// Target node name.
    pub target_name:             String,
    /// Resolved target node index.
    pub target_node:             Option<usize>,
    /// Translation keys.
    pub translation:             PacketKeyTrack,
    /// Axis-angle rotation keys.
    pub rotation_axis_angle:     PacketKeyTrack,
    /// Per-axis scale keys.
    pub scale:                   PacketKeyTrack,
    /// Node color keys.
    pub color:                   PacketKeyTrack,
    /// Node radius keys.
    pub radius:                  PacketKeyTrack,
    /// Node alpha keys.
    pub alpha:                   PacketKeyTrack,
    /// Material self-illumination keys.
    pub self_illum_color:        PacketKeyTrack,
    /// Light multiplier keys.
    pub multiplier:              PacketKeyTrack,
    /// Light shadow-radius keys.
    pub shadow_radius:           PacketKeyTrack,
    /// Light vertical-displacement keys.
    pub vertical_displacement:   PacketKeyTrack,
    /// Emitter controller curves.
    pub emitter_controllers:     Vec<PacketEmitterTrack>,
    /// Animmesh frames.
    pub animmesh:                Option<PacketAnimMeshTrack>,
    /// Controller names authored with Bezier interpolation.
    pub bezier_controllers:      Vec<String>,
    /// Number of unknown compiled controllers retained by the Rust scene.
    pub opaque_controller_count: usize,
}

/// Animation metadata and packed node tracks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PacketAnimation {
    /// Animation name.
    pub name:            String,
    /// Duration in seconds.
    pub length:          f32,
    /// Transition duration in seconds.
    pub transition_time: f32,
    /// Animation root name.
    pub root_name:       Option<String>,
    /// Animation root node.
    pub root_node:       Option<usize>,
    /// Timed events.
    pub events:          Vec<PacketAnimationEvent>,
    /// Whether this payload includes complete node tracks.
    pub tracks_loaded:   bool,
    /// Complete per-node tracks.
    pub node_tracks:     Vec<PacketNodeAnimationTrack>,
}

/// Attachment relationship between packed models.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PacketAttachment {
    /// Target node name on the parent model.
    pub target_node_name: String,
    /// Child packed-model index.
    pub model:            usize,
}

/// One packed model asset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PacketModel {
    /// Model name.
    pub name:                  String,
    /// Supermodel name.
    pub supermodel:            Option<String>,
    /// Model classification.
    pub classification:        Option<String>,
    /// Animation scale.
    pub animation_scale:       Option<f32>,
    /// Fog override.
    pub ignore_fog:            Option<i32>,
    /// Scene nodes.
    pub nodes:                 Vec<PacketNode>,
    /// Meshes.
    pub meshes:                Vec<PacketMesh>,
    /// Materials.
    pub materials:             Vec<PacketMaterial>,
    /// Effective MTR/TXI/texture resolution aligned with `materials`.
    pub resolved_materials:    Vec<RenderMaterialAssets>,
    /// Resolved emitter and lens-flare textures.
    pub node_textures:         Vec<RenderNodeTexture>,
    /// Animation catalog.
    pub animations:            Vec<PacketAnimation>,
    /// Hidden geometry node names.
    pub hidden_geometry_nodes: Vec<String>,
    /// Referenced child models.
    pub attachments:           Vec<PacketAttachment>,
}

/// One decoded texture in the packed binary payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PacketTexture {
    /// Resolved texture resource.
    pub resource:    String,
    /// Container provenance.
    pub origin:      String,
    /// Original texture storage kind.
    pub kind:        RenderTextureKind,
    /// Pixel width.
    pub width:       u32,
    /// Pixel height.
    pub height:      u32,
    /// Authored GPU compression, when available.
    pub compression: Option<RenderTextureCompression>,
    /// Number of authored mip levels.
    pub mip_count:   usize,
    /// Packed top-left-origin RGBA8 pixels.
    pub rgba8:       Option<BufferView>,
}

/// JSON manifest at the front of a binary scene packet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScenePacketManifest {
    /// Packet schema identity.
    pub schema:       String,
    /// Opaque native cache identity used to request lazy scene assets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset_key:    Option<String>,
    /// Scene name.
    pub name:         String,
    /// Source category.
    pub source:       SceneSource,
    /// Viewer environment.
    pub environment:  RenderEnvironment,
    /// Module area catalog for IFO-backed scenes.
    pub module:       Option<RenderModule>,
    /// Scene instances.
    pub instances:    Vec<RenderInstance>,
    /// Logical authored GIT objects for area navigation and selection.
    pub area_objects: Vec<crate::RenderAreaObject>,
    /// Packed model assets.
    pub models:       Vec<PacketModel>,
    /// Packed-model index for each top-level [`RenderScene`] model.
    pub root_models:  Vec<usize>,
    /// Decoded textures referenced by resolved model materials.
    pub textures:     Vec<PacketTexture>,
    /// Resolved custom shader sources.
    pub shaders:      Vec<RenderShaderSource>,
    /// Dependency graph.
    pub dependencies: DependencyGraph,
    /// Diagnostics.
    pub diagnostics:  Vec<RenderDiagnostic>,
}

/// A compact scene manifest plus its packed numeric payload.
#[derive(Debug, Clone, PartialEq)]
pub struct ScenePacket {
    /// Structured scene metadata.
    pub manifest: ScenePacketManifest,
    /// Packed geometry and numeric streams.
    pub binary:   Vec<u8>,
}

impl ScenePacket {
    /// Builds a packed scene from the shared renderer-neutral document.
    ///
    /// # Errors
    ///
    /// Returns [`RendererError`] when a stream cannot be represented.
    pub fn from_scene(scene: &RenderScene) -> RendererResult<Self> {
        Self::from_scene_with_payloads(scene, true, true)
    }

    /// Builds the initial scene catalog without animation tracks or pixels.
    /// Geometry remains immediately renderable while large optional assets are
    /// fetched independently by the frontend when they become visible.
    pub fn catalog_from_scene(scene: &RenderScene) -> RendererResult<Self> {
        Self::from_scene_with_payloads(scene, false, false)
    }

    fn from_scene_with_payloads(
        scene: &RenderScene,
        include_animation_tracks: bool,
        include_texture_pixels: bool,
    ) -> RendererResult<Self> {
        if scene.models.len() != scene.model_assets.len() {
            return Err(RendererError::scene(format!(
                "model asset tree count {} does not match model count {}",
                scene.model_assets.len(),
                scene.models.len()
            )));
        }
        let mut builder = PacketBuilder::default();
        let mut models = Vec::new();
        let mut root_models = Vec::with_capacity(scene.models.len());
        for (model, assets) in scene.models.iter().zip(&scene.model_assets) {
            let root_model = match model {
                ModelScene::Composed(composed) => builder.pack_composed(
                    composed,
                    assets,
                    &mut models,
                    include_animation_tracks,
                )?,
                ModelScene::Auxiliary(auxiliary) => {
                    let index = models.len();
                    models.push(builder.pack_scene(
                        auxiliary,
                        Vec::new(),
                        assets.materials.clone(),
                        assets.node_textures.clone(),
                        include_animation_tracks,
                    )?);
                    index
                }
            };
            root_models.push(root_model);
        }
        let mut instances = scene.instances.clone();
        for instance in &mut instances {
            if let Some(top_level) = instance.model {
                instance.model = root_models.get(top_level).copied();
            }
        }
        let textures = scene
            .textures
            .iter()
            .map(|texture| {
                Ok(PacketTexture {
                    resource:    texture.resource.clone(),
                    origin:      texture.origin.clone(),
                    kind:        texture.kind,
                    width:       texture.width,
                    height:      texture.height,
                    compression: texture.compressed.as_ref().map(|value| value.compression),
                    mip_count:   texture
                        .compressed
                        .as_ref()
                        .map_or(1, |value| value.mip_levels.len()),
                    rgba8:       include_texture_pixels
                        .then(|| {
                            decoded_texture_rgba8(texture).and_then(|rgba| builder.push_u8(rgba, 4))
                        })
                        .transpose()?,
                })
            })
            .collect::<RendererResult<Vec<_>>>()?;
        Ok(Self {
            manifest: ScenePacketManifest {
                schema: "nwnrs.scene".into(),
                asset_key: None,
                name: scene.name.clone(),
                source: scene.source,
                environment: scene.environment.clone(),
                module: scene.module.clone(),
                instances,
                area_objects: scene
                    .area
                    .as_ref()
                    .map_or_else(Vec::new, |area| crate::area_object_catalog(&area.instances)),
                models,
                root_models,
                textures,
                shaders: scene.shaders.clone(),
                dependencies: scene.dependencies.clone(),
                diagnostics: scene.diagnostics.clone(),
            },
            binary:   builder.binary,
        })
    }

    /// Packs one animation selected from the flattened packet-model catalog.
    pub fn animation_from_scene(
        scene: &RenderScene,
        model_index: usize,
        animation_index: usize,
    ) -> RendererResult<SceneAnimationPacket> {
        let model = flattened_model_scene(scene, model_index).ok_or_else(|| {
            RendererError::invalid(format!("scene model index {model_index} is out of range"))
        })?;
        let animation = model.animations.get(animation_index).ok_or_else(|| {
            RendererError::invalid(format!(
                "animation index {animation_index} is out of range for {}",
                model.name
            ))
        })?;
        let mut builder = PacketBuilder::default();
        let animation = builder.pack_animation(animation)?;
        Ok(SceneAnimationPacket {
            manifest: SceneAnimationPacketManifest {
                schema: "nwnrs.scene.animation".into(),
                asset_key: None,
                model_index,
                animation_index,
                animation,
            },
            binary:   builder.binary,
        })
    }

    /// Packs one decoded texture independently from the scene catalog.
    pub fn texture_from_scene(
        scene: &RenderScene,
        texture_index: usize,
        prefer_compressed: bool,
    ) -> RendererResult<SceneTexturePacket> {
        let texture = scene.textures.get(texture_index).ok_or_else(|| {
            RendererError::invalid(format!(
                "scene texture index {texture_index} is out of range"
            ))
        })?;
        let mut builder = PacketBuilder::default();
        let (compression, mip_levels, rgba8) = if prefer_compressed {
            if let Some(compressed) = &texture.compressed {
                let levels = compressed
                    .mip_levels
                    .iter()
                    .map(|mip| {
                        Ok(PacketTextureMip {
                            width:  mip.width,
                            height: mip.height,
                            // NWN DDS blocks are authored bottom-first already,
                            // exactly matching WebGL compressed-texture coordinates.
                            // Unlike decoded RGBA, these bytes must not be flipped.
                            data:   builder.push_u8(mip.data.iter().copied(), 1)?,
                        })
                    })
                    .collect::<RendererResult<Vec<_>>>()?;
                (Some(compressed.compression), levels, None)
            } else {
                (
                    None,
                    Vec::new(),
                    Some(builder.push_u8(decoded_texture_rgba8(texture)?, 4)?),
                )
            }
        } else {
            (
                None,
                Vec::new(),
                Some(builder.push_u8(decoded_texture_rgba8(texture)?, 4)?),
            )
        };
        Ok(SceneTexturePacket {
            manifest: SceneTexturePacketManifest {
                schema: "nwnrs.scene.texture".into(),
                asset_key: None,
                texture_index,
                resource: texture.resource.clone(),
                kind: texture.kind,
                width: texture.width,
                height: texture.height,
                compression,
                mip_levels,
                rgba8,
            },
            binary:   builder.binary,
        })
    }

    /// Encodes the scene as one transferable packet.
    ///
    /// Layout: eight-byte magic, little-endian manifest-segment length, JSON
    /// manifest padded with trailing whitespace to four-byte alignment, then
    /// packed binary data. Buffer-view offsets are relative to the packed-data
    /// segment.
    ///
    /// # Errors
    ///
    /// Returns [`RendererError`] when serialization or size conversion fails.
    pub fn encode(&self) -> RendererResult<Vec<u8>> {
        encode_packet(&self.manifest, &self.binary)
    }

    /// Decodes a transferable packet.
    ///
    /// # Errors
    ///
    /// Returns [`RendererError`] when the packet is truncated or malformed.
    pub fn decode(bytes: &[u8]) -> RendererResult<Self> {
        if bytes.get(..PACKET_MAGIC.len()) != Some(PACKET_MAGIC) {
            return Err(RendererError::invalid("invalid nwnrs scene packet magic"));
        }
        let length_start = PACKET_MAGIC.len();
        let length_end = length_start + 4;
        let length_bytes: [u8; 4] = bytes
            .get(length_start..length_end)
            .and_then(|slice| slice.try_into().ok())
            .ok_or_else(|| RendererError::invalid("truncated scene packet header"))?;
        let manifest_length = usize::try_from(u32::from_le_bytes(length_bytes))
            .map_err(|error| RendererError::invalid(format!("scene manifest length: {error}")))?;
        let manifest_end = length_end
            .checked_add(manifest_length)
            .ok_or_else(|| RendererError::invalid("scene manifest length overflow"))?;
        let manifest_bytes = bytes
            .get(length_end..manifest_end)
            .ok_or_else(|| RendererError::invalid("truncated scene packet manifest"))?;
        let manifest = serde_json::from_slice(manifest_bytes)
            .map_err(|error| RendererError::invalid(format!("decode scene manifest: {error}")))?;
        let binary = bytes
            .get(manifest_end..)
            .ok_or_else(|| RendererError::invalid("truncated scene packet payload"))?
            .to_vec();
        Ok(Self {
            manifest,
            binary,
        })
    }
}

/// Manifest for one lazily requested animation payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneAnimationPacketManifest {
    /// Packet schema identity.
    pub schema:          String,
    /// Scene-cache identity that owns this animation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset_key:       Option<String>,
    /// Flattened scene model index.
    pub model_index:     usize,
    /// Animation index in that model's catalog.
    pub animation_index: usize,
    /// Complete selected animation and its packed tracks.
    pub animation:       PacketAnimation,
}

/// One independently transferable animation payload.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneAnimationPacket {
    /// Animation metadata and packed track views.
    pub manifest: SceneAnimationPacketManifest,
    /// Packed numeric track data.
    pub binary:   Vec<u8>,
}

impl SceneAnimationPacket {
    /// Encodes the payload with the shared binary packet envelope.
    pub fn encode(&self) -> RendererResult<Vec<u8>> {
        encode_packet(&self.manifest, &self.binary)
    }
}

/// Manifest for one lazily requested texture payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneTexturePacketManifest {
    /// Packet schema identity.
    pub schema:        String,
    /// Scene-cache identity that owns this texture.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset_key:     Option<String>,
    /// Texture index in the scene catalog.
    pub texture_index: usize,
    /// Resolved resource identity.
    pub resource:      String,
    /// Original resource storage kind.
    pub kind:          RenderTextureKind,
    /// Pixel width.
    pub width:         u32,
    /// Pixel height.
    pub height:        u32,
    /// GPU compression used by authored mip blocks.
    pub compression:   Option<RenderTextureCompression>,
    /// Authored compressed mip chain.
    pub mip_levels:    Vec<PacketTextureMip>,
    /// Packed RGBA8 fallback pixels.
    pub rgba8:         Option<BufferView>,
}

/// One packed compressed texture mip.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PacketTextureMip {
    /// Pixel width.
    pub width:  u32,
    /// Pixel height.
    pub height: u32,
    /// Packed compressed blocks.
    pub data:   BufferView,
}

/// One independently transferable texture payload.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneTexturePacket {
    /// Texture metadata.
    pub manifest: SceneTexturePacketManifest,
    /// Packed pixel data.
    pub binary:   Vec<u8>,
}

impl SceneTexturePacket {
    /// Encodes the payload with the shared binary packet envelope.
    pub fn encode(&self) -> RendererResult<Vec<u8>> {
        encode_packet(&self.manifest, &self.binary)
    }
}

fn encode_packet(manifest: &impl Serialize, binary: &[u8]) -> RendererResult<Vec<u8>> {
    let mut manifest = serde_json::to_vec(manifest)
        .map_err(|error| RendererError::scene(format!("encode packet manifest: {error}")))?;
    // BufferView offsets are relative to the binary segment and every typed
    // view is four-byte aligned within that segment. Keep the segment itself
    // aligned in the complete packet as well so JavaScript can construct
    // zero-copy Float32Array/Uint32Array views over it. JSON permits trailing
    // whitespace, so the recorded manifest segment includes this padding.
    while !(PACKET_MAGIC.len() + 4 + manifest.len()).is_multiple_of(4) {
        manifest.push(b' ');
    }
    let manifest_len = u32::try_from(manifest.len())
        .map_err(|error| RendererError::scene(format!("packet manifest is too large: {error}")))?;
    let capacity = PACKET_MAGIC
        .len()
        .checked_add(4)
        .and_then(|length| length.checked_add(manifest.len()))
        .and_then(|length| length.checked_add(binary.len()))
        .ok_or_else(|| RendererError::scene("packet length overflow"))?;
    let mut packet = Vec::with_capacity(capacity);
    packet.extend_from_slice(PACKET_MAGIC);
    packet.extend_from_slice(&manifest_len.to_le_bytes());
    packet.extend_from_slice(&manifest);
    packet.extend_from_slice(binary);
    Ok(packet)
}

#[derive(Default)]
struct PacketBuilder {
    binary: Vec<u8>,
}

impl PacketBuilder {
    fn pack_composed(
        &mut self,
        composed: &NwnComposedScene,
        assets: &RenderModelAssets,
        target: &mut Vec<PacketModel>,
        include_animation_tracks: bool,
    ) -> RendererResult<usize> {
        if composed.attachments.len() != assets.attachments.len() {
            return Err(RendererError::scene(format!(
                "attachment asset count for {} does not match its composed scene",
                composed.model_name
            )));
        }
        let model_index = target.len();
        target.push(self.pack_scene(
            &composed.scene,
            composed.hidden_geometry_nodes.clone(),
            assets.materials.clone(),
            assets.node_textures.clone(),
            include_animation_tracks,
        )?);
        let mut attachments = Vec::new();
        for (attachment, attachment_assets) in composed.attachments.iter().zip(&assets.attachments)
        {
            let child = self.pack_attachment(
                attachment,
                attachment_assets,
                target,
                include_animation_tracks,
            )?;
            attachments.push(PacketAttachment {
                target_node_name: attachment.target_node_name.clone(),
                model:            child,
            });
        }
        target
            .get_mut(model_index)
            .ok_or_else(|| RendererError::scene("packed model index disappeared"))?
            .attachments = attachments;
        Ok(model_index)
    }

    fn pack_attachment(
        &mut self,
        attachment: &NwnSceneAttachment,
        assets: &RenderModelAssets,
        target: &mut Vec<PacketModel>,
        include_animation_tracks: bool,
    ) -> RendererResult<usize> {
        self.pack_composed(&attachment.scene, assets, target, include_animation_tracks)
    }

    fn pack_scene(
        &mut self,
        scene: &NwnScene,
        hidden_geometry_nodes: Vec<String>,
        resolved_materials: Vec<RenderMaterialAssets>,
        node_textures: Vec<RenderNodeTexture>,
        include_animation_tracks: bool,
    ) -> RendererResult<PacketModel> {
        Ok(PacketModel {
            name: scene.name.clone(),
            supermodel: scene.supermodel.clone(),
            classification: scene
                .classification
                .as_ref()
                .map(|value| format!("{value:?}")),
            animation_scale: scene.animation_scale,
            ignore_fog: scene.ignore_fog,
            nodes: scene.nodes.iter().map(pack_node).collect(),
            meshes: scene
                .meshes
                .iter()
                .map(|mesh| {
                    Ok(PacketMesh {
                        name:        mesh.name.clone(),
                        source_node: mesh.source_node,
                        primitives:  mesh
                            .primitives
                            .iter()
                            .map(|primitive| self.pack_primitive(primitive))
                            .collect::<RendererResult<Vec<_>>>()?,
                    })
                })
                .collect::<RendererResult<Vec<_>>>()?,
            materials: scene.materials.iter().map(pack_material).collect(),
            resolved_materials,
            node_textures,
            animations: scene
                .animations
                .iter()
                .map(|animation| {
                    if include_animation_tracks {
                        self.pack_animation(animation)
                    } else {
                        Ok(PacketAnimation {
                            name:            animation.name.clone(),
                            length:          animation.length,
                            transition_time: animation.transition_time,
                            root_name:       animation.root_name.clone(),
                            root_node:       animation.root_node,
                            events:          animation
                                .events
                                .iter()
                                .map(|event| PacketAnimationEvent {
                                    time: event.time,
                                    name: event.name.clone(),
                                })
                                .collect(),
                            tracks_loaded:   false,
                            node_tracks:     Vec::new(),
                        })
                    }
                })
                .collect::<RendererResult<Vec<_>>>()?,
            hidden_geometry_nodes,
            attachments: Vec::new(),
        })
    }

    fn pack_animation(
        &mut self,
        animation: &nwnrs_types::mdl::NwnAnimation,
    ) -> RendererResult<PacketAnimation> {
        Ok(PacketAnimation {
            name:            animation.name.clone(),
            length:          animation.length,
            transition_time: animation.transition_time,
            root_name:       animation.root_name.clone(),
            root_node:       animation.root_node,
            events:          animation
                .events
                .iter()
                .map(|event| PacketAnimationEvent {
                    time: event.time,
                    name: event.name.clone(),
                })
                .collect(),
            tracks_loaded:   true,
            node_tracks:     animation
                .node_tracks
                .iter()
                .map(|track| self.pack_animation_track(track))
                .collect::<RendererResult<Vec<_>>>()?,
        })
    }

    fn pack_animation_track(
        &mut self,
        track: &NwnNodeAnimationTrack,
    ) -> RendererResult<PacketNodeAnimationTrack> {
        Ok(PacketNodeAnimationTrack {
            target_name:             track.target_name.clone(),
            target_node:             track.target_node,
            translation:             self.pack_vec3_keys(&track.transform.translation_keys)?,
            rotation_axis_angle:     self
                .pack_vec4_keys(&track.transform.rotation_axis_angle_keys)?,
            scale:                   self.pack_vec3_keys(&track.transform.scale_keys)?,
            color:                   self.pack_vec3_keys(&track.material.color_keys)?,
            radius:                  self.pack_scalar_keys(&track.material.radius_keys)?,
            alpha:                   self.pack_scalar_keys(&track.material.alpha_keys)?,
            self_illum_color:        self.pack_vec3_keys(&track.material.self_illum_color_keys)?,
            multiplier:              self.pack_scalar_keys(&track.material.multiplier_keys)?,
            shadow_radius:           self.pack_scalar_keys(&track.material.shadow_radius_keys)?,
            vertical_displacement:   self
                .pack_scalar_keys(&track.material.vertical_displacement_keys)?,
            emitter_controllers:     track
                .effects
                .emitter_controllers
                .iter()
                .map(|controller| self.pack_emitter_track(controller))
                .collect::<RendererResult<Vec<_>>>()?,
            animmesh:                track
                .animmesh
                .as_ref()
                .map(|animmesh| self.pack_animmesh_track(animmesh))
                .transpose()?,
            bezier_controllers:      track.bezier_controllers.clone(),
            opaque_controller_count: track.opaque_controllers.len(),
        })
    }

    fn pack_scalar_keys(&mut self, keys: &[ScalarKey]) -> RendererResult<PacketKeyTrack> {
        Ok(PacketKeyTrack {
            times:  self.push_f32(keys.iter().map(|key| key.time))?,
            values: self.push_f32(keys.iter().map(|key| key.value))?,
        })
    }

    fn pack_vec3_keys(&mut self, keys: &[Vec3Key]) -> RendererResult<PacketKeyTrack> {
        Ok(PacketKeyTrack {
            times:  self.push_f32(keys.iter().map(|key| key.time))?,
            values: self.push_f32_rows(keys.iter().map(|key| key.value.as_slice()), 3)?,
        })
    }

    fn pack_vec4_keys(&mut self, keys: &[Vec4Key]) -> RendererResult<PacketKeyTrack> {
        Ok(PacketKeyTrack {
            times:  self.push_f32(keys.iter().map(|key| key.time))?,
            values: self.push_f32_rows(keys.iter().map(|key| key.value.as_slice()), 4)?,
        })
    }

    fn pack_emitter_track(
        &mut self,
        track: &NwnEmitterControllerTrack,
    ) -> RendererResult<PacketEmitterTrack> {
        let rows = track
            .keys
            .iter()
            .map(|key| key.values.clone())
            .collect::<Vec<_>>();
        Ok(PacketEmitterTrack {
            controller:   track.controller.property_name().to_string(),
            bezier_keyed: track.bezier_keyed,
            times:        self.push_f32(track.keys.iter().map(|key| key.time))?,
            values:       self.push_ragged_f32(&rows)?,
        })
    }

    fn pack_animmesh_track(
        &mut self,
        track: &NwnAnimMeshTrack,
    ) -> RendererResult<PacketAnimMeshTrack> {
        let vertices_per_frame = track
            .vertex_samples
            .first()
            .map_or(0, |sample| sample.values.len());
        let uvs_per_frame = track
            .uv_samples
            .first()
            .map_or(0, |sample| sample.values.len());
        if track
            .vertex_samples
            .iter()
            .any(|sample| sample.values.len() != vertices_per_frame)
            || track
                .uv_samples
                .iter()
                .any(|sample| sample.values.len() != uvs_per_frame)
        {
            return Err(RendererError::scene(
                "animmesh frames have inconsistent sample widths",
            ));
        }
        Ok(PacketAnimMeshTrack {
            sample_period: track.sample_period,
            vertex_frame_count: track.vertex_samples.len(),
            vertices_per_frame,
            vertex_samples: self.push_f32_rows(
                track
                    .vertex_samples
                    .iter()
                    .flat_map(|sample| sample.values.iter().map(|value| value.as_slice())),
                3,
            )?,
            uv_frame_count: track.uv_samples.len(),
            uvs_per_frame,
            uv_samples: self.push_f32_rows(
                track
                    .uv_samples
                    .iter()
                    .flat_map(|sample| sample.values.iter().map(|value| value.as_slice())),
                2,
            )?,
        })
    }

    fn pack_primitive(&mut self, primitive: &NwnPrimitive) -> RendererResult<PacketPrimitive> {
        let positions =
            self.push_f32_rows(primitive.positions.iter().map(|value| value.as_slice()), 3)?;
        let indices = self.push_u32_rows(
            primitive
                .faces
                .iter()
                .map(|face| face.vertex_indices.as_slice()),
            3,
        )?;
        let face_groups = self.push_i32(primitive.faces.iter().map(|face| face.group))?;
        let uv_indices = self.push_u32_rows(
            primitive
                .faces
                .iter()
                .map(|face| face.uv_indices.as_slice()),
            3,
        )?;
        let face_material_indices =
            self.push_i32(primitive.faces.iter().map(|face| face.material_index))?;
        let uv_sets = primitive
            .uv_sets
            .iter()
            .map(|uv| {
                Ok(PacketUvSet {
                    index:       uv.index,
                    coordinates: self
                        .push_f32_rows(uv.coordinates.iter().map(|value| value.as_slice()), 2)?,
                })
            })
            .collect::<RendererResult<Vec<_>>>()?;
        let normals = (!primitive.normals.is_empty())
            .then(|| self.push_f32_rows(primitive.normals.iter().map(|value| value.as_slice()), 3))
            .transpose()?;
        let tangents = self.push_ragged_f32(&primitive.tangents)?;
        let colors = self.push_ragged_f32(&primitive.color_rows)?;
        let constraints = self.push_ragged_f32(&primitive.constraint_rows)?;

        let mut bone_lookup = BTreeMap::<String, u32>::new();
        let mut bones = Vec::new();
        let mut bone_indices = Vec::new();
        let mut weights = Vec::new();
        let mut row_offsets = Vec::with_capacity(primitive.weight_rows.len() + 1);
        row_offsets.push(0_u32);
        for row in &primitive.weight_rows {
            for influence in row {
                let key = influence.bone.to_ascii_lowercase();
                let bone_index = if let Some(index) = bone_lookup.get(&key).copied() {
                    index
                } else {
                    let index = u32::try_from(bones.len()).map_err(|error| {
                        RendererError::scene(format!("skin bone table is too large: {error}"))
                    })?;
                    bones.push(influence.bone.clone());
                    bone_lookup.insert(key, index);
                    index
                };
                bone_indices.push(bone_index);
                weights.push(influence.weight);
            }
            row_offsets.push(u32::try_from(weights.len()).map_err(|error| {
                RendererError::scene(format!("skin influence table is too large: {error}"))
            })?);
        }

        Ok(PacketPrimitive {
            sample_period: primitive.sample_period,
            positions,
            indices,
            face_groups,
            uv_indices,
            face_material_indices,
            uv_sets,
            normals,
            tangents,
            colors,
            constraints,
            skin_bones: bones,
            skin_bone_indices: self.push_u32(bone_indices)?,
            skin_weights: self.push_f32(weights)?,
            skin_row_offsets: self.push_u32(row_offsets)?,
            surface_labels: primitive.surface_labels.clone(),
            texture_names: primitive.texture_names.clone(),
            material: primitive.material,
        })
    }

    fn push_ragged_f32(&mut self, rows: &[Vec<f32>]) -> RendererResult<RaggedBuffer> {
        let mut values = Vec::new();
        let mut offsets = Vec::with_capacity(rows.len() + 1);
        offsets.push(0_u32);
        for row in rows {
            values.extend_from_slice(row);
            offsets.push(u32::try_from(values.len()).map_err(|error| {
                RendererError::scene(format!("ragged numeric stream is too large: {error}"))
            })?);
        }
        Ok(RaggedBuffer {
            values:      self.push_f32(values)?,
            row_offsets: self.push_u32(offsets)?,
        })
    }

    fn push_f32_rows<'b>(
        &mut self,
        rows: impl Iterator<Item = &'b [f32]>,
        components: usize,
    ) -> RendererResult<BufferView> {
        let values = rows.flatten().copied().collect::<Vec<_>>();
        if components == 0 || values.len() % components != 0 {
            return Err(RendererError::scene("invalid packed f32 row width"));
        }
        self.push_typed(
            values.len() / components,
            components,
            BufferComponent::F32,
            values.into_iter().flat_map(f32::to_le_bytes),
        )
    }

    fn push_u32_rows<'b>(
        &mut self,
        rows: impl Iterator<Item = &'b [u32]>,
        components: usize,
    ) -> RendererResult<BufferView> {
        let values = rows.flatten().copied().collect::<Vec<_>>();
        if components == 0 || values.len() % components != 0 {
            return Err(RendererError::scene("invalid packed u32 row width"));
        }
        self.push_typed(
            values.len() / components,
            components,
            BufferComponent::U32,
            values.into_iter().flat_map(u32::to_le_bytes),
        )
    }

    fn push_f32(&mut self, values: impl IntoIterator<Item = f32>) -> RendererResult<BufferView> {
        let values = values.into_iter().collect::<Vec<_>>();
        self.push_typed(
            values.len(),
            1,
            BufferComponent::F32,
            values.into_iter().flat_map(f32::to_le_bytes),
        )
    }

    fn push_u32(&mut self, values: impl IntoIterator<Item = u32>) -> RendererResult<BufferView> {
        let values = values.into_iter().collect::<Vec<_>>();
        self.push_typed(
            values.len(),
            1,
            BufferComponent::U32,
            values.into_iter().flat_map(u32::to_le_bytes),
        )
    }

    fn push_i32(&mut self, values: impl IntoIterator<Item = i32>) -> RendererResult<BufferView> {
        let values = values.into_iter().collect::<Vec<_>>();
        self.push_typed(
            values.len(),
            1,
            BufferComponent::I32,
            values.into_iter().flat_map(i32::to_le_bytes),
        )
    }

    fn push_u8(
        &mut self,
        values: impl IntoIterator<Item = u8>,
        components: usize,
    ) -> RendererResult<BufferView> {
        let values = values.into_iter().collect::<Vec<_>>();
        if components == 0 || values.len() % components != 0 {
            return Err(RendererError::scene("invalid packed u8 row width"));
        }
        self.push_typed(
            values.len() / components,
            components,
            BufferComponent::U8,
            values,
        )
    }

    fn push_typed(
        &mut self,
        element_count: usize,
        components_per_element: usize,
        component: BufferComponent,
        bytes: impl IntoIterator<Item = u8>,
    ) -> RendererResult<BufferView> {
        while !self.binary.len().is_multiple_of(4) {
            self.binary.push(0);
        }
        let byte_offset = self.binary.len();
        self.binary.extend(bytes);
        let byte_length = self
            .binary
            .len()
            .checked_sub(byte_offset)
            .ok_or_else(|| RendererError::scene("packed buffer length underflow"))?;
        Ok(BufferView {
            byte_offset,
            byte_length,
            component,
            components_per_element,
            element_count,
        })
    }
}

fn pack_node(node: &NwnSceneNode) -> PacketNode {
    PacketNode {
        kind:                    node_kind_name(&node.kind),
        node_type:               node.node_type.clone(),
        name:                    node.name.clone(),
        parent:                  node.parent,
        part_number:             node.part_number,
        translation:             node.local_transform.translation,
        rotation_axis_angle:     node.local_transform.rotation_axis_angle,
        scale:                   node.local_transform.scale,
        center:                  node.center,
        color:                   node.color,
        radius:                  node.radius,
        alpha:                   node.alpha,
        wirecolor:               node.wirecolor,
        mesh:                    node.mesh,
        light:                   node.light.as_ref().map(|light| PacketLight {
            multiplier:            light.multiplier,
            ambient_only:          light.ambient_only,
            n_dynamic_type:        light.n_dynamic_type,
            is_dynamic:            light.is_dynamic,
            affect_dynamic:        light.affect_dynamic,
            negative_light:        light.negative_light,
            light_priority:        light.light_priority,
            fading_light:          light.fading_light,
            lens_flares:           light.lens_flares,
            flare_radius:          light.flare_radius,
            shadow_radius:         light.shadow_radius,
            vertical_displacement: light.vertical_displacement,
            flare_textures:        light.flare_textures.clone(),
            flare_sizes:           light.flare_sizes.clone(),
            flare_positions:       light.flare_positions.clone(),
            flare_color_shifts:    light.flare_color_shifts.clone(),
        }),
        emitter:                 node.emitter.as_ref().map(|emitter| PacketEmitter {
            x_size:     emitter.x_size,
            y_size:     emitter.y_size,
            properties: emitter
                .properties
                .iter()
                .map(|property| PacketEmitterProperty {
                    name:   property.name.clone(),
                    values: property
                        .values
                        .iter()
                        .map(|value| match value {
                            NwnPropertyValue::Bool(value) => PacketPropertyValue::Bool(*value),
                            NwnPropertyValue::Int(value) => PacketPropertyValue::Int(*value),
                            NwnPropertyValue::Float(value) => PacketPropertyValue::Float(*value),
                            NwnPropertyValue::Text(value) => {
                                PacketPropertyValue::Text(value.clone())
                            }
                        })
                        .collect(),
                })
                .collect(),
        }),
        dangly:                  node.dangly.as_ref().map(|dangly| PacketDangly {
            displacement: dangly.displacement,
            tightness:    dangly.tightness,
            period:       dangly.period,
        }),
        reference_model:         node
            .reference
            .as_ref()
            .and_then(|reference| reference.model.clone()),
        reference_reattachable:  node
            .reference
            .as_ref()
            .map(|reference| reference.reattachable),
        opaque_controller_count: node.opaque_controllers.len(),
    }
}

fn pack_material(material: &NwnMaterial) -> PacketMaterial {
    PacketMaterial {
        source_node:       material.source_node,
        render_enabled:    material.render_enabled,
        shadow_enabled:    material.shadow_enabled,
        beaming:           material.beaming,
        inherit_color:     material.inherit_color,
        tilefade:          material.tilefade,
        rotate_texture:    material.rotate_texture,
        light_mapped:      material.light_mapped,
        transparency_hint: material.transparency_hint,
        shininess:         material.shininess,
        alpha:             material.alpha,
        ambient:           material.ambient,
        diffuse:           material.diffuse,
        specular:          material.specular,
        self_illum_color:  material.self_illum_color,
        material_name:     material.material_name.clone(),
        render_hint:       material.render_hint.clone(),
        helper_bitmap:     material.helper_bitmap.clone(),
        textures:          material
            .textures
            .iter()
            .map(|texture| PacketTextureBinding {
                slot: match texture.slot {
                    NwnTextureSlot::Bitmap => "bitmap".into(),
                    NwnTextureSlot::Texture(index) => format!("texture{index}"),
                },
                name: texture.name.clone(),
            })
            .collect(),
    }
}

fn flattened_model_scene(scene: &RenderScene, target: usize) -> Option<&NwnScene> {
    fn visit_composed<'a>(
        composed: &'a NwnComposedScene,
        target: usize,
        cursor: &mut usize,
    ) -> Option<&'a NwnScene> {
        if *cursor == target {
            return Some(&composed.scene);
        }
        *cursor += 1;
        for attachment in &composed.attachments {
            if let Some(scene) = visit_composed(&attachment.scene, target, cursor) {
                return Some(scene);
            }
        }
        None
    }

    let mut cursor = 0;
    for model in &scene.models {
        match model {
            ModelScene::Composed(composed) => {
                if let Some(scene) = visit_composed(composed, target, &mut cursor) {
                    return Some(scene);
                }
            }
            ModelScene::Auxiliary(scene) => {
                if cursor == target {
                    return Some(scene);
                }
                cursor += 1;
            }
        }
    }
    None
}

fn decoded_texture_rgba8(texture: &RenderTexture) -> RendererResult<Vec<u8>> {
    if !texture.rgba8.is_empty() {
        return Ok(texture.rgba8.clone());
    }
    let compressed = texture.compressed.as_ref().ok_or_else(|| {
        RendererError::scene(format!("{} has no texture pixel payload", texture.resource))
    })?;
    let mip = compressed.mip_levels.first().ok_or_else(|| {
        RendererError::scene(format!("{} has no DDS mip levels", texture.resource))
    })?;
    DdsMipLevel {
        level:  0,
        width:  mip.width,
        height: mip.height,
        data:   mip.data.clone(),
    }
    .decode_rgba8(match compressed.compression {
        RenderTextureCompression::Dxt1 => DdsFormat::Dxt1,
        RenderTextureCompression::Dxt5 => DdsFormat::Dxt5,
    })
    .map_err(|error| RendererError::scene(format!("decode {}: {error}", texture.resource)))
}

fn node_kind_name(kind: &NodeKind) -> String {
    match kind {
        NodeKind::Dummy => "dummy",
        NodeKind::Trimesh => "trimesh",
        NodeKind::Danglymesh => "danglymesh",
        NodeKind::Skin => "skin",
        NodeKind::Emitter => "emitter",
        NodeKind::Light => "light",
        NodeKind::Aabb => "aabb",
        NodeKind::Reference => "reference",
        NodeKind::Camera => "camera",
        NodeKind::Patch => "patch",
        NodeKind::Animmesh => "animmesh",
        NodeKind::Other(value) => value,
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use nwnrs_types::mdl::{ModelResourceKind, parse_scene_resource_auto};

    use crate::{
        DependencyGraph, ModelScene, RenderEnvironment, RenderInstance, RenderInstanceKind,
        RenderScene, ScenePacket, SceneSource,
    };

    #[test]
    fn scene_packet_roundtrip_preserves_manifest_and_binary_geometry() {
        let model = parse_scene_resource_auto(
            ModelResourceKind::Model,
            "triangle",
            b"newmodel triangle\nsetsupermodel triangle null\nbeginmodelgeom triangle\nnode trimesh triangle\n parent null\n verts 3\n  0 0 0\n  1 0 0\n  0 1 0\n faces 1\n  0 1 2 0 0 1 2 0\nendnode\nendmodelgeom triangle\nnewanim pulse triangle\n length 1\n node dummy triangle\n  parent null\n  positionkey 2\n   0 0 0 0\n   1 1 2 3\n  alphakey 2\n   0 1\n   1 0.5\n endnode\ndoneanim pulse triangle\ndonemodel triangle\n",
        )
        .unwrap_or_else(|error| panic!("parse model: {error}"));
        let scene = RenderScene {
            name:         "triangle".into(),
            source:       SceneSource::Model,
            models:       vec![ModelScene::Auxiliary(model)],
            model_assets: vec![crate::RenderModelAssets {
                model_name:    "triangle".into(),
                materials:     Vec::new(),
                node_textures: Vec::new(),
                attachments:   Vec::new(),
            }],
            textures:     Vec::new(),
            shaders:      Vec::new(),
            instances:    vec![RenderInstance {
                id:                    0,
                object_key:            None,
                label:                 "triangle".into(),
                kind:                  RenderInstanceKind::Model,
                model:                 Some(0),
                resource:              Some("triangle.mdl".into()),
                position:              [0.0; 3],
                rotation_axis_angle:   [0.0, 0.0, 1.0, 0.0],
                scale:                 [1.0; 3],
                polygon:               Vec::new(),
                light_color_overrides: [None; 4],
            }],
            area:         None,
            module:       None,
            environment:  RenderEnvironment::Studio,
            dependencies: DependencyGraph::default(),
            diagnostics:  Vec::new(),
        };

        let packet = ScenePacket::from_scene(&scene)
            .unwrap_or_else(|error| panic!("build scene packet: {error}"));
        let encoded = packet
            .encode()
            .unwrap_or_else(|error| panic!("encode packet: {error}"));
        let manifest_length = u32::from_le_bytes(
            encoded[8..12]
                .try_into()
                .unwrap_or_else(|_| panic!("packet manifest length")),
        ) as usize;
        assert_eq!((12 + manifest_length) % 4, 0);
        let decoded =
            ScenePacket::decode(&encoded).unwrap_or_else(|error| panic!("decode packet: {error}"));

        assert_eq!(decoded.manifest.name, "triangle");
        assert_eq!(decoded.manifest.models.len(), 1);
        let model = decoded
            .manifest
            .models
            .first()
            .unwrap_or_else(|| panic!("expected a packed model"));
        assert_eq!(model.meshes.len(), 1);
        let animation = model
            .animations
            .first()
            .unwrap_or_else(|| panic!("expected a packed animation"));
        assert_eq!(animation.name, "pulse");
        assert_eq!(animation.node_tracks.len(), 1);
        let node_track = animation
            .node_tracks
            .first()
            .unwrap_or_else(|| panic!("expected a packed node track"));
        assert_eq!(node_track.translation.times.element_count, 2);
        assert_eq!(node_track.translation.values.components_per_element, 3);
        assert!(!decoded.binary.is_empty());
        assert_eq!(decoded, packet);

        let catalog = ScenePacket::catalog_from_scene(&scene)
            .unwrap_or_else(|error| panic!("build scene catalog: {error}"));
        let catalog_animation = &catalog.manifest.models[0].animations[0];
        assert!(!catalog_animation.tracks_loaded);
        assert!(catalog_animation.node_tracks.is_empty());
        assert!(catalog.binary.len() < packet.binary.len());

        let selected = ScenePacket::animation_from_scene(&scene, 0, 0)
            .unwrap_or_else(|error| panic!("build animation packet: {error}"));
        assert_eq!(selected.manifest.model_index, 0);
        assert_eq!(selected.manifest.animation_index, 0);
        assert!(selected.manifest.animation.tracks_loaded);
        assert_eq!(selected.manifest.animation.node_tracks.len(), 1);
        assert!(!selected.binary.is_empty());
        let selected_encoded = selected
            .encode()
            .unwrap_or_else(|error| panic!("encode animation packet: {error}"));
        let selected_manifest_length = u32::from_le_bytes(
            selected_encoded[8..12]
                .try_into()
                .unwrap_or_else(|_| panic!("animation manifest length")),
        ) as usize;
        assert_eq!((12 + selected_manifest_length) % 4, 0);
    }

    #[test]
    fn compressed_texture_packet_preserves_authored_bottom_first_rows() {
        let authored = vec![0, 1, 2, 3, 9, 8, 7, 6];
        let scene = RenderScene {
            name:         "texture".into(),
            source:       SceneSource::Model,
            models:       Vec::new(),
            model_assets: Vec::new(),
            textures:     vec![crate::RenderTexture {
                resource:   "texture.dds".into(),
                origin:     "test".into(),
                kind:       crate::RenderTextureKind::Dds,
                width:      4,
                height:     4,
                rgba8:      Vec::new(),
                compressed: Some(crate::RenderCompressedTexture {
                    compression: crate::RenderTextureCompression::Dxt1,
                    mip_levels:  vec![crate::RenderTextureMip {
                        width:  4,
                        height: 4,
                        data:   authored.clone(),
                    }],
                }),
            }],
            shaders:      Vec::new(),
            instances:    Vec::new(),
            area:         None,
            module:       None,
            environment:  RenderEnvironment::Studio,
            dependencies: DependencyGraph::default(),
            diagnostics:  Vec::new(),
        };
        let packet = ScenePacket::texture_from_scene(&scene, 0, true)
            .unwrap_or_else(|error| panic!("build compressed texture packet: {error}"));
        let encoded = packet
            .encode()
            .unwrap_or_else(|error| panic!("encode compressed texture packet: {error}"));
        let manifest_length = u32::from_le_bytes(
            encoded[8..12]
                .try_into()
                .unwrap_or_else(|_| panic!("texture manifest length")),
        ) as usize;
        assert_eq!((12 + manifest_length) % 4, 0);
        let view = packet.manifest.mip_levels[0].data;
        assert_eq!(
            &packet.binary[view.byte_offset..view.byte_offset + view.byte_length],
            authored
        );
    }
}
