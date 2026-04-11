use std::{collections::BTreeSet, fs::File, io::Read, path::Path};

use nwnrs_resman::prelude::*;
use tracing::instrument;

use crate::{
    MODEL_RES_TYPE, Model, ModelDiagnostic, ModelDiagnosticKind, ModelError, ModelResult, NodeKind,
};

const FILE_HEADER_SIZE: usize = 12;
const MODEL_HEADER_SIZE: usize = 232;
const NODE_HEADER_SIZE: usize = 112;
const LIGHT_HEADER_SIZE: usize = 92;
const EMITTER_HEADER_SIZE: usize = 216;
const REFERENCE_HEADER_SIZE: usize = 68;
const MESH_HEADER_SIZE: usize = 512;
const SKIN_HEADER_SIZE: usize = 100;
const ANIM_HEADER_SIZE: usize = 56;
const DANGLY_HEADER_SIZE: usize = 24;
const AABB_HEADER_SIZE: usize = 4;
const CONTROLLER_SIZE: usize = 12;
const FACE_SIZE: usize = 32;
const EVENT_SIZE: usize = 36;
const AABB_ENTRY_SIZE: usize = 36;

/// On-disk encoding used by an NWN model payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelEncoding {
    /// Source-style ASCII MDL text.
    Ascii,
    /// Binary compiled MDL.
    Compiled,
}

/// A parsed MDL payload, dispatched by encoding.
#[derive(Debug, Clone, PartialEq)]
pub enum ParsedModel {
    /// Parsed ASCII MDL.
    Ascii(crate::AsciiModel),
    /// Parsed compiled MDL.
    Compiled(BinaryModel),
}

/// Top-level compiled MDL header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinaryHeader {
    /// Always zero for a compiled model.
    pub binary_id:       u32,
    /// Offset from the start of model data to the raw section.
    pub raw_data_offset: u32,
    /// Size of the raw section in bytes.
    pub raw_data_size:   u32,
    /// Size of the model-data section in bytes.
    pub model_data_size: u32,
}

/// One compiled-model array descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinaryArrayDefinition {
    /// Model-data offset to the first element.
    pub pointer:           u32,
    /// Number of used entries.
    pub used_entries:      u32,
    /// Number of allocated entries.
    pub allocated_entries: u32,
}

/// Parsed compiled model.
#[derive(Debug, Clone, PartialEq)]
pub struct BinaryModel {
    /// File header.
    pub header:           BinaryHeader,
    /// Model name from the compiled header.
    pub name:             String,
    /// Supermodel name from the compiled header.
    pub supermodel_name:  Option<String>,
    /// Raw geometry/model type byte.
    pub geometry_type:    u8,
    /// Model flags byte from the compiled header.
    pub flags:            u8,
    /// Fog byte from the compiled header.
    pub fog:              u8,
    /// Reported geometry node count.
    pub node_count_hint:  u32,
    /// Geometry-root node offset in model data.
    pub root_node_offset: u32,
    /// Animation pointer table.
    pub animation_table:  BinaryArrayDefinition,
    /// Animation scale value from the compiled header.
    pub animation_scale:  f32,
    /// Bounding box minimum.
    pub bound_min:        [f32; 3],
    /// Bounding box maximum.
    pub bound_max:        [f32; 3],
    /// Model radius.
    pub radius:           f32,
    /// Geometry tree in source order.
    pub nodes:            Vec<BinaryNode>,
    /// Animations in source order.
    pub animations:       Vec<BinaryAnimation>,
    /// Gaps or unsupported regions preserved from the original file.
    pub unknown_blocks:   Vec<UnknownBinaryBlock>,
    /// Non-fatal binary parsing diagnostics.
    pub diagnostics:      Vec<ModelDiagnostic>,
}

impl BinaryModel {
    /// Returns the first geometry node named `name`, case-insensitively.
    pub fn node(&self, name: &str) -> Option<&BinaryNode> {
        self.nodes
            .iter()
            .find(|node| node.name.eq_ignore_ascii_case(name))
    }

    /// Returns the first animation named `name`, case-insensitively.
    pub fn animation(&self, name: &str) -> Option<&BinaryAnimation> {
        self.animations
            .iter()
            .find(|animation| animation.name.eq_ignore_ascii_case(name))
    }
}

/// One parsed compiled-model node.
#[derive(Debug, Clone, PartialEq)]
pub struct BinaryNode {
    /// Offset of this node within model data.
    pub offset:            u32,
    /// Typed node kind derived from the content mask.
    pub kind:              NodeKind,
    /// Raw node content flags.
    pub content:           BinaryNodeContent,
    /// Node name.
    pub name:              String,
    /// Part number / node number field.
    pub part_number:       Option<i32>,
    /// Parent node offset determined from tree traversal.
    pub parent_offset:     Option<u32>,
    /// Stored parent-node pointer from the file, when non-zero.
    pub stored_parent:     Option<u32>,
    /// Child node offsets in source order.
    pub child_offsets:     Vec<u32>,
    /// `inheritcolor`-style field from the header.
    pub color_inherit:     u32,
    /// Controllers attached to the node.
    pub controllers:       Vec<BinaryController>,
    /// Raw controller float buffer.
    pub controller_floats: Vec<f32>,
    /// Light payload.
    pub light:             Option<BinaryLight>,
    /// Emitter payload.
    pub emitter:           Option<BinaryEmitter>,
    /// Reference payload.
    pub reference:         Option<BinaryReference>,
    /// Mesh payload.
    pub mesh:              Option<BinaryMesh>,
    /// Skin payload.
    pub skin:              Option<BinarySkin>,
    /// Animmesh payload.
    pub animmesh:          Option<BinaryAnimMesh>,
    /// Dangly payload.
    pub dangly:            Option<BinaryDangly>,
    /// AABB payload.
    pub aabb:              Option<BinaryAabb>,
}

#[derive(Debug, Clone, PartialEq)]
struct BinaryNodeTree {
    node:     BinaryNode,
    children: Vec<BinaryNodeTree>,
}

/// Raw node content flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BinaryNodeContent {
    /// Original bitfield value.
    pub raw:           u32,
    /// Header bit.
    pub has_header:    bool,
    /// Light bit.
    pub has_light:     bool,
    /// Emitter bit.
    pub has_emitter:   bool,
    /// Camera bit.
    pub has_camera:    bool,
    /// Reference bit.
    pub has_reference: bool,
    /// Mesh bit.
    pub has_mesh:      bool,
    /// Skin bit.
    pub has_skin:      bool,
    /// Animmesh bit.
    pub has_anim:      bool,
    /// Danglymesh bit.
    pub has_dangly:    bool,
    /// AABB bit.
    pub has_aabb:      bool,
}

/// One parsed controller.
#[derive(Debug, Clone, PartialEq)]
pub struct BinaryController {
    /// Raw controller type id.
    pub type_id:          i32,
    /// Number of controller rows.
    pub row_count:        u16,
    /// Start index of time keys in the float buffer.
    pub timekey_start:    u16,
    /// Start index of values in the float buffer.
    pub data_start:       u16,
    /// Raw column-count byte.
    pub raw_column_count: i8,
    /// Whether bezier-key mode is flagged.
    pub bezier_keyed:     bool,
    /// Actual number of value columns per row.
    pub value_columns:    usize,
    /// Time keys.
    pub time_keys:        Vec<f32>,
    /// Controller values by row.
    pub values:           Vec<Vec<f32>>,
}

/// One animation event.
#[derive(Debug, Clone, PartialEq)]
pub struct BinaryEvent {
    /// Event time in seconds.
    pub time: f32,
    /// Event name.
    pub name: String,
}

/// One compiled animation block.
#[derive(Debug, Clone, PartialEq)]
pub struct BinaryAnimation {
    /// Offset of the animation header within model data.
    pub offset:           u32,
    /// Animation name.
    pub name:             String,
    /// Root node name.
    pub root_name:        Option<String>,
    /// Animation length.
    pub length:           f32,
    /// Transition time.
    pub transition_time:  f32,
    /// Geometry node count hint for this animation tree.
    pub node_count_hint:  u32,
    /// Animation root-node offset.
    pub root_node_offset: u32,
    /// Animation events.
    pub events:           Vec<BinaryEvent>,
    /// Animation nodes in source order.
    pub nodes:            Vec<BinaryNode>,
}

/// Light node payload.
#[derive(Debug, Clone, PartialEq)]
pub struct BinaryLight {
    /// Flare radius.
    pub flare_radius:       f32,
    /// Flare texture names.
    pub flare_textures:     Vec<String>,
    /// Flare sizes.
    pub flare_sizes:        Vec<f32>,
    /// Flare positions.
    pub flare_positions:    Vec<f32>,
    /// Flare color shifts.
    pub flare_color_shifts: Vec<[f32; 3]>,
    /// Light priority.
    pub light_priority:     u32,
    /// Ambient-only flag.
    pub ambient_only:       u32,
    /// Dynamic type.
    pub dynamic_type:       u32,
    /// Affect-dynamic flag.
    pub affect_dynamic:     u32,
    /// Shadow flag.
    pub shadow:             u32,
    /// Generate-flare flag.
    pub generate_flare:     u32,
    /// Fading-light flag.
    pub fading:             u32,
}

/// Emitter flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BinaryEmitterFlags {
    /// Original bitfield value.
    pub raw:              u32,
    /// `P2P`
    pub p2p:              bool,
    /// `P2P Sel`
    pub p2p_sel:          bool,
    /// `Affected by Wind`
    pub affected_by_wind: bool,
    /// `Is Tinted`
    pub tinted:           bool,
    /// `Bounce`
    pub bounce:           bool,
    /// `Random`
    pub random:           bool,
    /// `Inherit`
    pub inherit:          bool,
    /// `Inherit Vel`
    pub inherit_vel:      bool,
    /// `Inherit Local`
    pub inherit_local:    bool,
    /// `Splat`
    pub splat:            bool,
    /// `Inherit Part`
    pub inherit_part:     bool,
}

/// Emitter node payload.
#[derive(Debug, Clone, PartialEq)]
pub struct BinaryEmitter {
    /// Dead-space value.
    pub dead_space:        f32,
    /// Blast radius.
    pub blast_radius:      f32,
    /// Blast length.
    pub blast_length:      f32,
    /// X grid.
    pub grid_x:            u32,
    /// Y grid.
    pub grid_y:            u32,
    /// Space type.
    pub space:             u32,
    /// Update function name.
    pub update:            String,
    /// Render function name.
    pub render:            String,
    /// Blend mode.
    pub blend:             String,
    /// Texture name.
    pub texture:           String,
    /// Chunk name.
    pub chunk:             String,
    /// Two-sided texture flag.
    pub texture_is_2sided: u32,
    /// Loop flag.
    pub loop_flag:         u32,
    /// Render order.
    pub render_order:      u16,
    /// Emitter bitflags.
    pub flags:             BinaryEmitterFlags,
}

/// Reference node payload.
#[derive(Debug, Clone, PartialEq)]
pub struct BinaryReference {
    /// Referenced model name.
    pub referenced_model_name: String,
    /// Reattachable flag.
    pub reattachable:          u32,
}

/// One mesh face row.
#[derive(Debug, Clone, PartialEq)]
pub struct BinaryFace {
    /// Plane normal.
    pub normal:         [f32; 3],
    /// Plane distance.
    pub distance:       f32,
    /// Surface id / material slot.
    pub surface_id:     i32,
    /// Adjacent face ids.
    pub adjacent_faces: [u16; 3],
    /// Vertex indices.
    pub vertex_indices: [u16; 3],
}

/// Mesh payload.
#[derive(Debug, Clone, PartialEq)]
pub struct BinaryMesh {
    /// Face rows.
    pub faces:             Vec<BinaryFace>,
    /// Bounding box minimum.
    pub bound_min:         [f32; 3],
    /// Bounding box maximum.
    pub bound_max:         [f32; 3],
    /// Mesh radius.
    pub radius:            f32,
    /// Average position.
    pub average:           [f32; 3],
    /// Diffuse color.
    pub diffuse:           [f32; 3],
    /// Ambient color.
    pub ambient:           [f32; 3],
    /// Specular color.
    pub specular:          [f32; 3],
    /// Shininess.
    pub shininess:         f32,
    /// Shadow flag.
    pub shadow:            u32,
    /// Beaming flag.
    pub beaming:           u32,
    /// Render flag.
    pub render:            u32,
    /// Transparency hint.
    pub transparency_hint: u32,
    /// Texture bitmap.
    pub texture0:          Option<String>,
    /// Texture1 name.
    pub texture1:          Option<String>,
    /// Texture2 name.
    pub texture2:          Option<String>,
    /// Texture3 name.
    pub texture3:          Option<String>,
    /// Tile-fade flag.
    pub tile_fade:         u32,
    /// Vertex count.
    pub vertex_count:      u16,
    /// Texture-layer count.
    pub texture_count:     u16,
    /// Rotate-texture flag.
    pub rotate_texture:    u8,
    /// Vertex positions from the raw section.
    pub vertices:          Vec<[f32; 3]>,
    /// UV layers.
    pub uv_sets:           Vec<BinaryUvSet>,
    /// Vertex normals.
    pub normals:           Vec<[f32; 3]>,
    /// Vertex colors.
    pub colors:            Vec<[u8; 4]>,
}

/// One UV layer from the mesh raw section.
#[derive(Debug, Clone, PartialEq)]
pub struct BinaryUvSet {
    /// UV set index.
    pub index:       usize,
    /// Coordinates.
    pub coordinates: Vec<[f32; 2]>,
}

/// Skin payload.
#[derive(Debug, Clone, PartialEq)]
pub struct BinarySkin {
    /// Bone mapping array.
    pub bone_mapping:        Vec<u16>,
    /// Per-vertex weights.
    pub vertex_weights:      Vec<[f32; 4]>,
    /// Per-vertex bone indices.
    pub vertex_bone_indices: Vec<[u16; 4]>,
    /// Bone-part numbers.
    pub bone_parts:          Vec<u16>,
}

/// Animmesh payload.
#[derive(Debug, Clone, PartialEq)]
pub struct BinaryAnimMesh {
    /// Sample period.
    pub sample_period:       f32,
    /// Number of vertex sets.
    pub vertex_set_count:    u32,
    /// Number of texture-vertex sets.
    pub texcoord_set_count:  u32,
    /// Flattened animation vertices.
    pub animation_vertices:  Vec<[f32; 3]>,
    /// Flattened animation texture vertices.
    pub animation_texcoords: Vec<[f32; 2]>,
}

/// Danglymesh payload.
#[derive(Debug, Clone, PartialEq)]
pub struct BinaryDangly {
    /// Constraint values.
    pub constraints:  Vec<f32>,
    /// Displacement.
    pub displacement: f32,
    /// Tightness.
    pub tightness:    f32,
    /// Period.
    pub period:       f32,
}

/// AABB payload.
#[derive(Debug, Clone, PartialEq)]
pub struct BinaryAabb {
    /// Root entry pointer.
    pub root_offset: Option<u32>,
    /// Parsed root entry.
    pub root:        Option<BinaryAabbEntry>,
}

/// One AABB entry.
#[derive(Debug, Clone, PartialEq)]
pub struct BinaryAabbEntry {
    /// Entry offset within model data.
    pub offset:       u32,
    /// Min bounds.
    pub bound_min:    [f32; 3],
    /// Max bounds.
    pub bound_max:    [f32; 3],
    /// Left child offset.
    pub left_offset:  Option<u32>,
    /// Right child offset.
    pub right_offset: Option<u32>,
    /// Leaf part number.
    pub leaf_part:    i32,
    /// Plane field.
    pub plane:        u32,
    /// Left child entry.
    pub left:         Option<Box<BinaryAabbEntry>>,
    /// Right child entry.
    pub right:        Option<Box<BinaryAabbEntry>>,
}

/// One unparsed or unknown binary block preserved from the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownBinaryBlock {
    /// Absolute file offset.
    pub offset: u32,
    /// Byte length.
    pub length: u32,
    /// Raw bytes.
    pub bytes:  Vec<u8>,
}

impl Model {
    /// Detects the raw payload encoding.
    pub fn encoding(&self) -> ModelEncoding {
        detect_model_encoding(self.bytes())
    }

    /// Parses the raw payload into a format-dispatched model.
    pub fn parse_parsed(&self) -> ModelResult<ParsedModel> {
        parse_model_bytes(self.bytes())
    }

    /// Parses the raw payload as a compiled binary model.
    pub fn parse_binary(&self) -> ModelResult<BinaryModel> {
        parse_binary_model_bytes(self.bytes())
    }
}

/// Detects whether a raw MDL payload is ASCII or compiled.
pub fn detect_model_encoding(bytes: &[u8]) -> ModelEncoding {
    if read_u32_at(bytes, 0) == Some(0) {
        return ModelEncoding::Compiled;
    }

    let _prefix = bytes
        .get(..bytes.len().min(2048))
        .map(String::from_utf8_lossy)
        .unwrap_or_default();
    ModelEncoding::Ascii
}

/// Parses a raw MDL payload using automatic encoding detection.
pub fn parse_model_bytes(bytes: &[u8]) -> ModelResult<ParsedModel> {
    match detect_model_encoding(bytes) {
        ModelEncoding::Ascii => {
            let text = std::str::from_utf8(bytes).map_err(|error| {
                ModelError::msg(format!("ASCII mdl payload is not valid UTF-8: {error}"))
            })?;
            Ok(ParsedModel::Ascii(crate::parse_ascii_model(text)?))
        }
        ModelEncoding::Compiled => Ok(ParsedModel::Compiled(parse_binary_model_bytes(bytes)?)),
    }
}

/// Reads a parsed MDL payload from `reader`.
#[instrument(level = "debug", skip_all, err)]
pub fn read_parsed_model<R: Read>(reader: &mut R) -> ModelResult<ParsedModel> {
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes)?;
    parse_model_bytes(&bytes)
}

/// Reads a parsed MDL payload from disk.
#[instrument(level = "debug", skip_all, err, fields(path = %path.as_ref().display()))]
pub fn read_parsed_model_from_file(path: impl AsRef<Path>) -> ModelResult<ParsedModel> {
    let mut file = File::open(path.as_ref())?;
    read_parsed_model(&mut file)
}

/// Reads a parsed MDL payload from a [`Res`].
#[instrument(level = "debug", skip_all, err, fields(resref = %res.resref(), use_cache))]
pub fn read_parsed_model_from_res(res: &Res, use_cache: bool) -> ModelResult<ParsedModel> {
    if res.resref().res_type() != MODEL_RES_TYPE {
        return Err(ModelError::msg(format!(
            "expected mdl resource, got {}",
            res.resref()
        )));
    }
    parse_model_bytes(&res.read_all(use_cache)?)
}

/// Parses a compiled binary MDL payload from raw bytes.
pub fn parse_binary_model_bytes(bytes: &[u8]) -> ModelResult<BinaryModel> {
    let header = parse_binary_header(bytes)?;
    let mut parser = BinaryParser::new(bytes, header.clone());
    parser.parse_model()
}

/// Reads a compiled binary MDL from `reader`.
#[instrument(level = "debug", skip_all, err)]
pub fn read_binary_model<R: Read>(reader: &mut R) -> ModelResult<BinaryModel> {
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes)?;
    parse_binary_model_bytes(&bytes)
}

/// Reads a compiled binary MDL from disk.
#[instrument(level = "debug", skip_all, err, fields(path = %path.as_ref().display()))]
pub fn read_binary_model_from_file(path: impl AsRef<Path>) -> ModelResult<BinaryModel> {
    let mut file = File::open(path.as_ref())?;
    read_binary_model(&mut file)
}

/// Reads a compiled binary MDL from a [`Res`].
#[instrument(level = "debug", skip_all, err, fields(resref = %res.resref(), use_cache))]
pub fn read_binary_model_from_res(res: &Res, use_cache: bool) -> ModelResult<BinaryModel> {
    if res.resref().res_type() != MODEL_RES_TYPE {
        return Err(ModelError::msg(format!(
            "expected mdl resource, got {}",
            res.resref()
        )));
    }
    parse_binary_model_bytes(&res.read_all(use_cache)?)
}

fn parse_binary_header(bytes: &[u8]) -> ModelResult<BinaryHeader> {
    if bytes.len() < FILE_HEADER_SIZE {
        return Err(ModelError::msg(
            "compiled mdl is shorter than the 12-byte file header",
        ));
    }

    let binary_id = read_u32_at(bytes, 0)
        .ok_or_else(|| ModelError::msg("compiled mdl header is truncated at binary id"))?;
    let raw_data_offset = read_u32_at(bytes, 4)
        .ok_or_else(|| ModelError::msg("compiled mdl header is truncated at raw-data offset"))?;
    let raw_data_size = read_u32_at(bytes, 8)
        .ok_or_else(|| ModelError::msg("compiled mdl header is truncated at raw-data size"))?;
    if binary_id != 0 {
        return Err(ModelError::msg(format!(
            "expected compiled mdl binary id 0, found {binary_id:#010x}"
        )));
    }

    let model_data_size = raw_data_offset;
    let total_size = u64::try_from(FILE_HEADER_SIZE)
        .ok()
        .and_then(|prefix| {
            prefix
                .checked_add(u64::from(model_data_size))
                .and_then(|value| value.checked_add(u64::from(raw_data_size)))
        })
        .ok_or_else(|| ModelError::msg("compiled mdl total size overflow"))?;
    let actual_size = u64::try_from(bytes.len())
        .map_err(|error| ModelError::msg(format!("compiled mdl length out of range: {error}")))?;
    if actual_size < total_size {
        return Err(ModelError::msg(format!(
            "compiled mdl is truncated: expected at least {total_size} bytes, got {actual_size}"
        )));
    }

    Ok(BinaryHeader {
        binary_id,
        raw_data_offset,
        raw_data_size,
        model_data_size,
    })
}

struct BinaryParser<'a> {
    bytes:       &'a [u8],
    header:      BinaryHeader,
    diagnostics: Vec<ModelDiagnostic>,
    known_spans: Vec<(u32, u32)>,
}

impl<'a> BinaryParser<'a> {
    fn new(bytes: &'a [u8], header: BinaryHeader) -> Self {
        Self {
            bytes,
            header,
            diagnostics: Vec::new(),
            known_spans: Vec::new(),
        }
    }

    fn parse_model(&mut self) -> ModelResult<BinaryModel> {
        self.mark_file_range(0, FILE_HEADER_SIZE);
        self.mark_model_range(0, MODEL_HEADER_SIZE);

        let name = self.read_model_string(8, 64)?.unwrap_or_default();
        let root_node_offset = self.read_model_u32(72)?;
        let node_count_hint = self.read_model_u32(76)?;
        let animation_table = self.read_array_definition(120)?;
        let bound_min = self.read_model_vec3(136)?;
        let bound_max = self.read_model_vec3(148)?;
        let radius = self.read_model_f32(160)?;
        let animation_scale = self.read_model_f32(164)?;
        let supermodel_name = self.read_model_string(168, 64)?;
        let geometry_type = self.read_model_u8(108)?;
        let flags = self.read_model_u8(114)?;
        let fog = self.read_model_u8(115)?;

        let mut active = BTreeSet::new();
        let root = self.parse_node(root_node_offset, None, &mut active)?;
        let mut nodes = Vec::new();
        flatten_nodes(&root, &mut nodes);

        let mut animations = Vec::new();
        for animation_offset in self.read_model_pointer_array(&animation_table)? {
            match self.parse_animation(animation_offset) {
                Ok(animation) => animations.push(animation),
                Err(error) => self.push_diagnostic(
                    ModelDiagnosticKind::MalformedValue,
                    format!("failed to parse animation at {animation_offset:#x}: {error}"),
                ),
            }
        }

        let mut unknown_blocks = self.collect_unknown_blocks()?;
        unknown_blocks.sort_by_key(|block| block.offset);

        let mut diagnostics = std::mem::take(&mut self.diagnostics);
        diagnostics.sort_by(|left, right| left.kind.cmp(&right.kind));

        Ok(BinaryModel {
            header: self.header.clone(),
            name,
            supermodel_name,
            geometry_type,
            flags,
            fog,
            node_count_hint,
            root_node_offset,
            animation_table,
            animation_scale,
            bound_min,
            bound_max,
            radius,
            nodes,
            animations,
            unknown_blocks,
            diagnostics,
        })
    }

    fn parse_animation(&mut self, offset: u32) -> ModelResult<BinaryAnimation> {
        self.ensure_model_range(offset, 196, "animation header")?;
        self.mark_model_range(offset, 196);
        let base = offset;
        let name = self.read_model_string(base + 8, 64)?.unwrap_or_default();
        let root_node_offset = self.read_model_u32(base + 72)?;
        let node_count_hint = self.read_model_u32(base + 76)?;
        let length = self.read_model_f32(base + 112)?;
        let transition_time = self.read_model_f32(base + 116)?;
        let root_name = self.read_model_string(base + 120, 64)?;
        let events_def = self.read_array_definition(base + 184)?;
        let events = self.read_events(&events_def)?;

        let mut active = BTreeSet::new();
        let root = self.parse_node(root_node_offset, None, &mut active)?;
        let mut nodes = Vec::new();
        flatten_nodes(&root, &mut nodes);

        Ok(BinaryAnimation {
            offset,
            name,
            root_name,
            length,
            transition_time,
            node_count_hint,
            root_node_offset,
            events,
            nodes,
        })
    }

    fn parse_node(
        &mut self,
        offset: u32,
        parent_offset: Option<u32>,
        active: &mut BTreeSet<u32>,
    ) -> ModelResult<BinaryNodeTree> {
        self.ensure_model_range(offset, NODE_HEADER_SIZE, "node header")?;
        if !active.insert(offset) {
            self.push_diagnostic(
                ModelDiagnosticKind::MalformedValue,
                format!("detected recursive node reference at {offset:#x}"),
            );
            return Err(ModelError::msg(format!(
                "detected recursive node reference at {offset:#x}"
            )));
        }

        self.mark_model_range(offset, NODE_HEADER_SIZE);
        let name = self.read_model_string(offset + 32, 32)?.unwrap_or_default();
        let part_number_raw = self.read_model_u32(offset + 28)?;
        let part_number = (part_number_raw != u32::MAX)
            .then(|| i32::try_from(part_number_raw).ok())
            .flatten();
        let color_inherit = self.read_model_u32(offset + 24)?;
        let stored_parent = nonzero(self.read_model_u32(offset + 68)?);
        let children_def = self.read_array_definition(offset + 72)?;
        let controller_headers_def = self.read_array_definition(offset + 84)?;
        let controller_data_def = self.read_array_definition(offset + 96)?;
        let content = self.read_node_content(offset + 108)?;

        let mut cursor = offset + u32::try_from(NODE_HEADER_SIZE).unwrap_or(112);
        let light = if content.has_light {
            let parsed = self.parse_light(cursor)?;
            cursor = cursor.saturating_add(u32::try_from(LIGHT_HEADER_SIZE).unwrap_or(92));
            Some(parsed)
        } else {
            None
        };
        let emitter = if content.has_emitter {
            let parsed = self.parse_emitter(cursor)?;
            cursor = cursor.saturating_add(u32::try_from(EMITTER_HEADER_SIZE).unwrap_or(216));
            Some(parsed)
        } else {
            None
        };
        let reference = if content.has_reference {
            let parsed = self.parse_reference(cursor)?;
            cursor = cursor.saturating_add(u32::try_from(REFERENCE_HEADER_SIZE).unwrap_or(68));
            Some(parsed)
        } else {
            None
        };
        let mesh = if content.has_mesh {
            let parsed = self.parse_mesh(cursor)?;
            cursor = cursor.saturating_add(u32::try_from(MESH_HEADER_SIZE).unwrap_or(512));
            Some(parsed)
        } else {
            None
        };
        let skin = if content.has_skin {
            let parsed = self.parse_skin(cursor, mesh.as_ref())?;
            cursor = cursor.saturating_add(u32::try_from(SKIN_HEADER_SIZE).unwrap_or(100));
            Some(parsed)
        } else {
            None
        };
        let animmesh = if content.has_anim {
            let parsed = self.parse_animmesh(cursor, mesh.as_ref())?;
            cursor = cursor.saturating_add(u32::try_from(ANIM_HEADER_SIZE).unwrap_or(56));
            Some(parsed)
        } else {
            None
        };
        let dangly = if content.has_dangly {
            let parsed = self.parse_dangly(cursor)?;
            cursor = cursor.saturating_add(u32::try_from(DANGLY_HEADER_SIZE).unwrap_or(24));
            Some(parsed)
        } else {
            None
        };
        let aabb = if content.has_aabb {
            Some(self.parse_aabb(cursor)?)
        } else {
            None
        };

        let controllers = self.parse_controllers(&controller_headers_def, &controller_data_def)?;
        let controller_floats = self.read_model_f32_array(&controller_data_def)?;

        let child_offsets = self.read_model_pointer_array(&children_def)?;
        let mut children = Vec::new();
        for child_offset in &child_offsets {
            match self.parse_node(*child_offset, Some(offset), active) {
                Ok(child) => children.push(child),
                Err(error) => self.push_diagnostic(
                    ModelDiagnosticKind::MalformedValue,
                    format!("failed to parse child node at {child_offset:#x}: {error}"),
                ),
            }
        }

        active.remove(&offset);

        Ok(BinaryNodeTree {
            node: BinaryNode {
                offset,
                kind: node_kind_from_content(content),
                content,
                name,
                part_number,
                parent_offset,
                stored_parent,
                child_offsets,
                color_inherit,
                controllers,
                controller_floats,
                light,
                emitter,
                reference,
                mesh,
                skin,
                animmesh,
                dangly,
                aabb,
            },
            children,
        })
    }

    fn parse_light(&mut self, offset: u32) -> ModelResult<BinaryLight> {
        self.mark_model_range(offset, LIGHT_HEADER_SIZE);
        let flare_radius = self.read_model_f32(offset)?;
        let unknown = self.read_array_definition(offset + 4)?;
        let flare_sizes = self.read_array_definition(offset + 16)?;
        let flare_positions = self.read_array_definition(offset + 28)?;
        let flare_color_shifts = self.read_array_definition(offset + 40)?;
        let flare_textures = self.read_array_definition(offset + 52)?;
        let light_priority = self.read_model_u32(offset + 64)?;
        let ambient_only = self.read_model_u32(offset + 68)?;
        let dynamic_type = self.read_model_u32(offset + 72)?;
        let affect_dynamic = self.read_model_u32(offset + 76)?;
        let shadow = self.read_model_u32(offset + 80)?;
        let generate_flare = self.read_model_u32(offset + 84)?;
        let fading = self.read_model_u32(offset + 88)?;

        let _unknown_values = self.read_model_u32_array(&unknown)?;
        let flare_sizes_values = self.read_model_f32_array(&flare_sizes)?;
        let flare_positions_values = self.read_model_f32_array(&flare_positions)?;
        let flare_color_shift_values = self.read_model_vec3_array(&flare_color_shifts)?;
        let flare_textures_values = self
            .read_model_pointer_array(&flare_textures)?
            .into_iter()
            .filter_map(|pointer| self.read_model_cstring(pointer).transpose())
            .collect::<ModelResult<Vec<_>>>()?;

        Ok(BinaryLight {
            flare_radius,
            flare_textures: flare_textures_values,
            flare_sizes: flare_sizes_values,
            flare_positions: flare_positions_values,
            flare_color_shifts: flare_color_shift_values,
            light_priority,
            ambient_only,
            dynamic_type,
            affect_dynamic,
            shadow,
            generate_flare,
            fading,
        })
    }

    fn parse_emitter(&mut self, offset: u32) -> ModelResult<BinaryEmitter> {
        self.mark_model_range(offset, EMITTER_HEADER_SIZE);
        Ok(BinaryEmitter {
            dead_space:        self.read_model_f32(offset)?,
            blast_radius:      self.read_model_f32(offset + 4)?,
            blast_length:      self.read_model_f32(offset + 8)?,
            grid_x:            self.read_model_u32(offset + 12)?,
            grid_y:            self.read_model_u32(offset + 16)?,
            space:             self.read_model_u32(offset + 20)?,
            update:            self.read_model_string(offset + 24, 32)?.unwrap_or_default(),
            render:            self.read_model_string(offset + 56, 32)?.unwrap_or_default(),
            blend:             self.read_model_string(offset + 88, 32)?.unwrap_or_default(),
            texture:           self
                .read_model_string(offset + 120, 64)?
                .unwrap_or_default(),
            chunk:             self
                .read_model_string(offset + 184, 16)?
                .unwrap_or_default(),
            texture_is_2sided: self.read_model_u32(offset + 200)?,
            loop_flag:         self.read_model_u32(offset + 204)?,
            render_order:      self.read_model_u16(offset + 208)?,
            flags:             read_emitter_flags(self.read_model_u32(offset + 212)?),
        })
    }

    fn parse_reference(&mut self, offset: u32) -> ModelResult<BinaryReference> {
        self.mark_model_range(offset, REFERENCE_HEADER_SIZE);
        Ok(BinaryReference {
            referenced_model_name: self.read_model_string(offset, 64)?.unwrap_or_default(),
            reattachable:          self.read_model_u32(offset + 64)?,
        })
    }

    fn parse_mesh(&mut self, offset: u32) -> ModelResult<BinaryMesh> {
        self.mark_model_range(offset, MESH_HEADER_SIZE);
        let faces_def = self.read_array_definition(offset + 8)?;
        let faces = self.read_faces(&faces_def)?;
        let ambient = self.read_model_vec3(offset + 72)?;
        let diffuse = self.read_model_vec3(offset + 60)?;
        let specular = self.read_model_vec3(offset + 84)?;
        let texture0 = self.read_model_string(offset + 120, 64)?;
        let texture1 = self.read_model_string(offset + 184, 64)?;
        let texture2 = self.read_model_string(offset + 248, 64)?;
        let texture3 = self.read_model_string(offset + 312, 64)?;
        let tile_fade = self.read_model_u32(offset + 376)?;
        let p_mdx_vertex = self.read_model_i32(offset + 444)?;
        let vertex_count = self.read_model_u16(offset + 448)?;
        let texture_count = self.read_model_u16(offset + 450)?;
        let p_mdx_texture0 = self.read_model_i32(offset + 452)?;
        let p_mdx_texture1 = self.read_model_i32(offset + 456)?;
        let p_mdx_texture2 = self.read_model_i32(offset + 460)?;
        let p_mdx_texture3 = self.read_model_i32(offset + 464)?;
        let p_mdx_normals = self.read_model_i32(offset + 468)?;
        let p_mdx_colors = self.read_model_i32(offset + 472)?;
        let rotate_texture = self.read_model_u8(offset + 501)?;

        let vertex_count_usize = usize::from(vertex_count);
        let vertices = self.read_raw_vec3_array(p_mdx_vertex, vertex_count_usize)?;
        let uv_sets = [
            p_mdx_texture0,
            p_mdx_texture1,
            p_mdx_texture2,
            p_mdx_texture3,
        ]
        .into_iter()
        .enumerate()
        .filter_map(|(index, pointer)| (pointer >= 0).then_some((index, pointer)))
        .map(|(index, pointer)| {
            self.read_raw_vec2_array(pointer, vertex_count_usize)
                .map(|coordinates| BinaryUvSet {
                    index,
                    coordinates,
                })
        })
        .collect::<ModelResult<Vec<_>>>()?;
        let normals = self.read_raw_vec3_array(p_mdx_normals, vertex_count_usize)?;
        let colors = self.read_raw_rgba_array(p_mdx_colors, vertex_count_usize)?;

        Ok(BinaryMesh {
            faces,
            bound_min: self.read_model_vec3(offset + 20)?,
            bound_max: self.read_model_vec3(offset + 32)?,
            radius: self.read_model_f32(offset + 44)?,
            average: self.read_model_vec3(offset + 48)?,
            diffuse,
            ambient,
            specular,
            shininess: self.read_model_f32(offset + 96)?,
            shadow: self.read_model_u32(offset + 100)?,
            beaming: self.read_model_u32(offset + 104)?,
            render: self.read_model_u32(offset + 108)?,
            transparency_hint: self.read_model_u32(offset + 112)?,
            texture0,
            texture1,
            texture2,
            texture3,
            tile_fade,
            vertex_count,
            texture_count,
            rotate_texture,
            vertices,
            uv_sets,
            normals,
            colors,
        })
    }

    fn parse_skin(&mut self, offset: u32, mesh: Option<&BinaryMesh>) -> ModelResult<BinarySkin> {
        self.mark_model_range(offset, SKIN_HEADER_SIZE);
        let vertex_count = mesh.map(|mesh| usize::from(mesh.vertex_count)).unwrap_or(0);
        let bone_mapping_ptr = self.read_model_i32(offset + 20)?;
        let bone_mapping_count = self.read_model_i32(offset + 24)?;
        let p_weight_vertex = self.read_model_i32(offset + 12)?;
        let p_bone_ref_index = self.read_model_i32(offset + 16)?;

        let bone_mapping = if bone_mapping_ptr > 0 && bone_mapping_count > 0 {
            let count = usize::try_from(bone_mapping_count).map_err(|error| {
                ModelError::msg(format!("skin bone mapping count out of range: {error}"))
            })?;
            self.read_model_u16_array(
                u32::try_from(bone_mapping_ptr).map_err(|error| {
                    ModelError::msg(format!("skin bone mapping pointer out of range: {error}"))
                })?,
                count,
            )?
        } else {
            Vec::new()
        };

        let vertex_weights = self.read_raw_vec4_array(p_weight_vertex, vertex_count)?;
        let vertex_bone_indices = self.read_raw_u16x4_array(p_bone_ref_index, vertex_count)?;
        let mut bone_parts = Vec::new();
        for index in 0..17usize {
            bone_parts
                .push(self.read_model_u16(offset + 64 + u32::try_from(index * 2).unwrap_or(0))?);
        }

        Ok(BinarySkin {
            bone_mapping,
            vertex_weights,
            vertex_bone_indices,
            bone_parts,
        })
    }

    fn parse_animmesh(
        &mut self,
        offset: u32,
        mesh: Option<&BinaryMesh>,
    ) -> ModelResult<BinaryAnimMesh> {
        self.mark_model_range(offset, ANIM_HEADER_SIZE);
        let vertex_count = mesh.map(|mesh| usize::from(mesh.vertex_count)).unwrap_or(0);
        let p_animation_vertex = self.read_model_u32(offset + 40)?;
        let p_animation_texcoord = self.read_model_u32(offset + 44)?;
        let vertex_set_count = self.read_model_u32(offset + 48)?;
        let texcoord_set_count = self.read_model_u32(offset + 52)?;

        let animation_vertices = if p_animation_vertex > 0 {
            let total = usize::try_from(vertex_set_count)
                .ok()
                .and_then(|sets| sets.checked_mul(vertex_count))
                .ok_or_else(|| ModelError::msg("animmesh vertex-set count overflow"))?;
            self.read_model_vec3_array_exact(p_animation_vertex, total)?
        } else {
            Vec::new()
        };
        let animation_texcoords = if p_animation_texcoord > 0 {
            let total = usize::try_from(texcoord_set_count)
                .ok()
                .and_then(|sets| sets.checked_mul(vertex_count))
                .ok_or_else(|| ModelError::msg("animmesh texcoord-set count overflow"))?;
            self.read_model_vec2_array_exact(p_animation_texcoord, total)?
        } else {
            Vec::new()
        };

        Ok(BinaryAnimMesh {
            sample_period: self.read_model_f32(offset)?,
            vertex_set_count,
            texcoord_set_count,
            animation_vertices,
            animation_texcoords,
        })
    }

    fn parse_dangly(&mut self, offset: u32) -> ModelResult<BinaryDangly> {
        self.mark_model_range(offset, DANGLY_HEADER_SIZE);
        let constraints_def = self.read_array_definition(offset)?;
        Ok(BinaryDangly {
            constraints:  self.read_model_f32_array(&constraints_def)?,
            displacement: self.read_model_f32(offset + 12)?,
            tightness:    self.read_model_f32(offset + 16)?,
            period:       self.read_model_f32(offset + 20)?,
        })
    }

    fn parse_aabb(&mut self, offset: u32) -> ModelResult<BinaryAabb> {
        self.mark_model_range(offset, AABB_HEADER_SIZE);
        let root_offset = nonzero(self.read_model_u32(offset)?);
        let root = root_offset
            .map(|root_offset| self.parse_aabb_entry(root_offset))
            .transpose()?
            .map(Box::new)
            .map(|entry| *entry);
        Ok(BinaryAabb {
            root_offset,
            root,
        })
    }

    fn parse_aabb_entry(&mut self, offset: u32) -> ModelResult<BinaryAabbEntry> {
        self.mark_model_range(offset, AABB_ENTRY_SIZE);
        let left_offset = nonzero(self.read_model_u32(offset + 24)?);
        let right_offset = nonzero(self.read_model_u32(offset + 28)?);
        Ok(BinaryAabbEntry {
            offset,
            bound_min: self.read_model_vec3(offset)?,
            bound_max: self.read_model_vec3(offset + 12)?,
            left_offset,
            right_offset,
            leaf_part: self.read_model_i32(offset + 32)?,
            plane: self.read_model_u32(offset + 32)?,
            left: left_offset
                .map(|left| self.parse_aabb_entry(left))
                .transpose()?
                .map(Box::new),
            right: right_offset
                .map(|right| self.parse_aabb_entry(right))
                .transpose()?
                .map(Box::new),
        })
    }

    fn parse_controllers(
        &mut self,
        headers: &BinaryArrayDefinition,
        data: &BinaryArrayDefinition,
    ) -> ModelResult<Vec<BinaryController>> {
        let data_floats = self.read_model_f32_array(data)?;
        let mut controllers = Vec::new();
        if headers.used_entries == 0 {
            return Ok(controllers);
        }

        let used = usize::try_from(headers.used_entries)
            .map_err(|error| ModelError::msg(format!("controller count out of range: {error}")))?;
        let pointer = usize::try_from(headers.pointer).map_err(|error| {
            ModelError::msg(format!("controller pointer out of range: {error}"))
        })?;
        let byte_len = used
            .checked_mul(CONTROLLER_SIZE)
            .ok_or_else(|| ModelError::msg("controller byte length overflow"))?;
        self.mark_model_range(headers.pointer, byte_len);

        for index in 0..used {
            let item_offset = pointer
                .checked_add(
                    index
                        .checked_mul(CONTROLLER_SIZE)
                        .ok_or_else(|| ModelError::msg("controller item offset overflow"))?,
                )
                .ok_or_else(|| ModelError::msg("controller item offset overflow"))?;
            let absolute = FILE_HEADER_SIZE
                .checked_add(item_offset)
                .ok_or_else(|| ModelError::msg("controller absolute offset overflow"))?;
            let absolute_u32 = u32::try_from(absolute).map_err(|error| {
                ModelError::msg(format!("controller absolute offset out of range: {error}"))
            })?;
            let type_id = self.read_file_i32(absolute_u32)?;
            let row_count = self.read_file_u16(absolute_u32 + 4)?;
            let timekey_start = self.read_file_u16(absolute_u32 + 6)?;
            let data_start = self.read_file_u16(absolute_u32 + 8)?;
            let raw_column_count = self.read_file_i8(absolute_u32 + 10)?;
            let bezier_keyed = raw_column_count >= 0 && (raw_column_count & 0x10) != 0;
            let value_columns = if raw_column_count < 0 {
                0
            } else {
                usize::try_from(raw_column_count & 0x0f).unwrap_or(0)
            };
            let row_count_usize = usize::from(row_count);
            let timekey_start_usize = usize::from(timekey_start);
            let data_start_usize = usize::from(data_start);
            let time_keys = slice_with_diagnostic(
                &data_floats,
                timekey_start_usize,
                row_count_usize,
                &mut self.diagnostics,
                format!(
                    "controller type {type_id} time keys start {timekey_start} with row count \
                     {row_count} exceed the controller float buffer"
                ),
            )
            .to_vec();
            let value_len = row_count_usize
                .checked_mul(value_columns)
                .ok_or_else(|| ModelError::msg("controller row-value length overflow"))?;
            let flat_values = slice_with_diagnostic(
                &data_floats,
                data_start_usize,
                value_len,
                &mut self.diagnostics,
                format!(
                    "controller type {type_id} values start {data_start} with {value_len} floats \
                     exceed the controller float buffer"
                ),
            );
            let values = flat_values
                .chunks(value_columns.max(1))
                .map(|chunk| chunk.to_vec())
                .collect::<Vec<_>>();
            controllers.push(BinaryController {
                type_id,
                row_count,
                timekey_start,
                data_start,
                raw_column_count,
                bezier_keyed,
                value_columns,
                time_keys,
                values,
            });
        }

        Ok(controllers)
    }

    fn read_faces(&mut self, def: &BinaryArrayDefinition) -> ModelResult<Vec<BinaryFace>> {
        if def.used_entries == 0 {
            return Ok(Vec::new());
        }
        let used = usize::try_from(def.used_entries)
            .map_err(|error| ModelError::msg(format!("face count out of range: {error}")))?;
        let pointer = usize::try_from(def.pointer)
            .map_err(|error| ModelError::msg(format!("face pointer out of range: {error}")))?;
        let byte_len = used
            .checked_mul(FACE_SIZE)
            .ok_or_else(|| ModelError::msg("face byte length overflow"))?;
        self.mark_model_range(def.pointer, byte_len);

        let mut faces = Vec::with_capacity(used);
        for index in 0..used {
            let item_offset = pointer
                .checked_add(
                    index
                        .checked_mul(FACE_SIZE)
                        .ok_or_else(|| ModelError::msg("face item offset overflow"))?,
                )
                .ok_or_else(|| ModelError::msg("face item offset overflow"))?;
            let model_offset = u32::try_from(item_offset)
                .map_err(|error| ModelError::msg(format!("face offset out of range: {error}")))?;
            faces.push(BinaryFace {
                normal:         self.read_model_vec3(model_offset)?,
                distance:       self.read_model_f32(model_offset + 12)?,
                surface_id:     self.read_model_i32(model_offset + 16)?,
                adjacent_faces: [
                    self.read_model_u16(model_offset + 20)?,
                    self.read_model_u16(model_offset + 22)?,
                    self.read_model_u16(model_offset + 24)?,
                ],
                vertex_indices: [
                    self.read_model_u16(model_offset + 26)?,
                    self.read_model_u16(model_offset + 28)?,
                    self.read_model_u16(model_offset + 30)?,
                ],
            });
        }
        Ok(faces)
    }

    fn read_events(&mut self, def: &BinaryArrayDefinition) -> ModelResult<Vec<BinaryEvent>> {
        if def.used_entries == 0 {
            return Ok(Vec::new());
        }
        let used = usize::try_from(def.used_entries)
            .map_err(|error| ModelError::msg(format!("event count out of range: {error}")))?;
        let pointer = usize::try_from(def.pointer)
            .map_err(|error| ModelError::msg(format!("event pointer out of range: {error}")))?;
        let byte_len = used
            .checked_mul(EVENT_SIZE)
            .ok_or_else(|| ModelError::msg("event byte length overflow"))?;
        self.mark_model_range(def.pointer, byte_len);

        let mut events = Vec::with_capacity(used);
        for index in 0..used {
            let item_offset = pointer
                .checked_add(
                    index
                        .checked_mul(EVENT_SIZE)
                        .ok_or_else(|| ModelError::msg("event item offset overflow"))?,
                )
                .ok_or_else(|| ModelError::msg("event item offset overflow"))?;
            let model_offset = u32::try_from(item_offset)
                .map_err(|error| ModelError::msg(format!("event offset out of range: {error}")))?;
            events.push(BinaryEvent {
                time: self.read_model_f32(model_offset)?,
                name: self
                    .read_model_string(model_offset + 4, 32)?
                    .unwrap_or_default(),
            });
        }
        Ok(events)
    }

    fn read_model_pointer_array(&mut self, def: &BinaryArrayDefinition) -> ModelResult<Vec<u32>> {
        self.read_model_u32_array(def)
    }

    fn read_model_u32_array(&mut self, def: &BinaryArrayDefinition) -> ModelResult<Vec<u32>> {
        if def.used_entries == 0 {
            return Ok(Vec::new());
        }
        let used = usize::try_from(def.used_entries)
            .map_err(|error| ModelError::msg(format!("array size out of range: {error}")))?;
        let pointer = usize::try_from(def.pointer)
            .map_err(|error| ModelError::msg(format!("array pointer out of range: {error}")))?;
        let byte_len = used
            .checked_mul(4)
            .ok_or_else(|| ModelError::msg("array byte length overflow"))?;
        self.mark_model_range(def.pointer, byte_len);
        let mut values = Vec::with_capacity(used);
        for index in 0..used {
            let item_offset = pointer
                .checked_add(
                    index
                        .checked_mul(4)
                        .ok_or_else(|| ModelError::msg("array item offset overflow"))?,
                )
                .ok_or_else(|| ModelError::msg("array item offset overflow"))?;
            let model_offset = u32::try_from(item_offset)
                .map_err(|error| ModelError::msg(format!("array offset out of range: {error}")))?;
            values.push(self.read_model_u32(model_offset)?);
        }
        Ok(values)
    }

    fn read_model_u16_array(&mut self, pointer: u32, count: usize) -> ModelResult<Vec<u16>> {
        if count == 0 {
            return Ok(Vec::new());
        }
        let byte_len = count
            .checked_mul(2)
            .ok_or_else(|| ModelError::msg("u16 array byte length overflow"))?;
        self.mark_model_range(pointer, byte_len);
        let mut values = Vec::with_capacity(count);
        for index in 0..count {
            let item_offset = usize::try_from(pointer)
                .ok()
                .and_then(|pointer| pointer.checked_add(index.checked_mul(2)?))
                .ok_or_else(|| ModelError::msg("u16 array item offset overflow"))?;
            let model_offset = u32::try_from(item_offset).map_err(|error| {
                ModelError::msg(format!("u16 array offset out of range: {error}"))
            })?;
            values.push(self.read_model_u16(model_offset)?);
        }
        Ok(values)
    }

    fn read_model_f32_array(&mut self, def: &BinaryArrayDefinition) -> ModelResult<Vec<f32>> {
        if def.used_entries == 0 {
            return Ok(Vec::new());
        }
        let used = usize::try_from(def.used_entries)
            .map_err(|error| ModelError::msg(format!("float array size out of range: {error}")))?;
        let pointer = usize::try_from(def.pointer).map_err(|error| {
            ModelError::msg(format!("float array pointer out of range: {error}"))
        })?;
        let byte_len = used
            .checked_mul(4)
            .ok_or_else(|| ModelError::msg("float array byte length overflow"))?;
        self.mark_model_range(def.pointer, byte_len);
        let mut values = Vec::with_capacity(used);
        for index in 0..used {
            let item_offset = pointer
                .checked_add(
                    index
                        .checked_mul(4)
                        .ok_or_else(|| ModelError::msg("float array item offset overflow"))?,
                )
                .ok_or_else(|| ModelError::msg("float array item offset overflow"))?;
            let model_offset = u32::try_from(item_offset).map_err(|error| {
                ModelError::msg(format!("float array offset out of range: {error}"))
            })?;
            values.push(self.read_model_f32(model_offset)?);
        }
        Ok(values)
    }

    fn read_model_vec3_array(&mut self, def: &BinaryArrayDefinition) -> ModelResult<Vec<[f32; 3]>> {
        if def.used_entries == 0 {
            return Ok(Vec::new());
        }
        let used = usize::try_from(def.used_entries)
            .map_err(|error| ModelError::msg(format!("vec3 array size out of range: {error}")))?;
        self.read_model_vec3_array_exact(def.pointer, used)
    }

    fn read_model_vec3_array_exact(
        &mut self,
        pointer: u32,
        count: usize,
    ) -> ModelResult<Vec<[f32; 3]>> {
        if count == 0 {
            return Ok(Vec::new());
        }
        let byte_len = count
            .checked_mul(12)
            .ok_or_else(|| ModelError::msg("vec3 array byte length overflow"))?;
        self.mark_model_range(pointer, byte_len);
        let mut values = Vec::with_capacity(count);
        for index in 0..count {
            let item_offset = usize::try_from(pointer)
                .ok()
                .and_then(|pointer| pointer.checked_add(index.checked_mul(12)?))
                .ok_or_else(|| ModelError::msg("vec3 array item offset overflow"))?;
            let model_offset = u32::try_from(item_offset).map_err(|error| {
                ModelError::msg(format!("vec3 array offset out of range: {error}"))
            })?;
            values.push(self.read_model_vec3(model_offset)?);
        }
        Ok(values)
    }

    fn read_model_vec2_array_exact(
        &mut self,
        pointer: u32,
        count: usize,
    ) -> ModelResult<Vec<[f32; 2]>> {
        if count == 0 {
            return Ok(Vec::new());
        }
        let byte_len = count
            .checked_mul(8)
            .ok_or_else(|| ModelError::msg("vec2 array byte length overflow"))?;
        self.mark_model_range(pointer, byte_len);
        let mut values = Vec::with_capacity(count);
        for index in 0..count {
            let item_offset = usize::try_from(pointer)
                .ok()
                .and_then(|pointer| pointer.checked_add(index.checked_mul(8)?))
                .ok_or_else(|| ModelError::msg("vec2 array item offset overflow"))?;
            let model_offset = u32::try_from(item_offset).map_err(|error| {
                ModelError::msg(format!("vec2 array offset out of range: {error}"))
            })?;
            values.push(self.read_model_vec2(model_offset)?);
        }
        Ok(values)
    }

    fn read_raw_vec3_array(&mut self, pointer: i32, count: usize) -> ModelResult<Vec<[f32; 3]>> {
        if pointer < 0 || count == 0 {
            return Ok(Vec::new());
        }
        let raw_offset = u32::try_from(pointer)
            .map_err(|error| ModelError::msg(format!("raw vec3 pointer out of range: {error}")))?;
        let byte_len = count
            .checked_mul(12)
            .ok_or_else(|| ModelError::msg("raw vec3 byte length overflow"))?;
        self.mark_raw_range(raw_offset, byte_len);
        let mut values = Vec::with_capacity(count);
        for index in 0..count {
            let item_offset = usize::try_from(raw_offset)
                .ok()
                .and_then(|pointer| pointer.checked_add(index.checked_mul(12)?))
                .ok_or_else(|| ModelError::msg("raw vec3 item offset overflow"))?;
            let raw_item = u32::try_from(item_offset).map_err(|error| {
                ModelError::msg(format!("raw vec3 item offset out of range: {error}"))
            })?;
            values.push(self.read_raw_vec3(raw_item)?);
        }
        Ok(values)
    }

    fn read_raw_vec4_array(&mut self, pointer: i32, count: usize) -> ModelResult<Vec<[f32; 4]>> {
        if pointer < 0 || count == 0 {
            return Ok(Vec::new());
        }
        let raw_offset = u32::try_from(pointer)
            .map_err(|error| ModelError::msg(format!("raw vec4 pointer out of range: {error}")))?;
        let byte_len = count
            .checked_mul(16)
            .ok_or_else(|| ModelError::msg("raw vec4 byte length overflow"))?;
        self.mark_raw_range(raw_offset, byte_len);
        let mut values = Vec::with_capacity(count);
        for index in 0..count {
            let item_offset = usize::try_from(raw_offset)
                .ok()
                .and_then(|pointer| pointer.checked_add(index.checked_mul(16)?))
                .ok_or_else(|| ModelError::msg("raw vec4 item offset overflow"))?;
            let raw_item = u32::try_from(item_offset).map_err(|error| {
                ModelError::msg(format!("raw vec4 item offset out of range: {error}"))
            })?;
            values.push([
                self.read_raw_f32(raw_item)?,
                self.read_raw_f32(raw_item + 4)?,
                self.read_raw_f32(raw_item + 8)?,
                self.read_raw_f32(raw_item + 12)?,
            ]);
        }
        Ok(values)
    }

    fn read_raw_u16x4_array(&mut self, pointer: i32, count: usize) -> ModelResult<Vec<[u16; 4]>> {
        if pointer < 0 || count == 0 {
            return Ok(Vec::new());
        }
        let raw_offset = u32::try_from(pointer)
            .map_err(|error| ModelError::msg(format!("raw u16x4 pointer out of range: {error}")))?;
        let byte_len = count
            .checked_mul(8)
            .ok_or_else(|| ModelError::msg("raw u16x4 byte length overflow"))?;
        self.mark_raw_range(raw_offset, byte_len);
        let mut values = Vec::with_capacity(count);
        for index in 0..count {
            let item_offset = usize::try_from(raw_offset)
                .ok()
                .and_then(|pointer| pointer.checked_add(index.checked_mul(8)?))
                .ok_or_else(|| ModelError::msg("raw u16x4 item offset overflow"))?;
            let raw_item = u32::try_from(item_offset).map_err(|error| {
                ModelError::msg(format!("raw u16x4 item offset out of range: {error}"))
            })?;
            values.push([
                self.read_raw_u16(raw_item)?,
                self.read_raw_u16(raw_item + 2)?,
                self.read_raw_u16(raw_item + 4)?,
                self.read_raw_u16(raw_item + 6)?,
            ]);
        }
        Ok(values)
    }

    fn read_raw_vec2_array(&mut self, pointer: i32, count: usize) -> ModelResult<Vec<[f32; 2]>> {
        if pointer < 0 || count == 0 {
            return Ok(Vec::new());
        }
        let raw_offset = u32::try_from(pointer)
            .map_err(|error| ModelError::msg(format!("raw vec2 pointer out of range: {error}")))?;
        let byte_len = count
            .checked_mul(8)
            .ok_or_else(|| ModelError::msg("raw vec2 byte length overflow"))?;
        self.mark_raw_range(raw_offset, byte_len);
        let mut values = Vec::with_capacity(count);
        for index in 0..count {
            let item_offset = usize::try_from(raw_offset)
                .ok()
                .and_then(|pointer| pointer.checked_add(index.checked_mul(8)?))
                .ok_or_else(|| ModelError::msg("raw vec2 item offset overflow"))?;
            let raw_item = u32::try_from(item_offset).map_err(|error| {
                ModelError::msg(format!("raw vec2 item offset out of range: {error}"))
            })?;
            values.push(self.read_raw_vec2(raw_item)?);
        }
        Ok(values)
    }

    fn read_raw_rgba_array(&mut self, pointer: i32, count: usize) -> ModelResult<Vec<[u8; 4]>> {
        if pointer < 0 || count == 0 {
            return Ok(Vec::new());
        }
        let raw_offset = u32::try_from(pointer)
            .map_err(|error| ModelError::msg(format!("raw rgba pointer out of range: {error}")))?;
        self.mark_raw_range(
            raw_offset,
            count
                .checked_mul(4)
                .ok_or_else(|| ModelError::msg("raw rgba byte length overflow"))?,
        );
        let mut values = Vec::with_capacity(count);
        for index in 0..count {
            let item_offset = usize::try_from(raw_offset)
                .ok()
                .and_then(|pointer| pointer.checked_add(index.checked_mul(4)?))
                .ok_or_else(|| ModelError::msg("raw rgba item offset overflow"))?;
            let raw_item = u32::try_from(item_offset).map_err(|error| {
                ModelError::msg(format!("raw rgba item offset out of range: {error}"))
            })?;
            values.push(self.read_raw_rgba(raw_item)?);
        }
        Ok(values)
    }

    fn read_array_definition(&mut self, offset: u32) -> ModelResult<BinaryArrayDefinition> {
        Ok(BinaryArrayDefinition {
            pointer:           self.read_model_u32(offset)?,
            used_entries:      self.read_model_u32(offset + 4)?,
            allocated_entries: self.read_model_u32(offset + 8)?,
        })
    }

    fn read_node_content(&mut self, offset: u32) -> ModelResult<BinaryNodeContent> {
        let raw = self.read_model_u32(offset)?;
        Ok(BinaryNodeContent {
            raw,
            has_header: raw & 0x0000_0001 != 0,
            has_light: raw & 0x0000_0002 != 0,
            has_emitter: raw & 0x0000_0004 != 0,
            has_camera: raw & 0x0000_0008 != 0,
            has_reference: raw & 0x0000_0010 != 0,
            has_mesh: raw & 0x0000_0020 != 0,
            has_skin: raw & 0x0000_0040 != 0,
            has_anim: raw & 0x0000_0080 != 0,
            has_dangly: raw & 0x0000_0100 != 0,
            has_aabb: raw & 0x0000_0200 != 0,
        })
    }

    fn read_model_string(&mut self, offset: u32, len: usize) -> ModelResult<Option<String>> {
        let bytes = self.read_model_bytes(offset, len)?;
        Ok(trimmed_cstring(bytes))
    }

    fn read_model_cstring(&mut self, offset: u32) -> ModelResult<Option<String>> {
        let bytes = self.model_bytes_from(offset)?;
        let end = bytes
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(bytes.len());
        Ok(trimmed_cstring(bytes.get(..end).unwrap_or(bytes)))
    }

    fn read_model_bytes(&mut self, offset: u32, len: usize) -> ModelResult<&'a [u8]> {
        let start = usize::try_from(offset)
            .map_err(|error| ModelError::msg(format!("model offset out of range: {error}")))?;
        let file_start = FILE_HEADER_SIZE
            .checked_add(start)
            .ok_or_else(|| ModelError::msg("model byte slice start overflow"))?;
        let file_end = file_start
            .checked_add(len)
            .ok_or_else(|| ModelError::msg("model byte slice end overflow"))?;
        self.bytes.get(file_start..file_end).ok_or_else(|| {
            ModelError::msg(format!(
                "model byte slice {offset:#x}+{len} is out of range"
            ))
        })
    }

    fn model_bytes_from(&mut self, offset: u32) -> ModelResult<&'a [u8]> {
        let start = usize::try_from(offset)
            .map_err(|error| ModelError::msg(format!("model offset out of range: {error}")))?;
        let file_start = FILE_HEADER_SIZE
            .checked_add(start)
            .ok_or_else(|| ModelError::msg("model byte slice start overflow"))?;
        let file_end = FILE_HEADER_SIZE
            .checked_add(
                usize::try_from(self.header.model_data_size).map_err(|error| {
                    ModelError::msg(format!("model data size out of range: {error}"))
                })?,
            )
            .ok_or_else(|| ModelError::msg("model byte slice end overflow"))?;
        self.bytes.get(file_start..file_end).ok_or_else(|| {
            ModelError::msg(format!("model byte slice from {offset:#x} is out of range"))
        })
    }

    fn read_model_u8(&mut self, offset: u32) -> ModelResult<u8> {
        self.read_model_bytes(offset, 1)?
            .first()
            .copied()
            .ok_or_else(|| ModelError::msg(format!("failed to read model u8 at {offset:#x}")))
    }

    fn read_model_u16(&mut self, offset: u32) -> ModelResult<u16> {
        read_u16_slice(self.read_model_bytes(offset, 2)?)
            .ok_or_else(|| ModelError::msg(format!("failed to read model u16 at {offset:#x}")))
    }

    fn read_model_i32(&mut self, offset: u32) -> ModelResult<i32> {
        read_i32_slice(self.read_model_bytes(offset, 4)?)
            .ok_or_else(|| ModelError::msg(format!("failed to read model i32 at {offset:#x}")))
    }

    fn read_model_u32(&mut self, offset: u32) -> ModelResult<u32> {
        read_u32_slice(self.read_model_bytes(offset, 4)?)
            .ok_or_else(|| ModelError::msg(format!("failed to read model u32 at {offset:#x}")))
    }

    fn read_model_f32(&mut self, offset: u32) -> ModelResult<f32> {
        read_f32_slice(self.read_model_bytes(offset, 4)?)
            .ok_or_else(|| ModelError::msg(format!("failed to read model f32 at {offset:#x}")))
    }

    fn read_model_vec3(&mut self, offset: u32) -> ModelResult<[f32; 3]> {
        Ok([
            self.read_model_f32(offset)?,
            self.read_model_f32(offset + 4)?,
            self.read_model_f32(offset + 8)?,
        ])
    }

    fn read_model_vec2(&mut self, offset: u32) -> ModelResult<[f32; 2]> {
        Ok([
            self.read_model_f32(offset)?,
            self.read_model_f32(offset + 4)?,
        ])
    }

    fn read_raw_f32(&mut self, raw_offset: u32) -> ModelResult<f32> {
        read_f32_slice(self.read_raw_bytes(raw_offset, 4)?)
            .ok_or_else(|| ModelError::msg(format!("failed to read raw f32 at {raw_offset:#x}")))
    }

    fn read_raw_u16(&mut self, raw_offset: u32) -> ModelResult<u16> {
        read_u16_slice(self.read_raw_bytes(raw_offset, 2)?)
            .ok_or_else(|| ModelError::msg(format!("failed to read raw u16 at {raw_offset:#x}")))
    }

    fn read_raw_vec3(&mut self, raw_offset: u32) -> ModelResult<[f32; 3]> {
        Ok([
            self.read_raw_f32(raw_offset)?,
            self.read_raw_f32(raw_offset + 4)?,
            self.read_raw_f32(raw_offset + 8)?,
        ])
    }

    fn read_raw_vec2(&mut self, raw_offset: u32) -> ModelResult<[f32; 2]> {
        Ok([
            self.read_raw_f32(raw_offset)?,
            self.read_raw_f32(raw_offset + 4)?,
        ])
    }

    fn read_raw_rgba(&mut self, raw_offset: u32) -> ModelResult<[u8; 4]> {
        let bytes = self.read_raw_bytes(raw_offset, 4)?;
        let mut rgba = [0u8; 4];
        rgba.copy_from_slice(bytes);
        Ok(rgba)
    }

    fn read_raw_bytes(&mut self, raw_offset: u32, len: usize) -> ModelResult<&'a [u8]> {
        let start = usize::try_from(raw_offset)
            .map_err(|error| ModelError::msg(format!("raw offset out of range: {error}")))?;
        let raw_base = FILE_HEADER_SIZE
            .checked_add(
                usize::try_from(self.header.raw_data_offset).map_err(|error| {
                    ModelError::msg(format!("raw-data base offset out of range: {error}"))
                })?,
            )
            .ok_or_else(|| ModelError::msg("raw byte slice start overflow"))?;
        let file_start = raw_base
            .checked_add(start)
            .ok_or_else(|| ModelError::msg("raw byte slice start overflow"))?;
        let file_end = file_start
            .checked_add(len)
            .ok_or_else(|| ModelError::msg("raw byte slice end overflow"))?;
        self.bytes.get(file_start..file_end).ok_or_else(|| {
            ModelError::msg(format!(
                "raw byte slice {raw_offset:#x}+{len} is out of range"
            ))
        })
    }

    fn read_file_i32(&self, file_offset: u32) -> ModelResult<i32> {
        let bytes =
            read_bytes(self.bytes, usize::try_from(file_offset).ok(), 4).ok_or_else(|| {
                ModelError::msg(format!("failed to read file i32 at {file_offset:#x}"))
            })?;
        read_i32_slice(bytes).ok_or_else(|| {
            ModelError::msg(format!("failed to decode file i32 at {file_offset:#x}"))
        })
    }

    fn read_file_u16(&self, file_offset: u32) -> ModelResult<u16> {
        let bytes =
            read_bytes(self.bytes, usize::try_from(file_offset).ok(), 2).ok_or_else(|| {
                ModelError::msg(format!("failed to read file u16 at {file_offset:#x}"))
            })?;
        read_u16_slice(bytes).ok_or_else(|| {
            ModelError::msg(format!("failed to decode file u16 at {file_offset:#x}"))
        })
    }

    fn read_file_i8(&self, file_offset: u32) -> ModelResult<i8> {
        let bytes =
            read_bytes(self.bytes, usize::try_from(file_offset).ok(), 1).ok_or_else(|| {
                ModelError::msg(format!("failed to read file i8 at {file_offset:#x}"))
            })?;
        bytes
            .first()
            .map(|value| i8::from_le_bytes([*value]))
            .ok_or_else(|| ModelError::msg(format!("failed to decode file i8 at {file_offset:#x}")))
    }

    fn mark_file_range(&mut self, start: u32, len: usize) {
        if let Ok(length) = u32::try_from(len) {
            self.known_spans.push((start, start.saturating_add(length)));
        }
    }

    fn mark_model_range(&mut self, offset: u32, len: usize) {
        if let Ok(length) = u32::try_from(len) {
            let start = u32::try_from(FILE_HEADER_SIZE)
                .unwrap_or(12)
                .saturating_add(offset);
            self.known_spans.push((start, start.saturating_add(length)));
        }
    }

    fn mark_raw_range(&mut self, offset: u32, len: usize) {
        if let Ok(length) = u32::try_from(len) {
            let start = u32::try_from(FILE_HEADER_SIZE)
                .unwrap_or(12)
                .saturating_add(self.header.raw_data_offset)
                .saturating_add(offset);
            self.known_spans.push((start, start.saturating_add(length)));
        }
    }

    fn collect_unknown_blocks(&self) -> ModelResult<Vec<UnknownBinaryBlock>> {
        let mut spans = self.known_spans.clone();
        spans.sort_unstable_by_key(|(start, _end)| *start);
        let file_len = u32::try_from(self.bytes.len())
            .map_err(|error| ModelError::msg(format!("file length out of range: {error}")))?;

        let mut merged = Vec::new();
        for (start, end) in spans {
            if let Some((_last_start, last_end)) = merged.last_mut()
                && start <= *last_end
            {
                *last_end = (*last_end).max(end);
                continue;
            }
            merged.push((start, end));
        }

        let mut cursor = 0u32;
        let mut unknown_blocks = Vec::new();
        for (start, end) in merged {
            if cursor < start {
                unknown_blocks.push(self.make_unknown_block(cursor, start)?);
            }
            cursor = cursor.max(end);
        }
        if cursor < file_len {
            unknown_blocks.push(self.make_unknown_block(cursor, file_len)?);
        }
        Ok(unknown_blocks)
    }

    fn make_unknown_block(&self, start: u32, end: u32) -> ModelResult<UnknownBinaryBlock> {
        let start_usize = usize::try_from(start).map_err(|error| {
            ModelError::msg(format!("unknown block start out of range: {error}"))
        })?;
        let end_usize = usize::try_from(end)
            .map_err(|error| ModelError::msg(format!("unknown block end out of range: {error}")))?;
        let bytes = self
            .bytes
            .get(start_usize..end_usize)
            .ok_or_else(|| {
                ModelError::msg(format!(
                    "unknown block slice {start:#x}..{end:#x} is out of range"
                ))
            })?
            .to_vec();
        Ok(UnknownBinaryBlock {
            offset: start,
            length: end.saturating_sub(start),
            bytes,
        })
    }

    fn push_diagnostic(&mut self, kind: ModelDiagnosticKind, message: impl Into<String>) {
        self.diagnostics.push(ModelDiagnostic {
            kind,
            message: message.into(),
        });
    }

    fn ensure_model_range(&self, offset: u32, len: usize, label: &str) -> ModelResult<()> {
        let len_u32 = u32::try_from(len)
            .map_err(|error| ModelError::msg(format!("{label} length out of range: {error}")))?;
        let end = offset
            .checked_add(len_u32)
            .ok_or_else(|| ModelError::msg(format!("{label} offset overflow at {offset:#x}")))?;
        if end > self.header.model_data_size {
            return Err(ModelError::msg(format!(
                "{label} at {offset:#x} extends past model data size {:#x}",
                self.header.model_data_size
            )));
        }
        Ok(())
    }
}

fn flatten_nodes(node: &BinaryNodeTree, into: &mut Vec<BinaryNode>) {
    into.push(node.node.clone());
    for child in &node.children {
        flatten_nodes(child, into);
    }
}

fn node_kind_from_content(content: BinaryNodeContent) -> NodeKind {
    if content.has_emitter {
        NodeKind::Emitter
    } else if content.has_light {
        NodeKind::Light
    } else if content.has_reference {
        NodeKind::Reference
    } else if content.has_mesh && content.has_skin {
        NodeKind::Skin
    } else if content.has_mesh && content.has_anim {
        NodeKind::Animmesh
    } else if content.has_mesh && content.has_dangly {
        NodeKind::Danglymesh
    } else if content.has_mesh && content.has_aabb {
        NodeKind::Aabb
    } else if content.has_mesh {
        NodeKind::Trimesh
    } else {
        NodeKind::Dummy
    }
}

fn read_emitter_flags(raw: u32) -> BinaryEmitterFlags {
    BinaryEmitterFlags {
        raw,
        p2p: raw & 0x0001 != 0,
        p2p_sel: raw & 0x0002 != 0,
        affected_by_wind: raw & 0x0004 != 0,
        tinted: raw & 0x0008 != 0,
        bounce: raw & 0x0010 != 0,
        random: raw & 0x0020 != 0,
        inherit: raw & 0x0040 != 0,
        inherit_vel: raw & 0x0080 != 0,
        inherit_local: raw & 0x0100 != 0,
        splat: raw & 0x0200 != 0,
        inherit_part: raw & 0x0400 != 0,
    }
}

fn slice_with_diagnostic<'a>(
    values: &'a [f32],
    start: usize,
    len: usize,
    diagnostics: &mut Vec<ModelDiagnostic>,
    message: String,
) -> &'a [f32] {
    match start
        .checked_add(len)
        .and_then(|end| values.get(start..end))
    {
        Some(slice) => slice,
        None => {
            diagnostics.push(ModelDiagnostic {
                kind: ModelDiagnosticKind::MalformedValue,
                message,
            });
            &[]
        }
    }
}

fn trimmed_cstring(bytes: &[u8]) -> Option<String> {
    let meaningful = bytes
        .iter()
        .position(|byte| *byte == 0)
        .and_then(|index| bytes.get(..index))
        .unwrap_or(bytes);
    let trimmed = meaningful
        .iter()
        .copied()
        .take_while(|byte| *byte != 0)
        .collect::<Vec<_>>();
    if trimmed.is_empty() {
        None
    } else {
        Some(String::from_utf8_lossy(&trimmed).trim().to_string())
    }
}

fn nonzero(value: u32) -> Option<u32> {
    (value != 0).then_some(value)
}

fn read_bytes(bytes: &[u8], start: Option<usize>, len: usize) -> Option<&[u8]> {
    let start = start?;
    let end = start.checked_add(len)?;
    bytes.get(start..end)
}

fn read_u16_slice(bytes: &[u8]) -> Option<u16> {
    let array: [u8; 2] = bytes.try_into().ok()?;
    Some(u16::from_le_bytes(array))
}

fn read_u32_at(bytes: &[u8], start: usize) -> Option<u32> {
    read_u32_slice(read_bytes(bytes, Some(start), 4)?)
}

fn read_u32_slice(bytes: &[u8]) -> Option<u32> {
    let array: [u8; 4] = bytes.try_into().ok()?;
    Some(u32::from_le_bytes(array))
}

fn read_i32_slice(bytes: &[u8]) -> Option<i32> {
    let array: [u8; 4] = bytes.try_into().ok()?;
    Some(i32::from_le_bytes(array))
}

fn read_f32_slice(bytes: &[u8]) -> Option<f32> {
    let array: [u8; 4] = bytes.try_into().ok()?;
    Some(f32::from_le_bytes(array))
}

#[allow(clippy::panic)]
#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::FILE_HEADER_SIZE;
    use crate::{
        ModelEncoding, ParsedModel, detect_model_encoding, parse_binary_model_bytes,
        parse_model_bytes, read_binary_model_from_file,
    };

    fn ascii_fixture() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/testing/test.mdl")
    }

    fn compiled_fixture() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/testing/a_ba2_compiled.mdl")
    }

    #[test]
    fn detects_ascii_fixture_encoding() {
        let bytes = std::fs::read(ascii_fixture()).unwrap_or_else(|error| {
            panic!("read ascii fixture: {error}");
        });
        assert_eq!(detect_model_encoding(&bytes), ModelEncoding::Ascii);
    }

    #[test]
    fn detects_compiled_fixture_encoding() {
        let bytes = std::fs::read(compiled_fixture()).unwrap_or_else(|error| {
            panic!("read compiled fixture: {error}");
        });
        assert_eq!(detect_model_encoding(&bytes), ModelEncoding::Compiled);
    }

    #[test]
    fn parses_compiled_fixture_header_and_summary() {
        let model = read_binary_model_from_file(compiled_fixture()).unwrap_or_else(|error| {
            panic!("parse compiled fixture: {error}");
        });

        assert_eq!(model.name, "a_ba2");
        assert_eq!(model.node_count_hint, 222);
        assert_eq!(model.nodes.len(), 57);
        assert_eq!(model.animations.len(), 20);
        assert_eq!(model.header.binary_id, 0);
        assert_eq!(model.header.raw_data_offset, 760_200);
        assert_eq!(model.header.raw_data_size, 77_606);
        assert!(model.node("torso_g").is_some());
        assert!(model.animation("salute").is_some());
        assert_eq!(
            model
                .animation("salute")
                .map(|animation| animation.nodes.len()),
            Some(36)
        );
        assert_eq!(
            model
                .animation("castout")
                .map(|animation| animation.nodes.len()),
            Some(55)
        );
    }

    #[test]
    fn auto_parsing_dispatches_to_compiled_model() {
        let bytes = std::fs::read(compiled_fixture()).unwrap_or_else(|error| {
            panic!("read compiled fixture: {error}");
        });

        let parsed = parse_model_bytes(&bytes).unwrap_or_else(|error| {
            panic!("parse compiled bytes: {error}");
        });
        match parsed {
            ParsedModel::Compiled(model) => {
                assert_eq!(model.name, "a_ba2");
            }
            ParsedModel::Ascii(_ascii) => panic!("expected compiled model"),
        }
    }

    #[test]
    fn malformed_animation_pointer_becomes_diagnostic() {
        let mut bytes = std::fs::read(compiled_fixture()).unwrap_or_else(|error| {
            panic!("read compiled fixture: {error}");
        });
        let animation_pointer_offset = FILE_HEADER_SIZE + 232;
        let replacement = u32::MAX.to_le_bytes();
        let target = bytes
            .get_mut(animation_pointer_offset..animation_pointer_offset + 4)
            .unwrap_or_else(|| panic!("compiled fixture missing first animation pointer"));
        target.copy_from_slice(&replacement);

        let model = parse_binary_model_bytes(&bytes).unwrap_or_else(|error| {
            panic!("parse corrupted compiled bytes: {error}");
        });

        assert!(
            model
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("failed to parse animation"))
        );
    }
}
