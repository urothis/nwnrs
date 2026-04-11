use bevy::{
    asset::Handle,
    math::{Affine2, Vec2, Vec3},
    mesh::Mesh,
    pbr::StandardMaterial,
    prelude::{Asset, Name, Resource, TypePath},
};
use nwnrs_mdl::prelude::*;

/// A Bevy-facing NWN model asset built from an `mdl` file.
#[derive(Asset, Debug, Clone, TypePath)]
pub struct NwnModelAsset {
    /// Lowered NWN scene captured from the source model.
    pub scene:      NwnScene,
    /// Scene nodes preserved in source order.
    pub nodes:      Vec<NwnModelNodeAsset>,
    /// Root node indices within [`NwnModelAsset::nodes`].
    pub root_nodes: Vec<usize>,
    /// Labeled material handles created while loading the model.
    pub materials:  Vec<Handle<StandardMaterial>>,
    /// Labeled mesh handles created while loading the model.
    pub meshes:     Vec<Handle<Mesh>>,
    /// Labeled texture handles created while loading the model.
    pub textures:   Vec<Handle<bevy::image::Image>>,
    /// Texture references that were requested but not found or not supported.
    pub unresolved: Vec<NwnUnresolvedTexture>,
}

impl NwnModelAsset {
    /// Returns the scene name as a Bevy [`Name`] component.
    pub fn root_name(&self) -> Name {
        Name::new(self.scene.name.clone())
    }
}

/// One scene node prepared for Bevy spawning.
#[derive(Debug, Clone)]
pub struct NwnModelNodeAsset {
    /// Scene node name.
    pub name:           String,
    /// Source node kind.
    pub kind:           NodeKind,
    /// Parent node index, when present.
    pub parent:         Option<usize>,
    /// Local Bevy transform for this node.
    pub transform:      bevy::transform::components::Transform,
    /// Optional NWN-authored local light attached to this node.
    pub light:          Option<NwnModelLightAsset>,
    /// Referenced child models attached to this node.
    pub references:     Vec<NwnModelReferenceAsset>,
    /// Optional helper-surface metadata attached to this node.
    pub helper_surface: Option<NwnModelHelperSurfaceAsset>,
    /// Static mesh instances attached to this node.
    pub primitives:     Vec<NwnPrimitiveAsset>,
}

/// One referenced child model attached to a scene node.
#[derive(Debug, Clone)]
pub struct NwnModelReferenceAsset {
    /// Referenced model name from `refmodel`.
    pub model_name: String,
    /// Loaded child model asset.
    pub model:      Box<NwnModelAsset>,
}

/// Helper-surface metadata attached to non-render helper geometry such as
/// `Aabb` walkmeshes or collision meshes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NwnModelHelperSurfaceAsset {
    /// Helper bitmap tokens authored on the node materials.
    pub bitmaps:        Vec<String>,
    /// Surface labels captured from the source mesh.
    pub surface_labels: Vec<String>,
    /// Per-primitive texture-name labels captured from the source mesh.
    pub texture_names:  Vec<String>,
}

/// One NWN-authored point-light description attached to a model node.
#[derive(Debug, Clone, PartialEq)]
pub struct NwnModelLightAsset {
    /// Static base color from the source node.
    pub base_color:     [f32; 3],
    /// Static base radius from the source node.
    pub base_radius:    f32,
    /// Static base alpha from the source node.
    pub base_alpha:     f32,
    /// NWN light multiplier.
    pub multiplier:     f32,
    /// Whether Bevy shadows should be enabled for this light.
    pub shadow_enabled: bool,
    /// Optional looping animation payload for the light.
    pub animation:      Option<NwnModelLightAnimationAsset>,
}

/// One looping light-animation payload derived from NWN model tracks.
#[derive(Debug, Clone, PartialEq)]
pub struct NwnModelLightAnimationAsset {
    /// Loop length in seconds.
    pub length:      f32,
    /// Animated color keys.
    pub color_keys:  Vec<Vec3Key>,
    /// Animated radius keys.
    pub radius_keys: Vec<ScalarKey>,
    /// Animated alpha keys.
    pub alpha_keys:  Vec<ScalarKey>,
}

/// One static primitive instance attached to a scene node.
#[derive(Debug, Clone)]
pub struct NwnPrimitiveAsset {
    /// Human-readable primitive label.
    pub label: String,
    /// Source primitive index within the owning scene mesh.
    pub scene_primitive_index: usize,
    /// Optional parsed TXI sidecar behavior for the bound bitmap texture.
    pub txi: Option<NwnModelTxiAsset>,
    /// Optional affine mapping from authored UVs into local Bevy X/Z space.
    pub txi_uv_to_local_horizontal: Option<Affine2>,
    /// Mesh handle for this primitive.
    pub mesh: Handle<Mesh>,
    /// Material handle for this primitive.
    pub material: Handle<StandardMaterial>,
    /// Optional runtime tilefade behavior for this primitive.
    pub tilefade: Option<NwnModelTileFadeAsset>,
    /// Whether this primitive should start visible when spawned.
    pub initially_visible: bool,
    /// Whether the primitive should cast and receive shadows.
    pub shadow_enabled: bool,
}

/// Camera-driven tilefade behavior attached to a primitive.
#[derive(Debug, Clone, PartialEq)]
pub struct NwnModelTileFadeAsset {
    /// Authored tilefade mode from the MDL material.
    pub mode:               i32,
    /// Whether the source material authored `render 1`.
    pub authored_visible:   bool,
    /// Primitive bounds center in the primitive's local Bevy space.
    pub local_center:       Vec3,
    /// Primitive bounds half extents in the primitive's local Bevy space.
    pub local_half_extents: Vec3,
}

/// One parsed TXI behavior attached to a material/primitive.
#[derive(Debug, Clone, PartialEq)]
pub struct NwnModelTxiAsset {
    /// Authored `rotatetexture` flag from the MDL material.
    pub rotate_texture:      i32,
    /// Optional `bumpmaptexture`.
    pub bump_map_texture:    Option<String>,
    /// Optional `bumpyshinytexture`.
    pub bumpy_shiny_texture: Option<String>,
    /// Optional parsed procedural animation behavior.
    pub procedure:           Option<NwnModelTxiProcedureAsset>,
}

/// Supported TXI procedure variants used by the Bevy runtime.
#[derive(Debug, Clone, PartialEq)]
pub enum NwnModelTxiProcedureAsset {
    /// `proceduretype arturo`
    Arturo {
        /// Channel scale values in authored order.
        channel_scale:        Vec<f32>,
        /// Channel translate values in authored order.
        channel_translate:    Vec<f32>,
        /// Optional `distort` flag.
        distort:              Option<i32>,
        /// Optional `arturowidth`.
        arturo_width:         Option<i32>,
        /// Optional `arturoheight`.
        arturo_height:        Option<i32>,
        /// Optional `distortionamplitude`.
        distortion_amplitude: Option<f32>,
        /// Optional `speed`.
        speed:                Option<f32>,
        /// Optional `defaultheight`.
        default_height:       Option<i32>,
        /// Optional `defaultwidth`.
        default_width:        Option<i32>,
        /// Optional `alphamean`.
        alpha_mean:           Option<f32>,
    },
}

/// Shared world-space wind settings used by procedural NWN materials.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct NwnAreaWind {
    /// Horizontal Bevy X/Z direction projected into a 2D vector.
    pub direction: Vec2,
    /// Wind magnitude scalar.
    pub magnitude: f32,
}

impl Default for NwnAreaWind {
    fn default() -> Self {
        Self {
            direction: Vec2::new(1.0, -1.0).normalize(),
            magnitude: 0.0,
        }
    }
}

/// One unresolved texture reference captured while loading the model.
#[derive(Debug, Clone)]
pub struct NwnUnresolvedTexture {
    /// Material index that referenced the texture.
    pub material_index: usize,
    /// Texture binding slot.
    pub slot:           NwnTextureSlot,
    /// Authored texture reference string.
    pub name:           String,
    /// Candidate asset paths attempted in order.
    pub attempted:      Vec<String>,
    /// Why this texture was not loaded.
    pub reason:         NwnTextureLoadReason,
}

/// Why a model texture could not be turned into a Bevy image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NwnTextureLoadReason {
    /// No matching texture asset could be found.
    Missing,
    /// The texture kind exists but is not enabled by the current loader policy.
    UnsupportedPlt,
    /// The file existed but could not be decoded.
    DecodeFailed,
}
