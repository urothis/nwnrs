use bevy::{
    asset::Handle,
    mesh::Mesh,
    pbr::StandardMaterial,
    prelude::{Asset, Name, TypePath},
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
    pub name:       String,
    /// Source node kind.
    pub kind:       NodeKind,
    /// Parent node index, when present.
    pub parent:     Option<usize>,
    /// Local Bevy transform for this node.
    pub transform:  bevy::transform::components::Transform,
    /// Static mesh instances attached to this node.
    pub primitives: Vec<NwnPrimitiveAsset>,
}

/// One static primitive instance attached to a scene node.
#[derive(Debug, Clone)]
pub struct NwnPrimitiveAsset {
    /// Human-readable primitive label.
    pub label:          String,
    /// Mesh handle for this primitive.
    pub mesh:           Handle<Mesh>,
    /// Material handle for this primitive.
    pub material:       Handle<StandardMaterial>,
    /// Whether the primitive should cast and receive shadows.
    pub shadow_enabled: bool,
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
