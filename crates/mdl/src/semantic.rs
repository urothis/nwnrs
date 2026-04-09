use std::{collections::BTreeSet, fs::File, io::Read, path::Path};

use nwnrs_resman::prelude::*;
use tracing::instrument;

use crate::{
    AsciiAnimation, AsciiBodyItem, AsciiElement, AsciiModel, AsciiNode, AsciiStatement,
    MODEL_RES_TYPE, Model, ModelError, ModelResult, parse_ascii_model, read_ascii_model,
};

/// A validated semantic MDL model lowered from the source-faithful ASCII AST.
#[derive(Debug, Clone, PartialEq)]
pub struct SemanticModel {
    /// Parsed model header data.
    pub header: SemanticHeader,
    /// Model name used by `beginmodelgeom`.
    pub geometry_name: String,
    /// Lowered geometry nodes in source order.
    pub nodes: Vec<SemanticNode>,
    /// Non-node geometry elements preserved from the source model.
    pub geometry_extras: Vec<AsciiElement>,
    /// Elements between `endmodelgeom` and the first animation or `donemodel`.
    pub between_geometry_and_animations: Vec<AsciiElement>,
    /// Lowered animations in source order.
    pub animations: Vec<SemanticAnimation>,
    /// Elements between adjacent animations in source order.
    pub between_animations: Vec<Vec<AsciiElement>>,
    /// Elements between the last animation and `donemodel`.
    pub suffix: Vec<AsciiElement>,
    /// Non-fatal diagnostics raised while lowering.
    pub diagnostics: Vec<ModelDiagnostic>,
}

impl SemanticModel {
    /// Returns the first lowered geometry node named `name`,
    /// case-insensitively.
    pub fn node(&self, name: &str) -> Option<&SemanticNode> {
        self.nodes
            .iter()
            .find(|node| node.name.eq_ignore_ascii_case(name))
    }

    /// Returns the first lowered animation named `name`, case-insensitively.
    pub fn animation(&self, name: &str) -> Option<&SemanticAnimation> {
        self.animations
            .iter()
            .find(|animation| animation.name.eq_ignore_ascii_case(name))
    }
}

/// Typed model header data lowered from top-level ASCII statements.
#[derive(Debug, Clone, PartialEq)]
pub struct SemanticHeader {
    /// Model name from `newmodel`.
    pub model_name:      String,
    /// Supermodel name from `setsupermodel`.
    pub supermodel:      Option<String>,
    /// Classification token from `classification`.
    pub classification:  Option<ModelClassification>,
    /// Animation scale from `setanimationscale`.
    pub animation_scale: Option<f32>,
    /// Comments preserved from the prefix section.
    pub comments:        Vec<String>,
    /// Unlowered prefix elements.
    pub extras:          Vec<AsciiElement>,
}

/// Known model classification values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelClassification {
    /// Character or creature model.
    Character,
    /// Tile model.
    Tile,
    /// Door model.
    Door,
    /// Effect or VFX model.
    Effect,
    /// GUI model.
    Gui,
    /// Item model.
    Item,
    /// Any other classification token.
    Other(String),
}

/// Known MDL node kinds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeKind {
    /// `dummy`
    Dummy,
    /// `trimesh`
    Trimesh,
    /// `danglymesh`
    Danglymesh,
    /// `skin`
    Skin,
    /// `emitter`
    Emitter,
    /// `light`
    Light,
    /// `aabb`
    Aabb,
    /// `reference`
    Reference,
    /// `patch`
    Patch,
    /// `animmesh`
    Animmesh,
    /// Any other node kind token.
    Other(String),
}

/// One lowered geometry node.
#[derive(Debug, Clone, PartialEq)]
pub struct SemanticNode {
    /// Typed node kind.
    pub kind:        NodeKind,
    /// Authored node type token.
    pub node_type:   String,
    /// Node name.
    pub name:        String,
    /// Parent node name, if not `NULL`.
    pub parent:      Option<String>,
    /// Parsed `#part-number` comment value, when present.
    pub part_number: Option<i32>,
    /// Static local position.
    pub position:    Option<[f32; 3]>,
    /// Static local orientation in source axis-angle order.
    pub orientation: Option<[f32; 4]>,
    /// Static uniform scale.
    pub scale:       Option<f32>,
    /// Static light/object color.
    pub color:       Option<[f32; 3]>,
    /// Static node radius.
    pub radius:      Option<f32>,
    /// Node center value when authored.
    pub center:      Option<[f32; 3]>,
    /// Node wireframe color when authored.
    pub wirecolor:   Option<[f32; 3]>,
    /// Lowered material and render flags.
    pub material:    SemanticMaterial,
    /// Light-specific payloads when this node is a light.
    pub light:       Option<SemanticLight>,
    /// Emitter-specific payloads when this node is an emitter.
    pub emitter:     Option<SemanticEmitter>,
    /// Reference-node payloads when this node is a reference.
    pub reference:   Option<SemanticReference>,
    /// Lowered mesh payloads when present.
    pub mesh:        Option<SemanticMesh>,
    /// Preserved node comments.
    pub comments:    Vec<String>,
    /// Unlowered node entries.
    pub extras:      Vec<AsciiElement>,
}

/// Material and render state attached to a geometry node.
#[derive(Debug, Clone, PartialEq)]
pub struct SemanticMaterial {
    /// `render`
    pub render:            Option<bool>,
    /// `shadow`
    pub shadow:            Option<bool>,
    /// `beaming`
    pub beaming:           Option<i32>,
    /// `inheritcolor`
    pub inherit_color:     Option<i32>,
    /// `tilefade`
    pub tilefade:          Option<i32>,
    /// `rotatetexture`
    pub rotate_texture:    Option<i32>,
    /// `transparencyhint`
    pub transparency_hint: Option<i32>,
    /// `shininess`
    pub shininess:         Option<f32>,
    /// `alpha`
    pub alpha:             Option<f32>,
    /// `ambient`
    pub ambient:           Option<[f32; 3]>,
    /// `diffuse`
    pub diffuse:           Option<[f32; 3]>,
    /// `specular`
    pub specular:          Option<[f32; 3]>,
    /// `selfillumcolor`
    pub self_illum_color:  Option<[f32; 3]>,
    /// `materialname`
    pub material_name:     Option<String>,
    /// `renderhint`
    pub render_hint:       Option<String>,
    /// `bitmap`
    pub bitmap:            Option<String>,
    /// `textureN` bindings in authored order.
    pub textures:          Vec<SemanticTextureBinding>,
}

/// One `textureN` binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticTextureBinding {
    /// Texture slot index.
    pub index: usize,
    /// Bound texture name.
    pub name:  String,
}

/// Typed mesh payloads captured from a geometry node.
#[derive(Debug, Clone, PartialEq)]
pub struct SemanticMesh {
    /// Vertex positions from `verts`.
    pub vertices:      Vec<[f32; 3]>,
    /// Triangle faces from `faces`.
    pub faces:         Vec<SemanticFace>,
    /// UV layers from `tverts` and `tvertsN`.
    pub uv_layers:     Vec<SemanticUvLayer>,
    /// Vertex normals from `normals`.
    pub normals:       Vec<[f32; 3]>,
    /// Tangent rows from `tangents`.
    pub tangents:      Vec<Vec<f32>>,
    /// Vertex color rows from `colors`.
    pub colors:        Vec<Vec<f32>>,
    /// Skin weight rows from `weights`.
    pub weights:       Vec<Vec<SemanticSkinWeight>>,
    /// Danglymesh constraint rows from `constraints`.
    pub constraints:   Vec<Vec<f32>>,
    /// Multimaterial labels from `multimaterial`.
    pub multimaterial: Vec<String>,
    /// Additional texture names from `texturenames`.
    pub texture_names: Vec<String>,
}

/// One named skin-weight influence.
#[derive(Debug, Clone, PartialEq)]
pub struct SemanticSkinWeight {
    /// Bone or node name referenced by the skin row.
    pub bone:   String,
    /// Influence weight for this bone.
    pub weight: f32,
}

/// Typed light payloads for `light` nodes.
#[derive(Debug, Clone, PartialEq)]
pub struct SemanticLight {
    /// `multiplier`
    pub multiplier:         Option<f32>,
    /// `ambientonly`
    pub ambient_only:       Option<i32>,
    /// `ndynamictype`
    pub n_dynamic_type:     Option<i32>,
    /// `isdynamic`
    pub is_dynamic:         Option<i32>,
    /// `affectdynamic`
    pub affect_dynamic:     Option<i32>,
    /// `negativelight`
    pub negative_light:     Option<i32>,
    /// `lightpriority`
    pub light_priority:     Option<i32>,
    /// `fadinglight`
    pub fading_light:       Option<i32>,
    /// `lensflares`
    pub lens_flares:        Option<i32>,
    /// `flareradius`
    pub flare_radius:       Option<f32>,
    /// `texturenames` for lens flares.
    pub flare_textures:     Vec<String>,
    /// `flaresizes`
    pub flare_sizes:        Vec<f32>,
    /// `flarepositions`
    pub flare_positions:    Vec<f32>,
    /// `flarecolorshifts`
    pub flare_color_shifts: Vec<[f32; 3]>,
}

/// Typed emitter payloads for `emitter` nodes.
#[derive(Debug, Clone, PartialEq)]
pub struct SemanticEmitter {
    /// `xsize`
    pub x_size:     Option<f32>,
    /// `ysize`
    pub y_size:     Option<f32>,
    /// Remaining authored emitter properties in source order.
    pub properties: Vec<SemanticEmitterProperty>,
}

/// One typed emitter property statement.
#[derive(Debug, Clone, PartialEq)]
pub struct SemanticEmitterProperty {
    /// Source keyword.
    pub name:   String,
    /// Typed property values in authored order.
    pub values: Vec<SemanticPropertyValue>,
}

/// One typed scalar/string property value.
#[derive(Debug, Clone, PartialEq)]
pub enum SemanticPropertyValue {
    /// Boolean token such as `true` or `0/1` where explicitly parsed as bool.
    Bool(bool),
    /// Integer token.
    Int(i32),
    /// Floating-point token.
    Float(f32),
    /// Text token preserved as-authored.
    Text(String),
}

/// Typed reference payloads for `reference` nodes.
#[derive(Debug, Clone, PartialEq)]
pub struct SemanticReference {
    /// `refmodel`
    pub model:        Option<String>,
    /// `reattachable`
    pub reattachable: Option<i32>,
}

/// One UV layer.
#[derive(Debug, Clone, PartialEq)]
pub struct SemanticUvLayer {
    /// UV layer index derived from `tverts` or `tvertsN`.
    pub index:       usize,
    /// UV coordinates for the layer.
    pub coordinates: Vec<[f32; 2]>,
}

/// One lowered face row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticFace {
    /// Vertex indices.
    pub vertex_indices: [u32; 3],
    /// Face group / smoothing / surface field from column 4.
    pub group:          i32,
    /// UV indices.
    pub uv_indices:     [u32; 3],
    /// Material slot / surface type field from column 8.
    pub material_index: i32,
}

/// One lowered animation block.
#[derive(Debug, Clone, PartialEq)]
pub struct SemanticAnimation {
    /// Animation name.
    pub name:       String,
    /// Referenced model name.
    pub model_name: String,
    /// `length`
    pub length:     Option<f32>,
    /// `transtime`
    pub transtime:  Option<f32>,
    /// `animroot`
    pub animroot:   Option<String>,
    /// `event` rows.
    pub events:     Vec<AnimationEvent>,
    /// Lowered animation node overlays.
    pub nodes:      Vec<SemanticAnimationNode>,
    /// Preserved animation comments.
    pub comments:   Vec<String>,
    /// Unlowered animation header/body elements.
    pub extras:     Vec<AsciiElement>,
}

impl SemanticAnimation {
    /// Returns the first lowered animation node named `name`,
    /// case-insensitively.
    pub fn node(&self, name: &str) -> Option<&SemanticAnimationNode> {
        self.nodes
            .iter()
            .find(|node| node.name.eq_ignore_ascii_case(name))
    }
}

/// One animation event.
#[derive(Debug, Clone, PartialEq)]
pub struct AnimationEvent {
    /// Event time in animation seconds.
    pub time: f32,
    /// Event name.
    pub name: String,
}

/// One lowered animation node overlay.
#[derive(Debug, Clone, PartialEq)]
pub struct SemanticAnimationNode {
    /// Typed node kind.
    pub kind:                  NodeKind,
    /// Authored node type token.
    pub node_type:             String,
    /// Target node name.
    pub name:                  String,
    /// Parent node name, if not `NULL`.
    pub parent:                Option<String>,
    /// Parsed `#part-number` comment value, when present.
    pub part_number:           Option<i32>,
    /// Static position override.
    pub position:              Option<[f32; 3]>,
    /// Static orientation override in source axis-angle order.
    pub orientation:           Option<[f32; 4]>,
    /// Static scale override.
    pub scale:                 Option<f32>,
    /// Static color override.
    pub color:                 Option<[f32; 3]>,
    /// Static radius override.
    pub radius:                Option<f32>,
    /// Static alpha override.
    pub alpha:                 Option<f32>,
    /// Static self-illumination override.
    pub self_illum_color:      Option<[f32; 3]>,
    /// `positionkey`
    pub position_keys:         Vec<Vec3Key>,
    /// `orientationkey`
    pub orientation_keys:      Vec<Vec4Key>,
    /// `scalekey`
    pub scale_keys:            Vec<ScalarKey>,
    /// `colorkey`
    pub color_keys:            Vec<Vec3Key>,
    /// `radiuskey`
    pub radius_keys:           Vec<ScalarKey>,
    /// `alphakey`
    pub alpha_keys:            Vec<ScalarKey>,
    /// `selfillumcolorkey` or `setfillumcolorkey`
    pub self_illum_color_keys: Vec<Vec3Key>,
    /// `sampleperiod`
    pub sample_period:         Option<f32>,
    /// `faces`
    pub faces:                 Vec<SemanticFace>,
    /// `animverts`
    pub animverts:             Vec<[f32; 3]>,
    /// `animtverts`
    pub animtverts:            Vec<[f32; 2]>,
    /// Preserved animation-node comments.
    pub comments:              Vec<String>,
    /// Unlowered animation-node entries.
    pub extras:                Vec<AsciiElement>,
}

/// One scalar animation key.
#[derive(Debug, Clone, PartialEq)]
pub struct ScalarKey {
    /// Key time in animation seconds.
    pub time:  f32,
    /// Scalar value.
    pub value: f32,
}

/// One 3D animation key.
#[derive(Debug, Clone, PartialEq)]
pub struct Vec3Key {
    /// Key time in animation seconds.
    pub time:  f32,
    /// 3D value.
    pub value: [f32; 3],
}

/// One 4D animation key.
#[derive(Debug, Clone, PartialEq)]
pub struct Vec4Key {
    /// Key time in animation seconds.
    pub time:  f32,
    /// 4D value.
    pub value: [f32; 4],
}

/// One non-fatal semantic lowering diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelDiagnostic {
    /// Diagnostic kind.
    pub kind:    ModelDiagnosticKind,
    /// Human-readable message.
    pub message: String,
}

/// Diagnostic categories raised by semantic lowering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ModelDiagnosticKind {
    /// Duplicate geometry node name.
    DuplicateNodeName,
    /// Parent reference did not resolve.
    MissingParent,
    /// Animation node targets an unknown geometry node.
    UnknownAnimationTarget,
    /// A statement value could not be parsed into the expected type.
    MalformedValue,
    /// A payload row did not match the expected width or numeric shape.
    MalformedPayloadRow,
}

impl Model {
    /// Parses and lowers the raw payload into a typed semantic model.
    pub fn parse_semantic(&self) -> ModelResult<SemanticModel> {
        lower_ascii_model(&self.parse_ascii()?)
    }
}

/// Parses and lowers a semantic model from ASCII MDL text.
pub fn parse_semantic_model(text: &str) -> ModelResult<SemanticModel> {
    lower_ascii_model(&parse_ascii_model(text)?)
}

/// Reads and lowers a semantic model from `reader`.
#[instrument(level = "debug", skip_all, err)]
pub fn read_semantic_model<R: Read>(reader: &mut R) -> ModelResult<SemanticModel> {
    let ascii = read_ascii_model(reader)?;
    lower_ascii_model(&ascii)
}

/// Reads and lowers a semantic model from disk.
#[instrument(level = "debug", skip_all, err, fields(path = %path.as_ref().display()))]
pub fn read_semantic_model_from_file(path: impl AsRef<Path>) -> ModelResult<SemanticModel> {
    let mut file = File::open(path.as_ref())?;
    read_semantic_model(&mut file)
}

/// Reads and lowers a semantic model from a [`Res`].
#[instrument(level = "debug", skip_all, err, fields(resref = %res.resref(), use_cache))]
pub fn read_semantic_model_from_res(res: &Res, use_cache: bool) -> ModelResult<SemanticModel> {
    if res.resref().res_type() != MODEL_RES_TYPE {
        return Err(ModelError::msg(format!(
            "expected mdl resource, got {}",
            res.resref()
        )));
    }

    let ascii = crate::read_ascii_model_from_res(res, use_cache)?;
    lower_ascii_model(&ascii)
}

/// Lowers a source-faithful ASCII MDL model into typed semantic data.
pub fn lower_ascii_model(model: &AsciiModel) -> ModelResult<SemanticModel> {
    let mut diagnostics = Vec::new();
    let header = lower_header(model, &mut diagnostics);

    let mut nodes = Vec::new();
    let mut geometry_extras = Vec::new();
    for item in &model.geometry {
        match item {
            AsciiBodyItem::Node(node) => nodes.push(lower_geometry_node(node, &mut diagnostics)),
            AsciiBodyItem::Element(element) => geometry_extras.push(element.clone()),
        }
    }

    validate_geometry_nodes(&nodes, &mut diagnostics);

    let node_names = lowercased_node_names(&nodes);
    let animations = model
        .animations
        .iter()
        .map(|animation| lower_animation(animation, &node_names, &mut diagnostics))
        .collect();

    Ok(SemanticModel {
        header,
        geometry_name: model.geometry_name.clone(),
        nodes,
        geometry_extras,
        between_geometry_and_animations: model.between_geometry_and_animations.clone(),
        animations,
        between_animations: model.between_animations.clone(),
        suffix: model.suffix.clone(),
        diagnostics,
    })
}

fn lower_header(model: &AsciiModel, diagnostics: &mut Vec<ModelDiagnostic>) -> SemanticHeader {
    let mut model_name = model.geometry_name.clone();
    let mut supermodel = None;
    let mut classification = None;
    let mut animation_scale = None;
    let mut comments = Vec::new();
    let mut extras = Vec::new();

    for element in &model.prefix {
        match element {
            AsciiElement::Comment(comment) => comments.push(comment.clone()),
            AsciiElement::Statement(statement) if statement.keyword_is("newmodel") => {
                if let Some(name) = statement.argument(0) {
                    model_name = name.to_string();
                    if !name.eq_ignore_ascii_case(&model.geometry_name) {
                        diagnostics.push(ModelDiagnostic {
                            kind:    ModelDiagnosticKind::MalformedValue,
                            message: format!(
                                "newmodel name {} does not match geometry name {}",
                                name, model.geometry_name
                            ),
                        });
                    }
                } else {
                    diagnostics.push(ModelDiagnostic {
                        kind:    ModelDiagnosticKind::MalformedValue,
                        message: "newmodel requires a model name".to_string(),
                    });
                }
            }
            AsciiElement::Statement(statement) if statement.keyword_is("setsupermodel") => {
                if let Some(declared_model) = statement.argument(0)
                    && !declared_model.eq_ignore_ascii_case(&model.geometry_name)
                {
                    diagnostics.push(ModelDiagnostic {
                        kind:    ModelDiagnosticKind::MalformedValue,
                        message: format!(
                            "setsupermodel model {} does not match geometry name {}",
                            declared_model, model.geometry_name
                        ),
                    });
                }
                supermodel = statement.argument(1).and_then(parse_optional_name);
            }
            AsciiElement::Statement(statement) if statement.keyword_is("classification") => {
                if let Some(value) = statement.argument(0) {
                    classification = Some(parse_classification(value));
                } else {
                    diagnostics.push(ModelDiagnostic {
                        kind:    ModelDiagnosticKind::MalformedValue,
                        message: "classification requires a value".to_string(),
                    });
                }
            }
            AsciiElement::Statement(statement) if statement.keyword_is("setanimationscale") => {
                animation_scale =
                    parse_f32_statement(statement, 0, "setanimationscale", diagnostics);
            }
            _ => extras.push(element.clone()),
        }
    }

    SemanticHeader {
        model_name,
        supermodel,
        classification,
        animation_scale,
        comments,
        extras,
    }
}

fn lower_geometry_node(node: &AsciiNode, diagnostics: &mut Vec<ModelDiagnostic>) -> SemanticNode {
    let mut lowered = SemanticNode {
        kind:        parse_node_kind(&node.node_type),
        node_type:   node.node_type.clone(),
        name:        node.name.clone(),
        parent:      None,
        part_number: None,
        position:    None,
        orientation: None,
        scale:       None,
        color:       None,
        radius:      None,
        center:      None,
        wirecolor:   None,
        material:    SemanticMaterial {
            render:            None,
            shadow:            None,
            beaming:           None,
            inherit_color:     None,
            tilefade:          None,
            rotate_texture:    None,
            transparency_hint: None,
            shininess:         None,
            alpha:             None,
            ambient:           None,
            diffuse:           None,
            specular:          None,
            self_illum_color:  None,
            material_name:     None,
            render_hint:       None,
            bitmap:            None,
            textures:          Vec::new(),
        },
        light:       None,
        emitter:     None,
        reference:   None,
        mesh:        None,
        comments:    Vec::new(),
        extras:      Vec::new(),
    };

    let mut mesh = SemanticMesh {
        vertices:      Vec::new(),
        faces:         Vec::new(),
        uv_layers:     Vec::new(),
        normals:       Vec::new(),
        tangents:      Vec::new(),
        colors:        Vec::new(),
        weights:       Vec::new(),
        constraints:   Vec::new(),
        multimaterial: Vec::new(),
        texture_names: Vec::new(),
    };

    for element in &node.entries {
        match element {
            AsciiElement::Comment(comment) => {
                lowered.comments.push(comment.clone());
                if lowered.part_number.is_none() {
                    lowered.part_number = parse_part_number_comment(comment);
                }
            }
            AsciiElement::Statement(statement) => {
                if !lower_common_node_statement(
                    statement,
                    &mut lowered.parent,
                    &mut lowered.position,
                    &mut lowered.orientation,
                    &mut lowered.scale,
                    &mut lowered.color,
                    &mut lowered.radius,
                    &mut lowered.center,
                    &mut lowered.wirecolor,
                    &mut lowered.material,
                    diagnostics,
                ) && !lower_special_node_statement(
                    &lowered.kind,
                    statement,
                    &mut lowered.light,
                    &mut lowered.emitter,
                    &mut lowered.reference,
                    &mut mesh,
                    diagnostics,
                ) && !lower_mesh_statement(statement, &mut mesh, diagnostics)
                {
                    lowered
                        .extras
                        .push(AsciiElement::Statement(statement.clone()));
                }
            }
        }
    }

    lowered.mesh = mesh_present(mesh);
    lowered
}

fn lower_animation(
    animation: &AsciiAnimation,
    geometry_node_names: &BTreeSet<String>,
    diagnostics: &mut Vec<ModelDiagnostic>,
) -> SemanticAnimation {
    let mut lowered = SemanticAnimation {
        name:       animation.name.clone(),
        model_name: animation.model_name.clone(),
        length:     None,
        transtime:  None,
        animroot:   None,
        events:     Vec::new(),
        nodes:      Vec::new(),
        comments:   Vec::new(),
        extras:     Vec::new(),
    };

    for item in &animation.body {
        match item {
            AsciiBodyItem::Element(AsciiElement::Comment(comment)) => {
                lowered.comments.push(comment.clone());
            }
            AsciiBodyItem::Element(AsciiElement::Statement(statement)) => {
                if statement.keyword_is("length") {
                    lowered.length = parse_f32_statement(statement, 0, "length", diagnostics);
                } else if statement.keyword_is("transtime") {
                    lowered.transtime = parse_f32_statement(statement, 0, "transtime", diagnostics);
                } else if statement.keyword_is("animroot") {
                    if let Some(name) = statement.argument(0) {
                        lowered.animroot =
                            parse_optional_name(name).or_else(|| Some(name.to_string()));
                    } else {
                        diagnostics.push(ModelDiagnostic {
                            kind:    ModelDiagnosticKind::MalformedValue,
                            message: format!(
                                "animation {} has animroot without a value",
                                animation.name
                            ),
                        });
                    }
                } else if statement.keyword_is("event") {
                    match (
                        parse_f32_statement(statement, 0, "event", diagnostics),
                        statement.argument(1),
                    ) {
                        (Some(time), Some(name)) => lowered.events.push(AnimationEvent {
                            time,
                            name: name.to_string(),
                        }),
                        _ => diagnostics.push(ModelDiagnostic {
                            kind:    ModelDiagnosticKind::MalformedValue,
                            message: format!(
                                "animation {} has malformed event statement",
                                animation.name
                            ),
                        }),
                    }
                } else {
                    lowered
                        .extras
                        .push(AsciiElement::Statement(statement.clone()));
                }
            }
            AsciiBodyItem::Node(node) => {
                let lowered_node = lower_animation_node(node, diagnostics);
                if !geometry_node_names.contains(&lowered_node.name.to_ascii_lowercase()) {
                    diagnostics.push(ModelDiagnostic {
                        kind:    ModelDiagnosticKind::UnknownAnimationTarget,
                        message: format!(
                            "animation {} targets unknown node {}",
                            animation.name, lowered_node.name
                        ),
                    });
                }
                if let Some(parent) = &lowered_node.parent
                    && !geometry_node_names.contains(&parent.to_ascii_lowercase())
                {
                    diagnostics.push(ModelDiagnostic {
                        kind:    ModelDiagnosticKind::MissingParent,
                        message: format!(
                            "animation node {} in {} references missing parent {}",
                            lowered_node.name, animation.name, parent
                        ),
                    });
                }
                lowered.nodes.push(lowered_node);
            }
        }
    }

    lowered
}

fn lower_animation_node(
    node: &AsciiNode,
    diagnostics: &mut Vec<ModelDiagnostic>,
) -> SemanticAnimationNode {
    let mut lowered = SemanticAnimationNode {
        kind:                  parse_node_kind(&node.node_type),
        node_type:             node.node_type.clone(),
        name:                  node.name.clone(),
        parent:                None,
        part_number:           None,
        position:              None,
        orientation:           None,
        scale:                 None,
        color:                 None,
        radius:                None,
        alpha:                 None,
        self_illum_color:      None,
        position_keys:         Vec::new(),
        orientation_keys:      Vec::new(),
        scale_keys:            Vec::new(),
        color_keys:            Vec::new(),
        radius_keys:           Vec::new(),
        alpha_keys:            Vec::new(),
        self_illum_color_keys: Vec::new(),
        sample_period:         None,
        faces:                 Vec::new(),
        animverts:             Vec::new(),
        animtverts:            Vec::new(),
        comments:              Vec::new(),
        extras:                Vec::new(),
    };

    for element in &node.entries {
        match element {
            AsciiElement::Comment(comment) => {
                lowered.comments.push(comment.clone());
                if lowered.part_number.is_none() {
                    lowered.part_number = parse_part_number_comment(comment);
                }
            }
            AsciiElement::Statement(statement) => {
                if statement.keyword_is("parent") {
                    lowered.parent = statement.argument(0).and_then(parse_optional_name);
                } else if statement.keyword_is("position") {
                    lowered.position = parse_vec3_statement(statement, "position", diagnostics);
                } else if statement.keyword_is("orientation") {
                    lowered.orientation =
                        parse_vec4_statement(statement, "orientation", diagnostics);
                } else if statement.keyword_is("scale") {
                    lowered.scale = parse_f32_statement(statement, 0, "scale", diagnostics);
                } else if statement.keyword_is("color") {
                    lowered.color = parse_vec3_statement(statement, "color", diagnostics);
                } else if statement.keyword_is("radius") {
                    lowered.radius = parse_f32_statement(statement, 0, "radius", diagnostics);
                } else if statement.keyword_is("alpha") {
                    lowered.alpha = parse_f32_statement(statement, 0, "alpha", diagnostics);
                } else if statement.keyword_is("selfillumcolor")
                    || statement.keyword_is("setfillumcolor")
                {
                    lowered.self_illum_color =
                        parse_vec3_statement(statement, "selfillumcolor", diagnostics);
                } else if statement.keyword_is("positionkey") {
                    lowered.position_keys = parse_vec3_keys(statement, "positionkey", diagnostics);
                } else if statement.keyword_is("orientationkey") {
                    lowered.orientation_keys =
                        parse_vec4_keys(statement, "orientationkey", diagnostics);
                } else if statement.keyword_is("scalekey") {
                    lowered.scale_keys = parse_scalar_keys(statement, "scalekey", diagnostics);
                } else if statement.keyword_is("colorkey") {
                    lowered.color_keys = parse_vec3_keys(statement, "colorkey", diagnostics);
                } else if statement.keyword_is("radiuskey") {
                    lowered.radius_keys = parse_scalar_keys(statement, "radiuskey", diagnostics);
                } else if statement.keyword_is("alphakey") {
                    lowered.alpha_keys = parse_scalar_keys(statement, "alphakey", diagnostics);
                } else if statement.keyword_is("selfillumcolorkey")
                    || statement.keyword_is("setfillumcolorkey")
                {
                    lowered.self_illum_color_keys =
                        parse_vec3_keys(statement, "selfillumcolorkey", diagnostics);
                } else if statement.keyword_is("sampleperiod") {
                    lowered.sample_period =
                        parse_f32_statement(statement, 0, "sampleperiod", diagnostics);
                } else if statement.keyword_is("faces") {
                    lowered.faces = parse_faces(statement, diagnostics);
                } else if statement.keyword_is("animverts") {
                    lowered.animverts = parse_vec3_payload(statement, "animverts", diagnostics);
                } else if statement.keyword_is("animtverts") {
                    lowered.animtverts = parse_vec2_payload(statement, "animtverts", diagnostics);
                } else {
                    lowered
                        .extras
                        .push(AsciiElement::Statement(statement.clone()));
                }
            }
        }
    }

    lowered
}

#[allow(clippy::too_many_arguments)]
fn lower_common_node_statement(
    statement: &AsciiStatement,
    parent: &mut Option<String>,
    position: &mut Option<[f32; 3]>,
    orientation: &mut Option<[f32; 4]>,
    scale: &mut Option<f32>,
    color: &mut Option<[f32; 3]>,
    radius: &mut Option<f32>,
    center: &mut Option<[f32; 3]>,
    wirecolor: &mut Option<[f32; 3]>,
    material: &mut SemanticMaterial,
    diagnostics: &mut Vec<ModelDiagnostic>,
) -> bool {
    if statement.keyword_is("parent") {
        *parent = statement.argument(0).and_then(parse_optional_name);
    } else if statement.keyword_is("position") {
        *position = parse_vec3_statement(statement, "position", diagnostics);
    } else if statement.keyword_is("orientation") {
        *orientation = parse_vec4_statement(statement, "orientation", diagnostics);
    } else if statement.keyword_is("scale") {
        *scale = parse_f32_statement(statement, 0, "scale", diagnostics);
    } else if statement.keyword_is("color") {
        *color = parse_vec3_statement(statement, "color", diagnostics);
    } else if statement.keyword_is("radius") {
        *radius = parse_f32_statement(statement, 0, "radius", diagnostics);
    } else if statement.keyword_is("center") {
        *center = parse_vec3_statement(statement, "center", diagnostics);
    } else if statement.keyword_is("wirecolor") {
        *wirecolor = parse_vec3_statement(statement, "wirecolor", diagnostics);
    } else if statement.keyword_is("render") {
        material.render = parse_bool_statement(statement, 0, "render", diagnostics);
    } else if statement.keyword_is("shadow") {
        material.shadow = parse_bool_statement(statement, 0, "shadow", diagnostics);
    } else if statement.keyword_is("beaming") {
        material.beaming = parse_i32_statement(statement, 0, "beaming", diagnostics);
    } else if statement.keyword_is("inheritcolor") {
        material.inherit_color = parse_i32_statement(statement, 0, "inheritcolor", diagnostics);
    } else if statement.keyword_is("tilefade") {
        material.tilefade = parse_i32_statement(statement, 0, "tilefade", diagnostics);
    } else if statement.keyword_is("rotatetexture") {
        material.rotate_texture = parse_i32_statement(statement, 0, "rotatetexture", diagnostics);
    } else if statement.keyword_is("transparencyhint") {
        material.transparency_hint =
            parse_i32_statement(statement, 0, "transparencyhint", diagnostics);
    } else if statement.keyword_is("shininess") {
        material.shininess = parse_f32_statement(statement, 0, "shininess", diagnostics);
    } else if statement.keyword_is("alpha") {
        material.alpha = parse_f32_statement(statement, 0, "alpha", diagnostics);
    } else if statement.keyword_is("ambient") {
        material.ambient = parse_vec3_statement(statement, "ambient", diagnostics);
    } else if statement.keyword_is("diffuse") {
        material.diffuse = parse_vec3_statement(statement, "diffuse", diagnostics);
    } else if statement.keyword_is("specular") {
        material.specular = parse_vec3_statement(statement, "specular", diagnostics);
    } else if statement.keyword_is("selfillumcolor") || statement.keyword_is("setfillumcolor") {
        material.self_illum_color = parse_vec3_statement(statement, "selfillumcolor", diagnostics);
    } else if statement.keyword_is("materialname") {
        material.material_name = statement.argument(0).map(ToOwned::to_owned);
    } else if statement.keyword_is("renderhint") {
        material.render_hint = statement.argument(0).map(ToOwned::to_owned);
    } else if statement.keyword_is("bitmap") {
        material.bitmap = statement.argument(0).map(ToOwned::to_owned);
    } else if let Some(index) = parse_texture_index(&statement.keyword) {
        if let Some(name) = statement.argument(0) {
            material.textures.push(SemanticTextureBinding {
                index,
                name: name.to_string(),
            });
        } else {
            diagnostics.push(ModelDiagnostic {
                kind:    ModelDiagnosticKind::MalformedValue,
                message: format!("{} requires a texture name", statement.keyword),
            });
        }
    } else {
        return false;
    }

    true
}

fn lower_mesh_statement(
    statement: &AsciiStatement,
    mesh: &mut SemanticMesh,
    diagnostics: &mut Vec<ModelDiagnostic>,
) -> bool {
    let keyword = statement.keyword.to_ascii_lowercase();
    if keyword == "verts" {
        mesh.vertices = parse_vec3_payload(statement, "verts", diagnostics);
    } else if keyword == "faces" {
        mesh.faces = parse_faces(statement, diagnostics);
    } else if let Some(index) = parse_tverts_index(&keyword) {
        mesh.uv_layers.push(SemanticUvLayer {
            index,
            coordinates: parse_vec2_payload(statement, &keyword, diagnostics),
        });
    } else if keyword == "normals" {
        mesh.normals = parse_vec3_payload(statement, "normals", diagnostics);
    } else if keyword == "tangents" {
        mesh.tangents = parse_float_rows(statement, "tangents", diagnostics);
    } else if keyword == "colors" {
        mesh.colors = parse_float_rows(statement, "colors", diagnostics);
    } else if keyword == "weights" {
        mesh.weights = parse_skin_weights(statement, diagnostics);
    } else if keyword == "constraints" {
        mesh.constraints = parse_float_rows(statement, "constraints", diagnostics);
    } else if keyword == "multimaterial" {
        mesh.multimaterial = parse_string_rows(statement);
    } else if keyword == "texturenames" {
        mesh.texture_names = parse_string_rows(statement);
    } else {
        return false;
    }

    true
}

fn lower_special_node_statement(
    node_kind: &NodeKind,
    statement: &AsciiStatement,
    light: &mut Option<SemanticLight>,
    emitter: &mut Option<SemanticEmitter>,
    reference: &mut Option<SemanticReference>,
    mesh: &mut SemanticMesh,
    diagnostics: &mut Vec<ModelDiagnostic>,
) -> bool {
    match node_kind {
        NodeKind::Skin => {
            if statement.keyword_is("weights") {
                mesh.weights = parse_skin_weights(statement, diagnostics);
                true
            } else {
                false
            }
        }
        NodeKind::Light => lower_light_statement(statement, light, diagnostics),
        NodeKind::Emitter => lower_emitter_statement(statement, emitter),
        NodeKind::Reference => lower_reference_statement(statement, reference, diagnostics),
        _ => false,
    }
}

fn lower_light_statement(
    statement: &AsciiStatement,
    light: &mut Option<SemanticLight>,
    diagnostics: &mut Vec<ModelDiagnostic>,
) -> bool {
    let light = light.get_or_insert_with(|| SemanticLight {
        multiplier:         None,
        ambient_only:       None,
        n_dynamic_type:     None,
        is_dynamic:         None,
        affect_dynamic:     None,
        negative_light:     None,
        light_priority:     None,
        fading_light:       None,
        lens_flares:        None,
        flare_radius:       None,
        flare_textures:     Vec::new(),
        flare_sizes:        Vec::new(),
        flare_positions:    Vec::new(),
        flare_color_shifts: Vec::new(),
    });

    if statement.keyword_is("multiplier") {
        light.multiplier = parse_f32_statement(statement, 0, "multiplier", diagnostics);
    } else if statement.keyword_is("ambientonly") {
        light.ambient_only = parse_i32_statement(statement, 0, "ambientonly", diagnostics);
    } else if statement.keyword_is("ndynamictype") {
        light.n_dynamic_type = parse_i32_statement(statement, 0, "ndynamictype", diagnostics);
    } else if statement.keyword_is("isdynamic") {
        light.is_dynamic = parse_i32_statement(statement, 0, "isdynamic", diagnostics);
    } else if statement.keyword_is("affectdynamic") {
        light.affect_dynamic = parse_i32_statement(statement, 0, "affectdynamic", diagnostics);
    } else if statement.keyword_is("negativelight") {
        light.negative_light = parse_i32_statement(statement, 0, "negativelight", diagnostics);
    } else if statement.keyword_is("lightpriority") {
        light.light_priority = parse_i32_statement(statement, 0, "lightpriority", diagnostics);
    } else if statement.keyword_is("fadinglight") {
        light.fading_light = parse_i32_statement(statement, 0, "fadinglight", diagnostics);
    } else if statement.keyword_is("lensflares") {
        light.lens_flares = parse_i32_statement(statement, 0, "lensflares", diagnostics);
    } else if statement.keyword_is("flareradius") {
        light.flare_radius = parse_f32_statement(statement, 0, "flareradius", diagnostics);
    } else if statement.keyword_is("texturenames") {
        light.flare_textures = parse_string_rows(statement);
    } else if statement.keyword_is("flaresizes") {
        light.flare_sizes = parse_scalar_payload(statement, "flaresizes", diagnostics);
    } else if statement.keyword_is("flarepositions") {
        light.flare_positions = parse_scalar_payload(statement, "flarepositions", diagnostics);
    } else if statement.keyword_is("flarecolorshifts") {
        light.flare_color_shifts = parse_vec3_payload(statement, "flarecolorshifts", diagnostics);
    } else {
        return false;
    }

    true
}

fn lower_emitter_statement(
    statement: &AsciiStatement,
    emitter: &mut Option<SemanticEmitter>,
) -> bool {
    let emitter = emitter.get_or_insert_with(|| SemanticEmitter {
        x_size:     None,
        y_size:     None,
        properties: Vec::new(),
    });

    if statement.keyword_is("xsize") {
        emitter.x_size = statement
            .argument(0)
            .and_then(|value| value.parse::<f32>().ok());
    } else if statement.keyword_is("ysize") {
        emitter.y_size = statement
            .argument(0)
            .and_then(|value| value.parse::<f32>().ok());
    } else {
        emitter.properties.push(SemanticEmitterProperty {
            name:   statement.keyword.clone(),
            values: statement
                .arguments
                .iter()
                .map(|value| parse_property_value(value))
                .collect(),
        });
    }

    true
}

fn lower_reference_statement(
    statement: &AsciiStatement,
    reference: &mut Option<SemanticReference>,
    diagnostics: &mut Vec<ModelDiagnostic>,
) -> bool {
    let reference = reference.get_or_insert_with(|| SemanticReference {
        model:        None,
        reattachable: None,
    });

    if statement.keyword_is("refmodel") {
        reference.model = statement.argument(0).and_then(parse_optional_name);
    } else if statement.keyword_is("reattachable") {
        reference.reattachable = parse_i32_statement(statement, 0, "reattachable", diagnostics);
    } else {
        return false;
    }

    true
}

fn validate_geometry_nodes(nodes: &[SemanticNode], diagnostics: &mut Vec<ModelDiagnostic>) {
    let mut seen = BTreeSet::new();
    let names = lowercased_node_names(nodes);
    for node in nodes {
        let lowered_name = node.name.to_ascii_lowercase();
        if !seen.insert(lowered_name) {
            diagnostics.push(ModelDiagnostic {
                kind:    ModelDiagnosticKind::DuplicateNodeName,
                message: format!("duplicate geometry node name {}", node.name),
            });
        }

        if let Some(parent) = &node.parent
            && !names.contains(&parent.to_ascii_lowercase())
        {
            diagnostics.push(ModelDiagnostic {
                kind:    ModelDiagnosticKind::MissingParent,
                message: format!("node {} references missing parent {}", node.name, parent),
            });
        }
    }
}

fn lowercased_node_names(nodes: &[SemanticNode]) -> BTreeSet<String> {
    nodes
        .iter()
        .map(|node| node.name.to_ascii_lowercase())
        .collect()
}

fn mesh_present(mesh: SemanticMesh) -> Option<SemanticMesh> {
    if mesh.vertices.is_empty()
        && mesh.faces.is_empty()
        && mesh.uv_layers.is_empty()
        && mesh.normals.is_empty()
        && mesh.tangents.is_empty()
        && mesh.colors.is_empty()
        && mesh.weights.is_empty()
        && mesh.constraints.is_empty()
        && mesh.multimaterial.is_empty()
        && mesh.texture_names.is_empty()
    {
        None
    } else {
        Some(mesh)
    }
}

fn parse_classification(value: &str) -> ModelClassification {
    match value.to_ascii_lowercase().as_str() {
        "character" => ModelClassification::Character,
        "tile" => ModelClassification::Tile,
        "door" => ModelClassification::Door,
        "effect" => ModelClassification::Effect,
        "gui" => ModelClassification::Gui,
        "item" => ModelClassification::Item,
        _ => ModelClassification::Other(value.to_string()),
    }
}

fn parse_node_kind(value: &str) -> NodeKind {
    match value.to_ascii_lowercase().as_str() {
        "dummy" => NodeKind::Dummy,
        "trimesh" => NodeKind::Trimesh,
        "danglymesh" => NodeKind::Danglymesh,
        "skin" => NodeKind::Skin,
        "emitter" => NodeKind::Emitter,
        "light" => NodeKind::Light,
        "aabb" => NodeKind::Aabb,
        "reference" => NodeKind::Reference,
        "patch" => NodeKind::Patch,
        "animmesh" => NodeKind::Animmesh,
        _ => NodeKind::Other(value.to_string()),
    }
}

fn parse_optional_name(value: &str) -> Option<String> {
    if value.eq_ignore_ascii_case("null") {
        None
    } else {
        Some(value.to_string())
    }
}

fn parse_part_number_comment(comment: &str) -> Option<i32> {
    comment
        .trim_start()
        .strip_prefix("#part-number")
        .and_then(|value| value.trim().parse::<i32>().ok())
}

fn parse_texture_index(keyword: &str) -> Option<usize> {
    let suffix = keyword.to_ascii_lowercase();
    suffix
        .strip_prefix("texture")
        .and_then(|value| value.parse::<usize>().ok())
}

fn parse_tverts_index(keyword: &str) -> Option<usize> {
    keyword.strip_prefix("tverts").and_then(|suffix| {
        if suffix.is_empty() {
            Some(0)
        } else {
            suffix.parse::<usize>().ok()
        }
    })
}

fn parse_bool_statement(
    statement: &AsciiStatement,
    index: usize,
    keyword: &str,
    diagnostics: &mut Vec<ModelDiagnostic>,
) -> Option<bool> {
    statement
        .argument(index)
        .and_then(|value| match value.to_ascii_lowercase().as_str() {
            "0" | "false" => Some(false),
            "1" | "true" => Some(true),
            _ => {
                diagnostics.push(ModelDiagnostic {
                    kind:    ModelDiagnosticKind::MalformedValue,
                    message: format!("{keyword} expects a boolean, got {value}"),
                });
                None
            }
        })
}

fn parse_i32_statement(
    statement: &AsciiStatement,
    index: usize,
    keyword: &str,
    diagnostics: &mut Vec<ModelDiagnostic>,
) -> Option<i32> {
    parse_i32_arg(statement.argument(index), keyword, diagnostics)
}

fn parse_f32_statement(
    statement: &AsciiStatement,
    index: usize,
    keyword: &str,
    diagnostics: &mut Vec<ModelDiagnostic>,
) -> Option<f32> {
    parse_f32_arg(statement.argument(index), keyword, diagnostics)
}

fn parse_vec3_statement(
    statement: &AsciiStatement,
    keyword: &str,
    diagnostics: &mut Vec<ModelDiagnostic>,
) -> Option<[f32; 3]> {
    parse_f32_array(&statement.arguments, keyword, diagnostics)
}

fn parse_vec4_statement(
    statement: &AsciiStatement,
    keyword: &str,
    diagnostics: &mut Vec<ModelDiagnostic>,
) -> Option<[f32; 4]> {
    parse_f32_array(&statement.arguments, keyword, diagnostics)
}

fn parse_scalar_keys(
    statement: &AsciiStatement,
    keyword: &str,
    diagnostics: &mut Vec<ModelDiagnostic>,
) -> Vec<ScalarKey> {
    statement
        .payload_rows
        .iter()
        .enumerate()
        .filter_map(|(row_index, row)| {
            parse_f32_row_array::<2>(row, keyword, row_index, diagnostics).map(|values| ScalarKey {
                time:  values[0],
                value: values[1],
            })
        })
        .collect()
}

fn parse_vec3_keys(
    statement: &AsciiStatement,
    keyword: &str,
    diagnostics: &mut Vec<ModelDiagnostic>,
) -> Vec<Vec3Key> {
    statement
        .payload_rows
        .iter()
        .enumerate()
        .filter_map(|(row_index, row)| {
            parse_f32_row_array::<4>(row, keyword, row_index, diagnostics).map(|values| Vec3Key {
                time:  values[0],
                value: [values[1], values[2], values[3]],
            })
        })
        .collect()
}

fn parse_vec4_keys(
    statement: &AsciiStatement,
    keyword: &str,
    diagnostics: &mut Vec<ModelDiagnostic>,
) -> Vec<Vec4Key> {
    statement
        .payload_rows
        .iter()
        .enumerate()
        .filter_map(|(row_index, row)| {
            parse_f32_row_array::<5>(row, keyword, row_index, diagnostics).map(|values| Vec4Key {
                time:  values[0],
                value: [values[1], values[2], values[3], values[4]],
            })
        })
        .collect()
}

fn parse_vec2_payload(
    statement: &AsciiStatement,
    keyword: &str,
    diagnostics: &mut Vec<ModelDiagnostic>,
) -> Vec<[f32; 2]> {
    statement
        .payload_rows
        .iter()
        .enumerate()
        .filter_map(|(row_index, row)| {
            parse_f32_row_array::<2>(row, keyword, row_index, diagnostics)
        })
        .collect()
}

fn parse_vec3_payload(
    statement: &AsciiStatement,
    keyword: &str,
    diagnostics: &mut Vec<ModelDiagnostic>,
) -> Vec<[f32; 3]> {
    statement
        .payload_rows
        .iter()
        .enumerate()
        .filter_map(|(row_index, row)| {
            parse_f32_row_array::<3>(row, keyword, row_index, diagnostics)
        })
        .collect()
}

fn parse_scalar_payload(
    statement: &AsciiStatement,
    keyword: &str,
    diagnostics: &mut Vec<ModelDiagnostic>,
) -> Vec<f32> {
    statement
        .payload_rows
        .iter()
        .enumerate()
        .filter_map(|(row_index, row)| {
            parse_f32_row_array::<1>(row, keyword, row_index, diagnostics).map(|values| values[0])
        })
        .collect()
}

fn parse_faces(
    statement: &AsciiStatement,
    diagnostics: &mut Vec<ModelDiagnostic>,
) -> Vec<SemanticFace> {
    statement
        .payload_rows
        .iter()
        .enumerate()
        .filter_map(|(row_index, row)| {
            parse_i32_row_array::<8>(row, "faces", row_index, diagnostics).and_then(|values| {
                let v0 = u32::try_from(values[0]).ok();
                let v1 = u32::try_from(values[1]).ok();
                let v2 = u32::try_from(values[2]).ok();
                let tv0 = u32::try_from(values[4]).ok();
                let tv1 = u32::try_from(values[5]).ok();
                let tv2 = u32::try_from(values[6]).ok();
                match (v0, v1, v2, tv0, tv1, tv2) {
                    (Some(v0), Some(v1), Some(v2), Some(tv0), Some(tv1), Some(tv2)) => {
                        Some(SemanticFace {
                            vertex_indices: [v0, v1, v2],
                            group:          values[3],
                            uv_indices:     [tv0, tv1, tv2],
                            material_index: values[7],
                        })
                    }
                    _ => {
                        diagnostics.push(ModelDiagnostic {
                            kind:    ModelDiagnosticKind::MalformedPayloadRow,
                            message: format!(
                                "faces row {} contains negative indices",
                                row_index + 1
                            ),
                        });
                        None
                    }
                }
            })
        })
        .collect()
}

fn parse_float_rows(
    statement: &AsciiStatement,
    keyword: &str,
    diagnostics: &mut Vec<ModelDiagnostic>,
) -> Vec<Vec<f32>> {
    statement
        .payload_rows
        .iter()
        .enumerate()
        .filter_map(|(row_index, row)| {
            let mut parsed = Vec::with_capacity(row.len());
            for value in row {
                match value.parse::<f32>() {
                    Ok(value) => parsed.push(value),
                    Err(_) => {
                        diagnostics.push(ModelDiagnostic {
                            kind:    ModelDiagnosticKind::MalformedPayloadRow,
                            message: format!(
                                "{keyword} row {} contains non-float token {}",
                                row_index + 1,
                                value
                            ),
                        });
                        return None;
                    }
                }
            }
            Some(parsed)
        })
        .collect()
}

fn parse_skin_weights(
    statement: &AsciiStatement,
    diagnostics: &mut Vec<ModelDiagnostic>,
) -> Vec<Vec<SemanticSkinWeight>> {
    statement
        .payload_rows
        .iter()
        .enumerate()
        .filter_map(|(row_index, row)| {
            if !row.len().is_multiple_of(2) {
                diagnostics.push(ModelDiagnostic {
                    kind:    ModelDiagnosticKind::MalformedPayloadRow,
                    message: format!(
                        "weights row {} expects name/weight pairs, got {} values",
                        row_index + 1,
                        row.len()
                    ),
                });
                return None;
            }

            let mut parsed = Vec::with_capacity(row.len() / 2);
            for chunk in row.chunks(2) {
                let Some(bone) = chunk.first() else {
                    diagnostics.push(ModelDiagnostic {
                        kind:    ModelDiagnosticKind::MalformedPayloadRow,
                        message: format!("weights row {} is missing a bone name", row_index + 1),
                    });
                    return None;
                };
                let Some(weight) = chunk.get(1).and_then(|value| value.parse::<f32>().ok()) else {
                    diagnostics.push(ModelDiagnostic {
                        kind:    ModelDiagnosticKind::MalformedPayloadRow,
                        message: format!(
                            "weights row {} contains invalid weight {}",
                            row_index + 1,
                            chunk.get(1).map_or("", String::as_str)
                        ),
                    });
                    return None;
                };
                parsed.push(SemanticSkinWeight {
                    bone: bone.clone(),
                    weight,
                });
            }
            Some(parsed)
        })
        .collect()
}

fn parse_string_rows(statement: &AsciiStatement) -> Vec<String> {
    statement
        .payload_rows
        .iter()
        .map(|row| row.join(" "))
        .collect()
}

fn parse_property_value(value: &str) -> SemanticPropertyValue {
    if value.eq_ignore_ascii_case("true") {
        SemanticPropertyValue::Bool(true)
    } else if value.eq_ignore_ascii_case("false") {
        SemanticPropertyValue::Bool(false)
    } else if let Ok(parsed) = value.parse::<i32>() {
        SemanticPropertyValue::Int(parsed)
    } else if let Ok(parsed) = value.parse::<f32>() {
        SemanticPropertyValue::Float(parsed)
    } else {
        SemanticPropertyValue::Text(value.to_string())
    }
}

fn parse_f32_arg(
    value: Option<&str>,
    keyword: &str,
    diagnostics: &mut Vec<ModelDiagnostic>,
) -> Option<f32> {
    match value {
        Some(value) => match value.parse::<f32>() {
            Ok(value) => Some(value),
            Err(_) => {
                diagnostics.push(ModelDiagnostic {
                    kind:    ModelDiagnosticKind::MalformedValue,
                    message: format!("{keyword} expects a float, got {value}"),
                });
                None
            }
        },
        None => {
            diagnostics.push(ModelDiagnostic {
                kind:    ModelDiagnosticKind::MalformedValue,
                message: format!("{keyword} is missing a value"),
            });
            None
        }
    }
}

fn parse_i32_arg(
    value: Option<&str>,
    keyword: &str,
    diagnostics: &mut Vec<ModelDiagnostic>,
) -> Option<i32> {
    match value {
        Some(value) => match value.parse::<i32>() {
            Ok(value) => Some(value),
            Err(_) => {
                diagnostics.push(ModelDiagnostic {
                    kind:    ModelDiagnosticKind::MalformedValue,
                    message: format!("{keyword} expects an integer, got {value}"),
                });
                None
            }
        },
        None => {
            diagnostics.push(ModelDiagnostic {
                kind:    ModelDiagnosticKind::MalformedValue,
                message: format!("{keyword} is missing a value"),
            });
            None
        }
    }
}

fn parse_f32_array<const N: usize>(
    arguments: &[String],
    keyword: &str,
    diagnostics: &mut Vec<ModelDiagnostic>,
) -> Option<[f32; N]> {
    if arguments.len() < N {
        diagnostics.push(ModelDiagnostic {
            kind:    ModelDiagnosticKind::MalformedValue,
            message: format!(
                "{keyword} expects at least {N} values, got {}",
                arguments.len()
            ),
        });
        return None;
    }

    let parsed = arguments
        .iter()
        .take(N)
        .map(|value| value.parse::<f32>())
        .collect::<Result<Vec<_>, _>>();
    match parsed {
        Ok(values) => match <Vec<f32> as TryInto<[f32; N]>>::try_into(values) {
            Ok(array) => Some(array),
            Err(_values) => {
                diagnostics.push(ModelDiagnostic {
                    kind:    ModelDiagnosticKind::MalformedValue,
                    message: format!("{keyword} could not be converted into a fixed-width array"),
                });
                None
            }
        },
        Err(_parse_error) => {
            diagnostics.push(ModelDiagnostic {
                kind:    ModelDiagnosticKind::MalformedValue,
                message: format!("{keyword} contains a non-float value"),
            });
            None
        }
    }
}

fn parse_f32_row_array<const N: usize>(
    row: &[String],
    keyword: &str,
    row_index: usize,
    diagnostics: &mut Vec<ModelDiagnostic>,
) -> Option<[f32; N]> {
    if row.len() < N {
        diagnostics.push(ModelDiagnostic {
            kind:    ModelDiagnosticKind::MalformedPayloadRow,
            message: format!(
                "{keyword} row {} expects at least {N} values, got {}",
                row_index + 1,
                row.len()
            ),
        });
        return None;
    }

    let parsed = row
        .iter()
        .take(N)
        .map(|value| value.parse::<f32>())
        .collect::<Result<Vec<_>, _>>();
    match parsed {
        Ok(values) => match <Vec<f32> as TryInto<[f32; N]>>::try_into(values) {
            Ok(array) => Some(array),
            Err(_values) => {
                diagnostics.push(ModelDiagnostic {
                    kind:    ModelDiagnosticKind::MalformedPayloadRow,
                    message: format!(
                        "{keyword} row {} could not be converted into a fixed-width array",
                        row_index + 1
                    ),
                });
                None
            }
        },
        Err(_parse_error) => {
            diagnostics.push(ModelDiagnostic {
                kind:    ModelDiagnosticKind::MalformedPayloadRow,
                message: format!("{keyword} row {} contains a non-float value", row_index + 1),
            });
            None
        }
    }
}

fn parse_i32_row_array<const N: usize>(
    row: &[String],
    keyword: &str,
    row_index: usize,
    diagnostics: &mut Vec<ModelDiagnostic>,
) -> Option<[i32; N]> {
    if row.len() < N {
        diagnostics.push(ModelDiagnostic {
            kind:    ModelDiagnosticKind::MalformedPayloadRow,
            message: format!(
                "{keyword} row {} expects at least {N} values, got {}",
                row_index + 1,
                row.len()
            ),
        });
        return None;
    }

    let parsed = row
        .iter()
        .take(N)
        .map(|value| value.parse::<i32>())
        .collect::<Result<Vec<_>, _>>();
    match parsed {
        Ok(values) => match <Vec<i32> as TryInto<[i32; N]>>::try_into(values) {
            Ok(array) => Some(array),
            Err(_values) => {
                diagnostics.push(ModelDiagnostic {
                    kind:    ModelDiagnosticKind::MalformedPayloadRow,
                    message: format!(
                        "{keyword} row {} could not be converted into a fixed-width array",
                        row_index + 1
                    ),
                });
                None
            }
        },
        Err(_parse_error) => {
            diagnostics.push(ModelDiagnostic {
                kind:    ModelDiagnosticKind::MalformedPayloadRow,
                message: format!(
                    "{keyword} row {} contains a non-integer value",
                    row_index + 1
                ),
            });
            None
        }
    }
}

#[allow(clippy::panic)]
#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, fs, path::PathBuf};

    use crate::{
        Model, ModelDiagnosticKind, NodeKind, SemanticPropertyValue, parse_semantic_model,
        read_semantic_model_from_file,
    };

    fn fixture_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/testing/test.mdl")
    }

    #[test]
    fn fixture_lowers_mesh_material_and_geometry() {
        let model = read_semantic_model_from_file(fixture_path()).unwrap_or_else(|error| {
            panic!("read mdl fixture: {error}");
        });
        assert_eq!(model.header.model_name, "a_ba_casts");
        let torso = model.node("torso_g").unwrap_or_else(|| {
            panic!("missing torso_g node");
        });
        assert_eq!(torso.kind, NodeKind::Trimesh);
        assert_eq!(torso.material.bitmap.as_deref(), Some("pmh0_chest001"));
        let torso_mesh = torso.mesh.as_ref().unwrap_or_else(|| {
            panic!("torso_g should have mesh data");
        });
        assert_eq!(torso_mesh.vertices.len(), 37);
        assert_eq!(torso_mesh.faces.len(), 70);
        assert_eq!(
            torso_mesh
                .uv_layers
                .first()
                .map(|layer| layer.coordinates.len()),
            Some(51)
        );
    }

    #[test]
    fn animated_fixture_lowers_headers_and_keyframes() {
        let model = read_semantic_model_from_file(fixture_path()).unwrap_or_else(|error| {
            panic!("read animated mdl fixture: {error}");
        });

        assert_eq!(model.animations.len(), 19);
        let conjure = model.animation("conjure1").unwrap_or_else(|| {
            panic!("missing conjure1 animation");
        });
        assert_eq!(conjure.length, Some(1.0));
        assert_eq!(conjure.transtime, Some(0.5));
        assert_eq!(conjure.animroot.as_deref(), Some("rootdummy"));

        let rootdummy = conjure.node("rootdummy").unwrap_or_else(|| {
            panic!("missing conjure1/rootdummy");
        });
        assert_eq!(rootdummy.position_keys.len(), 5);
        assert_eq!(rootdummy.orientation_keys.len(), 2);

        let castout = model.animation("castout").unwrap_or_else(|| {
            panic!("missing castout animation");
        });
        assert_eq!(
            castout.events.first().map(|event| event.name.as_str()),
            Some("cast")
        );
    }

    #[test]
    fn skin_fixture_lowers_named_weights() {
        let model = parse_semantic_model(
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
            panic!("parse skin sample: {error}");
        });

        let arm = model.node("Arm_L").unwrap_or_else(|| {
            panic!("missing Arm_L skin node");
        });
        assert_eq!(arm.kind, NodeKind::Skin);
        let mesh = arm.mesh.as_ref().unwrap_or_else(|| {
            panic!("Arm_L should have mesh data");
        });
        assert_eq!(mesh.weights.len(), 2);
        assert_eq!(
            mesh.weights
                .first()
                .and_then(|row| row.first())
                .map(|weight| weight.bone.as_str()),
            Some("torso_g")
        );
        assert_eq!(
            mesh.weights
                .first()
                .and_then(|row| row.first())
                .map(|weight| weight.weight),
            Some(1.0)
        );
        assert_eq!(mesh.weights.get(1).map(Vec::len), Some(2));
        assert_eq!(
            mesh.weights
                .get(1)
                .and_then(|row| row.first())
                .map(|weight| weight.bone.as_str()),
            Some("lforearm_g")
        );
    }

    #[test]
    fn emitter_and_reference_fixture_lower_special_payloads() {
        let model = parse_semantic_model(
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
            panic!("parse emitter sample: {error}");
        });

        let emitter = model.node("spark").unwrap_or_else(|| {
            panic!("missing emitter node");
        });
        assert_eq!(emitter.kind, NodeKind::Emitter);
        let emitter_payload = emitter.emitter.as_ref().unwrap_or_else(|| {
            panic!("emitter payload missing");
        });
        assert_eq!(emitter_payload.x_size, Some(0.0));
        assert_eq!(emitter_payload.y_size, Some(0.0));
        assert!(
            emitter_payload
                .properties
                .iter()
                .any(|property| {
                    property.name.eq_ignore_ascii_case("texture")
                        && property.values.iter().any(|value| {
                            matches!(value, SemanticPropertyValue::Text(name) if name == "fxpa_flare")
                        })
                })
        );

        let reference = model.node("omen").unwrap_or_else(|| {
            panic!("missing reference node");
        });
        let reference_payload = reference.reference.as_ref().unwrap_or_else(|| {
            panic!("reference payload missing");
        });
        assert_eq!(reference_payload.model.as_deref(), Some("fx_ref"));
        assert_eq!(reference_payload.reattachable, Some(0));
    }

    #[test]
    fn light_fixture_lowers_light_payloads() {
        let model = parse_semantic_model(
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
  color 1 1 1
endnode
endmodelgeom lantern
donemodel lantern
",
        )
        .unwrap_or_else(|error| {
            panic!("parse light sample: {error}");
        });

        let light = model.node("AuroraLight01").unwrap_or_else(|| {
            panic!("missing light node");
        });
        assert_eq!(light.kind, NodeKind::Light);
        let payload = light.light.as_ref().unwrap_or_else(|| {
            panic!("light payload missing");
        });
        assert_eq!(payload.ambient_only, Some(0));
        assert_eq!(payload.is_dynamic, Some(0));
        assert_eq!(payload.affect_dynamic, Some(1));
        assert_eq!(payload.light_priority, Some(3));
        assert_eq!(payload.fading_light, Some(1));
        assert_eq!(payload.flare_radius, Some(0.0));
    }

    #[test]
    fn semantic_lowering_reports_validation_diagnostics() {
        let sample = "\
newmodel demo
setsupermodel demo null
classification character
setanimationscale 1
beginmodelgeom demo
node dummy demo
  parent NULL
endnode
node dummy demo
  parent missing_parent
endnode
endmodelgeom demo
newanim idle demo
  length 1
  node dummy ghost
    parent missing_parent
    positionkey 1
      0 bad 0 0
  endnode
doneanim idle demo
donemodel demo
";

        let model = parse_semantic_model(sample).unwrap_or_else(|error| {
            panic!("parse semantic sample: {error}");
        });

        let counts = diagnostic_counts(&model);
        assert_eq!(
            counts.get(&ModelDiagnosticKind::DuplicateNodeName).copied(),
            Some(1)
        );
        assert_eq!(
            counts.get(&ModelDiagnosticKind::MissingParent).copied(),
            Some(2)
        );
        assert_eq!(
            counts
                .get(&ModelDiagnosticKind::UnknownAnimationTarget)
                .copied(),
            Some(1)
        );
        assert_eq!(
            counts
                .get(&ModelDiagnosticKind::MalformedPayloadRow)
                .copied(),
            Some(1)
        );
    }

    #[test]
    fn model_parse_semantic_lowers_raw_bytes() {
        let bytes = fs::read(fixture_path()).unwrap_or_else(|error| {
            panic!("read semantic bytes fixture: {error}");
        });
        let model = Model::new(bytes).parse_semantic().unwrap_or_else(|error| {
            panic!("parse semantic from model bytes: {error}");
        });
        assert!(model.node("torso_g").is_some());
    }

    fn diagnostic_counts(model: &crate::SemanticModel) -> BTreeMap<ModelDiagnosticKind, usize> {
        let mut counts = BTreeMap::new();
        for diagnostic in &model.diagnostics {
            let entry = counts.entry(diagnostic.kind).or_insert(0);
            *entry += 1;
        }
        counts
    }
}
