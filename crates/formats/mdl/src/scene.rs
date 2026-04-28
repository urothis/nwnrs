use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Write},
    path::Path,
};

use nwnrs_resman::prelude::*;
use tracing::instrument;

use crate::{
    AnimationEvent, MODEL_RES_TYPE, Model, ModelClassification, ModelDiagnostic,
    ModelDiagnosticKind, ModelError, ModelResult, NodeKind, ScalarKey, SemanticAnimation,
    SemanticAnimationNode, SemanticEmitter, SemanticEmitterProperty, SemanticFace, SemanticHeader,
    SemanticLight, SemanticMaterial, SemanticMesh, SemanticModel, SemanticNode,
    SemanticPropertyValue, SemanticReference, SemanticSkinWeight, SemanticTextureBinding,
    SemanticUvLayer, Vec3Key, Vec4Key, parse_semantic_model, parse_semantic_model_auto,
    read_semantic_model, read_semantic_model_auto,
};

/// An engine-neutral scene representation lowered from a semantic NWN model.
///
/// Scene nodes, meshes, materials, and animations remain in authored order so
/// downstream renderers or tools can preserve stable references back to the
/// source model. This layer makes normalization explicit without erasing NWN
/// model concepts such as references, helpers, and authored animation names.
#[derive(Debug, Clone, PartialEq)]
pub struct NwnScene {
    /// Model name.
    pub name:              String,
    /// Optional supermodel name.
    pub supermodel:        Option<String>,
    /// Optional model classification.
    pub classification:    Option<ModelClassification>,
    /// Optional animation scale.
    pub animation_scale:   Option<f32>,
    /// Coordinate system used by this scene.
    pub coordinate_system: NwnCoordinateSystem,
    /// Scene graph nodes in source order.
    pub nodes:             Vec<NwnSceneNode>,
    /// Mesh payloads referenced by scene nodes.
    pub meshes:            Vec<NwnMesh>,
    /// Materials referenced by scene meshes.
    pub materials:         Vec<NwnMaterial>,
    /// Animation tracks in source order.
    pub animations:        Vec<NwnAnimation>,
    /// Diagnostics propagated or produced during scene lowering.
    pub diagnostics:       Vec<ModelDiagnostic>,
}

impl NwnScene {
    /// Returns the first scene node named `name`, case-insensitively.
    #[must_use]
    pub fn node(&self, name: &str) -> Option<&NwnSceneNode> {
        self.nodes
            .iter()
            .find(|node| node.name.eq_ignore_ascii_case(name))
    }

    /// Returns the first animation named `name`, case-insensitively.
    #[must_use]
    pub fn animation(&self, name: &str) -> Option<&NwnAnimation> {
        self.animations
            .iter()
            .find(|animation| animation.name.eq_ignore_ascii_case(name))
    }

    /// Reads and lowers an engine-neutral scene from disk.
    ///
    /// # Errors
    ///
    /// Returns [`ModelError`] if the file cannot be opened or parsed.
    pub fn from_file(path: impl AsRef<Path>) -> ModelResult<Self> {
        let mut file = File::open(path.as_ref())?;
        read_scene_model(&mut file)
    }

    /// Reads and lowers an engine-neutral scene from disk using automatic
    /// ASCII/compiled dispatch.
    ///
    /// # Errors
    ///
    /// Returns [`ModelError`] if the file cannot be opened or parsed.
    pub fn from_auto_file(path: impl AsRef<Path>) -> ModelResult<Self> {
        let mut file = File::open(path.as_ref())?;
        read_scene_model_auto(&mut file)
    }

    /// Reads and lowers an engine-neutral scene from a [`Res`].
    ///
    /// # Errors
    ///
    /// Returns [`ModelError`] if the resource is not an MDL type or lowering
    /// fails.
    pub fn from_res(res: &Res, cache_policy: CachePolicy) -> ModelResult<Self> {
        if res.resref().res_type() != MODEL_RES_TYPE {
            return Err(ModelError::msg(format!(
                "expected mdl resource, got {}",
                res.resref()
            )));
        }

        let semantic = crate::SemanticModel::from_res(res, cache_policy)?;
        lower_semantic_model_to_scene(&semantic)
    }

    /// Reads and lowers an engine-neutral scene from a [`Res`] using automatic
    /// ASCII/compiled dispatch.
    ///
    /// # Errors
    ///
    /// Returns [`ModelError`] if the resource is not an MDL type or lowering
    /// fails.
    pub fn from_auto_res(res: &Res, cache_policy: CachePolicy) -> ModelResult<Self> {
        if res.resref().res_type() != MODEL_RES_TYPE {
            return Err(ModelError::msg(format!(
                "expected mdl resource, got {}",
                res.resref()
            )));
        }

        let semantic = crate::SemanticModel::from_auto_res(res, cache_policy)?;
        lower_semantic_model_to_scene(&semantic)
    }
}

/// Coordinate system metadata for the lowered scene.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NwnCoordinateSystem {
    /// Raw Aurora / NWN source-space coordinates and axis-angle rotations.
    AuroraSource,
}

/// One node in the lowered scene graph.
#[derive(Debug, Clone, PartialEq)]
pub struct NwnSceneNode {
    /// Typed node kind.
    pub kind:            NodeKind,
    /// Authored node type token.
    pub node_type:       String,
    /// Node name.
    pub name:            String,
    /// Parent node index in [`NwnScene::nodes`], when resolved.
    pub parent:          Option<usize>,
    /// Parsed `#part-number` value when present.
    pub part_number:     Option<i32>,
    /// Local transform in Aurora source space.
    pub local_transform: NwnTransform,
    /// Optional node center metadata.
    pub center:          Option<[f32; 3]>,
    /// Optional static node color metadata.
    pub color:           Option<[f32; 3]>,
    /// Optional static node radius metadata.
    pub radius:          Option<f32>,
    /// Optional static node alpha metadata.
    pub alpha:           Option<f32>,
    /// Optional wireframe color metadata.
    pub wirecolor:       Option<[f32; 3]>,
    /// Typed light payload when this scene node is a light.
    pub light:           Option<NwnLight>,
    /// Typed emitter payload when this scene node is an emitter.
    pub emitter:         Option<NwnEmitter>,
    /// Typed reference payload when this scene node is a reference.
    pub reference:       Option<NwnReference>,
    /// Referenced mesh index in [`NwnScene::meshes`], when present.
    pub mesh:            Option<usize>,
}

/// A local transform expressed in Aurora source-space conventions.
#[derive(Debug, Clone, PartialEq)]
pub struct NwnTransform {
    /// Translation vector.
    pub translation:         [f32; 3],
    /// Axis-angle rotation stored as `[axis_x, axis_y, axis_z, angle_radians]`.
    pub rotation_axis_angle: [f32; 4],
    /// Per-axis scale.
    pub scale:               [f32; 3],
}

/// One lowered mesh.
#[derive(Debug, Clone, PartialEq)]
pub struct NwnMesh {
    /// Mesh name, typically matching the source node name.
    pub name:        String,
    /// Scene node index that owns this mesh.
    pub source_node: usize,
    /// Mesh primitives in source order.
    pub primitives:  Vec<NwnPrimitive>,
}

/// One primitive inside a lowered mesh.
#[derive(Debug, Clone, PartialEq)]
pub struct NwnPrimitive {
    /// Position stream.
    pub positions:       Vec<[f32; 3]>,
    /// Face list with explicit vertex and UV indices.
    pub faces:           Vec<NwnFace>,
    /// UV sets in authored order.
    pub uv_sets:         Vec<NwnUvSet>,
    /// Normal stream.
    pub normals:         Vec<[f32; 3]>,
    /// Tangent rows preserved from the source mesh.
    pub tangents:        Vec<Vec<f32>>,
    /// Color rows preserved from the source mesh.
    pub color_rows:      Vec<Vec<f32>>,
    /// Named skin-weight rows preserved from the source mesh.
    pub weight_rows:     Vec<Vec<NwnSkinWeight>>,
    /// Constraint rows preserved from the source mesh.
    pub constraint_rows: Vec<Vec<f32>>,
    /// Surface labels from `multimaterial`.
    pub surface_labels:  Vec<String>,
    /// Additional texture names from `texturenames`.
    pub texture_names:   Vec<String>,
    /// Material index in [`NwnScene::materials`], when present.
    pub material:        Option<usize>,
}

/// One named skin-weight influence.
#[derive(Debug, Clone, PartialEq)]
pub struct NwnSkinWeight {
    /// Bone or node name referenced by the source skin row.
    pub bone:   String,
    /// Influence weight for this bone.
    pub weight: f32,
}

/// One lowered face entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NwnFace {
    /// Vertex indices.
    pub vertex_indices: [u32; 3],
    /// Face grouping / smoothing field.
    pub group:          i32,
    /// UV indices.
    pub uv_indices:     [u32; 3],
    /// Face material index / surface slot field.
    pub material_index: i32,
}

/// One UV set.
#[derive(Debug, Clone, PartialEq)]
pub struct NwnUvSet {
    /// UV set index.
    pub index:       usize,
    /// UV coordinates.
    pub coordinates: Vec<[f32; 2]>,
}

/// One lowered material.
#[derive(Debug, Clone, PartialEq)]
pub struct NwnMaterial {
    /// Scene node index that authored this material.
    pub source_node:       usize,
    /// Whether rendering is enabled.
    pub render_enabled:    bool,
    /// Whether shadows are enabled.
    pub shadow_enabled:    bool,
    /// `beaming`
    pub beaming:           i32,
    /// `inheritcolor`
    pub inherit_color:     i32,
    /// `tilefade`
    pub tilefade:          i32,
    /// `rotatetexture`
    pub rotate_texture:    i32,
    /// `transparencyhint`
    pub transparency_hint: i32,
    /// `shininess`
    pub shininess:         f32,
    /// `alpha`
    pub alpha:             f32,
    /// `ambient`
    pub ambient:           [f32; 3],
    /// `diffuse`
    pub diffuse:           [f32; 3],
    /// `specular`
    pub specular:          [f32; 3],
    /// `selfillumcolor`
    pub self_illum_color:  [f32; 3],
    /// `materialname`
    pub material_name:     Option<String>,
    /// `renderhint`
    pub render_hint:       Option<String>,
    /// Helper bitmap token authored on non-render helper geometry such as
    /// `Aabb` walkmeshes/collision meshes.
    pub helper_bitmap:     Option<String>,
    /// Texture references attached to this material.
    pub textures:          Vec<NwnTextureRef>,
}

/// One typed texture reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NwnTextureRef {
    /// Texture binding slot.
    pub slot: NwnTextureSlot,
    /// Resolved texture name.
    pub name: String,
}

/// Texture binding slots carried by scene materials.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NwnTextureSlot {
    /// Primary `bitmap` texture.
    Bitmap,
    /// `textureN`
    Texture(usize),
}

/// One lowered animation.
#[derive(Debug, Clone, PartialEq)]
pub struct NwnAnimation {
    /// Animation name.
    pub name:            String,
    /// Referenced model name.
    pub model_name:      String,
    /// Animation length in seconds.
    pub length:          f32,
    /// Transition time in seconds.
    pub transition_time: f32,
    /// Authored animation root name.
    pub root_name:       Option<String>,
    /// Resolved animation root node index.
    pub root_node:       Option<usize>,
    /// Animation events.
    pub events:          Vec<AnimationEvent>,
    /// Per-node animation tracks.
    pub node_tracks:     Vec<NwnNodeAnimationTrack>,
}

impl NwnAnimation {
    /// Returns the first node track named `name`, case-insensitively.
    #[must_use]
    pub fn node_track(&self, name: &str) -> Option<&NwnNodeAnimationTrack> {
        self.node_tracks
            .iter()
            .find(|track| track.target_name.eq_ignore_ascii_case(name))
    }
}

/// One lowered node animation track.
#[derive(Debug, Clone, PartialEq)]
pub struct NwnNodeAnimationTrack {
    /// Target node name from the source animation.
    pub target_name: String,
    /// Resolved target node index.
    pub target_node: Option<usize>,
    /// Typed node kind.
    pub kind:        NodeKind,
    /// Local transform animation channels.
    pub transform:   NwnTransformTrack,
    /// Material- or light-style channels.
    pub material:    NwnMaterialTrack,
    /// Animmesh payload, when present.
    pub animmesh:    Option<NwnAnimMeshTrack>,
}

/// Transform animation channels for one node.
#[derive(Debug, Clone, PartialEq)]
pub struct NwnTransformTrack {
    /// Translation keys.
    pub translation_keys:         Vec<Vec3Key>,
    /// Axis-angle rotation keys.
    pub rotation_axis_angle_keys: Vec<Vec4Key>,
    /// Per-axis scale keys.
    pub scale_keys:               Vec<Vec3Key>,
}

/// Non-transform animation channels for one node.
#[derive(Debug, Clone, PartialEq)]
pub struct NwnMaterialTrack {
    /// Color keys.
    pub color_keys:            Vec<Vec3Key>,
    /// Radius keys.
    pub radius_keys:           Vec<ScalarKey>,
    /// Alpha keys.
    pub alpha_keys:            Vec<ScalarKey>,
    /// Self-illumination color keys.
    pub self_illum_color_keys: Vec<Vec3Key>,
}

/// Typed light payload lowered onto a scene node.
#[derive(Debug, Clone, PartialEq)]
pub struct NwnLight {
    /// `multiplier`
    pub multiplier:         f32,
    /// `ambientonly`
    pub ambient_only:       i32,
    /// `ndynamictype`
    pub n_dynamic_type:     Option<i32>,
    /// `isdynamic`
    pub is_dynamic:         i32,
    /// `affectdynamic`
    pub affect_dynamic:     i32,
    /// `negativelight`
    pub negative_light:     i32,
    /// `lightpriority`
    pub light_priority:     i32,
    /// `fadinglight`
    pub fading_light:       i32,
    /// `lensflares`
    pub lens_flares:        i32,
    /// `flareradius`
    pub flare_radius:       f32,
    /// `texturenames`
    pub flare_textures:     Vec<String>,
    /// `flaresizes`
    pub flare_sizes:        Vec<f32>,
    /// `flarepositions`
    pub flare_positions:    Vec<f32>,
    /// `flarecolorshifts`
    pub flare_color_shifts: Vec<[f32; 3]>,
}

/// Typed emitter payload lowered onto a scene node.
#[derive(Debug, Clone, PartialEq)]
pub struct NwnEmitter {
    /// `xsize`
    pub x_size:     f32,
    /// `ysize`
    pub y_size:     f32,
    /// Remaining emitter properties in source order.
    pub properties: Vec<NwnEmitterProperty>,
}

/// One typed emitter property statement.
#[derive(Debug, Clone, PartialEq)]
pub struct NwnEmitterProperty {
    /// Source keyword.
    pub name:   String,
    /// Typed property values in authored order.
    pub values: Vec<NwnPropertyValue>,
}

/// One typed scalar/string property value.
#[derive(Debug, Clone, PartialEq)]
pub enum NwnPropertyValue {
    /// Boolean token.
    Bool(bool),
    /// Integer token.
    Int(i32),
    /// Floating-point token.
    Float(f32),
    /// Text token.
    Text(String),
}

/// Typed reference payload lowered onto a scene node.
#[derive(Debug, Clone, PartialEq)]
pub struct NwnReference {
    /// `refmodel`
    pub model:        Option<String>,
    /// `reattachable`
    pub reattachable: i32,
}

/// Animmesh payload lowered into sample groups.
#[derive(Debug, Clone, PartialEq)]
pub struct NwnAnimMeshTrack {
    /// Sample period in seconds.
    pub sample_period:  Option<f32>,
    /// Face overrides authored inside the animation node.
    pub face_overrides: Vec<NwnFace>,
    /// Animated vertex samples grouped by source-mesh vertex count.
    pub vertex_samples: Vec<NwnVec3Sample>,
    /// Animated UV samples grouped by source-mesh UV count.
    pub uv_samples:     Vec<NwnVec2Sample>,
}

/// One sampled vector3 frame.
#[derive(Debug, Clone, PartialEq)]
pub struct NwnVec3Sample {
    /// Sampled values.
    pub values: Vec<[f32; 3]>,
}

/// One sampled vector2 frame.
#[derive(Debug, Clone, PartialEq)]
pub struct NwnVec2Sample {
    /// Sampled values.
    pub values: Vec<[f32; 2]>,
}

type SceneWriteOwnership = (Vec<Option<usize>>, Vec<Option<usize>>);

impl Model {
    /// Parses and lowers the raw payload into an engine-neutral NWN scene.
    ///
    /// # Errors
    ///
    /// Returns [`ModelError`] if parsing or lowering fails.
    pub fn parse_scene(&self) -> ModelResult<NwnScene> {
        lower_semantic_model_to_scene(&self.parse_semantic()?)
    }

    /// Parses and lowers the raw payload into an engine-neutral NWN scene
    /// using automatic ASCII/compiled dispatch.
    ///
    /// # Errors
    ///
    /// Returns [`ModelError`] if parsing or lowering fails.
    pub fn parse_scene_auto(&self) -> ModelResult<NwnScene> {
        lower_semantic_model_to_scene(&self.parse_semantic_auto()?)
    }
}

/// Parses and lowers an engine-neutral scene from ASCII MDL text.
///
/// # Errors
///
/// Returns [`ModelError`] if the text cannot be parsed or lowered.
pub fn parse_scene_model(text: &str) -> ModelResult<NwnScene> {
    lower_semantic_model_to_scene(&parse_semantic_model(text)?)
}

/// Parses and lowers an engine-neutral scene from raw MDL bytes using
/// automatic ASCII/compiled dispatch.
///
/// # Errors
///
/// Returns [`ModelError`] if the bytes cannot be parsed or lowered.
pub fn parse_scene_model_auto(bytes: &[u8]) -> ModelResult<NwnScene> {
    lower_semantic_model_to_scene(&parse_semantic_model_auto(bytes)?)
}

/// Reads and lowers an engine-neutral scene from `reader`.
///
/// # Errors
///
/// Returns [`ModelError`] if the data cannot be read or lowered.
#[instrument(level = "debug", skip_all, err)]
pub fn read_scene_model<R: Read>(reader: &mut R) -> ModelResult<NwnScene> {
    let semantic = read_semantic_model(reader)?;
    lower_semantic_model_to_scene(&semantic)
}

/// Reads and lowers an engine-neutral scene from `reader` using automatic
/// ASCII/compiled dispatch.
///
/// # Errors
///
/// Returns [`ModelError`] if the data cannot be read or lowered.
#[instrument(level = "debug", skip_all, err)]
pub fn read_scene_model_auto<R: Read>(reader: &mut R) -> ModelResult<NwnScene> {
    let semantic = read_semantic_model_auto(reader)?;
    lower_semantic_model_to_scene(&semantic)
}

/// Writes an engine-neutral scene as canonical ASCII MDL.
///
/// # Errors
///
/// Returns [`ModelError`] if raising the scene or writing fails.
#[instrument(level = "debug", skip_all, err, fields(model_name = %scene.name))]
pub fn write_scene_model<W: Write>(writer: &mut W, scene: &NwnScene) -> ModelResult<()> {
    let semantic = raise_scene_to_semantic(scene)?;
    crate::write_semantic_model(writer, &semantic)
}

fn raise_scene_to_semantic(scene: &NwnScene) -> ModelResult<SemanticModel> {
    let (mesh_owners, material_owners) = validate_scene_for_write(scene)?;

    let nodes = scene
        .nodes
        .iter()
        .enumerate()
        .map(|(node_index, node)| {
            raise_scene_node(scene, node_index, node, &mesh_owners, &material_owners)
        })
        .collect::<ModelResult<Vec<_>>>()?;
    let animations = scene
        .animations
        .iter()
        .map(|animation| raise_scene_animation(scene, animation))
        .collect::<ModelResult<Vec<_>>>()?;

    Ok(SemanticModel {
        header: SemanticHeader {
            model_name:      scene.name.clone(),
            supermodel:      scene.supermodel.clone(),
            classification:  scene.classification.clone(),
            animation_scale: scene.animation_scale,
            comments:        Vec::new(),
            extras:          Vec::new(),
        },
        geometry_name: scene.name.clone(),
        nodes,
        geometry_extras: Vec::new(),
        between_geometry_and_animations: Vec::new(),
        animations,
        between_animations: if scene.animations.len() > 1 {
            vec![Vec::new(); scene.animations.len() - 1]
        } else {
            Vec::new()
        },
        suffix: Vec::new(),
        diagnostics: scene.diagnostics.clone(),
    })
}

fn validate_scene_for_write(scene: &NwnScene) -> ModelResult<SceneWriteOwnership> {
    let node_count = scene.nodes.len();
    let mut mesh_owners = vec![None; scene.meshes.len()];

    for (node_index, node) in scene.nodes.iter().enumerate() {
        if let Some(parent_index) = node.parent {
            if parent_index >= node_count {
                return Err(ModelError::msg(format!(
                    "scene node {} references invalid parent index {}",
                    node.name, parent_index
                )));
            }
            if parent_index == node_index {
                return Err(ModelError::msg(format!(
                    "scene node {} cannot parent itself",
                    node.name
                )));
            }
        }

        if let Some(mesh_index) = node.mesh {
            let owner = mesh_owners.get_mut(mesh_index).ok_or_else(|| {
                ModelError::msg(format!(
                    "scene node {} references invalid mesh index {}",
                    node.name, mesh_index
                ))
            })?;
            if let Some(previous_owner) = owner.replace(node_index) {
                let previous_name = scene
                    .nodes
                    .get(previous_owner)
                    .map_or("<invalid>", |node| node.name.as_str());
                return Err(ModelError::msg(format!(
                    "scene mesh {} is referenced by both {} and {}",
                    mesh_index, previous_name, node.name
                )));
            }
        }

        validate_uniform_scale(&node.local_transform.scale, &node.name)?;
    }

    let mut material_owners = vec![None; scene.materials.len()];
    for (mesh_index, mesh) in scene.meshes.iter().enumerate() {
        let owner_index = mesh_owners
            .get(mesh_index)
            .copied()
            .flatten()
            .ok_or_else(|| {
                ModelError::msg(format!(
                    "scene mesh {} is not referenced by any scene node",
                    mesh.name
                ))
            })?;
        if mesh.source_node >= node_count {
            return Err(ModelError::msg(format!(
                "scene mesh {} records invalid source node index {}",
                mesh.name, mesh.source_node
            )));
        }
        if mesh.source_node != owner_index {
            let owner_name = scene
                .nodes
                .get(owner_index)
                .map_or("<invalid>", |node| node.name.as_str());
            return Err(ModelError::msg(format!(
                "scene mesh {} source node {} does not match owner {}",
                mesh.name, mesh.source_node, owner_name
            )));
        }
        if mesh.primitives.len() != 1 {
            return Err(ModelError::msg(format!(
                "scene mesh {} must contain exactly one primitive to serialize as NWN MDL",
                mesh.name
            )));
        }

        let primitive = mesh.primitives.first().ok_or_else(|| {
            ModelError::msg(format!("scene mesh {} is missing its primitive", mesh.name))
        })?;
        let material_index = primitive.material.ok_or_else(|| {
            ModelError::msg(format!(
                "scene mesh {} is missing a material reference",
                mesh.name
            ))
        })?;
        let material_owner = material_owners.get_mut(material_index).ok_or_else(|| {
            ModelError::msg(format!(
                "scene mesh {} references invalid material index {}",
                mesh.name, material_index
            ))
        })?;
        if let Some(previous_owner) = material_owner.replace(owner_index) {
            let previous_name = scene
                .nodes
                .get(previous_owner)
                .map_or("<invalid>", |node| node.name.as_str());
            let owner_name = scene
                .nodes
                .get(owner_index)
                .map_or("<invalid>", |node| node.name.as_str());
            return Err(ModelError::msg(format!(
                "scene material {material_index} is referenced by both {previous_name} and \
                 {owner_name}"
            )));
        }
    }

    for (material_index, material) in scene.materials.iter().enumerate() {
        let owner_index = material_owners
            .get(material_index)
            .copied()
            .flatten()
            .ok_or_else(|| {
                ModelError::msg(format!(
                    "scene material {material_index} is not referenced by any mesh"
                ))
            })?;
        if material.source_node >= node_count {
            return Err(ModelError::msg(format!(
                "scene material {} records invalid source node index {}",
                material_index, material.source_node
            )));
        }
        if material.source_node != owner_index {
            let owner_name = scene
                .nodes
                .get(owner_index)
                .map_or("<invalid>", |node| node.name.as_str());
            return Err(ModelError::msg(format!(
                "scene material {} source node {} does not match owner {}",
                material_index, material.source_node, owner_name
            )));
        }
    }

    Ok((mesh_owners, material_owners))
}

fn raise_scene_node(
    scene: &NwnScene,
    node_index: usize,
    node: &NwnSceneNode,
    _mesh_owners: &[Option<usize>],
    material_owners: &[Option<usize>],
) -> ModelResult<SemanticNode> {
    let scale = validate_uniform_scale(&node.local_transform.scale, &node.name)?;
    let parent = node
        .parent
        .and_then(|parent_index| scene.nodes.get(parent_index))
        .map(|node| node.name.clone());
    let (material, mesh) = if let Some(mesh_index) = node.mesh {
        let mesh = scene.meshes.get(mesh_index).ok_or_else(|| {
            ModelError::msg(format!(
                "scene node {} references invalid mesh index {}",
                node.name, mesh_index
            ))
        })?;
        let primitive = mesh.primitives.first().ok_or_else(|| {
            ModelError::msg(format!("scene mesh {} is missing its primitive", mesh.name))
        })?;
        let material_index = primitive.material.ok_or_else(|| {
            ModelError::msg(format!(
                "scene mesh {} lost its material reference",
                mesh.name
            ))
        })?;
        let owner = material_owners
            .get(material_index)
            .copied()
            .flatten()
            .ok_or_else(|| {
                ModelError::msg(format!(
                    "scene material {material_index} is missing an owning node"
                ))
            })?;
        if owner != node_index {
            let owner_name = scene
                .nodes
                .get(owner)
                .map_or("<invalid>", |node| node.name.as_str());
            return Err(ModelError::msg(format!(
                "scene node {} references mesh {} whose material belongs to {}",
                node.name, mesh.name, owner_name
            )));
        }
        let material = raise_scene_material(
            &node.kind,
            scene.materials.get(material_index).ok_or_else(|| {
                ModelError::msg(format!(
                    "scene mesh {} references invalid material index {}",
                    mesh.name, material_index
                ))
            })?,
        )?;
        let mesh = raise_scene_mesh(mesh)?;
        (material, Some(mesh))
    } else {
        (default_scene_material(node.alpha), None)
    };

    if let Some(alpha) = node.alpha
        && material.alpha != Some(alpha)
    {
        return Err(ModelError::msg(format!(
            "scene node {} alpha {} does not match its material alpha {}",
            node.name,
            alpha,
            material.alpha.unwrap_or_default()
        )));
    }

    Ok(SemanticNode {
        kind: node.kind.clone(),
        node_type: node.node_type.clone(),
        name: node.name.clone(),
        parent,
        part_number: node.part_number,
        position: Some(node.local_transform.translation),
        orientation: Some(node.local_transform.rotation_axis_angle),
        scale: Some(scale),
        color: node.color,
        radius: node.radius,
        center: node.center,
        wirecolor: node.wirecolor,
        material,
        light: node.light.as_ref().map(raise_scene_light),
        emitter: node.emitter.as_ref().map(raise_scene_emitter),
        reference: node.reference.as_ref().map(raise_scene_reference),
        mesh,
        comments: Vec::new(),
        extras: Vec::new(),
    })
}

fn raise_scene_mesh(mesh: &NwnMesh) -> ModelResult<SemanticMesh> {
    let primitive = mesh.primitives.first().ok_or_else(|| {
        ModelError::msg(format!("scene mesh {} is missing its primitive", mesh.name))
    })?;
    Ok(SemanticMesh {
        vertices:      primitive.positions.clone(),
        faces:         primitive.faces.iter().map(raise_scene_face).collect(),
        uv_layers:     primitive.uv_sets.iter().map(raise_scene_uv_set).collect(),
        normals:       primitive.normals.clone(),
        tangents:      primitive.tangents.clone(),
        colors:        primitive.color_rows.clone(),
        weights:       primitive
            .weight_rows
            .iter()
            .map(|row| row.iter().map(raise_scene_skin_weight).collect())
            .collect(),
        constraints:   primitive.constraint_rows.clone(),
        multimaterial: primitive.surface_labels.clone(),
        texture_names: primitive.texture_names.clone(),
    })
}

fn raise_scene_material(
    node_kind: &NodeKind,
    material: &NwnMaterial,
) -> ModelResult<SemanticMaterial> {
    let mut bitmap = material.helper_bitmap.clone();
    let mut saw_bitmap_slot = false;
    let mut textures = Vec::new();

    if !matches!(node_kind, NodeKind::Aabb)
        && let Some(helper_bitmap) = &material.helper_bitmap
    {
        return Err(ModelError::msg(format!(
            "non-AABB material on node {} cannot use helper bitmap {}",
            material.source_node, helper_bitmap
        )));
    }

    for texture in &material.textures {
        match texture.slot {
            NwnTextureSlot::Bitmap => {
                if saw_bitmap_slot {
                    return Err(ModelError::msg(format!(
                        "material on node {} contains multiple bitmap slots",
                        material.source_node
                    )));
                }
                saw_bitmap_slot = true;
                if let Some(existing) = &bitmap
                    && existing != &texture.name
                {
                    return Err(ModelError::msg(format!(
                        "material on node {} has conflicting bitmap values {} and {}",
                        material.source_node, existing, texture.name
                    )));
                }
                bitmap = Some(texture.name.clone());
            }
            NwnTextureSlot::Texture(index) => textures.push(SemanticTextureBinding {
                index,
                name: texture.name.clone(),
            }),
        }
    }

    Ok(SemanticMaterial {
        render: Some(material.render_enabled),
        shadow: Some(material.shadow_enabled),
        beaming: Some(material.beaming),
        inherit_color: Some(material.inherit_color),
        tilefade: Some(material.tilefade),
        rotate_texture: Some(material.rotate_texture),
        transparency_hint: Some(material.transparency_hint),
        shininess: Some(material.shininess),
        alpha: Some(material.alpha),
        ambient: Some(material.ambient),
        diffuse: Some(material.diffuse),
        specular: Some(material.specular),
        self_illum_color: Some(material.self_illum_color),
        material_name: material.material_name.clone(),
        render_hint: material.render_hint.clone(),
        bitmap,
        textures,
    })
}

fn default_scene_material(alpha: Option<f32>) -> SemanticMaterial {
    SemanticMaterial {
        render: None,
        shadow: None,
        beaming: None,
        inherit_color: None,
        tilefade: None,
        rotate_texture: None,
        transparency_hint: None,
        shininess: None,
        alpha,
        ambient: None,
        diffuse: None,
        specular: None,
        self_illum_color: None,
        material_name: None,
        render_hint: None,
        bitmap: None,
        textures: Vec::new(),
    }
}

fn raise_scene_light(light: &NwnLight) -> SemanticLight {
    SemanticLight {
        multiplier:         Some(light.multiplier),
        ambient_only:       Some(light.ambient_only),
        n_dynamic_type:     light.n_dynamic_type,
        is_dynamic:         Some(light.is_dynamic),
        affect_dynamic:     Some(light.affect_dynamic),
        negative_light:     Some(light.negative_light),
        light_priority:     Some(light.light_priority),
        fading_light:       Some(light.fading_light),
        lens_flares:        Some(light.lens_flares),
        flare_radius:       Some(light.flare_radius),
        flare_textures:     light.flare_textures.clone(),
        flare_sizes:        light.flare_sizes.clone(),
        flare_positions:    light.flare_positions.clone(),
        flare_color_shifts: light.flare_color_shifts.clone(),
    }
}

fn raise_scene_emitter(emitter: &NwnEmitter) -> SemanticEmitter {
    SemanticEmitter {
        x_size:     Some(emitter.x_size),
        y_size:     Some(emitter.y_size),
        properties: emitter
            .properties
            .iter()
            .map(|property| SemanticEmitterProperty {
                name:   property.name.clone(),
                values: property
                    .values
                    .iter()
                    .map(raise_scene_property_value)
                    .collect(),
            })
            .collect(),
    }
}

fn raise_scene_property_value(value: &NwnPropertyValue) -> SemanticPropertyValue {
    match value {
        NwnPropertyValue::Bool(value) => SemanticPropertyValue::Bool(*value),
        NwnPropertyValue::Int(value) => SemanticPropertyValue::Int(*value),
        NwnPropertyValue::Float(value) => SemanticPropertyValue::Float(*value),
        NwnPropertyValue::Text(value) => SemanticPropertyValue::Text(value.clone()),
    }
}

fn raise_scene_reference(reference: &NwnReference) -> SemanticReference {
    SemanticReference {
        model:        reference.model.clone(),
        reattachable: Some(reference.reattachable),
    }
}

fn raise_scene_face(face: &NwnFace) -> SemanticFace {
    SemanticFace {
        vertex_indices: face.vertex_indices,
        group:          face.group,
        uv_indices:     face.uv_indices,
        material_index: face.material_index,
    }
}

fn raise_scene_uv_set(layer: &NwnUvSet) -> SemanticUvLayer {
    SemanticUvLayer {
        index:       layer.index,
        coordinates: layer.coordinates.clone(),
    }
}

fn raise_scene_skin_weight(weight: &NwnSkinWeight) -> SemanticSkinWeight {
    SemanticSkinWeight {
        bone:   weight.bone.clone(),
        weight: weight.weight,
    }
}

fn raise_scene_animation(
    scene: &NwnScene,
    animation: &NwnAnimation,
) -> ModelResult<SemanticAnimation> {
    let animroot = resolve_animation_root(scene, animation)?;
    let nodes = animation
        .node_tracks
        .iter()
        .map(|track| raise_scene_animation_track(scene, animation, track))
        .collect::<ModelResult<Vec<_>>>()?;

    Ok(SemanticAnimation {
        name: animation.name.clone(),
        model_name: animation.model_name.clone(),
        length: Some(animation.length),
        transtime: Some(animation.transition_time),
        animroot,
        events: animation.events.clone(),
        nodes,
        comments: Vec::new(),
        extras: Vec::new(),
    })
}

fn resolve_animation_root(
    scene: &NwnScene,
    animation: &NwnAnimation,
) -> ModelResult<Option<String>> {
    match (animation.root_name.as_deref(), animation.root_node) {
        (None, None) => Ok(None),
        (None, Some(_)) => Err(ModelError::msg(format!(
            "animation {} sets root_node without root_name",
            animation.name
        ))),
        (Some(name), None) => {
            let _index = find_unique_node_index(scene, name, "animation root")?;
            Ok(Some(name.to_string()))
        }
        (Some(name), Some(index)) => {
            let node = scene.nodes.get(index).ok_or_else(|| {
                ModelError::msg(format!(
                    "animation {} references invalid root node index {}",
                    animation.name, index
                ))
            })?;
            if !node.name.eq_ignore_ascii_case(name) {
                return Err(ModelError::msg(format!(
                    "animation {} root {} does not match node {} at index {}",
                    animation.name, name, node.name, index
                )));
            }
            Ok(Some(name.to_string()))
        }
    }
}

fn raise_scene_animation_track(
    scene: &NwnScene,
    animation: &NwnAnimation,
    track: &NwnNodeAnimationTrack,
) -> ModelResult<SemanticAnimationNode> {
    let target_index = resolve_track_target_index(scene, animation, track)?;
    let target_node = scene.nodes.get(target_index).ok_or_else(|| {
        ModelError::msg(format!(
            "animation {} track {} resolved to invalid node index {}",
            animation.name, track.target_name, target_index
        ))
    })?;
    let scale_keys = track
        .transform
        .scale_keys
        .iter()
        .map(|key| {
            validate_uniform_scale(
                &key.value,
                &format!("animation {} track {}", animation.name, track.target_name),
            )
            .map(|value| ScalarKey {
                time: key.time,
                value,
            })
        })
        .collect::<ModelResult<Vec<_>>>()?;
    let animmesh = track.animmesh.as_ref();

    Ok(SemanticAnimationNode {
        kind: track.kind.clone(),
        node_type: target_node.node_type.clone(),
        name: track.target_name.clone(),
        parent: target_node
            .parent
            .and_then(|parent_index| scene.nodes.get(parent_index))
            .map(|node| node.name.clone()),
        part_number: target_node.part_number,
        position: None,
        orientation: None,
        scale: None,
        color: None,
        radius: None,
        alpha: None,
        self_illum_color: None,
        position_keys: track.transform.translation_keys.clone(),
        orientation_keys: track.transform.rotation_axis_angle_keys.clone(),
        scale_keys,
        color_keys: track.material.color_keys.clone(),
        radius_keys: track.material.radius_keys.clone(),
        alpha_keys: track.material.alpha_keys.clone(),
        self_illum_color_keys: track.material.self_illum_color_keys.clone(),
        sample_period: animmesh.and_then(|animmesh| animmesh.sample_period),
        faces: animmesh
            .map(|animmesh| {
                animmesh
                    .face_overrides
                    .iter()
                    .map(raise_scene_face)
                    .collect()
            })
            .unwrap_or_default(),
        animverts: animmesh
            .map(|animmesh| {
                animmesh
                    .vertex_samples
                    .iter()
                    .flat_map(|sample| sample.values.iter().copied())
                    .collect()
            })
            .unwrap_or_default(),
        animtverts: animmesh
            .map(|animmesh| {
                animmesh
                    .uv_samples
                    .iter()
                    .flat_map(|sample| sample.values.iter().copied())
                    .collect()
            })
            .unwrap_or_default(),
        comments: Vec::new(),
        extras: Vec::new(),
    })
}

fn resolve_track_target_index(
    scene: &NwnScene,
    animation: &NwnAnimation,
    track: &NwnNodeAnimationTrack,
) -> ModelResult<usize> {
    match track.target_node {
        Some(index) => {
            let node = scene.nodes.get(index).ok_or_else(|| {
                ModelError::msg(format!(
                    "animation {} track {} references invalid target index {}",
                    animation.name, track.target_name, index
                ))
            })?;
            if !node.name.eq_ignore_ascii_case(&track.target_name) {
                return Err(ModelError::msg(format!(
                    "animation {} track {} does not match node {} at index {}",
                    animation.name, track.target_name, node.name, index
                )));
            }
            Ok(index)
        }
        None => find_unique_node_index(
            scene,
            &track.target_name,
            &format!("animation {} track target", animation.name),
        ),
    }
}

fn find_unique_node_index(scene: &NwnScene, name: &str, context: &str) -> ModelResult<usize> {
    let mut matches = scene
        .nodes
        .iter()
        .enumerate()
        .filter_map(|(index, node)| node.name.eq_ignore_ascii_case(name).then_some(index));
    let first = matches
        .next()
        .ok_or_else(|| ModelError::msg(format!("{context} {name} does not exist in the scene")))?;
    if matches.next().is_some() {
        return Err(ModelError::msg(format!(
            "{context} {name} is ambiguous because multiple scene nodes share that name"
        )));
    }
    Ok(first)
}

#[allow(clippy::float_cmp)]
fn validate_uniform_scale(scale: &[f32; 3], context: &str) -> ModelResult<f32> {
    if scale[0] != scale[1] || scale[1] != scale[2] {
        return Err(ModelError::msg(format!(
            "{context} uses non-uniform scale [{}, {}, {}], which NWN MDL cannot serialize",
            scale[0], scale[1], scale[2]
        )));
    }
    Ok(scale[0])
}

/// Lowers a semantic MDL model into an engine-neutral scene representation.
///
/// # Errors
///
/// Returns [`ModelError`] if the semantic model cannot be lowered into a scene.
pub fn lower_semantic_model_to_scene(model: &SemanticModel) -> ModelResult<NwnScene> {
    let mut diagnostics = model.diagnostics.clone();

    let node_name_to_index = model
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.name.to_ascii_lowercase(), index))
        .collect::<BTreeMap<_, _>>();
    let node_names = node_name_to_index
        .keys()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();

    let mut nodes = Vec::with_capacity(model.nodes.len());
    let mut meshes = Vec::new();
    let mut materials = Vec::new();
    let mut base_mesh_layouts = BTreeMap::new();

    for (node_index, node) in model.nodes.iter().enumerate() {
        let parent = node
            .parent
            .as_ref()
            .and_then(|parent| {
                node_name_to_index
                    .get(&parent.to_ascii_lowercase())
                    .copied()
            })
            .and_then(|parent_index| {
                if parent_index == node_index {
                    diagnostics.push(ModelDiagnostic {
                        kind:    ModelDiagnosticKind::MissingParent,
                        message: format!(
                            "scene node {} resolved its parent to itself; treating it as a root",
                            node.name
                        ),
                    });
                    None
                } else {
                    Some(parent_index)
                }
            });

        if let Some(mesh) = &node.mesh {
            base_mesh_layouts.insert(
                node.name.to_ascii_lowercase(),
                BaseMeshLayout {
                    vertex_count: mesh.vertices.len(),
                    uv_count:     mesh
                        .uv_layers
                        .iter()
                        .find(|layer| layer.index == 0)
                        .map_or(0, |layer| layer.coordinates.len()),
                },
            );
        }

        let mesh_index = node.mesh.as_ref().map(|mesh| {
            let material_index = materials.len();
            materials.push(lower_material(
                &node.material,
                node_index,
                &node.kind,
                &node_names,
            ));
            let lowered_mesh = lower_mesh(mesh, node_index, node.name.clone(), material_index);
            let mesh_index = meshes.len();
            meshes.push(lowered_mesh);
            mesh_index
        });

        nodes.push(NwnSceneNode {
            kind: node.kind.clone(),
            node_type: node.node_type.clone(),
            name: node.name.clone(),
            parent,
            part_number: node.part_number,
            local_transform: lower_transform(node.position, node.orientation, node.scale),
            center: node.center,
            color: node.color,
            radius: node.radius,
            alpha: node.material.alpha,
            wirecolor: node.wirecolor,
            light: node.light.as_ref().map(lower_light),
            emitter: node.emitter.as_ref().map(lower_emitter),
            reference: node.reference.as_ref().map(lower_reference),
            mesh: mesh_index,
        });
    }

    let animations = model
        .animations
        .iter()
        .map(|animation| {
            lower_animation(
                animation,
                &node_name_to_index,
                &base_mesh_layouts,
                &mut diagnostics,
            )
        })
        .collect();

    Ok(NwnScene {
        name: model.header.model_name.clone(),
        supermodel: model.header.supermodel.clone(),
        classification: model.header.classification.clone(),
        animation_scale: model.header.animation_scale,
        coordinate_system: NwnCoordinateSystem::AuroraSource,
        nodes,
        meshes,
        materials,
        animations,
        diagnostics,
    })
}

fn lower_transform(
    position: Option<[f32; 3]>,
    orientation: Option<[f32; 4]>,
    scale: Option<f32>,
) -> NwnTransform {
    let uniform_scale = scale.unwrap_or(1.0);
    NwnTransform {
        translation:         position.unwrap_or([0.0, 0.0, 0.0]),
        rotation_axis_angle: orientation.unwrap_or([0.0, 0.0, 0.0, 0.0]),
        scale:               [uniform_scale, uniform_scale, uniform_scale],
    }
}

fn lower_material(
    material: &SemanticMaterial,
    source_node: usize,
    node_kind: &NodeKind,
    node_names: &std::collections::BTreeSet<String>,
) -> NwnMaterial {
    let mut textures = Vec::new();
    let helper_bitmap = if matches!(node_kind, NodeKind::Aabb) {
        material
            .bitmap
            .as_deref()
            .and_then(|bitmap| normalize_texture_name(bitmap, node_names))
    } else {
        if let Some(bitmap) = &material.bitmap
            && let Some(name) = normalize_texture_name(bitmap, node_names)
        {
            textures.push(NwnTextureRef {
                slot: NwnTextureSlot::Bitmap,
                name,
            });
        }
        None
    };
    for texture in &material.textures {
        if let Some(texture) = lower_texture_ref(texture, node_names) {
            textures.push(texture);
        }
    }

    NwnMaterial {
        source_node,
        render_enabled: material.render.unwrap_or(true),
        shadow_enabled: material.shadow.unwrap_or(true),
        beaming: material.beaming.unwrap_or(0),
        inherit_color: material.inherit_color.unwrap_or(0),
        tilefade: material.tilefade.unwrap_or(0),
        rotate_texture: material.rotate_texture.unwrap_or(0),
        transparency_hint: material.transparency_hint.unwrap_or(0),
        shininess: material.shininess.unwrap_or(0.0),
        alpha: material.alpha.unwrap_or(1.0),
        ambient: material.ambient.unwrap_or([1.0, 1.0, 1.0]),
        diffuse: material.diffuse.unwrap_or([0.8, 0.8, 0.8]),
        specular: material.specular.unwrap_or([0.0, 0.0, 0.0]),
        self_illum_color: material.self_illum_color.unwrap_or([0.0, 0.0, 0.0]),
        material_name: material.material_name.clone(),
        render_hint: material.render_hint.clone(),
        helper_bitmap,
        textures,
    }
}

fn lower_texture_ref(
    binding: &SemanticTextureBinding,
    node_names: &std::collections::BTreeSet<String>,
) -> Option<NwnTextureRef> {
    let name = normalize_texture_name(&binding.name, node_names)?;
    Some(NwnTextureRef {
        slot: NwnTextureSlot::Texture(binding.index),
        name,
    })
}

fn normalize_texture_name(
    name: &str,
    node_names: &std::collections::BTreeSet<String>,
) -> Option<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("null") {
        return None;
    }

    if node_names.contains(&trimmed.to_ascii_lowercase()) {
        return None;
    }

    Some(trimmed.to_string())
}

fn lower_mesh(
    mesh: &SemanticMesh,
    source_node: usize,
    name: String,
    material_index: usize,
) -> NwnMesh {
    NwnMesh {
        name,
        source_node,
        primitives: vec![NwnPrimitive {
            positions:       mesh.vertices.clone(),
            faces:           mesh.faces.iter().map(lower_face).collect(),
            uv_sets:         mesh.uv_layers.iter().map(lower_uv_set).collect(),
            normals:         mesh.normals.clone(),
            tangents:        mesh.tangents.clone(),
            color_rows:      mesh.colors.clone(),
            weight_rows:     mesh
                .weights
                .iter()
                .map(|row| row.iter().map(lower_skin_weight).collect())
                .collect(),
            constraint_rows: mesh.constraints.clone(),
            surface_labels:  mesh.multimaterial.clone(),
            texture_names:   mesh.texture_names.clone(),
            material:        Some(material_index),
        }],
    }
}

fn lower_skin_weight(weight: &SemanticSkinWeight) -> NwnSkinWeight {
    NwnSkinWeight {
        bone:   weight.bone.clone(),
        weight: weight.weight,
    }
}

fn lower_light(light: &SemanticLight) -> NwnLight {
    NwnLight {
        multiplier:         light.multiplier.unwrap_or(1.0),
        ambient_only:       light.ambient_only.unwrap_or(0),
        n_dynamic_type:     light.n_dynamic_type,
        is_dynamic:         light.is_dynamic.unwrap_or(0),
        affect_dynamic:     light.affect_dynamic.unwrap_or(0),
        negative_light:     light.negative_light.unwrap_or(0),
        light_priority:     light.light_priority.unwrap_or(5),
        fading_light:       light.fading_light.unwrap_or(0),
        lens_flares:        light.lens_flares.unwrap_or(0),
        flare_radius:       light.flare_radius.unwrap_or(0.0),
        flare_textures:     light.flare_textures.clone(),
        flare_sizes:        light.flare_sizes.clone(),
        flare_positions:    light.flare_positions.clone(),
        flare_color_shifts: light.flare_color_shifts.clone(),
    }
}

fn lower_emitter(emitter: &SemanticEmitter) -> NwnEmitter {
    NwnEmitter {
        x_size:     emitter.x_size.unwrap_or(0.0),
        y_size:     emitter.y_size.unwrap_or(0.0),
        properties: emitter
            .properties
            .iter()
            .map(lower_emitter_property)
            .collect(),
    }
}

fn lower_emitter_property(property: &SemanticEmitterProperty) -> NwnEmitterProperty {
    NwnEmitterProperty {
        name:   property.name.clone(),
        values: property.values.iter().map(lower_property_value).collect(),
    }
}

fn lower_property_value(value: &SemanticPropertyValue) -> NwnPropertyValue {
    match value {
        SemanticPropertyValue::Bool(value) => NwnPropertyValue::Bool(*value),
        SemanticPropertyValue::Int(value) => NwnPropertyValue::Int(*value),
        SemanticPropertyValue::Float(value) => NwnPropertyValue::Float(*value),
        SemanticPropertyValue::Text(value) => NwnPropertyValue::Text(value.clone()),
    }
}

fn lower_reference(reference: &SemanticReference) -> NwnReference {
    NwnReference {
        model:        reference.model.clone(),
        reattachable: reference.reattachable.unwrap_or(0),
    }
}

fn lower_face(face: &SemanticFace) -> NwnFace {
    NwnFace {
        vertex_indices: face.vertex_indices,
        group:          face.group,
        uv_indices:     face.uv_indices,
        material_index: face.material_index,
    }
}

fn lower_uv_set(layer: &SemanticUvLayer) -> NwnUvSet {
    NwnUvSet {
        index:       layer.index,
        coordinates: layer.coordinates.clone(),
    }
}

fn lower_animation(
    animation: &SemanticAnimation,
    node_name_to_index: &BTreeMap<String, usize>,
    base_mesh_layouts: &BTreeMap<String, BaseMeshLayout>,
    diagnostics: &mut Vec<ModelDiagnostic>,
) -> NwnAnimation {
    let root_node = animation
        .animroot
        .as_ref()
        .and_then(|name| node_name_to_index.get(&name.to_ascii_lowercase()).copied());
    if animation.animroot.is_some() && root_node.is_none() {
        diagnostics.push(ModelDiagnostic {
            kind:    ModelDiagnosticKind::UnknownAnimationTarget,
            message: format!(
                "animation {} references missing animroot {}",
                animation.name,
                animation.animroot.as_deref().unwrap_or_default()
            ),
        });
    }

    let node_tracks = animation
        .nodes
        .iter()
        .map(|node| lower_animation_track(node, node_name_to_index, base_mesh_layouts, diagnostics))
        .collect();

    NwnAnimation {
        name: animation.name.clone(),
        model_name: animation.model_name.clone(),
        length: animation.length.unwrap_or(0.0),
        transition_time: animation.transtime.unwrap_or(0.0),
        root_name: animation.animroot.clone(),
        root_node,
        events: animation.events.clone(),
        node_tracks,
    }
}

fn lower_animation_track(
    node: &SemanticAnimationNode,
    node_name_to_index: &BTreeMap<String, usize>,
    base_mesh_layouts: &BTreeMap<String, BaseMeshLayout>,
    diagnostics: &mut Vec<ModelDiagnostic>,
) -> NwnNodeAnimationTrack {
    let target_node = node_name_to_index
        .get(&node.name.to_ascii_lowercase())
        .copied();

    let mut translation_keys = node.position_keys.clone();
    if translation_keys.is_empty()
        && let Some(position) = node.position
    {
        translation_keys.push(Vec3Key {
            time:  0.0,
            value: position,
        });
    }

    let mut rotation_axis_angle_keys = node.orientation_keys.clone();
    if rotation_axis_angle_keys.is_empty()
        && let Some(orientation) = node.orientation
    {
        rotation_axis_angle_keys.push(Vec4Key {
            time:  0.0,
            value: orientation,
        });
    }

    let mut scale_keys = node
        .scale_keys
        .iter()
        .map(|key| Vec3Key {
            time:  key.time,
            value: [key.value, key.value, key.value],
        })
        .collect::<Vec<_>>();
    if scale_keys.is_empty()
        && let Some(scale) = node.scale
    {
        scale_keys.push(Vec3Key {
            time:  0.0,
            value: [scale, scale, scale],
        });
    }

    let mut color_keys = node.color_keys.clone();
    if color_keys.is_empty()
        && let Some(color) = node.color
    {
        color_keys.push(Vec3Key {
            time:  0.0,
            value: color,
        });
    }

    let mut radius_keys = node.radius_keys.clone();
    if radius_keys.is_empty()
        && let Some(radius) = node.radius
    {
        radius_keys.push(ScalarKey {
            time:  0.0,
            value: radius,
        });
    }

    let mut alpha_keys = node.alpha_keys.clone();
    if alpha_keys.is_empty()
        && let Some(alpha) = node.alpha
    {
        alpha_keys.push(ScalarKey {
            time:  0.0,
            value: alpha,
        });
    }

    let mut self_illum_color_keys = node.self_illum_color_keys.clone();
    if self_illum_color_keys.is_empty()
        && let Some(self_illum_color) = node.self_illum_color
    {
        self_illum_color_keys.push(Vec3Key {
            time:  0.0,
            value: self_illum_color,
        });
    }

    let layout = base_mesh_layouts
        .get(&node.name.to_ascii_lowercase())
        .copied();
    let animmesh = if node.sample_period.is_some()
        || !node.faces.is_empty()
        || !node.animverts.is_empty()
        || !node.animtverts.is_empty()
    {
        Some(NwnAnimMeshTrack {
            sample_period:  node.sample_period,
            face_overrides: node.faces.iter().map(lower_face).collect(),
            vertex_samples: chunk_vec3_samples(
                &node.animverts,
                layout.map_or(0, |layout| layout.vertex_count),
                &node.name,
                "animverts",
                diagnostics,
            ),
            uv_samples:     chunk_vec2_samples(
                &node.animtverts,
                layout.map_or(0, |layout| layout.uv_count),
                &node.name,
                "animtverts",
                diagnostics,
            ),
        })
    } else {
        None
    };

    NwnNodeAnimationTrack {
        target_name: node.name.clone(),
        target_node,
        kind: node.kind.clone(),
        transform: NwnTransformTrack {
            translation_keys,
            rotation_axis_angle_keys,
            scale_keys,
        },
        material: NwnMaterialTrack {
            color_keys,
            radius_keys,
            alpha_keys,
            self_illum_color_keys,
        },
        animmesh,
    }
}

fn chunk_vec3_samples(
    values: &[[f32; 3]],
    width: usize,
    node_name: &str,
    keyword: &str,
    diagnostics: &mut Vec<ModelDiagnostic>,
) -> Vec<NwnVec3Sample> {
    if values.is_empty() {
        return Vec::new();
    }

    if width == 0 {
        diagnostics.push(ModelDiagnostic {
            kind:    ModelDiagnosticKind::MalformedPayloadRow,
            message: format!(
                "cannot split {keyword} samples for node {node_name} without a base mesh vertex \
                 count"
            ),
        });
        return vec![NwnVec3Sample {
            values: values.to_vec(),
        }];
    }

    if !values.len().is_multiple_of(width) {
        diagnostics.push(ModelDiagnostic {
            kind:    ModelDiagnosticKind::MalformedPayloadRow,
            message: format!(
                "{keyword} sample count {} for node {node_name} is not divisible by base width \
                 {width}",
                values.len()
            ),
        });
        return vec![NwnVec3Sample {
            values: values.to_vec(),
        }];
    }

    values
        .chunks(width)
        .map(|chunk| NwnVec3Sample {
            values: chunk.to_vec(),
        })
        .collect()
}

fn chunk_vec2_samples(
    values: &[[f32; 2]],
    width: usize,
    node_name: &str,
    keyword: &str,
    diagnostics: &mut Vec<ModelDiagnostic>,
) -> Vec<NwnVec2Sample> {
    if values.is_empty() {
        return Vec::new();
    }

    if width == 0 {
        diagnostics.push(ModelDiagnostic {
            kind:    ModelDiagnosticKind::MalformedPayloadRow,
            message: format!(
                "cannot split {keyword} samples for node {node_name} without a base mesh uv count"
            ),
        });
        return vec![NwnVec2Sample {
            values: values.to_vec(),
        }];
    }

    if !values.len().is_multiple_of(width) {
        diagnostics.push(ModelDiagnostic {
            kind:    ModelDiagnosticKind::MalformedPayloadRow,
            message: format!(
                "{keyword} sample count {} for node {node_name} is not divisible by base width \
                 {width}",
                values.len()
            ),
        });
        return vec![NwnVec2Sample {
            values: values.to_vec(),
        }];
    }

    values
        .chunks(width)
        .map(|chunk| NwnVec2Sample {
            values: chunk.to_vec(),
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BaseMeshLayout {
    vertex_count: usize,
    uv_count:     usize,
}

#[cfg(test)]
mod tests {
    use std::{error::Error, io::Cursor};

    use nwnrs_resman::CachePolicy;
    use nwnrs_test_support::{
        demand_resource, require_game_resource, skip_if_game_resources_unavailable,
    };

    use crate::{
        BinaryModel, MODEL_RES_TYPE, NwnPropertyValue, NwnScene, NwnTextureSlot,
        lower_binary_model_to_ascii, parse_scene_model, read_scene_model, write_scene_model,
    };

    #[test]
    fn fixture_lowers_to_scene_mesh_and_material() -> Result<(), Box<dyn Error>> {
        let scene = match shipped_ascii_scene_fixture() {
            Ok(scene) => scene,
            Err(error) => return skip_if_game_resources_unavailable(error),
        };

        assert_eq!(scene.name, "a_ba_casts");
        let torso = scene.node("torso_g").unwrap_or_else(|| {
            panic!("missing scene node torso_g");
        });
        let mesh = torso
            .mesh
            .and_then(|mesh_index| scene.meshes.get(mesh_index))
            .unwrap_or_else(|| panic!("torso_g missing mesh reference"));
        assert_eq!(mesh.primitives.len(), 1);
        let primitive = mesh
            .primitives
            .first()
            .unwrap_or_else(|| panic!("missing primitive"));
        assert_eq!(primitive.positions.len(), 122);
        assert_eq!(primitive.faces.len(), 70);
        assert_eq!(
            primitive.uv_sets.first().map(|set| set.coordinates.len()),
            Some(122)
        );

        let material = primitive
            .material
            .and_then(|material_index| scene.materials.get(material_index))
            .unwrap_or_else(|| panic!("torso_g missing material reference"));
        assert!(material.textures.iter().any(
            |texture| texture.slot == NwnTextureSlot::Bitmap && texture.name == "pmh0_chest001"
        ));
        Ok(())
    }

    #[test]
    fn animated_fixture_lowers_to_scene_tracks() -> Result<(), Box<dyn Error>> {
        let scene = match shipped_ascii_scene_fixture() {
            Ok(scene) => scene,
            Err(error) => return skip_if_game_resources_unavailable(error),
        };

        let conjure = scene.animation("conjure1").unwrap_or_else(|| {
            panic!("missing scene animation conjure1");
        });
        assert_eq!(conjure.length, 1.0);
        assert_eq!(conjure.transition_time, 0.5);
        assert_eq!(
            conjure
                .root_node
                .and_then(|index| scene.nodes.get(index))
                .map(|node| node.name.as_str()),
            Some("rootdummy")
        );

        let rootdummy = conjure.node_track("rootdummy").unwrap_or_else(|| {
            panic!("missing rootdummy animation track");
        });
        assert!(rootdummy.target_node.is_some());
        assert_eq!(rootdummy.transform.translation_keys.len(), 5);
        assert_eq!(rootdummy.transform.rotation_axis_angle_keys.len(), 2);
        Ok(())
    }

    #[test]
    fn compiled_fixture_lowers_to_scene_graph_and_tracks() -> Result<(), Box<dyn Error>> {
        let scene = match shipped_compiled_scene_fixture() {
            Ok(scene) => scene,
            Err(error) => return skip_if_game_resources_unavailable(error),
        };

        assert_eq!(scene.name, "a_ba2");
        assert_eq!(scene.supermodel.as_deref(), Some("a_ba"));
        assert_eq!(scene.nodes.len(), 57);
        assert_eq!(scene.animations.len(), 20);

        let torso = scene.node("torso_g").unwrap_or_else(|| {
            panic!("missing compiled torso_g scene node");
        });
        let parent_name = torso
            .parent
            .and_then(|index| scene.nodes.get(index))
            .map(|node| node.name.as_str());
        assert_eq!(parent_name, Some("rootdummy"));
        assert!(torso.mesh.is_some());

        let salute = scene.animation("salute").unwrap_or_else(|| {
            panic!("missing compiled salute animation");
        });
        assert_eq!(salute.length, 0.5);
        assert_eq!(salute.transition_time, 0.4);
        assert_eq!(salute.root_name.as_deref(), Some("torso_g"));
        assert!(salute.node_track("rootdummy").is_some());
        Ok(())
    }

    #[test]
    fn animmesh_fixture_lowers_sample_groups() {
        let scene = parse_scene_model(
            "\
newmodel ui
setsupermodel ui null
classification gui
setanimationscale 1
beginmodelgeom ui
node dummy ui
  parent NULL
endnode
node animmesh Plane14
  parent ui
  bitmap gui_replace01
  verts 4
    0 0 0
    1 0 0
    0 1 0
    1 1 0
  faces 2
    0 1 2  0  0 1 2  0
    1 3 2  0  1 3 2  0
  tverts 4
    0 0 0
    1 0 0
    0 1 0
    1 1 0
endnode
endmodelgeom ui
newanim up ui
  length 1
  node animmesh Plane14
    sampleperiod 1
    faces 2
      0 1 2  0  0 1 2  0
      1 3 2  0  1 3 2  0
    animverts 4
      0 0 0
      1 0 0
      0 1 0
      1 1 0
    animtverts 4
      0 0
      1 0
      0 1
      1 1
  endnode
doneanim up ui
donemodel ui
",
        )
        .unwrap_or_else(|error| {
            panic!("parse animmesh scene sample: {error}");
        });

        let up = scene.animation("up").unwrap_or_else(|| {
            panic!("missing scene animation up");
        });
        let plane = up.node_track("Plane14").unwrap_or_else(|| {
            panic!("missing Plane14 track");
        });
        let animmesh = plane.animmesh.as_ref().unwrap_or_else(|| {
            panic!("Plane14 should lower animmesh data");
        });
        assert_eq!(animmesh.sample_period, Some(1.0));
        assert_eq!(animmesh.face_overrides.len(), 2);
        assert_eq!(animmesh.vertex_samples.len(), 1);
        assert_eq!(
            animmesh
                .vertex_samples
                .first()
                .map(|sample| sample.values.len()),
            Some(4)
        );
        assert_eq!(animmesh.uv_samples.len(), 1);
        assert_eq!(
            animmesh
                .uv_samples
                .first()
                .map(|sample| sample.values.len()),
            Some(4)
        );
    }

    #[test]
    fn skin_fixture_lowers_named_weight_rows() {
        let scene = parse_scene_model(
            "\
newmodel demo
setsupermodel demo null
classification character
setanimationscale 1
beginmodelgeom demo
node dummy demo
  parent NULL
endnode
node skin Arm_L
  parent demo
  bitmap tex
  verts 2
    0 0 0
    1 0 0
  faces 1
    0 1 0  0  0 1 0  0
  tverts 2
    0 0 0
    1 0 0
  weights 2
    torso_g 1.0
    lforearm_g 0.25  lbicep_g 0.75
endnode
endmodelgeom demo
donemodel demo
",
        )
        .unwrap_or_else(|error| {
            panic!("parse skin scene sample: {error}");
        });

        let arm = scene.node("Arm_L").unwrap_or_else(|| {
            panic!("missing scene node Arm_L");
        });
        let mesh = arm
            .mesh
            .and_then(|mesh_index| scene.meshes.get(mesh_index))
            .unwrap_or_else(|| panic!("Arm_L missing mesh reference"));
        let primitive = mesh
            .primitives
            .first()
            .unwrap_or_else(|| panic!("missing primitive"));
        assert_eq!(primitive.weight_rows.len(), 2);
        assert_eq!(
            primitive
                .weight_rows
                .first()
                .and_then(|row| row.first())
                .map(|weight| weight.bone.as_str()),
            Some("torso_g")
        );
        assert_eq!(primitive.weight_rows.get(1).map(Vec::len), Some(2));
    }

    #[test]
    fn emitter_reference_and_light_fixtures_lower_special_nodes() {
        let fx_scene = parse_scene_model(
            "\
newmodel fx
setsupermodel fx null
classification effect
setanimationscale 1
beginmodelgeom fx
node dummy fx
  parent NULL
endnode
node emitter spark
  parent fx
  xsize 0
  ysize 0
  texture fxpa_flare
  render Linked
  renderorder 0
endnode
node reference omen
  parent spark
  refModel fx_ref
  reattachable 0
endnode
endmodelgeom fx
donemodel fx
",
        )
        .unwrap_or_else(|error| {
            panic!("parse emitter scene sample: {error}");
        });
        let emitter = fx_scene.node("spark").unwrap_or_else(|| {
            panic!("missing emitter scene node");
        });
        let emitter_payload = emitter.emitter.as_ref().unwrap_or_else(|| {
            panic!("missing emitter payload");
        });
        assert!(emitter_payload.properties.iter().any(|property| {
            property.name.eq_ignore_ascii_case("texture")
                && property.values.iter().any(
                    |value| matches!(value, NwnPropertyValue::Text(name) if name == "fxpa_flare"),
                )
        }));

        let reference = fx_scene.node("omen").unwrap_or_else(|| {
            panic!("missing reference scene node");
        });
        let reference_payload = reference.reference.as_ref().unwrap_or_else(|| {
            panic!("missing reference payload");
        });
        assert_eq!(reference_payload.model.as_deref(), Some("fx_ref"));

        let light_scene = parse_scene_model(
            "\
newmodel lantern
setsupermodel lantern null
classification item
setanimationscale 1
beginmodelgeom lantern
node dummy lantern
  parent NULL
endnode
node light AuroraLight01
  parent lantern
  ambientonly 0
  shadow 0
  isdynamic 0
  affectdynamic 1
  lightpriority 3
  fadingLight 1
  flareradius 0
  radius 5
  multiplier 1
  alpha 0.5
  color 1 1 1
endnode
endmodelgeom lantern
donemodel lantern
",
        )
        .unwrap_or_else(|error| {
            panic!("parse light scene sample: {error}");
        });
        let light = light_scene.node("AuroraLight01").unwrap_or_else(|| {
            panic!("missing light scene node");
        });
        let light_payload = light.light.as_ref().unwrap_or_else(|| {
            panic!("missing light payload");
        });
        assert_eq!(light.color, Some([1.0, 1.0, 1.0]));
        assert_eq!(light.radius, Some(5.0));
        assert_eq!(light.alpha, Some(0.5));
        assert_eq!(light_payload.light_priority, 3);
        assert_eq!(light_payload.fading_light, 1);
        assert_eq!(light_payload.multiplier, 1.0);
    }

    #[test]
    fn aabb_bitmap_lowers_to_helper_bitmap_instead_of_texture() {
        let scene = parse_scene_model(
            "\
newmodel demo
setsupermodel demo null
classification tile
setanimationscale 1
beginmodelgeom demo
node dummy demo
  parent NULL
endnode
node aabb wm_demo
  parent demo
  render 0
  bitmap Stone
  verts 3
    0 0 0
    1 0 0
    0 1 0
  faces 1
    0 1 2  0  0 1 2  0
  tverts 3
    0 0 0
    1 0 0
    0 1 0
endnode
endmodelgeom demo
donemodel demo
",
        )
        .unwrap_or_else(|error| {
            panic!("parse aabb helper scene sample: {error}");
        });

        let material = scene
            .materials
            .first()
            .unwrap_or_else(|| panic!("missing aabb helper material"));
        assert_eq!(material.helper_bitmap.as_deref(), Some("Stone"));
        assert!(material.textures.is_empty());
    }

    #[test]
    fn scene_writer_roundtrips_canonical_model() -> Result<(), Box<dyn Error>> {
        let scene = match shipped_ascii_scene_fixture() {
            Ok(scene) => scene,
            Err(error) => return skip_if_game_resources_unavailable(error),
        };

        let mut encoded = Vec::new();
        if let Err(error) = write_scene_model(&mut encoded, &scene) {
            panic!("write scene model: {error}");
        }

        let mut cursor = Cursor::new(encoded);
        let reparsed = read_scene_model(&mut cursor).unwrap_or_else(|error| {
            panic!("read rewritten scene model: {error}");
        });
        assert_eq!(reparsed.name, scene.name);
        assert_eq!(reparsed.nodes.len(), scene.nodes.len());
        assert_eq!(reparsed.meshes.len(), scene.meshes.len());
        assert_eq!(reparsed.materials.len(), scene.materials.len());
        assert_eq!(reparsed.animations.len(), scene.animations.len());
        assert_eq!(
            reparsed
                .animation("conjure1")
                .map(|animation| animation.root_name.as_deref()),
            scene
                .animation("conjure1")
                .map(|animation| animation.root_name.as_deref())
        );
        assert_eq!(
            reparsed
                .node("torso_g")
                .and_then(|node| node.mesh)
                .and_then(|mesh_index| reparsed.meshes.get(mesh_index))
                .and_then(|mesh| mesh.primitives.first())
                .map(|primitive| (primitive.positions.len(), primitive.faces.len())),
            Some((122, 70))
        );
        Ok(())
    }

    #[test]
    fn scene_writer_rejects_invalid_parent_indices() {
        let mut scene = writable_scene_fixture();
        scene
            .nodes
            .get_mut(1)
            .unwrap_or_else(|| panic!("writable scene fixture missing body node"))
            .parent = Some(usize::MAX);

        let error = write_scene_model(&mut Vec::new(), &scene).unwrap_err();
        assert!(error.to_string().contains("invalid parent index"));
    }

    #[test]
    fn scene_writer_rejects_invalid_material_indices() {
        let mut scene = writable_scene_fixture();
        scene
            .meshes
            .get_mut(0)
            .and_then(|mesh| mesh.primitives.get_mut(0))
            .unwrap_or_else(|| panic!("writable scene fixture missing body primitive"))
            .material = Some(usize::MAX);

        let error = write_scene_model(&mut Vec::new(), &scene).unwrap_err();
        assert!(error.to_string().contains("invalid material index"));
    }

    #[test]
    fn scene_writer_rejects_invalid_animation_targets() {
        let mut scene = writable_scene_fixture();
        scene
            .animations
            .get_mut(0)
            .and_then(|animation| animation.node_tracks.get_mut(0))
            .unwrap_or_else(|| panic!("writable scene fixture missing animation track"))
            .target_node = Some(usize::MAX);

        let error = write_scene_model(&mut Vec::new(), &scene).unwrap_err();
        assert!(error.to_string().contains("invalid target index"));
    }

    #[test]
    fn scene_writer_rejects_non_uniform_scale() {
        let mut scene = writable_scene_fixture();
        scene
            .nodes
            .get_mut(1)
            .unwrap_or_else(|| panic!("writable scene fixture missing body node"))
            .local_transform
            .scale = [1.0, 2.0, 1.0];

        let error = write_scene_model(&mut Vec::new(), &scene).unwrap_err();
        assert!(error.to_string().contains("non-uniform scale"));
    }

    fn shipped_ascii_scene_fixture() -> Result<crate::NwnScene, Box<dyn Error>> {
        let res = require_game_resource(demand_resource("a_ba_casts", MODEL_RES_TYPE))?;
        let binary = BinaryModel::from_res(&res, CachePolicy::Use)?;
        let ascii = lower_binary_model_to_ascii(&binary)?;
        Ok(parse_scene_model(&ascii.to_text())?)
    }

    fn shipped_compiled_scene_fixture() -> Result<crate::NwnScene, Box<dyn Error>> {
        let res = require_game_resource(demand_resource("a_ba2", MODEL_RES_TYPE))?;
        Ok(NwnScene::from_auto_res(&res, CachePolicy::Use)?)
    }

    fn writable_scene_fixture() -> NwnScene {
        parse_scene_model(
            "\
newmodel demo
setsupermodel demo null
classification character
setanimationscale 1
beginmodelgeom demo
node dummy demo
  parent NULL
endnode
node trimesh body
  parent demo
  bitmap tex01
  verts 3
    0 0 0
    1 0 0
    0 1 0
  faces 1
    0 1 2  0  0 1 2  0
  tverts 3
    0 0 0
    1 0 0
    0 1 0
endnode
endmodelgeom demo
newanim idle demo
  length 1
  transtime 0.25
  animroot body
  node trimesh body
    positionkey 1
      0 0 0 0
  endnode
doneanim idle demo
donemodel demo
",
        )
        .unwrap_or_else(|error| panic!("parse writable scene fixture: {error}"))
    }
}
