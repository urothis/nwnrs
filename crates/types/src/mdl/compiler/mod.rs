//! ASCII/semantic MDL to BioWare compiled-model writer.

// Binary layout construction relies on validated offsets and format-bounded
// narrowing conversions throughout this module.
#![allow(
    clippy::cast_possible_truncation,
    clippy::indexing_slicing,
    clippy::map_err_ignore
)]

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    mdl::{
        AsciiElement, BinaryModel, ModelClassification, ModelDiagnosticKind, ModelError,
        ModelResult, NodeKind, ScalarKey, SemanticAnimation, SemanticAnimationNode,
        SemanticEmitter, SemanticMesh, SemanticModel, SemanticNode, SemanticPropertyValue,
        SemanticSkinWeight, SemanticUvLayer, Vec3Key, Vec4Key,
        controllers::{
            ALPHA_CONTROLLER, LIGHT_COLOR_CONTROLLER, LIGHT_MULTIPLIER_CONTROLLER,
            LIGHT_RADIUS_CONTROLLER, LIGHT_SHADOW_RADIUS_CONTROLLER,
            LIGHT_VERTICAL_DISPLACEMENT_CONTROLLER, NwnEmitterController, ORIENTATION_CONTROLLER,
            POSITION_CONTROLLER, SCALE_CONTROLLER, SELF_ILLUM_COLOR_CONTROLLER,
            emitter_controller_definition, emitter_controller_definition_for,
        },
        layout::{MESH_HEADER_SIZE, MODEL_HEADER_SIZE},
        lower_ascii_model, parse_binary_model_bytes,
    },
    resman::RESREF_MAX_LENGTH,
};

const EE_MAX_MESH_FACES: usize = 21_845;
const MAX_SKIN_INFLUENCES: usize = 4;

/// Compiles a parsed ASCII MDL into a BioWare-compatible compiled model.
///
/// # Errors
///
/// Returns [`ModelError`] when the ASCII contains data that cannot be
/// represented safely in a compiled MDL, or when the authored node trees are
/// invalid.
pub fn compile_ascii_model(model: &crate::mdl::AsciiModel) -> ModelResult<BinaryModel> {
    let semantic = lower_ascii_model(model)?;
    reject_lossy_source_diagnostics(&semantic)?;
    compile_semantic_model(&semantic)
}

/// Compiles a semantic MDL into a BioWare-compatible compiled model.
///
/// The generated payload targets the NWN:EE layout. Strings, array indices,
/// engine limits, and unsupported authored statements are checked rather than
/// silently truncated or discarded. Diagnostics already attached to the
/// semantic value describe its source provenance and do not prevent compiling
/// an edited value; the current semantic fields are authoritative here.
///
/// # Errors
///
/// Returns [`ModelError`] for invalid trees, string overflow, invalid mesh
/// indices, unsupported retained statements, or binary-format and EE
/// engine-limit overflows. Source-only comments,
/// `filedependency`, authored AABB-cache rows, and known NWMax exporter-only
/// metadata (including display flags and redundant animation mesh data) have no
/// compiled representation and are intentionally ignored. Every other
/// unsupported statement is rejected.
pub fn compile_semantic_model(model: &SemanticModel) -> ModelResult<BinaryModel> {
    let bytes = compile_semantic_model_bytes(model)?;
    parse_binary_model_bytes(&bytes)
}

/// Compiles a semantic MDL and returns the complete file payload.
///
/// # Errors
///
/// Returns [`ModelError`] under the same conditions as
/// [`compile_semantic_model`].
pub fn compile_semantic_model_bytes(model: &SemanticModel) -> ModelResult<Vec<u8>> {
    Compiler::new(model)?.compile()
}

/// Validates whether the current semantic value can be compiled to NWN:EE
/// binary MDL without producing output.
///
/// Historical lowering diagnostics do not affect this validation. Structural
/// fields, retained source statements, numeric values, resource references,
/// and engine limits are validated from the current value.
///
/// # Errors
///
/// Returns [`ModelError`] when the current semantic value is not safely
/// representable in compiled MDL.
pub fn validate_semantic_model(model: &SemanticModel) -> ModelResult<()> {
    compile_semantic_model_bytes(model).map(|_| ())
}

#[derive(Default)]
struct PatchBuf {
    bytes: Vec<u8>,
}

impl PatchBuf {
    fn len(&self) -> usize {
        self.bytes.len()
    }

    fn u8(&mut self, value: u8) {
        self.bytes.push(value);
    }

    fn u16(&mut self, value: u16) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn u32(&mut self, value: u32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn i32(&mut self, value: i32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn f32(&mut self, value: f32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn vec2(&mut self, value: [f32; 2]) {
        self.f32(value[0]);
        self.f32(value[1]);
    }

    fn vec3(&mut self, value: [f32; 3]) {
        self.f32(value[0]);
        self.f32(value[1]);
        self.f32(value[2]);
    }

    fn zeros(&mut self, count: usize) {
        self.bytes.resize(self.bytes.len() + count, 0);
    }

    fn fixed_string(&mut self, value: &str, width: usize, field: &str) -> ModelResult<()> {
        let encoded = crate::mdl::ascii::text::encode_model_text(value);
        if encoded.contains(&0) {
            return Err(ModelError::msg(format!("{field} contains a NUL byte")));
        }
        if encoded.len() >= width {
            return Err(ModelError::msg(format!(
                "{field} is {} bytes; compiled MDL permits at most {}",
                encoded.len(),
                width - 1
            )));
        }
        self.bytes.extend_from_slice(&encoded);
        self.zeros(width - encoded.len());
        Ok(())
    }

    fn placeholder(&mut self) -> usize {
        let offset = self.len();
        self.u32(0);
        offset
    }

    fn patch_u32(&mut self, offset: usize, value: u32) -> ModelResult<()> {
        let target = self
            .bytes
            .get_mut(offset..offset + 4)
            .ok_or_else(|| ModelError::msg("internal compiler patch offset is out of range"))?;
        target.copy_from_slice(&value.to_le_bytes());
        Ok(())
    }

    fn array(&mut self, pointer: u32, count: u32) {
        self.u32(pointer);
        self.u32(count);
        self.u32(count);
    }

    fn empty_array(&mut self) {
        self.array(0, 0);
    }
}

#[derive(Clone)]
struct ControllerRows {
    type_id: i32,
    values:  Vec<(f32, Vec<f32>)>,
    bezier:  bool,
}

struct ExpandedMesh {
    positions:           Vec<[f32; 3]>,
    normals:             Vec<[f32; 3]>,
    tangents:            Vec<[f32; 4]>,
    colors:              Vec<[f32; 4]>,
    uv_layers:           Vec<Vec<[f32; 2]>>,
    original_vertices:   Vec<usize>,
    original_uvs:        Vec<usize>,
    source_vertex_count: usize,
    source_uv_count:     usize,
    face_vertices:       Vec<[u16; 3]>,
}

struct Compiler<'a> {
    model:             &'a SemanticModel,
    core:              PatchBuf,
    raw:               PatchBuf,
    children:          BTreeMap<String, Vec<usize>>,
    nodes_by_name:     BTreeMap<String, usize>,
    node_ids:          BTreeMap<String, i32>,
    node_ids_by_index: Vec<i32>,
    node_offsets:      BTreeMap<String, u32>,
}

impl<'a> Compiler<'a> {
    fn new(model: &'a SemanticModel) -> ModelResult<Self> {
        validate_lossless_compilation(model)?;
        validate_finite_model(model)?;

        let mut children = BTreeMap::<String, Vec<usize>>::new();
        let mut nodes_by_name = BTreeMap::new();
        for (index, node) in model.nodes.iter().enumerate() {
            let key = node.name.to_ascii_lowercase();
            if node.name.is_empty() {
                return Err(ModelError::msg("geometry node names cannot be empty"));
            }
            nodes_by_name.insert(key, index);
            children
                .entry(parent_key(node.parent.as_deref()))
                .or_default()
                .push(index);
        }

        let roots = children.get("").cloned().unwrap_or_default();
        if roots.len() != 1 {
            return Err(ModelError::msg(format!(
                "compiled MDL requires exactly one geometry root; found {}",
                roots.len()
            )));
        }
        for node in &model.nodes {
            if let Some(parent) = &node.parent
                && !nodes_by_name.contains_key(&parent.to_ascii_lowercase())
            {
                return Err(ModelError::msg(format!(
                    "geometry node {} references missing parent {}",
                    node.name, parent
                )));
            }
        }

        let mut compiler = Self {
            model,
            core: PatchBuf::default(),
            raw: PatchBuf::default(),
            children,
            nodes_by_name,
            node_ids: BTreeMap::new(),
            node_ids_by_index: vec![-1; model.nodes.len()],
            node_offsets: BTreeMap::new(),
        };
        compiler.assign_node_ids(roots[0])?;
        if compiler
            .node_ids_by_index
            .iter()
            .any(|part_number| *part_number < 0)
        {
            return Err(ModelError::msg(
                "geometry node tree is cyclic or contains unreachable nodes",
            ));
        }
        Ok(compiler)
    }

    fn compile(mut self) -> ModelResult<Vec<u8>> {
        self.write_model()?;
        let core_len = checked_u32(self.core.len(), "compiled model-data size")?;
        let raw_len = checked_u32(self.raw.len(), "compiled raw-data size")?;
        let mut bytes = Vec::with_capacity(12 + self.core.len() + self.raw.len());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&core_len.to_le_bytes());
        bytes.extend_from_slice(&raw_len.to_le_bytes());
        bytes.extend_from_slice(&self.core.bytes);
        bytes.extend_from_slice(&self.raw.bytes);
        Ok(bytes)
    }

    fn assign_node_ids(&mut self, root: usize) -> ModelResult<()> {
        let mut stack = vec![root];
        let mut visited = BTreeSet::new();
        let mut expanded_names = BTreeSet::new();
        while let Some(index) = stack.pop() {
            let node = &self.model.nodes[index];
            let key = node.name.to_ascii_lowercase();
            if !visited.insert(index) {
                return Err(ModelError::msg(format!(
                    "cycle detected at geometry node {}",
                    node.name
                )));
            }
            if let Some(id) = self.node_ids.get(&key).copied() {
                self.node_ids_by_index[index] = id;
            } else {
                let id = i32::try_from(self.node_ids.len())
                    .map_err(|_| ModelError::msg("geometry node count exceeds i32"))?;
                self.node_ids.entry(key.clone()).or_insert(id);
                self.node_ids_by_index[index] = id;
            }
            if expanded_names.insert(key.clone())
                && let Some(children) = self.children.get(&key)
            {
                stack.extend(children.iter().rev().copied());
            }
        }
        Ok(())
    }

    fn write_model(&mut self) -> ModelResult<()> {
        self.core.zeros(8);
        self.core
            .fixed_string(&self.model.header.model_name, 64, "model name")?;
        let root_pointer = self.core.placeholder();
        self.core.u32(checked_u32(
            self.nodes_by_name.len(),
            "geometry node count",
        )?);
        self.core.zeros(28);
        self.core.u32(2);

        self.core.zeros(2);
        self.core.u8(classification_code(
            self.model.header.classification.as_ref(),
        ));
        self.core
            .u8(u8::try_from(self.model.header.ignore_fog.unwrap_or(0))
                .map_err(|_| ModelError::msg("ignorefog must fit in an unsigned byte"))?);
        self.core.zeros(4);
        let animation_pointer = self.core.placeholder();
        let animation_count = checked_u32(self.model.animations.len(), "animation count")?;
        self.core.u32(animation_count);
        self.core.u32(animation_count);
        self.core.zeros(4);
        let (bound_min, bound_max, radius) = self.model_bounds();
        self.core.vec3(bound_min);
        self.core.vec3(bound_max);
        self.core.f32(radius);
        self.core
            .f32(self.model.header.animation_scale.unwrap_or(1.0));
        self.core.fixed_string(
            self.model.header.supermodel.as_deref().unwrap_or("NULL"),
            64,
            "supermodel name",
        )?;
        debug_assert_eq!(self.core.len(), MODEL_HEADER_SIZE);

        let mut animation_patches = Vec::with_capacity(self.model.animations.len());
        if !self.model.animations.is_empty() {
            self.core.patch_u32(
                animation_pointer,
                checked_u32(self.core.len(), "animation pointer table")?,
            )?;
            for _ in &self.model.animations {
                animation_patches.push(self.core.placeholder());
            }
        }

        let root_index = self.children[""][0];
        let root_offset = self.write_geometry_node(root_index, 0)?;
        self.core.patch_u32(root_pointer, root_offset)?;

        for (index, animation) in self.model.animations.iter().enumerate() {
            let offset = self.write_animation(animation)?;
            self.core.patch_u32(animation_patches[index], offset)?;
        }
        Ok(())
    }

    fn write_geometry_node(&mut self, index: usize, parent_offset: u32) -> ModelResult<u32> {
        let node = &self.model.nodes[index];
        let key = node.name.to_ascii_lowercase();
        if let Some(offset) = self.node_offsets.get(&key) {
            return Ok(*offset);
        }
        let offset = checked_u32(self.core.len(), "node offset")?;
        self.node_offsets.insert(key.clone(), offset);
        let children = self.children.get(&key).cloned().unwrap_or_default();
        let controllers = geometry_controllers(node);
        let content = node_content(&node.kind)?;
        let patches = self.write_node_header(
            node.material.inherit_color.unwrap_or(0),
            self.node_ids_by_index[index],
            &node.name,
            parent_offset,
            content,
        )?;

        self.write_type_headers(node, content)?;
        let expanded = if content & 0x20 != 0 {
            Some(self.write_mesh_header(node)?)
        } else {
            None
        };
        if content & 0x40 != 0 {
            self.write_skin_header(node, expanded.as_ref())?;
        }
        let animmesh_data = (content & 0x80 != 0)
            .then(|| self.write_animmesh_header(node.sample_period, None, expanded.as_ref()))
            .transpose()?;
        let dangly_data = (content & 0x100 != 0)
            .then(|| self.write_dangly_header(node, expanded.as_ref()))
            .transpose()?;
        let aabb_patch = (content & 0x200 != 0).then(|| self.core.placeholder());

        if let (Some(mesh), Some(expanded)) = (&node.mesh, expanded.as_ref()) {
            self.write_faces(mesh, expanded)?;
        }
        if let Some(data) = animmesh_data {
            self.write_animmesh_data(data)?;
        }
        if let Some(data) = dangly_data {
            self.write_dangly_data(data)?;
        }
        if let Some(aabb_patch) = aabb_patch {
            self.write_aabb(node, aabb_patch)?;
        }
        self.write_controllers(
            &controllers,
            patches.controller_keys,
            patches.controller_data,
        )?;

        if !children.is_empty() {
            self.core.patch_u32(
                patches.children,
                checked_u32(self.core.len(), "child pointer table")?,
            )?;
        }
        let child_count = checked_u32(children.len(), "child count")?;
        self.core.patch_u32(patches.children + 4, child_count)?;
        self.core.patch_u32(patches.children + 8, child_count)?;
        let mut child_patches = Vec::with_capacity(children.len());
        for _ in &children {
            child_patches.push(self.core.placeholder());
        }
        for (child, patch) in children.into_iter().zip(child_patches) {
            let child_offset = self.write_geometry_node(child, offset)?;
            self.core.patch_u32(patch, child_offset)?;
        }
        Ok(offset)
    }

    fn write_node_header(
        &mut self,
        inherit_color: i32,
        part_number: i32,
        name: &str,
        parent_offset: u32,
        content: u32,
    ) -> ModelResult<NodePatches> {
        self.core.zeros(24);
        self.core.i32(inherit_color);
        self.core.i32(part_number);
        self.core.fixed_string(name, 32, "node name")?;
        self.core.u32(0);
        self.core.u32(parent_offset);
        let children = self.core.placeholder();
        self.core.u32(0);
        self.core.u32(0);
        let controller_keys = self.core.placeholder();
        self.core.u32(0);
        self.core.u32(0);
        let controller_data = self.core.placeholder();
        self.core.u32(0);
        self.core.u32(0);
        self.core.u32(content);
        Ok(NodePatches {
            children,
            controller_keys,
            controller_data,
        })
    }

    fn write_type_headers(&mut self, node: &SemanticNode, content: u32) -> ModelResult<()> {
        if content & 0x02 != 0 {
            self.write_light_header(node)?;
        }
        if content & 0x04 != 0 {
            self.write_emitter_header(node)?;
        }
        if content & 0x10 != 0 {
            let reference = node.reference.as_ref();
            self.core.fixed_string(
                reference
                    .and_then(|value| value.model.as_deref())
                    .unwrap_or("NULL"),
                64,
                "reference model",
            )?;
            self.core
                .i32(reference.and_then(|value| value.reattachable).unwrap_or(0));
        }
        Ok(())
    }

    fn write_light_header(&mut self, node: &SemanticNode) -> ModelResult<()> {
        let Some(light) = node.light.as_ref() else {
            self.core.zeros(92);
            return Ok(());
        };
        self.core.f32(light.flare_radius.unwrap_or(0.0));
        self.core.empty_array();
        let size_ptr = self.core.placeholder();
        let size_count = checked_u32(light.flare_sizes.len(), "flare size count")?;
        self.core.u32(size_count);
        self.core.u32(size_count);
        let position_ptr = self.core.placeholder();
        let position_count = checked_u32(light.flare_positions.len(), "flare position count")?;
        self.core.u32(position_count);
        self.core.u32(position_count);
        let color_ptr = self.core.placeholder();
        let color_count = checked_u32(light.flare_color_shifts.len(), "flare color count")?;
        self.core.u32(color_count);
        self.core.u32(color_count);
        let texture_ptr = self.core.placeholder();
        let texture_count = checked_u32(light.flare_textures.len(), "flare texture count")?;
        self.core.u32(texture_count);
        self.core.u32(texture_count);
        self.core.i32(light.light_priority.unwrap_or(5));
        self.core.i32(light.ambient_only.unwrap_or(0));
        self.core
            .i32(light.n_dynamic_type.or(light.is_dynamic).unwrap_or(0));
        self.core.i32(light.affect_dynamic.unwrap_or(0));
        self.core
            .i32(node.material.shadow.map(i32::from).unwrap_or(0));
        self.core.i32(light.lens_flares.unwrap_or(0));
        self.core.i32(light.fading_light.unwrap_or(0));

        if !light.flare_sizes.is_empty() {
            self.core
                .patch_u32(size_ptr, checked_u32(self.core.len(), "flare sizes")?)?;
            for value in &light.flare_sizes {
                self.core.f32(*value);
            }
        }
        if !light.flare_positions.is_empty() {
            self.core.patch_u32(
                position_ptr,
                checked_u32(self.core.len(), "flare positions")?,
            )?;
            for value in &light.flare_positions {
                self.core.f32(*value);
            }
        }
        if !light.flare_color_shifts.is_empty() {
            self.core
                .patch_u32(color_ptr, checked_u32(self.core.len(), "flare colors")?)?;
            for value in &light.flare_color_shifts {
                self.core.vec3(*value);
            }
        }
        if !light.flare_textures.is_empty() {
            self.core
                .patch_u32(texture_ptr, checked_u32(self.core.len(), "flare textures")?)?;
            let mut patches = Vec::with_capacity(light.flare_textures.len());
            for _ in &light.flare_textures {
                patches.push(self.core.placeholder());
            }
            for (name, patch) in light.flare_textures.iter().zip(patches) {
                self.core
                    .patch_u32(patch, checked_u32(self.core.len(), "flare texture")?)?;
                self.core.fixed_string(name, 64, "flare texture name")?;
            }
        }
        Ok(())
    }

    fn write_emitter_header(&mut self, node: &SemanticNode) -> ModelResult<()> {
        let emitter = node.emitter.as_ref().ok_or_else(|| {
            ModelError::msg(format!("emitter node {} has no emitter payload", node.name))
        })?;
        self.core
            .f32(emitter_float(emitter, "deadspace").unwrap_or(0.0));
        self.core
            .f32(emitter_float(emitter, "blastradius").unwrap_or(0.0));
        self.core
            .f32(emitter_float(emitter, "blastlength").unwrap_or(0.0));
        self.core.u32(emitter_u32(emitter, "xgrid").unwrap_or(0));
        self.core.u32(emitter_u32(emitter, "ygrid").unwrap_or(0));
        self.core
            .u32(emitter_u32(emitter, "spawntype").unwrap_or(0));
        self.core.fixed_string(
            &emitter_text(emitter, "update").unwrap_or_default(),
            32,
            "emitter update",
        )?;
        self.core.fixed_string(
            &emitter_text(emitter, "render").unwrap_or_default(),
            32,
            "emitter render",
        )?;
        self.core.fixed_string(
            &emitter_text(emitter, "blend").unwrap_or_default(),
            32,
            "emitter blend",
        )?;
        self.core.fixed_string(
            &emitter_text(emitter, "texture").unwrap_or_default(),
            64,
            "emitter texture",
        )?;
        self.core.fixed_string(
            &emitter_text(emitter, "chunkname").unwrap_or_else(|| "CHUNK".to_string()),
            16,
            "emitter chunk name",
        )?;
        self.core
            .u32(emitter_u32(emitter, "twosidedtex").unwrap_or(0));
        self.core.u32(emitter_u32(emitter, "loop").unwrap_or(0));
        self.core.u16(checked_u16(
            emitter_u32(emitter, "renderorder").unwrap_or(0) as usize,
            "emitter render order",
        )?);
        self.core.u16(0);
        let mut flags = 0u32;
        for (bit, name) in [
            "p2p",
            "p2p_sel",
            "affectedbywind",
            "istinted",
            "bounce",
            "random",
            "inherit",
            "inheritvel",
            "inheritlocal",
            "splat",
            "inheritpart",
        ]
        .into_iter()
        .enumerate()
        {
            if emitter_bool(emitter, name).unwrap_or(false) {
                flags |= 1 << bit;
            }
        }
        self.core.u32(flags);
        Ok(())
    }

    fn write_mesh_header(&mut self, node: &SemanticNode) -> ModelResult<ExpandedMesh> {
        let mesh = node.mesh.as_ref().ok_or_else(|| {
            ModelError::msg(format!("mesh node {} has no mesh payload", node.name))
        })?;
        if mesh.faces.len() > EE_MAX_MESH_FACES {
            return Err(ModelError::msg(format!(
                "mesh node {} has {} faces; NWN:EE permits at most {EE_MAX_MESH_FACES}",
                node.name,
                mesh.faces.len()
            )));
        }
        let generate_missing_tangents = node
            .material
            .render_hint
            .as_deref()
            .is_some_and(|value| value.eq_ignore_ascii_case("NormalAndSpecMapped"));
        let expanded = expand_mesh(mesh, generate_missing_tangents)?;
        let header_start = self.core.len();
        self.core.zeros(8);
        let faces_pointer = self.core.placeholder();
        let face_count = checked_u32(mesh.faces.len(), "mesh face count")?;
        self.core.u32(face_count);
        self.core.u32(face_count);
        let (bound_min, bound_max, average, radius) = bounds(&expanded.positions);
        self.core.vec3(bound_min);
        self.core.vec3(bound_max);
        self.core.f32(radius);
        self.core.vec3(average);
        self.core
            .vec3(node.material.diffuse.unwrap_or([0.8, 0.8, 0.8]));
        self.core
            .vec3(node.material.ambient.unwrap_or([1.0, 1.0, 1.0]));
        self.core.vec3(node.material.specular.unwrap_or([0.0; 3]));
        self.core.f32(node.material.shininess.unwrap_or(0.0));
        self.core
            .u32(u32::from(node.material.shadow.unwrap_or(true)));
        self.core.i32(node.material.beaming.unwrap_or(0));
        self.core
            .u32(u32::from(node.material.render.unwrap_or(true)));
        self.core.i32(node.material.transparency_hint.unwrap_or(0));
        self.core.u32(
            if node
                .material
                .render_hint
                .as_deref()
                .is_some_and(|value| value.eq_ignore_ascii_case("NormalAndSpecMapped"))
            {
                2
            } else {
                0
            },
        );
        let textures = texture_slots(node);
        for (index, texture) in textures.iter().enumerate() {
            self.core.fixed_string(
                texture.as_deref().unwrap_or("NULL"),
                64,
                &format!("mesh texture{index}"),
            )?;
        }
        self.core.i32(node.material.tilefade.unwrap_or(0));
        for _ in 0..4 {
            self.core.empty_array();
        }
        self.core.i32(-1);
        self.core.u32(0);
        self.core.u32(0);
        self.core.i32(0);

        let vertex_pointer = self.core.placeholder();
        let vertex_count = checked_u16(expanded.positions.len(), "expanded mesh vertex count")?;
        self.core.u16(vertex_count);
        self.core.u16(checked_u16(
            expanded.uv_layers.len(),
            "mesh UV layer count",
        )?);
        let mut uv_pointers = [None; 4];
        for (index, pointer) in uv_pointers.iter_mut().enumerate() {
            if index < expanded.uv_layers.len() {
                *pointer = Some(self.core.placeholder());
            } else {
                self.core.i32(-1);
            }
        }
        let normal_pointer = (!expanded.normals.is_empty()).then(|| self.core.placeholder());
        if normal_pointer.is_none() {
            self.core.i32(-1);
        }
        let color_pointer = (!expanded.colors.is_empty()).then(|| self.core.placeholder());
        if color_pointer.is_none() {
            self.core.i32(-1);
        }
        self.core.i32(-1);
        self.core.i32(-1);
        self.core.i32(-1);
        let tangent_pointer = (!expanded.tangents.is_empty()).then(|| self.core.placeholder());
        if tangent_pointer.is_none() {
            self.core.i32(-1);
        }
        self.core.i32(-1);
        let bitangent_pointer = (!expanded.tangents.is_empty()).then(|| self.core.placeholder());
        if bitangent_pointer.is_none() {
            self.core.i32(-1);
        }
        self.core.u8(
            u8::try_from(node.material.light_mapped.unwrap_or(0)).map_err(|_| {
                ModelError::msg(format!(
                    "node {} lightmapped value must fit in u8",
                    node.name
                ))
            })?,
        );
        self.core.u8(
            u8::try_from(node.material.rotate_texture.unwrap_or(0)).map_err(|_| {
                ModelError::msg(format!(
                    "node {} rotatetexture value must fit in u8",
                    node.name
                ))
            })?,
        );
        self.core.u16(0);
        self.core.f32(0.0);
        self.core.u32(0);
        debug_assert_eq!(self.core.len() - header_start, MESH_HEADER_SIZE);

        self.core.patch_u32(
            vertex_pointer,
            checked_u32(self.raw.len(), "vertex raw offset")?,
        )?;
        for value in &expanded.positions {
            self.raw.vec3(*value);
        }
        for (layer, pointer) in expanded
            .uv_layers
            .iter()
            .zip(uv_pointers.into_iter().flatten())
        {
            self.core
                .patch_u32(pointer, checked_u32(self.raw.len(), "UV raw offset")?)?;
            for value in layer {
                self.raw.vec2(*value);
            }
        }
        if let Some(pointer) = normal_pointer {
            self.core
                .patch_u32(pointer, checked_u32(self.raw.len(), "normal raw offset")?)?;
            for value in &expanded.normals {
                self.raw.vec3(*value);
            }
        }
        if let Some(pointer) = color_pointer {
            self.core
                .patch_u32(pointer, checked_u32(self.raw.len(), "color raw offset")?)?;
            for value in &expanded.colors {
                for channel in value {
                    self.raw.u8(float_to_byte(*channel));
                }
            }
        }
        if let Some(pointer) = tangent_pointer {
            self.core
                .patch_u32(pointer, checked_u32(self.raw.len(), "tangent raw offset")?)?;
            for value in &expanded.tangents {
                self.raw.vec3([value[0], value[1], value[2]]);
            }
        }
        if let Some(pointer) = bitangent_pointer {
            self.core.patch_u32(
                pointer,
                checked_u32(self.raw.len(), "bitangent raw offset")?,
            )?;
            for (index, tangent) in expanded.tangents.iter().enumerate() {
                let normal = expanded
                    .normals
                    .get(index)
                    .copied()
                    .unwrap_or([0.0, 0.0, 1.0]);
                let cross = cross(normal, [tangent[0], tangent[1], tangent[2]]);
                self.raw.vec3(scale3(cross, tangent[3]));
            }
        }

        // Store the face pointer patch at the end without growing a public field.
        self.core.patch_u32(faces_pointer, 0)?;
        let mut expanded = expanded;
        // `face_vertices` always exists; the sentinel entry carries the patch location.
        expanded.face_vertices.push([
            u16::try_from(faces_pointer & 0xffff).unwrap_or(0),
            u16::try_from(faces_pointer >> 16).unwrap_or(0),
            u16::MAX,
        ]);
        Ok(expanded)
    }

    fn write_faces(&mut self, mesh: &SemanticMesh, expanded: &ExpandedMesh) -> ModelResult<()> {
        let sentinel = expanded
            .face_vertices
            .last()
            .ok_or_else(|| ModelError::msg("internal mesh face patch is missing"))?;
        let patch = usize::from(sentinel[0]) | (usize::from(sentinel[1]) << 16);
        self.core
            .patch_u32(patch, checked_u32(self.core.len(), "face array offset")?)?;
        let adjacency = face_adjacency(&mesh.faces);
        for (face_index, face) in mesh.faces.iter().enumerate() {
            let points = face.vertex_indices.map(|index| {
                mesh.vertices
                    .get(index as usize)
                    .copied()
                    .unwrap_or([0.0; 3])
            });
            let normal = normalize(cross(
                sub3(points[1], points[0]),
                sub3(points[2], points[0]),
            ));
            self.core.vec3(normal);
            self.core.f32(-dot(normal, points[0]));
            self.core.i32(face.material_index);
            for adjacent in adjacency[face_index] {
                self.core.u16(adjacent);
            }
            for vertex in expanded.face_vertices[face_index] {
                self.core.u16(vertex);
            }
        }
        Ok(())
    }

    fn write_skin_header(
        &mut self,
        node: &SemanticNode,
        expanded: Option<&ExpandedMesh>,
    ) -> ModelResult<()> {
        let mesh = node
            .mesh
            .as_ref()
            .ok_or_else(|| ModelError::msg("skin has no mesh"))?;
        let expanded = expanded.ok_or_else(|| ModelError::msg("skin has no expanded mesh"))?;
        let weight_rows = mesh
            .weights
            .iter()
            .map(|row| normalized_skin_influences(row))
            .collect::<Vec<_>>();
        let mut bones = Vec::<String>::new();
        let mut bone_indices = BTreeMap::<String, usize>::new();
        for row in &weight_rows {
            for influence in row {
                let key = influence.bone.to_ascii_lowercase();
                if let std::collections::btree_map::Entry::Vacant(entry) = bone_indices.entry(key) {
                    if bones.len() == 64 {
                        return Err(ModelError::msg(format!(
                            "skin node {} references more than 64 bones",
                            node.name
                        )));
                    }
                    entry.insert(bones.len());
                    bones.push(influence.bone.clone());
                }
            }
        }
        let weight_pointer = checked_i32(self.raw.len(), "skin weight raw offset")?;
        let bone_pointer = checked_i32(
            self.raw.len() + expanded.positions.len() * 16,
            "skin bone raw offset",
        )?;
        self.core.empty_array();
        self.core.i32(weight_pointer);
        self.core.i32(bone_pointer);
        self.core.i32(0);
        self.core.i32(0);
        self.core.empty_array();
        self.core.empty_array();
        self.core.empty_array();
        for index in 0..64 {
            let part = bones
                .get(index)
                .and_then(|name| self.node_ids.get(&name.to_ascii_lowercase()))
                .copied()
                .unwrap_or(-1);
            self.core.u16(if part < 0 { u16::MAX } else { part as u16 });
        }

        let mut rows = Vec::with_capacity(expanded.original_vertices.len());
        for original in &expanded.original_vertices {
            let influences = weight_rows.get(*original).map(Vec::as_slice).unwrap_or(&[]);
            let mut weights = [0.0; 4];
            let mut refs = [u16::MAX; 4];
            for (slot, influence) in influences.iter().enumerate() {
                weights[slot] = influence.weight;
                refs[slot] = u16::try_from(bone_indices[&influence.bone.to_ascii_lowercase()])
                    .map_err(|_| ModelError::msg("skin bone index exceeds u16"))?;
            }
            rows.push((weights, refs));
        }
        for (weights, _) in &rows {
            for value in weights {
                self.raw.f32(*value);
            }
        }
        for (_, refs) in &rows {
            for value in refs {
                self.raw.u16(*value);
            }
        }
        Ok(())
    }

    fn write_animmesh_header(
        &mut self,
        sample_period: Option<f32>,
        animation: Option<&SemanticAnimationNode>,
        expanded: Option<&ExpandedMesh>,
    ) -> ModelResult<AnimMeshData> {
        let samples = expand_animation_samples(animation, expanded)?;
        self.core.f32(sample_period.unwrap_or(0.0));
        for _ in 0..3 {
            self.core.empty_array();
        }
        let vertex_pointer = self.core.placeholder();
        let texture_pointer = self.core.placeholder();
        self.core.u32(samples.vertex_sets);
        self.core.u32(samples.texture_sets);
        Ok(AnimMeshData {
            vertex_pointer,
            texture_pointer,
            vertices: samples.vertices,
            textures: samples.textures,
        })
    }

    fn write_animmesh_data(&mut self, data: AnimMeshData) -> ModelResult<()> {
        if !data.vertices.is_empty() {
            self.core.patch_u32(
                data.vertex_pointer,
                checked_u32(self.core.len(), "animverts offset")?,
            )?;
            for value in data.vertices {
                self.core.vec3(value);
            }
        }
        if !data.textures.is_empty() {
            self.core.patch_u32(
                data.texture_pointer,
                checked_u32(self.core.len(), "animtverts offset")?,
            )?;
            for value in data.textures {
                self.core.vec2(value);
            }
        }
        Ok(())
    }

    fn write_dangly_header(
        &mut self,
        node: &SemanticNode,
        expanded: Option<&ExpandedMesh>,
    ) -> ModelResult<DanglyData> {
        let dangly = node
            .dangly
            .as_ref()
            .ok_or_else(|| ModelError::msg("danglymesh has no payload"))?;
        let constraints = node
            .mesh
            .as_ref()
            .map(|mesh| &mesh.constraints)
            .ok_or_else(|| ModelError::msg("danglymesh has no mesh"))?;
        let expanded_constraints = expanded
            .map(|mesh| {
                mesh.original_vertices
                    .iter()
                    .map(|index| {
                        constraints
                            .get(*index)
                            .and_then(|row| row.first())
                            .copied()
                            .unwrap_or(0.0)
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let pointer = self.core.placeholder();
        let count = checked_u32(expanded_constraints.len(), "constraint count")?;
        self.core.u32(count);
        self.core.u32(count);
        self.core.f32(dangly.displacement.unwrap_or(0.0));
        self.core.f32(dangly.tightness.unwrap_or(0.0));
        self.core.f32(dangly.period.unwrap_or(0.0));
        Ok(DanglyData {
            pointer,
            constraints: expanded_constraints,
        })
    }

    fn write_dangly_data(&mut self, data: DanglyData) -> ModelResult<()> {
        if !data.constraints.is_empty() {
            self.core.patch_u32(
                data.pointer,
                checked_u32(self.core.len(), "dangly constraint offset")?,
            )?;
            for value in data.constraints {
                self.core.f32(value);
            }
        }
        Ok(())
    }

    fn write_aabb(&mut self, node: &SemanticNode, pointer_patch: usize) -> ModelResult<()> {
        let mesh = node
            .mesh
            .as_ref()
            .ok_or_else(|| ModelError::msg("AABB node has no mesh"))?;
        if mesh.faces.is_empty() {
            return Ok(());
        }
        let tree = build_aabb_tree(mesh)?;
        let base = checked_u32(self.core.len(), "AABB tree offset")?;
        self.core.patch_u32(pointer_patch, base)?;
        let offsets = (0..tree.len())
            .map(|index| base + u32::try_from(index * 40).unwrap_or(0))
            .collect::<Vec<_>>();
        for entry in &tree {
            self.core.vec3(entry.min);
            self.core.vec3(entry.max);
            self.core.u32(entry.left.map_or(0, |index| offsets[index]));
            self.core.u32(entry.right.map_or(0, |index| offsets[index]));
            self.core.i32(entry.face.map_or(-1, |face| face as i32));
            self.core.u32(entry.plane);
        }
        Ok(())
    }

    fn write_controllers(
        &mut self,
        rows: &[ControllerRows],
        key_pointer_patch: usize,
        data_pointer_patch: usize,
    ) -> ModelResult<()> {
        if rows.is_empty() {
            return Ok(());
        }
        let mut times = Vec::new();
        let mut values = Vec::new();
        struct Header {
            type_id:    i32,
            count:      u16,
            time_start: u16,
            data_start: u16,
            columns:    u8,
        }
        let mut headers = Vec::with_capacity(rows.len());
        for controller in rows {
            let columns = controller.values.first().map_or(0, |row| row.1.len());
            if controller.values.iter().any(|row| row.1.len() != columns) {
                return Err(ModelError::msg(format!(
                    "controller {} has inconsistent row widths",
                    controller.type_id
                )));
            }
            if columns > 15 {
                return Err(ModelError::msg(format!(
                    "controller {} has {columns} columns; maximum is 15",
                    controller.type_id
                )));
            }
            let time_start = checked_u16(times.len(), "controller time index")?;
            let data_start = checked_u16(values.len(), "controller data index")?;
            for (time, row) in &controller.values {
                times.push(*time);
                values.extend_from_slice(row);
            }
            headers.push(Header {
                type_id: controller.type_id,
                count: checked_u16(controller.values.len(), "controller row count")?,
                time_start,
                data_start,
                columns: (columns as u8) | if controller.bezier { 0x10 } else { 0 },
            });
        }
        let times_len = checked_u16(times.len(), "controller time array length")?;
        for header in &mut headers {
            header.data_start = header
                .data_start
                .checked_add(times_len)
                .ok_or_else(|| ModelError::msg("controller data index exceeds u16"))?;
        }
        self.core.patch_u32(
            key_pointer_patch,
            checked_u32(self.core.len(), "controller headers offset")?,
        )?;
        let header_count = checked_u32(headers.len(), "controller header count")?;
        self.core.patch_u32(key_pointer_patch + 4, header_count)?;
        self.core.patch_u32(key_pointer_patch + 8, header_count)?;
        for header in headers {
            self.core.i32(header.type_id);
            self.core.u16(header.count);
            self.core.u16(header.time_start);
            self.core.u16(header.data_start);
            self.core.u8(header.columns);
            self.core.u8(0);
        }
        self.core.patch_u32(
            data_pointer_patch,
            checked_u32(self.core.len(), "controller data offset")?,
        )?;
        let float_count = checked_u32(times.len() + values.len(), "controller float count")?;
        self.core.patch_u32(data_pointer_patch + 4, float_count)?;
        self.core.patch_u32(data_pointer_patch + 8, float_count)?;
        for value in times.into_iter().chain(values) {
            self.core.f32(value);
        }
        Ok(())
    }

    fn write_animation(&mut self, animation: &SemanticAnimation) -> ModelResult<u32> {
        validate_animation_tree(animation)?;
        let offset = checked_u32(self.core.len(), "animation offset")?;
        self.core.zeros(8);
        self.core
            .fixed_string(&animation.name, 64, "animation name")?;
        let root_pointer = self.core.placeholder();
        let unique_node_count = animation
            .nodes
            .iter()
            .map(|node| node.name.to_ascii_lowercase())
            .collect::<BTreeSet<_>>()
            .len();
        self.core
            .u32(checked_u32(unique_node_count, "animation node count")?);
        self.core.zeros(28);
        self.core.u32(1);
        self.core.f32(animation.length.unwrap_or(0.0));
        self.core.f32(animation.transtime.unwrap_or(0.0));
        self.core.fixed_string(
            animation
                .animroot
                .as_deref()
                .unwrap_or(&self.model.header.model_name),
            64,
            "animation root name",
        )?;
        let event_pointer = self.core.placeholder();
        let event_count = checked_u32(animation.events.len(), "animation event count")?;
        self.core.u32(event_count);
        self.core.u32(event_count);
        if !animation.events.is_empty() {
            self.core
                .patch_u32(event_pointer, checked_u32(self.core.len(), "event offset")?)?;
            for event in &animation.events {
                self.core.f32(event.time);
                self.core
                    .fixed_string(&event.name, 32, "animation event name")?;
            }
        }
        if animation.nodes.is_empty() {
            return Ok(offset);
        }
        let mut children = BTreeMap::<String, Vec<usize>>::new();
        for (index, node) in animation.nodes.iter().enumerate() {
            children
                .entry(parent_key(node.parent.as_deref()))
                .or_default()
                .push(index);
        }
        let root = children[""][0];
        let mut offsets = BTreeMap::new();
        let control_root = animation
            .animroot
            .as_deref()
            .unwrap_or(&self.model.header.model_name);
        let root_offset = self.write_animation_node(
            animation,
            &children,
            root,
            0,
            false,
            control_root,
            &mut offsets,
        )?;
        self.core.patch_u32(root_pointer, root_offset)?;
        Ok(offset)
    }

    fn write_animation_node(
        &mut self,
        animation: &SemanticAnimation,
        children: &BTreeMap<String, Vec<usize>>,
        index: usize,
        parent_offset: u32,
        parent_is_controlled: bool,
        control_root: &str,
        offsets: &mut BTreeMap<String, u32>,
    ) -> ModelResult<u32> {
        let node = &animation.nodes[index];
        let key = node.name.to_ascii_lowercase();
        let is_controlled = parent_is_controlled || node.name.eq_ignore_ascii_case(control_root);
        if let Some(offset) = offsets.get(&key) {
            return Ok(*offset);
        }
        let geometry_base = self
            .nodes_by_name
            .get(&key)
            .and_then(|index| self.model.nodes.get(*index));
        let mut geometry_override = geometry_base.cloned();
        if let Some(geometry) = &mut geometry_override {
            if !node.faces.is_empty()
                && let Some(mesh) = &mut geometry.mesh
            {
                mesh.faces.clone_from(&node.faces);
            }
            if node.dangly.is_some() {
                geometry.dangly.clone_from(&node.dangly);
            }
        }
        let geometry = geometry_override.as_ref();
        let geometry_content = geometry
            .map(|node| node_content(&node.kind))
            .transpose()?
            .unwrap_or(node_content(&node.kind)?);
        let mut content = 1;
        if geometry_content & 0x02 != 0 {
            content |= 0x02;
        }
        if geometry_content & 0x04 != 0 {
            content |= 0x04;
        }
        if !node.animverts.is_empty() || !node.animtverts.is_empty() || node.sample_period.is_some()
        {
            content |= geometry_content & (0x20 | 0x40 | 0x80 | 0x100 | 0x200);
            content |= 0x20 | 0x80;
        }
        let offset = checked_u32(self.core.len(), "animation node offset")?;
        offsets.insert(key.clone(), offset);
        let patches = self.write_node_header(
            0,
            self.node_ids.get(&key).copied().unwrap_or(-1),
            &node.name,
            parent_offset,
            content,
        )?;
        if content & 0x02 != 0 {
            if let Some(geometry) = geometry {
                self.write_light_header(geometry)?;
            } else {
                self.core.zeros(92);
            }
        }
        if content & 0x04 != 0 {
            if let Some(geometry) = geometry {
                self.write_emitter_header(geometry)?;
            } else {
                self.core.zeros(216);
            }
        }
        let expanded = if content & 0x20 != 0 {
            let geometry = geometry.ok_or_else(|| {
                ModelError::msg(format!("animmesh node {} has no geometry node", node.name))
            })?;
            Some(self.write_mesh_header(geometry)?)
        } else {
            None
        };
        if content & 0x40 != 0 {
            self.write_skin_header(
                geometry.ok_or_else(|| ModelError::msg("animated skin has no geometry node"))?,
                expanded.as_ref(),
            )?;
        }
        let animmesh_data = (content & 0x80 != 0)
            .then(|| self.write_animmesh_header(node.sample_period, Some(node), expanded.as_ref()))
            .transpose()?;
        let dangly_data = (content & 0x100 != 0)
            .then(|| {
                geometry
                    .ok_or_else(|| ModelError::msg("animated danglymesh has no geometry node"))
                    .and_then(|geometry| self.write_dangly_header(geometry, expanded.as_ref()))
            })
            .transpose()?;
        if content & 0x200 != 0 {
            self.core.u32(0);
        }
        if let (Some(geometry), Some(expanded)) = (geometry, expanded.as_ref()) {
            self.write_faces(
                geometry
                    .mesh
                    .as_ref()
                    .ok_or_else(|| ModelError::msg("animated mesh has no geometry mesh"))?,
                expanded,
            )?;
        }
        if let Some(data) = animmesh_data {
            self.write_animmesh_data(data)?;
        }
        if let Some(data) = dangly_data {
            self.write_dangly_data(data)?;
        }
        let controllers = if is_controlled {
            animation_controllers(node, geometry_content)?
        } else {
            Vec::new()
        };
        self.write_controllers(
            &controllers,
            patches.controller_keys,
            patches.controller_data,
        )?;
        let node_children = children.get(&key).cloned().unwrap_or_default();
        if !node_children.is_empty() {
            self.core.patch_u32(
                patches.children,
                checked_u32(self.core.len(), "animation child table")?,
            )?;
        }
        let child_count = checked_u32(node_children.len(), "animation child count")?;
        self.core.patch_u32(patches.children + 4, child_count)?;
        self.core.patch_u32(patches.children + 8, child_count)?;
        let mut child_patches = Vec::new();
        for _ in &node_children {
            child_patches.push(self.core.placeholder());
        }
        for (child, patch) in node_children.into_iter().zip(child_patches) {
            let child_offset = self.write_animation_node(
                animation,
                children,
                child,
                offset,
                is_controlled,
                control_root,
                offsets,
            )?;
            self.core.patch_u32(patch, child_offset)?;
        }
        Ok(offset)
    }

    fn model_bounds(&self) -> ([f32; 3], [f32; 3], f32) {
        let root = self.children[""][0];
        let mut points = Vec::new();
        self.collect_world_points(
            root,
            WorldTransform {
                translation: [0.0; 3],
                rotation:    [0.0, 0.0, 0.0, 1.0],
                scale:       1.0,
            },
            &mut points,
        );
        let (min, max, _, radius) = bounds(&points);
        (min, max, radius)
    }

    fn collect_world_points(
        &self,
        index: usize,
        parent: WorldTransform,
        output: &mut Vec<[f32; 3]>,
    ) {
        let node = &self.model.nodes[index];
        let local_translation = node.position.unwrap_or([0.0; 3]);
        let local_rotation = node
            .orientation
            .map(axis_angle_to_quaternion)
            .unwrap_or([0.0, 0.0, 0.0, 1.0]);
        let transform = WorldTransform {
            translation: add3(
                parent.translation,
                quaternion_rotate(parent.rotation, scale3(local_translation, parent.scale)),
            ),
            rotation:    quaternion_multiply(parent.rotation, local_rotation),
            scale:       parent.scale * node.scale.unwrap_or(1.0),
        };
        if let Some(mesh) = &node.mesh {
            output.extend(mesh.vertices.iter().map(|point| {
                add3(
                    transform.translation,
                    quaternion_rotate(transform.rotation, scale3(*point, transform.scale)),
                )
            }));
        }
        let key = node.name.to_ascii_lowercase();
        if self
            .nodes_by_name
            .get(&key)
            .is_some_and(|first| *first == index)
            && let Some(children) = self.children.get(&key)
        {
            for child in children {
                self.collect_world_points(*child, transform, output);
            }
        }
    }
}

struct NodePatches {
    children:        usize,
    controller_keys: usize,
    controller_data: usize,
}

struct AnimMeshData {
    vertex_pointer:  usize,
    texture_pointer: usize,
    vertices:        Vec<[f32; 3]>,
    textures:        Vec<[f32; 2]>,
}

struct ExpandedAnimationSamples {
    vertices:     Vec<[f32; 3]>,
    vertex_sets:  u32,
    textures:     Vec<[f32; 2]>,
    texture_sets: u32,
}

struct DanglyData {
    pointer:     usize,
    constraints: Vec<f32>,
}

#[derive(Clone, Copy)]
struct WorldTransform {
    translation: [f32; 3],
    rotation:    [f32; 4],
    scale:       f32,
}

fn validate_finite_model(model: &SemanticModel) -> ModelResult<()> {
    validate_optional_finite(model.header.animation_scale, "animation scale")?;
    for node in &model.nodes {
        let field = |name: &str| format!("node {} {name}", node.name);
        validate_finite_values(node.position.into_iter().flatten(), &field("position"))?;
        validate_finite_values(
            node.orientation.into_iter().flatten(),
            &field("orientation"),
        )?;
        validate_optional_finite(node.scale, &field("scale"))?;
        validate_finite_values(node.color.into_iter().flatten(), &field("color"))?;
        validate_optional_finite(node.radius, &field("radius"))?;
        validate_finite_values(node.center.into_iter().flatten(), &field("center"))?;
        validate_finite_values(node.wirecolor.into_iter().flatten(), &field("wirecolor"))?;

        validate_optional_finite(node.material.shininess, &field("shininess"))?;
        validate_optional_finite(node.material.alpha, &field("alpha"))?;
        validate_finite_values(
            node.material.ambient.into_iter().flatten(),
            &field("ambient"),
        )?;
        validate_finite_values(
            node.material.diffuse.into_iter().flatten(),
            &field("diffuse"),
        )?;
        validate_finite_values(
            node.material.specular.into_iter().flatten(),
            &field("specular"),
        )?;
        validate_finite_values(
            node.material.self_illum_color.into_iter().flatten(),
            &field("selfillumcolor"),
        )?;

        if let Some(mesh) = &node.mesh {
            validate_finite_values(mesh.vertices.iter().flatten().copied(), &field("vertices"))?;
            for layer in &mesh.uv_layers {
                validate_finite_values(
                    layer.coordinates.iter().flatten().copied(),
                    &field(&format!("tverts{}", layer.index)),
                )?;
            }
            validate_finite_values(mesh.normals.iter().flatten().copied(), &field("normals"))?;
            validate_finite_values(mesh.colors.iter().flatten().copied(), &field("colors"))?;
            validate_finite_values(
                mesh.weights.iter().flatten().map(|weight| weight.weight),
                &field("weights"),
            )?;
            validate_finite_values(
                mesh.constraints.iter().flatten().copied(),
                &field("constraints"),
            )?;
        }
        if let Some(light) = &node.light {
            validate_optional_finite(light.multiplier, &field("multiplier"))?;
            validate_optional_finite(light.flare_radius, &field("flareradius"))?;
            validate_optional_finite(light.shadow_radius, &field("shadowradius"))?;
            validate_optional_finite(light.vertical_displacement, &field("verticaldisplacement"))?;
            validate_finite_values(light.flare_sizes.iter().copied(), &field("flaresizes"))?;
            validate_finite_values(
                light.flare_positions.iter().copied(),
                &field("flarepositions"),
            )?;
            validate_finite_values(
                light.flare_color_shifts.iter().flatten().copied(),
                &field("flarecolorshifts"),
            )?;
        }
        if let Some(emitter) = &node.emitter {
            validate_optional_finite(emitter.x_size, &field("xsize"))?;
            validate_optional_finite(emitter.y_size, &field("ysize"))?;
            for property in &emitter.properties {
                validate_finite_values(
                    property.values.iter().filter_map(|value| match value {
                        SemanticPropertyValue::Float(value) => Some(*value),
                        _ => None,
                    }),
                    &field(&property.name),
                )?;
            }
        }
        if let Some(dangly) = &node.dangly {
            validate_optional_finite(dangly.displacement, &field("displacement"))?;
            validate_optional_finite(dangly.tightness, &field("tightness"))?;
            validate_optional_finite(dangly.period, &field("period"))?;
        }
        for controller in &node.opaque_controllers {
            validate_finite_values(
                controller
                    .keys
                    .iter()
                    .flat_map(|key| std::iter::once(key.time).chain(key.values.iter().copied())),
                &field(&format!("controller {}", controller.type_id)),
            )?;
        }
    }

    for animation in &model.animations {
        let field = |name: &str| format!("animation {} {name}", animation.name);
        validate_optional_finite(animation.length, &field("length"))?;
        validate_optional_finite(animation.transtime, &field("transtime"))?;
        validate_finite_values(
            animation.events.iter().map(|event| event.time),
            &field("event times"),
        )?;
        for node in &animation.nodes {
            let field =
                |name: &str| format!("animation {} node {} {name}", animation.name, node.name);
            validate_finite_values(node.position.into_iter().flatten(), &field("position"))?;
            validate_finite_values(
                node.orientation.into_iter().flatten(),
                &field("orientation"),
            )?;
            validate_optional_finite(node.scale, &field("scale"))?;
            validate_finite_values(node.color.into_iter().flatten(), &field("color"))?;
            validate_optional_finite(node.radius, &field("radius"))?;
            validate_optional_finite(node.alpha, &field("alpha"))?;
            validate_finite_values(
                node.self_illum_color.into_iter().flatten(),
                &field("selfillumcolor"),
            )?;
            validate_optional_finite(node.multiplier, &field("multiplier"))?;
            validate_optional_finite(node.shadow_radius, &field("shadowradius"))?;
            validate_optional_finite(node.vertical_displacement, &field("verticaldisplacement"))?;
            validate_vec3_keys(&node.position_keys, &field("positionkey"))?;
            validate_vec4_keys(&node.orientation_keys, &field("orientationkey"))?;
            validate_scalar_keys(&node.scale_keys, &field("scalekey"))?;
            validate_vec3_keys(&node.color_keys, &field("colorkey"))?;
            validate_scalar_keys(&node.radius_keys, &field("radiuskey"))?;
            validate_scalar_keys(&node.alpha_keys, &field("alphakey"))?;
            validate_vec3_keys(&node.self_illum_color_keys, &field("selfillumcolorkey"))?;
            validate_scalar_keys(&node.multiplier_keys, &field("multiplierkey"))?;
            validate_scalar_keys(&node.shadow_radius_keys, &field("shadowradiuskey"))?;
            validate_scalar_keys(
                &node.vertical_displacement_keys,
                &field("verticaldisplacementkey"),
            )?;
            for controller in &node.emitter_controllers {
                validate_finite_values(
                    controller.keys.iter().flat_map(|key| {
                        std::iter::once(key.time).chain(key.values.iter().copied())
                    }),
                    &field(&format!("{}key", controller.name)),
                )?;
            }
            for controller in &node.opaque_controllers {
                validate_finite_values(
                    controller.keys.iter().flat_map(|key| {
                        std::iter::once(key.time).chain(key.values.iter().copied())
                    }),
                    &field(&format!("controller {}", controller.type_id)),
                )?;
            }
            validate_finite_values(
                node.animverts.iter().flatten().copied(),
                &field("animverts"),
            )?;
            validate_finite_values(
                node.animtverts.iter().flatten().copied(),
                &field("animtverts"),
            )?;
            if let Some(dangly) = &node.dangly {
                validate_optional_finite(dangly.displacement, &field("displacement"))?;
                validate_optional_finite(dangly.tightness, &field("tightness"))?;
                validate_optional_finite(dangly.period, &field("period"))?;
            }
        }
    }
    Ok(())
}

fn validate_optional_finite(value: Option<f32>, field: &str) -> ModelResult<()> {
    validate_finite_values(value, field)
}

fn validate_finite_values(values: impl IntoIterator<Item = f32>, field: &str) -> ModelResult<()> {
    if values.into_iter().all(f32::is_finite) {
        Ok(())
    } else {
        Err(ModelError::msg(format!(
            "{field} must contain only finite values"
        )))
    }
}

fn validate_scalar_keys(keys: &[ScalarKey], field: &str) -> ModelResult<()> {
    validate_finite_values(keys.iter().flat_map(|key| [key.time, key.value]), field)
}

fn validate_vec3_keys(keys: &[Vec3Key], field: &str) -> ModelResult<()> {
    validate_finite_values(
        keys.iter()
            .flat_map(|key| std::iter::once(key.time).chain(key.value)),
        field,
    )
}

fn validate_vec4_keys(keys: &[Vec4Key], field: &str) -> ModelResult<()> {
    validate_finite_values(
        keys.iter()
            .flat_map(|key| std::iter::once(key.time).chain(key.value)),
        field,
    )
}

fn validate_resource_references(model: &SemanticModel) -> ModelResult<()> {
    if let Some(supermodel) = &model.header.supermodel {
        validate_resref(supermodel, "supermodel")?;
    }

    for node in &model.nodes {
        let field = |name: &str| format!("node {} {name}", node.name);
        if let Some(bitmap) = &node.material.bitmap {
            validate_resref(bitmap, &field("bitmap"))?;
        }
        if let Some(material) = &node.material.material_name {
            validate_resref(material, &field("materialname"))?;
        }
        for texture in &node.material.textures {
            if texture.index >= 3 {
                return Err(ModelError::msg(format!(
                    "node {} texture{} cannot be represented in compiled MDL",
                    node.name, texture.index
                )));
            }
            validate_resref(&texture.name, &field(&format!("texture{}", texture.index)))?;
        }
        if let Some(mesh) = &node.mesh {
            for (index, texture) in mesh.texture_names.iter().enumerate() {
                validate_resref(texture, &field(&format!("texturenames[{index}]")))?;
            }
        }
        if let Some(light) = &node.light {
            for (index, texture) in light.flare_textures.iter().enumerate() {
                validate_resref(texture, &field(&format!("flare texture {index}")))?;
            }
        }
        if let Some(emitter) = &node.emitter {
            if let Some(texture) = emitter_text(emitter, "texture") {
                validate_resref(&texture, &field("emitter texture"))?;
            }
            if let Some(chunk) = emitter_text(emitter, "chunkname") {
                validate_resref(&chunk, &field("emitter chunkname"))?;
            }
        }
        if let Some(reference) = &node.reference
            && let Some(referenced_model) = &reference.model
        {
            validate_resref(referenced_model, &field("referenced model"))?;
        }
    }

    Ok(())
}

fn validate_resref(value: &str, field: &str) -> ModelResult<()> {
    let length = crate::mdl::ascii::text::model_text_byte_len(value);
    if value.is_empty() || value.eq_ignore_ascii_case("NULL") || length <= RESREF_MAX_LENGTH {
        return Ok(());
    }
    Err(ModelError::msg(format!(
        "{field} is {length} bytes; NWN resource references permit at most {RESREF_MAX_LENGTH}"
    )))
}

fn validate_animation_tree(animation: &SemanticAnimation) -> ModelResult<()> {
    if animation.nodes.is_empty() {
        return Ok(());
    }
    let mut parents = BTreeMap::new();
    for node in &animation.nodes {
        let key = node.name.to_ascii_lowercase();
        parents
            .entry(key)
            .or_insert_with(|| node.parent.as_deref().map(str::to_ascii_lowercase));
    }
    let roots = parents.values().filter(|parent| parent.is_none()).count();
    if roots != 1 {
        return Err(ModelError::msg(format!(
            "animation {} requires exactly one root node; found {roots}",
            animation.name
        )));
    }
    for node in &animation.nodes {
        if let Some(parent) = &node.parent
            && !parents.contains_key(&parent.to_ascii_lowercase())
        {
            return Err(ModelError::msg(format!(
                "animation {} node {} references missing animation parent {}",
                animation.name, node.name, parent
            )));
        }
    }
    for name in parents.keys() {
        let mut current = Some(name.as_str());
        let mut visited = BTreeSet::new();
        while let Some(key) = current {
            if !visited.insert(key.to_string()) {
                return Err(ModelError::msg(format!(
                    "animation {} contains a parent cycle at node {}",
                    animation.name, name
                )));
            }
            current = parents.get(key).and_then(Option::as_deref);
        }
    }
    Ok(())
}

fn classification_code(classification: Option<&ModelClassification>) -> u8 {
    match classification {
        Some(ModelClassification::Effect) => 1,
        Some(ModelClassification::Tile) => 2,
        Some(ModelClassification::Character) => 4,
        Some(ModelClassification::Door) => 8,
        _ => 0,
    }
}

fn validate_lossless_compilation(model: &SemanticModel) -> ModelResult<()> {
    validate_resource_references(model)?;

    for element in &model.header.extras {
        let AsciiElement::Statement(statement) = element else {
            continue;
        };
        if !statement.keyword_is("filedependency") && !statement.keyword_is("filedependancy") {
            return unsupported_statement("model header", statement.keyword.as_str());
        }
    }
    reject_statements("model geometry", &model.geometry_extras)?;
    reject_statements(
        "between geometry and animations",
        &model.between_geometry_and_animations,
    )?;
    for (index, elements) in model.between_animations.iter().enumerate() {
        reject_statements(&format!("after animation {index}"), elements)?;
    }
    reject_statements("model suffix", &model.suffix)?;

    for node in &model.nodes {
        reject_statements_except(
            &format!("geometry node {}", node.name),
            &node.extras,
            |keyword| {
                (matches!(node.kind, NodeKind::Aabb) && keyword.eq_ignore_ascii_case("aabb"))
                    || (matches!(node.kind, NodeKind::Danglymesh)
                        && matches!(
                            keyword.to_ascii_lowercase().as_str(),
                            "danglymesh" | "displtype" | "gizmo" | "showdispl"
                        ))
                    || (matches!(
                        node.kind,
                        NodeKind::Trimesh | NodeKind::Skin | NodeKind::Animmesh
                    ) && matches!(
                        keyword.to_ascii_lowercase().as_str(),
                        "period"
                            | "tightness"
                            | "displacement"
                            | "displtype"
                            | "gizmo"
                            | "showdispl"
                    ))
                    || matches!(
                        keyword.to_ascii_lowercase().as_str(),
                        "dummy" | "node" | "ex3dorientation::do" | "ex3dorientation::end" | "data"
                    )
                    || keyword.to_ascii_lowercase().starts_with("objpos=")
            },
        )?;
    }
    for animation in &model.animations {
        reject_statements(&format!("animation {}", animation.name), &animation.extras)?;
        for node in &animation.nodes {
            reject_statements_except(
                &format!("animation {} node {}", animation.name, node.name),
                &node.extras,
                |keyword| {
                    is_engine_ignored_animation_mesh_statement(keyword)
                        || (matches!(node.kind, NodeKind::Emitter)
                            && is_engine_ignored_emitter_header_key(keyword))
                },
            )?;
        }
    }
    Ok(())
}

fn reject_lossy_source_diagnostics(model: &SemanticModel) -> ModelResult<()> {
    if let Some(diagnostic) = model.diagnostics.iter().find(|diagnostic| {
        matches!(
            diagnostic.kind,
            ModelDiagnosticKind::MalformedValue
                | ModelDiagnosticKind::MalformedPayloadRow
                | ModelDiagnosticKind::UnsupportedValue
        )
    }) {
        return Err(ModelError::msg(format!(
            "refusing lossy MDL compilation: {}",
            diagnostic.message
        )));
    }
    Ok(())
}

fn reject_statements(location: &str, elements: &[AsciiElement]) -> ModelResult<()> {
    reject_statements_except(location, elements, |_| false)
}

fn reject_statements_except(
    location: &str,
    elements: &[AsciiElement],
    allowed: impl Fn(&str) -> bool,
) -> ModelResult<()> {
    if let Some(statement) = elements.iter().find_map(|element| {
        let AsciiElement::Statement(statement) = element else {
            return None;
        };
        (!allowed(statement.keyword.as_str())).then_some(statement)
    }) {
        return unsupported_statement(location, statement.keyword.as_str());
    }
    Ok(())
}

fn is_engine_ignored_animation_mesh_statement(keyword: &str) -> bool {
    matches!(
        keyword.to_ascii_lowercase().as_str(),
        "ambient"
            | "bitmap"
            | "cliph"
            | "clipu"
            | "clipv"
            | "clipw"
            | "centerkey"
            | "colors"
            | "diffuse"
            | "endlist"
            | "gizmokey"
            | "lockaxeskey"
            | "multimaterial"
            | "normals"
            | "shininess"
            | "shadow"
            | "specular"
            | "tangents"
            | "texturenames"
            | "transparencyhint"
            | "tverts"
            | "tverts1"
            | "tverts2"
            | "tverts3"
            | "verts"
            | "weights"
    )
}

fn is_engine_ignored_emitter_header_key(keyword: &str) -> bool {
    let lower = keyword.to_ascii_lowercase();
    let Some(name) = lower.strip_suffix("key") else {
        return false;
    };
    matches!(
        name,
        "affectedbywind"
            | "blastlength"
            | "blastradius"
            | "bounce"
            | "chunky"
            | "inherit"
            | "inheritvel"
            | "inherit_local"
            | "inherit_part"
            | "lockaxes"
            | "loop"
            | "m_istinted"
            | "opacity"
            | "p2p"
            | "random"
            | "renderorder"
            | "spawntype"
            | "splat"
            | "twosidedtex"
            | "xgrid"
            | "ygrid"
    )
}

fn unsupported_statement<T>(location: &str, keyword: &str) -> ModelResult<T> {
    Err(ModelError::msg(format!(
        "refusing lossy MDL compilation: unsupported {keyword} statement in {location}"
    )))
}

fn normalized_skin_influences(row: &[SemanticSkinWeight]) -> Vec<SemanticSkinWeight> {
    if row.len() <= MAX_SKIN_INFLUENCES {
        return row.to_vec();
    }
    let mut influences = row.to_vec();
    influences.sort_by(|left, right| right.weight.total_cmp(&left.weight));
    influences.truncate(MAX_SKIN_INFLUENCES);
    let sum = influences
        .iter()
        .map(|influence| influence.weight)
        .sum::<f32>();
    if sum > 0.0 {
        for influence in &mut influences {
            influence.weight /= sum;
        }
    }
    influences
}

fn node_content(kind: &NodeKind) -> ModelResult<u32> {
    match kind {
        NodeKind::Dummy | NodeKind::Patch => Ok(1),
        NodeKind::Light => Ok(1 | 2),
        NodeKind::Emitter => Ok(1 | 4),
        NodeKind::Camera => Ok(1 | 8),
        NodeKind::Reference => Ok(1 | 16),
        NodeKind::Trimesh => Ok(1 | 32),
        NodeKind::Skin => Ok(1 | 32 | 64),
        NodeKind::Animmesh => Ok(1 | 32 | 128),
        NodeKind::Danglymesh => Ok(1 | 32 | 256),
        NodeKind::Aabb => Ok(1 | 32 | 512),
        NodeKind::Other(name) => Err(ModelError::msg(format!(
            "node type {name} cannot be represented in compiled MDL"
        ))),
    }
}

fn geometry_controllers(node: &SemanticNode) -> Vec<ControllerRows> {
    let mut rows = Vec::new();
    add_static_vec(
        &mut rows,
        POSITION_CONTROLLER.binary_id(),
        node.position.map(Vec::from),
    );
    add_static_vec(
        &mut rows,
        ORIENTATION_CONTROLLER.binary_id(),
        node.orientation
            .map(axis_angle_to_quaternion)
            .map(Vec::from),
    );
    add_static_vec(
        &mut rows,
        SCALE_CONTROLLER.binary_id(),
        node.scale.map(|value| vec![value]),
    );
    if node.mesh.is_some() {
        add_static_vec(
            &mut rows,
            SELF_ILLUM_COLOR_CONTROLLER.binary_id(),
            node.material.self_illum_color.map(Vec::from),
        );
        add_static_vec(
            &mut rows,
            ALPHA_CONTROLLER.binary_id(),
            node.material.alpha.map(|value| vec![value]),
        );
    }
    if let Some(light) = &node.light {
        add_static_vec(
            &mut rows,
            LIGHT_COLOR_CONTROLLER.binary_id(),
            node.color.map(Vec::from),
        );
        add_static_vec(
            &mut rows,
            LIGHT_RADIUS_CONTROLLER.binary_id(),
            node.radius.map(|value| vec![value]),
        );
        add_static_vec(
            &mut rows,
            LIGHT_MULTIPLIER_CONTROLLER.binary_id(),
            light.multiplier.map(|value| vec![value]),
        );
        add_static_vec(
            &mut rows,
            LIGHT_SHADOW_RADIUS_CONTROLLER.binary_id(),
            light.shadow_radius.map(|value| vec![value]),
        );
        add_static_vec(
            &mut rows,
            LIGHT_VERTICAL_DISPLACEMENT_CONTROLLER.binary_id(),
            light.vertical_displacement.map(|value| vec![value]),
        );
    }
    if let Some(emitter) = &node.emitter {
        for property in &emitter.properties {
            if let Some(type_id) = emitter_controller_definition(&property.name)
                .map(|definition| definition.binary_id(false))
            {
                let values = property
                    .values
                    .iter()
                    .filter_map(property_float)
                    .collect::<Vec<_>>();
                if !values.is_empty() {
                    add_static_vec(&mut rows, type_id, Some(values));
                }
            }
        }
        add_static_vec(
            &mut rows,
            emitter_controller_definition_for(NwnEmitterController::XSize).binary_id(false),
            emitter.x_size.map(|value| vec![value]),
        );
        add_static_vec(
            &mut rows,
            emitter_controller_definition_for(NwnEmitterController::YSize).binary_id(false),
            emitter.y_size.map(|value| vec![value]),
        );
    }
    add_opaque_controllers(&mut rows, &node.opaque_controllers);
    rows
}

fn animation_controllers(
    node: &SemanticAnimationNode,
    geometry_content: u32,
) -> ModelResult<Vec<ControllerRows>> {
    let mut rows = Vec::new();
    add_vec3_keys(
        &mut rows,
        POSITION_CONTROLLER.binary_id(),
        &node.position_keys,
        is_bezier(node, POSITION_CONTROLLER.name()),
    );
    if node.position_keys.is_empty() {
        add_static_vec(
            &mut rows,
            POSITION_CONTROLLER.binary_id(),
            node.position.map(Vec::from),
        );
    }
    add_vec4_keys(
        &mut rows,
        ORIENTATION_CONTROLLER.binary_id(),
        &node.orientation_keys,
        is_bezier(node, ORIENTATION_CONTROLLER.name()),
        true,
    );
    if node.orientation_keys.is_empty() {
        add_static_vec(
            &mut rows,
            ORIENTATION_CONTROLLER.binary_id(),
            node.orientation
                .map(axis_angle_to_quaternion)
                .map(Vec::from),
        );
    }
    add_scalar_keys(
        &mut rows,
        SCALE_CONTROLLER.binary_id(),
        &node.scale_keys,
        is_bezier(node, SCALE_CONTROLLER.name()),
    );
    if node.scale_keys.is_empty() {
        add_static_vec(
            &mut rows,
            SCALE_CONTROLLER.binary_id(),
            node.scale.map(|value| vec![value]),
        );
    }
    if geometry_content & 0x20 != 0 {
        add_vec3_keys(
            &mut rows,
            SELF_ILLUM_COLOR_CONTROLLER.binary_id(),
            &node.self_illum_color_keys,
            is_bezier(node, SELF_ILLUM_COLOR_CONTROLLER.name()),
        );
        if node.self_illum_color_keys.is_empty() {
            add_static_vec(
                &mut rows,
                SELF_ILLUM_COLOR_CONTROLLER.binary_id(),
                node.self_illum_color.map(Vec::from),
            );
        }
        add_scalar_keys(
            &mut rows,
            ALPHA_CONTROLLER.binary_id(),
            &node.alpha_keys,
            is_bezier(node, ALPHA_CONTROLLER.name()),
        );
        if node.alpha_keys.is_empty() {
            add_static_vec(
                &mut rows,
                ALPHA_CONTROLLER.binary_id(),
                node.alpha.map(|value| vec![value]),
            );
        }
    }
    if geometry_content & 0x02 != 0 {
        add_vec3_keys(
            &mut rows,
            LIGHT_COLOR_CONTROLLER.binary_id(),
            &node.color_keys,
            is_bezier(node, LIGHT_COLOR_CONTROLLER.name()),
        );
        if node.color_keys.is_empty() {
            add_static_vec(
                &mut rows,
                LIGHT_COLOR_CONTROLLER.binary_id(),
                node.color.map(Vec::from),
            );
        }
        add_scalar_keys(
            &mut rows,
            LIGHT_RADIUS_CONTROLLER.binary_id(),
            &node.radius_keys,
            is_bezier(node, LIGHT_RADIUS_CONTROLLER.name()),
        );
        if node.radius_keys.is_empty() {
            add_static_vec(
                &mut rows,
                LIGHT_RADIUS_CONTROLLER.binary_id(),
                node.radius.map(|value| vec![value]),
            );
        }
        add_scalar_keys(
            &mut rows,
            LIGHT_MULTIPLIER_CONTROLLER.binary_id(),
            &node.multiplier_keys,
            is_bezier(node, LIGHT_MULTIPLIER_CONTROLLER.name()),
        );
        if node.multiplier_keys.is_empty() {
            add_static_vec(
                &mut rows,
                LIGHT_MULTIPLIER_CONTROLLER.binary_id(),
                node.multiplier.map(|value| vec![value]),
            );
        }
        add_scalar_keys(
            &mut rows,
            LIGHT_SHADOW_RADIUS_CONTROLLER.binary_id(),
            &node.shadow_radius_keys,
            is_bezier(node, LIGHT_SHADOW_RADIUS_CONTROLLER.name()),
        );
        if node.shadow_radius_keys.is_empty() {
            add_static_vec(
                &mut rows,
                LIGHT_SHADOW_RADIUS_CONTROLLER.binary_id(),
                node.shadow_radius.map(|value| vec![value]),
            );
        }
        add_scalar_keys(
            &mut rows,
            LIGHT_VERTICAL_DISPLACEMENT_CONTROLLER.binary_id(),
            &node.vertical_displacement_keys,
            is_bezier(node, LIGHT_VERTICAL_DISPLACEMENT_CONTROLLER.name()),
        );
        if node.vertical_displacement_keys.is_empty() {
            add_static_vec(
                &mut rows,
                LIGHT_VERTICAL_DISPLACEMENT_CONTROLLER.binary_id(),
                node.vertical_displacement.map(|value| vec![value]),
            );
        }
    }
    for controller in &node.emitter_controllers {
        let definition = emitter_controller_definition(&controller.name).ok_or_else(|| {
            ModelError::msg(format!(
                "emitter animation controller {} is not representable",
                controller.name
            ))
        })?;
        rows.push(ControllerRows {
            type_id: definition.binary_id(false),
            values:  controller
                .keys
                .iter()
                .map(|key| (key.time, key.values.clone()))
                .collect(),
            bezier:  controller.bezier_keyed,
        });
    }
    add_opaque_controllers(&mut rows, &node.opaque_controllers);
    Ok(rows)
}

fn add_opaque_controllers(
    rows: &mut Vec<ControllerRows>,
    controllers: &[crate::mdl::SemanticController],
) {
    rows.extend(controllers.iter().map(|controller| {
        ControllerRows {
            type_id: controller.type_id,
            values:  controller
                .keys
                .iter()
                .map(|key| (key.time, key.values.clone()))
                .collect(),
            bezier:  controller.bezier_keyed,
        }
    }));
}

fn add_static_vec(rows: &mut Vec<ControllerRows>, type_id: i32, value: Option<Vec<f32>>) {
    if let Some(value) = value {
        rows.push(ControllerRows {
            type_id,
            values: vec![(0.0, value)],
            bezier: false,
        });
    }
}

fn add_scalar_keys(rows: &mut Vec<ControllerRows>, id: i32, keys: &[ScalarKey], bezier: bool) {
    if !keys.is_empty() {
        rows.push(ControllerRows {
            type_id: id,
            values: keys.iter().map(|key| (key.time, vec![key.value])).collect(),
            bezier,
        });
    }
}

fn add_vec3_keys(rows: &mut Vec<ControllerRows>, id: i32, keys: &[Vec3Key], bezier: bool) {
    if !keys.is_empty() {
        rows.push(ControllerRows {
            type_id: id,
            values: keys
                .iter()
                .map(|key| (key.time, key.value.to_vec()))
                .collect(),
            bezier,
        });
    }
}

fn add_vec4_keys(
    rows: &mut Vec<ControllerRows>,
    id: i32,
    keys: &[Vec4Key],
    bezier: bool,
    quaternion: bool,
) {
    if !keys.is_empty() {
        rows.push(ControllerRows {
            type_id: id,
            values: keys
                .iter()
                .map(|key| {
                    let value = if quaternion {
                        axis_angle_to_quaternion(key.value)
                    } else {
                        key.value
                    };
                    (key.time, value.to_vec())
                })
                .collect(),
            bezier,
        });
    }
}

fn is_bezier(node: &SemanticAnimationNode, name: &str) -> bool {
    node.bezier_controllers
        .iter()
        .any(|controller| controller.eq_ignore_ascii_case(name))
}

fn expand_mesh(mesh: &SemanticMesh, generate_missing_tangents: bool) -> ModelResult<ExpandedMesh> {
    let layers = ordered_uv_layers(&mesh.uv_layers)?;
    let mut expanded = ExpandedMesh {
        positions:           Vec::new(),
        normals:             Vec::new(),
        tangents:            Vec::new(),
        colors:              Vec::new(),
        uv_layers:           vec![Vec::new(); layers.len()],
        original_vertices:   Vec::new(),
        original_uvs:        Vec::new(),
        source_vertex_count: mesh.vertices.len(),
        source_uv_count:     layers.first().map_or(0, |layer| layer.coordinates.len()),
        face_vertices:       Vec::with_capacity(mesh.faces.len() + 1),
    };
    if mesh.faces.is_empty() {
        return Ok(expanded);
    }

    let generated_normals = generate_corner_normals(mesh)?;
    let use_authored_normals = mesh.normals.len() >= mesh.vertices.len();
    let mut vertex_cache = BTreeMap::<Vec<u32>, u16>::new();
    for (face_index, face) in mesh.faces.iter().enumerate() {
        let mut face_vertices = [0u16; 3];
        for (corner, face_vertex) in face_vertices.iter_mut().enumerate() {
            let original = usize::try_from(face.vertex_indices[corner])
                .map_err(|_| ModelError::msg("mesh vertex index exceeds usize"))?;
            let position = mesh.vertices.get(original).copied().ok_or_else(|| {
                ModelError::msg(format!(
                    "face {face_index} references missing vertex {original}"
                ))
            })?;
            let normal = if use_authored_normals {
                mesh.normals[original]
            } else {
                generated_normals[face_index][corner]
            };
            let tangent = tangent_row(mesh.tangents.get(original));
            let color = color_row(mesh.colors.get(original));
            let uv_index = usize::try_from(face.uv_indices[corner])
                .map_err(|_| ModelError::msg("mesh UV index exceeds usize"))?;
            let uv_values = layers
                .iter()
                .map(|layer| {
                    layer.coordinates.get(uv_index).copied().ok_or_else(|| {
                        ModelError::msg(format!(
                            "face {face_index} references missing UV {uv_index} in layer {}",
                            layer.index
                        ))
                    })
                })
                .collect::<ModelResult<Vec<_>>>()?;
            let key = vertex_key(
                position,
                normal,
                color,
                &uv_values,
                face.group,
                face.material_index,
            );
            if let Some(existing) = vertex_cache.get(&key) {
                *face_vertex = *existing;
                continue;
            }
            let output_index = expanded.positions.len();
            let gpu_index = checked_u16(output_index, "expanded face vertex index")?;
            vertex_cache.insert(key, gpu_index);
            *face_vertex = gpu_index;
            expanded.positions.push(position);
            expanded.original_vertices.push(original);
            expanded.original_uvs.push(uv_index);
            expanded.normals.push(normal);
            if let Some(tangent) = tangent {
                expanded.tangents.push(tangent);
            }
            if let Some(color) = color {
                expanded.colors.push(color);
            }
            for (output, value) in expanded.uv_layers.iter_mut().zip(uv_values) {
                output.push(value);
            }
        }
        expanded.face_vertices.push(face_vertices);
    }
    normalize_optional_streams(&mut expanded);
    if generate_missing_tangents && expanded.tangents.is_empty() && !expanded.uv_layers.is_empty() {
        expanded.tangents = generate_tangents(&expanded);
    }
    Ok(expanded)
}

fn expand_animation_samples(
    animation: Option<&SemanticAnimationNode>,
    expanded: Option<&ExpandedMesh>,
) -> ModelResult<ExpandedAnimationSamples> {
    let (Some(animation), Some(expanded)) = (animation, expanded) else {
        return Ok(ExpandedAnimationSamples {
            vertices:     Vec::new(),
            vertex_sets:  0,
            textures:     Vec::new(),
            texture_sets: 0,
        });
    };
    let source_vertex_count = expanded.source_vertex_count;
    let source_uv_count = expanded.source_uv_count;

    let (vertices, vertex_sets) = if animation.animverts.is_empty() {
        (Vec::new(), 0)
    } else {
        if source_vertex_count == 0 || animation.animverts.len() % source_vertex_count != 0 {
            return Err(ModelError::msg(format!(
                "animation node {} animverts count {} is not a multiple of base vertex count {}",
                animation.name,
                animation.animverts.len(),
                source_vertex_count
            )));
        }
        let set_count = animation.animverts.len() / source_vertex_count;
        let mut output = Vec::with_capacity(set_count * expanded.original_vertices.len());
        for sample in animation.animverts.chunks_exact(source_vertex_count) {
            output.extend(
                expanded
                    .original_vertices
                    .iter()
                    .map(|index| sample[*index]),
            );
        }
        (output, checked_u32(set_count, "animmesh vertex-set count")?)
    };

    let (textures, texture_sets) = if animation.animtverts.is_empty() {
        (Vec::new(), 0)
    } else {
        if source_uv_count == 0 || animation.animtverts.len() % source_uv_count != 0 {
            return Err(ModelError::msg(format!(
                "animation node {} animtverts count {} is not a multiple of base UV count {}",
                animation.name,
                animation.animtverts.len(),
                source_uv_count
            )));
        }
        let set_count = animation.animtverts.len() / source_uv_count;
        let mut output = Vec::with_capacity(set_count * expanded.original_uvs.len());
        for sample in animation.animtverts.chunks_exact(source_uv_count) {
            output.extend(expanded.original_uvs.iter().map(|index| sample[*index]));
        }
        (output, checked_u32(set_count, "animmesh UV-set count")?)
    };
    Ok(ExpandedAnimationSamples {
        vertices,
        vertex_sets,
        textures,
        texture_sets,
    })
}

fn ordered_uv_layers(layers: &[SemanticUvLayer]) -> ModelResult<Vec<&SemanticUvLayer>> {
    let mut output = layers.iter().collect::<Vec<_>>();
    output.sort_by_key(|layer| layer.index);
    if output.len() > 4 {
        return Err(ModelError::msg(
            "compiled MDL supports at most four UV layers",
        ));
    }
    if output
        .iter()
        .enumerate()
        .any(|(index, layer)| layer.index != index)
    {
        return Err(ModelError::msg(
            "compiled MDL UV layers must be contiguous starting at layer 0",
        ));
    }
    Ok(output)
}

fn normalize_optional_streams(mesh: &mut ExpandedMesh) {
    if mesh.normals.len() != mesh.positions.len() {
        mesh.normals.clear();
    }
    if mesh.tangents.len() != mesh.positions.len() {
        mesh.tangents.clear();
    }
    if mesh.colors.len() != mesh.positions.len() {
        mesh.colors.clear();
    }
}

fn tangent_row(row: Option<&Vec<f32>>) -> Option<[f32; 4]> {
    let row = row?;
    (row.len() >= 3).then(|| [row[0], row[1], row[2], row.get(3).copied().unwrap_or(1.0)])
}

fn color_row(row: Option<&Vec<f32>>) -> Option<[f32; 4]> {
    let row = row?;
    (row.len() >= 3).then(|| [row[0], row[1], row[2], row.get(3).copied().unwrap_or(1.0)])
}

fn vertex_key(
    position: [f32; 3],
    normal: [f32; 3],
    color: Option<[f32; 4]>,
    uvs: &[[f32; 2]],
    smoothing_group: i32,
    material: i32,
) -> Vec<u32> {
    let mut key = Vec::with_capacity(3 + 3 + 4 + uvs.len() * 2 + 2);
    key.extend(position.map(f32::to_bits));
    key.extend(normal.map(f32::to_bits));
    key.extend(color.unwrap_or([0.0; 4]).map(f32::to_bits));
    for uv in uvs {
        key.extend(uv.map(f32::to_bits));
    }
    key.push(smoothing_group as u32);
    key.push(material as u32);
    key
}

fn generate_corner_normals(mesh: &SemanticMesh) -> ModelResult<Vec<[[f32; 3]; 3]>> {
    let mut face_normals = Vec::with_capacity(mesh.faces.len());
    for (face_index, face) in mesh.faces.iter().enumerate() {
        let mut points = [[0.0; 3]; 3];
        for (corner, index) in face.vertex_indices.into_iter().enumerate() {
            points[corner] = mesh.vertices.get(index as usize).copied().ok_or_else(|| {
                ModelError::msg(format!(
                    "face {face_index} references missing vertex {index}"
                ))
            })?;
        }
        face_normals.push(cross(
            sub3(points[1], points[0]),
            sub3(points[2], points[1]),
        ));
    }
    let mut output = vec![[[0.0; 3]; 3]; mesh.faces.len()];
    for (face_index, face) in mesh.faces.iter().enumerate() {
        for (corner, vertex) in face.vertex_indices.iter().enumerate() {
            if face.group == 0 {
                output[face_index][corner] =
                    normalized_f64(face_normals[face_index].map(f64::from));
                continue;
            }
            let mut sum = [0.0_f64; 3];
            for (other_index, other) in mesh.faces.iter().enumerate() {
                if other.group & face.group != 0 && other.vertex_indices.contains(vertex) {
                    for axis in 0..3 {
                        sum[axis] += f64::from(face_normals[other_index][axis]);
                    }
                }
            }
            output[face_index][corner] = normalized_f64(sum);
        }
    }
    Ok(output)
}

fn normalized_f64(value: [f64; 3]) -> [f32; 3] {
    let length = (value[0] * value[0] + value[1] * value[1] + value[2] * value[2]).sqrt();
    if length > 1.0e-10 {
        [
            (value[0] / length) as f32,
            (value[1] / length) as f32,
            (value[2] / length) as f32,
        ]
    } else {
        [0.0, 0.0, 1.0]
    }
}

fn generate_tangents(mesh: &ExpandedMesh) -> Vec<[f32; 4]> {
    let mut tangent_sums = vec![[0.0_f64; 3]; mesh.positions.len()];
    let mut bitangent_sums = vec![[0.0_f64; 3]; mesh.positions.len()];
    let uvs = &mesh.uv_layers[0];
    for vertices in &mesh.face_vertices {
        let [a, b, c] = vertices.map(usize::from);
        let edge1 = sub3(mesh.positions[b], mesh.positions[a]).map(f64::from);
        let edge2 = sub3(mesh.positions[c], mesh.positions[a]).map(f64::from);
        let duv1 = [
            f64::from(uvs[b][0] - uvs[a][0]),
            f64::from(uvs[b][1] - uvs[a][1]),
        ];
        let duv2 = [
            f64::from(uvs[c][0] - uvs[a][0]),
            f64::from(uvs[c][1] - uvs[a][1]),
        ];
        let determinant = duv1[0] * duv2[1] - duv1[1] * duv2[0];
        if determinant.abs() < 1.0e-20 {
            continue;
        }
        let inverse = determinant.recip();
        let tangent = [
            inverse * (duv2[1] * edge1[0] - duv1[1] * edge2[0]),
            inverse * (duv2[1] * edge1[1] - duv1[1] * edge2[1]),
            inverse * (duv2[1] * edge1[2] - duv1[1] * edge2[2]),
        ];
        let bitangent = [
            inverse * (-duv2[0] * edge1[0] + duv1[0] * edge2[0]),
            inverse * (-duv2[0] * edge1[1] + duv1[0] * edge2[1]),
            inverse * (-duv2[0] * edge1[2] + duv1[0] * edge2[2]),
        ];
        if tangent
            .iter()
            .chain(&bitangent)
            .any(|value| !value.is_finite())
        {
            continue;
        }
        for index in [a, b, c] {
            for axis in 0..3 {
                tangent_sums[index][axis] += tangent[axis];
                bitangent_sums[index][axis] += bitangent[axis];
            }
        }
    }
    tangent_sums
        .into_iter()
        .zip(bitangent_sums)
        .enumerate()
        .map(|(index, (mut tangent, bitangent))| {
            let normal = mesh.normals[index].map(f64::from);
            let normal_dot_tangent =
                normal[0] * tangent[0] + normal[1] * tangent[1] + normal[2] * tangent[2];
            for axis in 0..3 {
                tangent[axis] -= normal[axis] * normal_dot_tangent;
            }
            let mut length =
                (tangent[0] * tangent[0] + tangent[1] * tangent[1] + tangent[2] * tangent[2])
                    .sqrt();
            if length < 1.0e-12 {
                tangent = orthogonal_axis(normal);
                length =
                    (tangent[0] * tangent[0] + tangent[1] * tangent[1] + tangent[2] * tangent[2])
                        .sqrt();
            }
            if length < 1.0e-12 {
                tangent = [1.0, 0.0, 0.0];
            } else {
                for value in &mut tangent {
                    *value /= length;
                }
            }
            let normal_cross_tangent = [
                normal[1] * tangent[2] - normal[2] * tangent[1],
                normal[2] * tangent[0] - normal[0] * tangent[2],
                normal[0] * tangent[1] - normal[1] * tangent[0],
            ];
            let handedness = if normal_cross_tangent[0] * bitangent[0]
                + normal_cross_tangent[1] * bitangent[1]
                + normal_cross_tangent[2] * bitangent[2]
                < 0.0
            {
                -1.0
            } else {
                1.0
            };
            [
                tangent[0] as f32,
                tangent[1] as f32,
                tangent[2] as f32,
                handedness,
            ]
        })
        .collect()
}

fn orthogonal_axis(normal: [f64; 3]) -> [f64; 3] {
    let [x, y, z] = normal;
    let [abs_x, abs_y, abs_z] = normal.map(f64::abs);
    if abs_x <= abs_y && abs_x <= abs_z {
        [0.0, z, -y]
    } else if abs_y <= abs_z {
        [-z, 0.0, x]
    } else {
        [y, -x, 0.0]
    }
}

fn texture_slots(node: &SemanticNode) -> [Option<String>; 4] {
    let mut output = [None, None, None, node.material.material_name.clone()];
    output[0] = node.material.bitmap.clone();
    for texture in &node.material.textures {
        if texture.index < 3 {
            output[texture.index] = Some(texture.name.clone());
        }
    }
    output
}

fn face_adjacency(faces: &[crate::mdl::SemanticFace]) -> Vec<[u16; 3]> {
    let mut adjacency = vec![[u16::MAX; 3]; faces.len()];
    let mut edges = BTreeMap::<(u32, u32), (usize, usize)>::new();
    for (face_index, face) in faces.iter().enumerate() {
        for edge_index in 0..3 {
            let a = face.vertex_indices[edge_index];
            let b = face.vertex_indices[(edge_index + 1) % 3];
            let edge = if a < b { (a, b) } else { (b, a) };
            if let Some((other_face, other_edge)) = edges.remove(&edge) {
                if let (Ok(left), Ok(right)) =
                    (u16::try_from(other_face), u16::try_from(face_index))
                {
                    adjacency[face_index][edge_index] = left;
                    adjacency[other_face][other_edge] = right;
                }
            } else {
                edges.insert(edge, (face_index, edge_index));
            }
        }
    }
    adjacency
}

#[derive(Clone)]
struct AabbEntry {
    min:   [f32; 3],
    max:   [f32; 3],
    left:  Option<usize>,
    right: Option<usize>,
    face:  Option<usize>,
    plane: u32,
}

fn build_aabb_tree(mesh: &SemanticMesh) -> ModelResult<Vec<AabbEntry>> {
    let mut output = Vec::new();
    let faces = (0..mesh.faces.len()).collect::<Vec<_>>();
    build_aabb_node(mesh, &faces, &mut output)?;
    Ok(output)
}

fn build_aabb_node(
    mesh: &SemanticMesh,
    faces: &[usize],
    output: &mut Vec<AabbEntry>,
) -> ModelResult<usize> {
    let points = faces
        .iter()
        .flat_map(|face| mesh.faces[*face].vertex_indices)
        .map(|index| {
            mesh.vertices
                .get(index as usize)
                .copied()
                .ok_or_else(|| ModelError::msg(format!("AABB references missing vertex {index}")))
        })
        .collect::<ModelResult<Vec<_>>>()?;
    let (min, max, _, _) = bounds(&points);
    let axis = longest_axis(min, max);
    let index = output.len();
    output.push(AabbEntry {
        min,
        max,
        left: None,
        right: None,
        face: None,
        plane: 1 << axis,
    });
    if faces.len() == 1 {
        output[index].face = Some(faces[0]);
        return Ok(index);
    }
    let mut sorted = faces.to_vec();
    sorted.sort_by(|left, right| {
        face_centroid(mesh, *left, axis).total_cmp(&face_centroid(mesh, *right, axis))
    });
    let middle = sorted.len() / 2;
    output[index].left = Some(build_aabb_node(mesh, &sorted[..middle], output)?);
    output[index].right = Some(build_aabb_node(mesh, &sorted[middle..], output)?);
    Ok(index)
}

fn face_centroid(mesh: &SemanticMesh, face: usize, axis: usize) -> f32 {
    mesh.faces[face]
        .vertex_indices
        .iter()
        .filter_map(|index| mesh.vertices.get(*index as usize))
        .map(|vertex| vertex[axis])
        .sum::<f32>()
        / 3.0
}

fn longest_axis(min: [f32; 3], max: [f32; 3]) -> usize {
    let size = sub3(max, min);
    if size[0] >= size[1] && size[0] >= size[2] {
        0
    } else if size[1] >= size[2] {
        1
    } else {
        2
    }
}

fn emitter_property<'a>(
    emitter: &'a SemanticEmitter,
    name: &str,
) -> Option<&'a [SemanticPropertyValue]> {
    emitter
        .properties
        .iter()
        .find(|property| property.name.eq_ignore_ascii_case(name))
        .map(|property| property.values.as_slice())
}

fn emitter_float(emitter: &SemanticEmitter, name: &str) -> Option<f32> {
    emitter_property(emitter, name)
        .and_then(|values| values.first())
        .and_then(property_float)
}

fn emitter_u32(emitter: &SemanticEmitter, name: &str) -> Option<u32> {
    emitter_property(emitter, name)
        .and_then(|values| values.first())
        .and_then(|value| match value {
            SemanticPropertyValue::Bool(value) => Some(u32::from(*value)),
            SemanticPropertyValue::Int(value) => u32::try_from(*value).ok(),
            SemanticPropertyValue::Float(value) if *value >= 0.0 => Some(*value as u32),
            SemanticPropertyValue::Text(value) => value.parse().ok(),
            _ => None,
        })
}

fn emitter_bool(emitter: &SemanticEmitter, name: &str) -> Option<bool> {
    emitter_property(emitter, name)
        .and_then(|values| values.first())
        .and_then(|value| match value {
            SemanticPropertyValue::Bool(value) => Some(*value),
            SemanticPropertyValue::Int(value) => Some(*value != 0),
            SemanticPropertyValue::Float(value) => Some(*value != 0.0),
            SemanticPropertyValue::Text(value) if value.eq_ignore_ascii_case("true") => Some(true),
            SemanticPropertyValue::Text(value) if value.eq_ignore_ascii_case("false") => {
                Some(false)
            }
            SemanticPropertyValue::Text(value) => value.parse::<i32>().ok().map(|value| value != 0),
        })
}

fn emitter_text(emitter: &SemanticEmitter, name: &str) -> Option<String> {
    emitter_property(emitter, name)
        .and_then(|values| values.first())
        .map(|value| match value {
            SemanticPropertyValue::Bool(value) => value.to_string(),
            SemanticPropertyValue::Int(value) => value.to_string(),
            SemanticPropertyValue::Float(value) => value.to_string(),
            SemanticPropertyValue::Text(value) => value.clone(),
        })
}

fn property_float(value: &SemanticPropertyValue) -> Option<f32> {
    match value {
        SemanticPropertyValue::Bool(value) => Some(f32::from(u8::from(*value))),
        SemanticPropertyValue::Int(value) => Some(*value as f32),
        SemanticPropertyValue::Float(value) => Some(*value),
        SemanticPropertyValue::Text(value) => value.parse().ok(),
    }
}

fn parent_key(parent: Option<&str>) -> String {
    parent
        .filter(|value| !value.eq_ignore_ascii_case("NULL"))
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn checked_u16(value: usize, field: &str) -> ModelResult<u16> {
    u16::try_from(value).map_err(|_| ModelError::msg(format!("{field} exceeds u16")))
}

fn checked_u32(value: usize, field: &str) -> ModelResult<u32> {
    u32::try_from(value).map_err(|_| ModelError::msg(format!("{field} exceeds u32")))
}

fn checked_i32(value: usize, field: &str) -> ModelResult<i32> {
    i32::try_from(value).map_err(|_| ModelError::msg(format!("{field} exceeds i32")))
}

fn float_to_byte(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn axis_angle_to_quaternion(value: [f32; 4]) -> [f32; 4] {
    let axis = normalize([value[0], value[1], value[2]]);
    if dot(axis, axis) <= f32::EPSILON {
        return [0.0, 0.0, 0.0, 1.0];
    }
    let half = value[3] * 0.5;
    let sin = half.sin();
    [axis[0] * sin, axis[1] * sin, axis[2] * sin, half.cos()]
}

fn quaternion_multiply(left: [f32; 4], right: [f32; 4]) -> [f32; 4] {
    [
        left[3] * right[0] + left[0] * right[3] + left[1] * right[2] - left[2] * right[1],
        left[3] * right[1] - left[0] * right[2] + left[1] * right[3] + left[2] * right[0],
        left[3] * right[2] + left[0] * right[1] - left[1] * right[0] + left[2] * right[3],
        left[3] * right[3] - left[0] * right[0] - left[1] * right[1] - left[2] * right[2],
    ]
}

fn quaternion_rotate(rotation: [f32; 4], point: [f32; 3]) -> [f32; 3] {
    let vector = [point[0], point[1], point[2], 0.0];
    let inverse = [-rotation[0], -rotation[1], -rotation[2], rotation[3]];
    let rotated = quaternion_multiply(quaternion_multiply(rotation, vector), inverse);
    [rotated[0], rotated[1], rotated[2]]
}

fn bounds(points: &[[f32; 3]]) -> ([f32; 3], [f32; 3], [f32; 3], f32) {
    if points.is_empty() {
        return ([0.0; 3], [0.0; 3], [0.0; 3], 0.0);
    }
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    let mut average = [0.0; 3];
    for point in points {
        for axis in 0..3 {
            min[axis] = min[axis].min(point[axis]);
            max[axis] = max[axis].max(point[axis]);
            average[axis] += point[axis];
        }
    }
    average = scale3(average, (points.len() as f32).recip());
    let radius = points
        .iter()
        .map(|point| dot(sub3(*point, average), sub3(*point, average)).sqrt())
        .fold(0.0, f32::max);
    (min, max, average, radius)
}

fn add3(left: [f32; 3], right: [f32; 3]) -> [f32; 3] {
    [left[0] + right[0], left[1] + right[1], left[2] + right[2]]
}

fn sub3(left: [f32; 3], right: [f32; 3]) -> [f32; 3] {
    [left[0] - right[0], left[1] - right[1], left[2] - right[2]]
}

fn scale3(value: [f32; 3], scale: f32) -> [f32; 3] {
    [value[0] * scale, value[1] * scale, value[2] * scale]
}

fn dot(left: [f32; 3], right: [f32; 3]) -> f32 {
    left[0] * right[0] + left[1] * right[1] + left[2] * right[2]
}

fn cross(left: [f32; 3], right: [f32; 3]) -> [f32; 3] {
    [
        left[1] * right[2] - left[2] * right[1],
        left[2] * right[0] - left[0] * right[2],
        left[0] * right[1] - left[1] * right[0],
    ]
}

fn normalize(value: [f32; 3]) -> [f32; 3] {
    let length = dot(value, value).sqrt();
    if length <= f32::EPSILON {
        [0.0; 3]
    } else {
        scale3(value, length.recip())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use crate::mdl::{
        SemanticController, SemanticControllerKey, SemanticEmitterController, SemanticEmitterKey,
        SemanticSkinWeight, compile_ascii_model, compile_semantic_model,
        controllers::EMITTER_CONTROLLER_DEFINITIONS, lower_ascii_model, lower_binary_model,
        lower_binary_model_to_ascii, parse_ascii_model, parse_binary_model_bytes,
    };

    #[test]
    fn compiles_trimesh_and_animation_to_parseable_binary() {
        let ascii = parse_ascii_model(
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
  tverts 3
    0 0 0
    1 0 0
    0 1 0
  faces 1
    0 1 2 1 0 1 2 0
endnode
endmodelgeom demo
newanim idle demo
  length 1
  transtime 0.25
  animroot demo
  node dummy demo
    parent NULL
    positionkey 2
      0 0 0 0
      1 1 0 0
  endnode
doneanim idle demo
donemodel demo
",
        )
        .expect("parse ASCII fixture");
        let binary = compile_ascii_model(&ascii).expect("compile ASCII fixture");
        assert_eq!(binary.name, "demo");
        assert_eq!(binary.nodes.len(), 2);
        assert_eq!(binary.animations.len(), 1);
        assert_eq!(binary.nodes[1].mesh.as_ref().unwrap().vertices.len(), 3);
        let semantic = lower_binary_model(&binary).expect("lower compiled fixture");
        assert_eq!(semantic.animations[0].nodes[0].position_keys.len(), 2);
    }

    #[test]
    fn rejects_strings_instead_of_silently_truncating_them() {
        let name = "x".repeat(32);
        let source = format!(
            "newmodel demo\nbeginmodelgeom demo\nnode dummy {name}\nparent \
             NULL\nendnode\nendmodelgeom demo\ndonemodel demo\n"
        );
        let ascii = parse_ascii_model(&source).expect("parse long-name fixture");
        let error = compile_ascii_model(&ascii).expect_err("long name should fail");
        assert!(error.to_string().contains("at most 31"));
    }

    #[test]
    fn compiles_specialized_node_payloads() {
        let ascii = parse_ascii_model(
            "\
newmodel special
setsupermodel special null
classification effect
ignorefog 1
beginmodelgeom special
node dummy special
  parent NULL
endnode
node dummy bone
  parent special
endnode
node light lamp
  parent special
  color 1 0.5 0.25
  radius 12
  multiplier 2
  shadowradius 3
  verticaldisplacement 4
  lensflares 1
  flareradius 1.5
  texturenames 1
    flare01
  flaresizes 1
    0.5
  flarepositions 1
    0.25
  flarecolorshifts 1
    1 1 1
endnode
node emitter sparks
  parent special
  update Fountain
  render Normal
  blend Lighten
  texture fxpa_default
  birthrate 10
  alphamid 0.5
  colormid 1 0 0
  lightningsubdiv 4
endnode
node reference linked
  parent special
  refmodel plc_chest1
  reattachable 1
endnode
node camera view
  parent special
endnode
node skin body
  parent special
  bitmap blank
  verts 3
    0 0 0
    1 0 0
    0 1 0
  tverts 3
    0 0 0
    1 0 0
    0 1 0
  faces 1
    0 1 2 1 0 1 2 0
  weights 3
    bone 1
    bone 1
    bone 1
endnode
node danglymesh cloth
  parent special
  displacement 0.5
  tightness 10
  period 1
  verts 3
    0 0 0
    1 0 0
    0 1 0
  tverts 3
    0 0 0
    1 0 0
    0 1 0
  faces 1
    0 1 2 1 0 1 2 0
  constraints 3
    0.25
    0.5
    1
endnode
node aabb walk
  parent special
  bitmap NULL
  verts 3
    0 0 0
    1 0 0
    0 1 0
  faces 1
    0 1 2 1 0 0 0 7
endnode
endmodelgeom special
donemodel special
",
        )
        .expect("parse specialized fixture");
        let binary = compile_ascii_model(&ascii).expect("compile specialized fixture");
        assert_eq!(binary.fog, 1);
        assert!(binary.node("lamp").unwrap().light.is_some());
        let emitter = binary.node("sparks").unwrap();
        assert!(emitter.emitter.is_some());
        assert!(
            emitter
                .controllers
                .iter()
                .any(|controller| controller.type_id == 448)
        );
        assert!(
            emitter
                .controllers
                .iter()
                .any(|controller| controller.type_id == 452)
        );
        let lowered = lower_binary_model(&binary).expect("lower specialized binary");
        let emitter_properties = &lowered
            .node("sparks")
            .unwrap()
            .emitter
            .as_ref()
            .unwrap()
            .properties;
        assert!(
            emitter_properties
                .iter()
                .any(|property| property.name == "alphamid")
        );
        assert!(
            !emitter_properties
                .iter()
                .any(|property| property.name == "percentstart")
        );
        assert_eq!(
            binary.node("view").unwrap().kind,
            crate::mdl::NodeKind::Camera
        );
        let skin = binary.node("body").unwrap().skin.as_ref().unwrap();
        assert_eq!(skin.bone_parts.len(), 64);
        assert_eq!(skin.vertex_weights.len(), 3);
        let dangly = binary.node("cloth").unwrap().dangly.as_ref().unwrap();
        assert_eq!(dangly.constraints, vec![0.25, 0.5, 1.0]);
        assert!(
            binary
                .node("walk")
                .unwrap()
                .aabb
                .as_ref()
                .unwrap()
                .root
                .is_some()
        );
    }

    #[test]
    fn rejects_unknown_statements_instead_of_discarding_them() {
        let ascii = parse_ascii_model(
            "newmodel demo\nbeginmodelgeom demo\nnode dummy demo\nparent NULL\nmystery \
             1\nendnode\nendmodelgeom demo\ndonemodel demo\n",
        )
        .expect("parse unknown statement fixture");
        let error = compile_ascii_model(&ascii).expect_err("unknown statement should fail");
        assert!(error.to_string().contains("unsupported mystery statement"));
    }

    #[test]
    fn keeps_four_strongest_skin_influences_and_renormalizes_them() {
        let row = [
            SemanticSkinWeight {
                bone:   "a".to_string(),
                weight: 0.05,
            },
            SemanticSkinWeight {
                bone:   "b".to_string(),
                weight: 0.4,
            },
            SemanticSkinWeight {
                bone:   "c".to_string(),
                weight: 0.1,
            },
            SemanticSkinWeight {
                bone:   "d".to_string(),
                weight: 0.3,
            },
            SemanticSkinWeight {
                bone:   "e".to_string(),
                weight: 0.15,
            },
        ];
        let normalized = super::normalized_skin_influences(&row);
        assert_eq!(normalized.len(), 4);
        assert_eq!(
            normalized
                .iter()
                .map(|influence| influence.bone.as_str())
                .collect::<Vec<_>>(),
            ["b", "d", "e", "c"]
        );
        let sum = normalized
            .iter()
            .map(|influence| influence.weight)
            .sum::<f32>();
        assert!((sum - 1.0).abs() < 1.0e-6);
    }

    #[test]
    fn rejects_meshes_over_the_ee_face_limit() {
        let ascii = parse_ascii_model(
            "newmodel demo\nbeginmodelgeom demo\nnode trimesh demo\nparent NULL\nverts 3\n0 0 \
             0\n1 0 0\n0 1 0\ntverts 3\n0 0 0\n1 0 0\n0 1 0\nfaces 1\n0 1 2 1 0 1 2 \
             0\nendnode\nendmodelgeom demo\ndonemodel demo\n",
        )
        .expect("parse face-limit fixture");
        let mut semantic = lower_ascii_model(&ascii).expect("lower face-limit fixture");
        let mesh = semantic.nodes[0].mesh.as_mut().expect("fixture mesh");
        mesh.faces
            .resize(super::EE_MAX_MESH_FACES + 1, mesh.faces[0].clone());
        let error = compile_semantic_model(&semantic).expect_err("face limit should fail");
        assert!(error.to_string().contains("permits at most 21845"));
    }

    #[test]
    fn aliases_duplicate_geometry_node_names_deterministically() {
        let ascii = parse_ascii_model(
            "newmodel demo\nbeginmodelgeom demo\nnode dummy demo\nparent NULL\nendnode\nnode \
             dummy child\nparent demo\nendnode\nnode dummy child\nparent \
             demo\nendnode\nendmodelgeom demo\ndonemodel demo\n",
        )
        .expect("parse duplicate-node fixture");
        let binary = compile_ascii_model(&ascii).expect("duplicate nodes should compile");
        assert_eq!(binary.node_count_hint, 2);
        let offsets = binary
            .nodes
            .iter()
            .filter(|node| node.name.eq_ignore_ascii_case("child"))
            .map(|node| node.offset)
            .collect::<BTreeSet<_>>();
        assert_eq!(offsets.len(), 1);
    }

    #[test]
    fn permits_animation_only_external_targets() {
        let ascii = parse_ascii_model(
            "newmodel demo\nbeginmodelgeom demo\nnode dummy demo\nparent \
             NULL\nendnode\nendmodelgeom demo\nnewanim idle demo\nlength 1\nanimroot demo\nnode \
             dummy missing\nparent NULL\nendnode\ndoneanim idle demo\ndonemodel demo\n",
        )
        .expect("parse missing-animation-target fixture");
        let binary = compile_ascii_model(&ascii).expect("external target should compile");
        assert_eq!(binary.animations[0].nodes[0].name, "missing");
    }

    #[test]
    fn aliases_duplicate_animation_nodes_deterministically() {
        let ascii = parse_ascii_model(
            "newmodel demo\nbeginmodelgeom demo\nnode dummy demo\nparent \
             NULL\nendnode\nendmodelgeom demo\nnewanim idle demo\nlength 1\nanimroot demo\nnode \
             dummy demo\nparent NULL\nendnode\nnode dummy child\nparent demo\nendnode\nnode dummy \
             child\nparent demo\nendnode\ndoneanim idle demo\ndonemodel demo\n",
        )
        .expect("parse duplicate animation-node fixture");
        let binary = compile_ascii_model(&ascii).expect("duplicate animation nodes should compile");
        assert_eq!(binary.animations[0].node_count_hint, 2);
        assert_eq!(
            binary.animations[0]
                .nodes
                .iter()
                .filter(|node| node.name == "child")
                .map(|node| node.offset)
                .collect::<BTreeSet<_>>()
                .len(),
            1
        );
    }

    #[test]
    fn compiles_surface_labels_geometry_sampleperiod_and_static_emitter_animation_values() {
        let ascii = parse_ascii_model(
            "newmodel demo\nbeginmodelgeom demo\nnode dummy demo\nparent NULL\nendnode\nnode \
             trimesh walk\nparent demo\nmultimaterial 1\n  \"Walkmesh - \
             Obscuring\"\nendnode\nnode animmesh animated\nparent demo\nsampleperiod \
             0.1\nendnode\nnode emitter sparks\nparent demo\nupdate \
             Fountain\nendnode\nendmodelgeom demo\nnewanim idle demo\nlength 1\nanimroot \
             demo\nnode dummy demo\nparent NULL\nendnode\nnode emitter sparks\nparent demo\nxsize \
             30\nbirthrate -4\nendnode\ndoneanim idle demo\ndonemodel demo\n",
        )
        .expect("parse compatibility fixture");
        let binary = compile_ascii_model(&ascii).expect("compile compatibility fixture");
        assert_eq!(
            binary
                .node("animated")
                .and_then(|node| node.animmesh.as_ref())
                .map(|animmesh| animmesh.sample_period),
            Some(0.1)
        );
        let sparks = binary.animations[0]
            .nodes
            .iter()
            .find(|node| node.name == "sparks")
            .expect("compiled sparks animation node");
        assert!(sparks.controllers.iter().any(|controller| {
            controller.type_id == 196 && controller.values == vec![vec![30.0]]
        }));
        assert!(sparks.controllers.iter().any(|controller| {
            controller.type_id == 88 && controller.values == vec![vec![-4.0]]
        }));
    }

    #[test]
    fn preserves_unknown_compiled_controllers_semantically() {
        let ascii = parse_ascii_model(
            "newmodel demo\nbeginmodelgeom demo\nnode dummy demo\nparent \
             NULL\nendnode\nendmodelgeom demo\ndonemodel demo\n",
        )
        .expect("parse opaque controller fixture");
        let mut semantic = lower_ascii_model(&ascii).expect("lower opaque controller fixture");
        semantic.nodes[0]
            .opaque_controllers
            .push(SemanticController {
                type_id:      144,
                bezier_keyed: true,
                keys:         vec![
                    SemanticControllerKey {
                        time:   0.0,
                        values: vec![2.0, 3.0],
                    },
                    SemanticControllerKey {
                        time:   1.0,
                        values: vec![4.0, 5.0],
                    },
                ],
            });

        let binary = compile_semantic_model(&semantic).expect("compile opaque controller");
        let lowered = lower_binary_model(&binary).expect("lower opaque controller");
        assert_eq!(
            lowered.nodes[0].opaque_controllers,
            semantic.nodes[0].opaque_controllers
        );
        let error = lower_binary_model_to_ascii(&binary)
            .expect_err("opaque controller must not be silently discarded from ASCII");
        assert!(error.to_string().contains("opaque compiled controllers"));
    }

    #[test]
    fn every_emitter_controller_uses_the_shared_schema_across_binary_roundtrip() {
        let ascii = parse_ascii_model(
            "newmodel demo\nbeginmodelgeom demo\nnode emitter sparks\nparent NULL\nxsize 3\nysize \
             4\nendnode\nendmodelgeom demo\nnewanim pulse demo\nlength 1\nanimroot sparks\nnode \
             emitter sparks\nparent NULL\nendnode\ndoneanim pulse demo\ndonemodel demo\n",
        )
        .expect("parse complete emitter-controller fixture");
        let mut semantic = lower_ascii_model(&ascii).expect("lower emitter-controller fixture");
        let animation_node = semantic.animations[0]
            .nodes
            .first_mut()
            .expect("fixture animation node");
        animation_node.emitter_controllers = EMITTER_CONTROLLER_DEFINITIONS
            .iter()
            .map(|definition| SemanticEmitterController {
                name:         definition.name().to_string(),
                bezier_keyed: false,
                keys:         vec![SemanticEmitterKey {
                    time:   0.0,
                    values: (0..definition.value_width)
                        .map(|index| index as f32 + 1.0)
                        .collect(),
                }],
            })
            .collect();
        let expected = animation_node.emitter_controllers.clone();

        let binary = compile_semantic_model(&semantic).expect("compile every emitter controller");
        let lowered = lower_binary_model(&binary).expect("lower every emitter controller");
        let lowered_node = lowered.animations[0]
            .nodes
            .first()
            .expect("lowered animation node");

        assert_eq!(lowered_node.emitter_controllers, expected);
        let lowered_emitter = lowered.nodes[0]
            .emitter
            .as_ref()
            .expect("lowered geometry emitter");
        assert_eq!(lowered_emitter.x_size, Some(3.0));
        assert_eq!(lowered_emitter.y_size, Some(4.0));
        assert!(!lowered_emitter.properties.iter().any(|property| {
            property.name.eq_ignore_ascii_case("xsize")
                || property.name.eq_ignore_ascii_case("ysize")
        }));

        let recompiled = compile_semantic_model(&lowered).expect("recompile shared schema model");
        let recompiled_emitter = recompiled
            .node("sparks")
            .expect("recompiled geometry emitter");
        assert_eq!(
            recompiled_emitter
                .controllers
                .iter()
                .filter(|controller| controller.type_id == 196)
                .count(),
            1
        );
        assert_eq!(
            recompiled_emitter
                .controllers
                .iter()
                .filter(|controller| controller.type_id == 200)
                .count(),
            1
        );
    }

    #[test]
    fn compiles_permissive_legacy_exporter_syntax() {
        let ascii = parse_ascii_model(
            "newmodel legacy_model_name_20\nbeginmodelgeom legacy_model_name_20\nnode dummy \
             legacy_model_name_20\nparent NULL\nendnode\nnode animmesh panel\nparent \
             legacy_model_name_20\nverts 3\n  0 0 0\n  1 0 0\n  0 1 0\nfaces 1\n  0 1 2 1 0 1 2 \
             0\ntverts 3\n  0 0 0\n  1 0 0\n  0 1 0\ntverts 3\n  0 0 0\n  1 0 0\n  0 1 \
             0\nsampleperiod 1.#INF\nendnodeendnode\nnode light lamp\nparent \
             legacy_model_name_20\nambient_only 0\nn_dynamic_type 0\nendnode\nnode light \
             empty_lamp\nparent legacy_model_name_20\nendnode\nendmodelgeom wrong_name\nnewanim \
             idle\n legacy_model_name_20\nlength 1\nanimroot legacy_model_name_20\nnode dummy \
             legacy_model_name_20\nparent NULL\nendnode\nnode animmesh panel\nparent \
             legacy_model_name_20\nshadow 0\ntransparencyhint 0\nclipu 0\nclipv 0\nclipw 1\ncliph \
             1\nendnode\nnode light lamp\nparent \
             legacy_model_name_20\ncolorkey\nendlist\nradiuskey\n0 1\nendlist\nendnode\ndoneanim \
             wrong_name\n legacy_model_name_20\ndonemodel also_wrong\n",
        )
        .expect("parse permissive legacy fixture");
        let binary = compile_ascii_model(&ascii).expect("compile permissive legacy fixture");
        assert_eq!(binary.name, "legacy_model_name_20");
        assert!(
            binary
                .node("panel")
                .and_then(|node| node.animmesh.as_ref())
                .is_some_and(|animmesh| animmesh.sample_period.is_infinite())
        );
        assert!(binary.node("empty_lamp").unwrap().light.is_some());
    }

    #[test]
    fn preserves_explicit_external_animation_targets_across_recompilation() {
        let ascii = parse_ascii_model(
            "newmodel demo\nbeginmodelgeom demo\nnode dummy demo\nparent \
             NULL\nendnode\nendmodelgeom demo\nnewanim idle demo\nlength 1\nanimroot \
             external\nnode dummy external\n#part-number -1\nparent NULL\npositionkey 1\n0 1 2 \
             3\nendnode\ndoneanim idle demo\ndonemodel demo\n",
        )
        .expect("parse external-animation fixture");
        let binary = compile_ascii_model(&ascii).expect("external target should compile");
        let target = binary.animations[0]
            .nodes
            .iter()
            .find(|node| node.name == "external")
            .expect("compiled external target");
        assert_eq!(target.part_number, Some(-1));

        let semantic = lower_binary_model(&binary).expect("lower compiled external target");
        let recompiled =
            compile_semantic_model(&semantic).expect("recompile lowered external target");
        assert_eq!(
            recompiled.animations[0]
                .nodes
                .iter()
                .find(|node| node.name == "external")
                .expect("recompiled external target")
                .part_number,
            Some(-1)
        );
    }

    #[test]
    fn rejects_non_finite_semantic_values() {
        let ascii = parse_ascii_model(
            "newmodel demo\nbeginmodelgeom demo\nnode dummy demo\nparent \
             NULL\nendnode\nendmodelgeom demo\ndonemodel demo\n",
        )
        .expect("parse finite-value fixture");
        let mut semantic = lower_ascii_model(&ascii).expect("lower finite-value fixture");
        semantic.nodes[0].material.alpha = Some(f32::NAN);
        let error = compile_semantic_model(&semantic).expect_err("NaN should fail");
        assert!(
            error
                .to_string()
                .contains("alpha must contain only finite values")
        );
    }

    #[test]
    fn rejects_resource_references_over_engine_limit() {
        let ascii = parse_ascii_model(
            "newmodel demo\nbeginmodelgeom demo\nnode trimesh demo\nparent NULL\nbitmap \
             abcdefghijklmnopq\nendnode\nendmodelgeom demo\ndonemodel demo\n",
        )
        .expect("parse long-resref fixture");
        let error = compile_ascii_model(&ascii).expect_err("long resref should fail");
        assert!(error.to_string().contains("permit at most 16"));
    }

    #[test]
    fn corrupted_array_count_is_rejected_before_allocation() {
        let ascii = parse_ascii_model(
            "newmodel demo\nbeginmodelgeom demo\nnode dummy demo\nparent \
             NULL\nendnode\nendmodelgeom demo\ndonemodel demo\n",
        )
        .expect("parse array-limit fixture");
        let binary = compile_ascii_model(&ascii).expect("compile array-limit fixture");
        let mut bytes = binary.source_bytes;
        let root = u32::from_le_bytes(bytes[84..88].try_into().expect("root pointer bytes"));
        let child_count_offset = 12 + usize::try_from(root).expect("root pointer") + 76;
        bytes[child_count_offset..child_count_offset + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        let error =
            parse_binary_model_bytes(&bytes).expect_err("oversized child array should fail");
        assert!(error.to_string().contains("u32 array"));
    }
}
