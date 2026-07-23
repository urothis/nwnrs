use nwnrs_types::{
    gff::{AreEnvironment, AreFile, GitFile},
    mdl::{NwnComposedScene, NwnScene},
    set::SetFile,
};
use serde::{Deserialize, Serialize};

use crate::scene::DependencyGraph;

/// Original resource category used to assemble a scene.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SceneSource {
    /// MDL model.
    Model,
    /// WOK tile walkmesh.
    Walkmesh,
    /// DWK door walkmesh.
    DoorWalkmesh,
    /// PWK placeable walkmesh.
    PlaceableWalkmesh,
    /// UTC creature blueprint.
    Creature,
    /// UTD door blueprint.
    Door,
    /// UTP placeable blueprint.
    Placeable,
    /// UTI item blueprint.
    Item,
    /// ARE/GIT area scene.
    Area,
    /// IFO module area collection.
    Module,
}

/// Severity of a scene diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SceneDiagnosticSeverity {
    /// Informational note.
    Information,
    /// Degraded but still renderable scene.
    Warning,
    /// Scene content could not be represented correctly.
    Error,
}

/// A resource-aware scene diagnostic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneDiagnostic {
    /// Diagnostic severity.
    pub severity: SceneDiagnosticSeverity,
    /// Stable machine-readable diagnostic code.
    pub code:     String,
    /// Human-readable description.
    pub message:  String,
    /// Related resource identity.
    pub resource: Option<String>,
}

/// Viewer environment policy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SceneEnvironment {
    /// Neutral studio lighting and background.
    Studio,
    /// Area-authored NWN lighting and weather data.
    Nwn(SceneAreaEnvironment),
}

/// Serializable ARE environment projection consumed by frontends.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneAreaEnvironment {
    /// Whether the day/night cycle is active.
    pub day_night_cycle:    Option<bool>,
    /// Whether night lighting is selected.
    pub is_night:           Option<bool>,
    /// Lighting-scheme row.
    pub lighting_scheme:    Option<i32>,
    /// Fog clipping distance.
    pub fog_clip_distance:  Option<f32>,
    /// Packed sun ambient color.
    pub sun_ambient_color:  Option<u32>,
    /// Packed sun diffuse color.
    pub sun_diffuse_color:  Option<u32>,
    /// Packed daytime fog color.
    pub sun_fog_color:      Option<u32>,
    /// Daytime fog amount.
    pub sun_fog_amount:     Option<i32>,
    /// Whether daytime shadows are enabled.
    pub sun_shadows:        Option<bool>,
    /// Packed moon ambient color.
    pub moon_ambient_color: Option<u32>,
    /// Packed moon diffuse color.
    pub moon_diffuse_color: Option<u32>,
    /// Packed nighttime fog color.
    pub moon_fog_color:     Option<u32>,
    /// Nighttime fog amount.
    pub moon_fog_amount:    Option<i32>,
    /// Whether nighttime shadows are enabled.
    pub moon_shadows:       Option<bool>,
    /// Skybox selector.
    pub skybox:             Option<i32>,
    /// Wind strength.
    pub wind_power:         Option<i32>,
    /// Authored shadow opacity.
    pub shadow_opacity:     Option<i32>,
    /// Rain probability.
    pub chance_rain:        Option<i32>,
    /// Snow probability.
    pub chance_snow:        Option<i32>,
    /// Lightning probability.
    pub chance_lightning:   Option<i32>,
}

impl From<&AreEnvironment> for SceneAreaEnvironment {
    fn from(value: &AreEnvironment) -> Self {
        Self {
            day_night_cycle:    value.day_night_cycle,
            is_night:           value.is_night,
            lighting_scheme:    value.lighting_scheme,
            fog_clip_distance:  value.fog_clip_distance,
            sun_ambient_color:  value.sun_ambient_color,
            sun_diffuse_color:  value.sun_diffuse_color,
            sun_fog_color:      value.sun_fog_color,
            sun_fog_amount:     value.sun_fog_amount,
            sun_shadows:        value.sun_shadows,
            moon_ambient_color: value.moon_ambient_color,
            moon_diffuse_color: value.moon_diffuse_color,
            moon_fog_color:     value.moon_fog_color,
            moon_fog_amount:    value.moon_fog_amount,
            moon_shadows:       value.moon_shadows,
            skybox:             value.skybox,
            wind_power:         value.wind_power,
            shadow_opacity:     value.shadow_opacity,
            chance_rain:        value.chance_rain,
            chance_snow:        value.chance_snow,
            chance_lightning:   value.chance_lightning,
        }
    }
}

/// Kind of one positioned render instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SceneInstanceKind {
    /// Standalone model.
    Model,
    /// Area tile.
    Tile,
    /// Camera-centered area skybox.
    Skybox,
    /// Creature.
    Creature,
    /// Door.
    Door,
    /// Placeable.
    Placeable,
    /// Store.
    Store,
    /// Item.
    Item,
    /// Walkmesh or collision geometry.
    Collision,
    /// Trigger polygon.
    Trigger,
    /// Encounter polygon.
    Encounter,
    /// Waypoint marker.
    Waypoint,
    /// Sound volume.
    Sound,
}

/// One model or overlay positioned in scene space.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneInstance {
    /// Stable instance id.
    pub id:                    usize,
    /// Stable authored GIT object identity shared by every visual component
    /// produced for the same logical object.
    pub object_key:            Option<String>,
    /// Display label.
    pub label:                 String,
    /// Instance category.
    pub kind:                  SceneInstanceKind,
    /// Index into [`SceneDocument::models`], when the instance has a model.
    pub model:                 Option<usize>,
    /// Exact source resource for this rendered component, when it has one.
    pub resource:              Option<String>,
    /// Position in Aurora world coordinates.
    pub position:              [f32; 3],
    /// Rotation axis and angle in radians.
    pub rotation_axis_angle:   [f32; 4],
    /// Scale vector.
    pub scale:                 [f32; 3],
    /// Optional polygon points for volume overlays.
    pub polygon:               Vec<[f32; 3]>,
    /// Tile main/source light colors in `main1`, `main2`, `source1`,
    /// `source2` order.
    pub light_color_overrides: [Option<[f32; 3]>; 4],
}

/// One logical authored object from an area's GIT resource.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneAreaObject {
    /// Stable identity used across the sidebar, viewer, and scene reloads.
    pub key:                 String,
    /// Best available user-facing label.
    pub label:               String,
    /// Authored object category.
    pub kind:                SceneInstanceKind,
    /// Zero-based index within the corresponding GIT list.
    pub source_index:        usize,
    /// Authored instance tag.
    pub tag:                 Option<String>,
    /// Referenced blueprint resource name without its extension.
    pub template_resref:     Option<String>,
    /// Authored world-space position.
    pub position:            [f32; 3],
    /// Authored axis-angle rotation.
    pub rotation_axis_angle: [f32; 4],
}

/// One resolved model tree retained by the shared Rust scene runtime.
#[derive(Debug, Clone, PartialEq)]
pub enum SceneModel {
    /// A normal model plus recursively resolved attachments and supermodels.
    Composed(NwnComposedScene),
    /// Standalone auxiliary walkmesh scene.
    Auxiliary(NwnScene),
}

/// Decoded texture storage kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SceneTextureKind {
    /// NWN compact DDS.
    Dds,
    /// Truevision TGA.
    Tga,
    /// Palette texture resolved through NWN palette resources.
    Plt,
}

/// GPU compression used by an authored texture payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SceneTextureCompression {
    /// BC1 / DXT1 blocks.
    Dxt1,
    /// BC3 / DXT5 blocks.
    Dxt5,
}

/// One authored compressed mip level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneTextureMip {
    /// Pixel width.
    pub width:  u32,
    /// Pixel height.
    pub height: u32,
    /// GPU-ready compressed blocks.
    pub data:   Vec<u8>,
}

/// Authored compressed texture retained without RGBA expansion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneCompressedTexture {
    /// Block compression algorithm.
    pub compression: SceneTextureCompression,
    /// Complete authored mip chain.
    pub mip_levels:  Vec<SceneTextureMip>,
}

/// One GPU-ready texture retained by the Rust scene service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneTexture {
    /// Fully resolved resource name.
    pub resource:   String,
    /// Container provenance.
    pub origin:     String,
    /// Source storage kind.
    pub kind:       SceneTextureKind,
    /// Pixel width.
    pub width:      u32,
    /// Pixel height.
    pub height:     u32,
    /// Top-left-origin RGBA8 pixels for uncompressed resources.
    pub rgba8:      Vec<u8>,
    /// Authored DDS blocks retained without eager RGBA expansion.
    pub compressed: Option<SceneCompressedTexture>,
}

/// One TXI directive retained for material inspection and renderer behavior.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneTxiDirective {
    /// Directive name.
    pub name:          String,
    /// Inline arguments.
    pub arguments:     Vec<String>,
    /// Continuation rows.
    pub continuations: Vec<String>,
}

/// One MTR parameter row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneMtrParameter {
    /// Parameter name.
    pub name:       String,
    /// Authored type token.
    pub param_type: String,
    /// Numeric values.
    pub values:     Vec<f32>,
}

/// Resolved MTR metadata for a scene material.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneMtr {
    /// Resolved material resource.
    pub resource:        String,
    /// MTR render hint.
    pub render_hint:     Option<String>,
    /// Named parameters.
    pub parameters:      Vec<SceneMtrParameter>,
    /// Custom vertex shader resource.
    pub vertex_shader:   Option<String>,
    /// Custom geometry shader resource.
    pub geometry_shader: Option<String>,
    /// Custom fragment shader resource.
    pub fragment_shader: Option<String>,
}

/// One effective material texture slot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneMaterialTexture {
    /// Renderer-neutral role label.
    pub role:       String,
    /// `mdl` or `mtr` source.
    pub source:     String,
    /// Effective authored texture token.
    pub name:       String,
    /// Index into [`SceneDocument::textures`].
    pub texture:    Option<usize>,
    /// Parsed TXI directive stream.
    pub directives: Vec<SceneTxiDirective>,
}

/// Resolved assets for one scene material.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneMaterialAssets {
    /// Material index in the model scene.
    pub material_index: usize,
    /// Source node index.
    pub source_node:    usize,
    /// Effective render hint.
    pub render_hint:    Option<String>,
    /// Resolved MTR metadata.
    pub mtr:            Option<SceneMtr>,
    /// Effective texture slots.
    pub textures:       Vec<SceneMaterialTexture>,
}

/// Asset resolution tree corresponding to one model and its attachments.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneModelAssets {
    /// Model name.
    pub model_name:    String,
    /// Resolved materials for this model.
    pub materials:     Vec<SceneMaterialAssets>,
    /// Textures referenced by light flares and particle emitters.
    pub node_textures: Vec<SceneNodeTexture>,
    /// Asset trees for reference-model attachments.
    pub attachments:   Vec<SceneModelAssets>,
}

/// One resolved texture owned by a non-mesh scene node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneNodeTexture {
    /// Owning scene node index.
    pub node_index: usize,
    /// Semantic usage such as `emitter` or `flare:0`.
    pub role:       String,
    /// Authored texture token.
    pub name:       String,
    /// Index into [`SceneDocument::textures`].
    pub texture:    Option<usize>,
    /// Parsed TXI directives.
    pub directives: Vec<SceneTxiDirective>,
}

/// Custom shader stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SceneShaderStage {
    /// Vertex stage.
    Vertex,
    /// Geometry stage.
    Geometry,
    /// Fragment stage.
    Fragment,
}

/// Resolved custom shader source retained for inspection and translation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneShaderSource {
    /// Resolved SHD resource.
    pub resource: String,
    /// Container provenance.
    pub origin:   String,
    /// Shader stage selected by the MTR.
    pub stage:    SceneShaderStage,
    /// UTF-8 source text.
    pub source:   String,
}

/// Complete parsed area inputs retained for inspection and future editing.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneArea {
    /// ARE definition.
    pub area:      AreFile,
    /// GIT instances.
    pub instances: GitFile,
    /// Referenced SET catalog.
    pub tileset:   SetFile,
}

/// Module area catalog and entry-point metadata used by the viewport's area
/// selector.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneModule {
    /// Areas declared by the IFO in authored order.
    pub areas:           Vec<String>,
    /// Initially selected entry area.
    pub entry_area:      String,
    /// Entry position in the selected area.
    pub entry_position:  [Option<f32>; 3],
    /// Entry facing vector.
    pub entry_direction: [Option<f32>; 2],
    /// Custom TLK resource name.
    pub custom_tlk:      Option<String>,
    /// HAK dependencies in authored order.
    pub haks:            Vec<String>,
}

/// Renderer-neutral document shared by every frontend.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneDocument {
    /// Scene name.
    pub name:         String,
    /// Source resource category.
    pub source:       SceneSource,
    /// Resolved model assets.
    pub models:       Vec<SceneModel>,
    /// Resolved material tree aligned with [`Self::models`].
    pub model_assets: Vec<SceneModelAssets>,
    /// Deduplicated decoded textures referenced by model materials.
    pub textures:     Vec<SceneTexture>,
    /// Deduplicated custom shader sources.
    pub shaders:      Vec<SceneShaderSource>,
    /// Positioned model and overlay instances.
    pub instances:    Vec<SceneInstance>,
    /// Parsed area source data for area scenes.
    pub area:         Option<SceneArea>,
    /// Module catalog when the scene was opened through an IFO.
    pub module:       Option<SceneModule>,
    /// Active environment policy.
    pub environment:  SceneEnvironment,
    /// Complete resource dependency graph.
    pub dependencies: DependencyGraph,
    /// Non-fatal and fatal scene diagnostics.
    pub diagnostics:  Vec<SceneDiagnostic>,
}
