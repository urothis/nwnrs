use std::io::Write;

use tracing::instrument;

use crate::{
    ModelError, ModelResult, NwnComposedScene, NwnCoordinateSystem, NwnPrimitive, NwnScene,
    bake_composed_scene_pose, bake_scene_pose,
    pose::{Mat4, scene_world_transforms},
};

/// Writes one scene as a flattened Wavefront OBJ mesh.
///
/// # Errors
///
/// Returns [`ModelError`] if scene baking or the write fails.
#[instrument(level = "debug", skip_all, err, fields(scene = %scene.name))]
pub fn write_scene_obj<W: Write>(writer: &mut W, scene: &NwnScene) -> ModelResult<()> {
    let composed = NwnComposedScene {
        model_name:            scene.name.clone(),
        scene:                 bake_scene_pose(scene)?,
        hidden_geometry_nodes: Vec::new(),
        attachments:           Vec::new(),
    };
    write_composed_scene_obj(writer, &composed)
}

/// Writes one composed scene tree as a flattened Wavefront OBJ mesh.
#[instrument(level = "debug", skip_all, err, fields(model = %scene.model_name))]
pub fn write_composed_scene_obj<W: Write>(
    writer: &mut W,
    scene: &NwnComposedScene,
) -> ModelResult<()> {
    let baked = bake_composed_scene_pose(scene)?;
    let mut state = ObjState {
        writer,
        vertex_offset: 1,
    };
    state.write_header(baked.model_name.as_str())?;
    write_composed_scene_recursive(&mut state, &baked, Mat4::identity())?;
    Ok(())
}

struct ObjState<'a, W> {
    writer:        &'a mut W,
    vertex_offset: u32,
}

impl<W: Write> ObjState<'_, W> {
    fn write_header(&mut self, name: &str) -> ModelResult<()> {
        writeln!(self.writer, "# Exported by nwnrs").map_err(ModelError::from)?;
        writeln!(self.writer, "o {name}").map_err(ModelError::from)?;
        Ok(())
    }
}

fn write_composed_scene_recursive<W: Write>(
    state: &mut ObjState<'_, W>,
    composed: &NwnComposedScene,
    parent_world: Mat4,
) -> ModelResult<()> {
    let world_transforms = scene_world_transforms(&composed.scene, parent_world);
    for (node_index, node) in composed.scene.nodes.iter().enumerate() {
        if composed
            .hidden_geometry_nodes
            .iter()
            .any(|hidden| hidden.eq_ignore_ascii_case(node.name.as_str()))
        {
            continue;
        }
        let Some(mesh_index) = node.mesh else {
            continue;
        };
        let Some(mesh) = composed.scene.meshes.get(mesh_index) else {
            continue;
        };
        let world = *world_transforms.get(node_index).ok_or_else(|| {
            ModelError::msg(format!("node transform {node_index} is out of range"))
        })?;
        for primitive in &mesh.primitives {
            write_primitive(state, primitive, composed.scene.coordinate_system, world)?;
        }
    }

    for attachment in &composed.attachments {
        let Some(node_index) = composed.scene.nodes.iter().position(|node| {
            node.name
                .eq_ignore_ascii_case(attachment.target_node_name.as_str())
        }) else {
            continue;
        };
        let world = *world_transforms.get(node_index).ok_or_else(|| {
            ModelError::msg(format!(
                "attachment node transform {node_index} is out of range"
            ))
        })?;
        write_composed_scene_recursive(state, &attachment.scene, world)?;
    }

    Ok(())
}

fn write_primitive<W: Write>(
    state: &mut ObjState<'_, W>,
    primitive: &NwnPrimitive,
    coordinate_system: NwnCoordinateSystem,
    world: Mat4,
) -> ModelResult<()> {
    let vertex_base = state.vertex_offset;
    for position in &primitive.positions {
        let transformed = world.transform_point(position_from_nwn(*position, coordinate_system));
        writeln!(
            state.writer,
            "v {} {} {}",
            transformed[0], transformed[1], transformed[2]
        )
        .map_err(ModelError::from)?;
        state.vertex_offset = state
            .vertex_offset
            .checked_add(1)
            .ok_or_else(|| ModelError::msg("obj vertex count overflow"))?;
    }

    for face in &primitive.faces {
        let indices = face.vertex_indices.map(|index| {
            vertex_base
                .checked_add(index)
                .ok_or_else(|| ModelError::msg("obj face index overflow"))
        });
        let [a, b, c] = indices;
        writeln!(state.writer, "f {} {} {}", a?, b?, c?,).map_err(ModelError::from)?;
    }

    Ok(())
}

fn position_from_nwn(position: [f32; 3], coordinate_system: NwnCoordinateSystem) -> [f32; 3] {
    match coordinate_system {
        NwnCoordinateSystem::AuroraSource => position,
    }
}

#[allow(clippy::panic)]
#[cfg(test)]
mod tests {
    use crate::{
        NodeKind, NwnComposedScene, NwnCoordinateSystem, NwnFace, NwnMesh, NwnPrimitive, NwnScene,
        NwnSceneAttachment, NwnSceneNode, NwnTransform, NwnUvSet, write_composed_scene_obj,
    };

    #[test]
    fn writes_flattened_obj_with_attached_child_scene() {
        let primitive = NwnPrimitive {
            positions:       vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            faces:           vec![NwnFace {
                vertex_indices: [0, 1, 2],
                group:          0,
                uv_indices:     [0, 1, 2],
                material_index: 0,
            }],
            uv_sets:         vec![NwnUvSet {
                index:       0,
                coordinates: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
            }],
            normals:         Vec::new(),
            tangents:        Vec::new(),
            color_rows:      Vec::new(),
            weight_rows:     Vec::new(),
            constraint_rows: Vec::new(),
            surface_labels:  Vec::new(),
            texture_names:   Vec::new(),
            material:        None,
        };
        let child = NwnComposedScene {
            model_name:            "child".to_string(),
            scene:                 NwnScene {
                name:              "child".to_string(),
                supermodel:        None,
                classification:    None,
                animation_scale:   None,
                coordinate_system: NwnCoordinateSystem::AuroraSource,
                nodes:             vec![NwnSceneNode {
                    kind:            NodeKind::Trimesh,
                    node_type:       "trimesh".to_string(),
                    name:            "child_root".to_string(),
                    parent:          None,
                    part_number:     None,
                    local_transform: NwnTransform {
                        translation:         [0.0, 0.0, 0.0],
                        rotation_axis_angle: [0.0, 1.0, 0.0, 0.0],
                        scale:               [1.0, 1.0, 1.0],
                    },
                    center:          None,
                    color:           None,
                    radius:          None,
                    alpha:           None,
                    wirecolor:       None,
                    light:           None,
                    emitter:         None,
                    reference:       None,
                    mesh:            Some(0),
                }],
                meshes:            vec![NwnMesh {
                    name:        "child_mesh".to_string(),
                    source_node: 0,
                    primitives:  vec![primitive.clone()],
                }],
                materials:         Vec::new(),
                animations:        Vec::new(),
                diagnostics:       Vec::new(),
            },
            hidden_geometry_nodes: Vec::new(),
            attachments:           Vec::new(),
        };
        let parent = NwnComposedScene {
            model_name:            "parent".to_string(),
            scene:                 NwnScene {
                name:              "parent".to_string(),
                supermodel:        None,
                classification:    None,
                animation_scale:   None,
                coordinate_system: NwnCoordinateSystem::AuroraSource,
                nodes:             vec![NwnSceneNode {
                    kind:            NodeKind::Dummy,
                    node_type:       "dummy".to_string(),
                    name:            "attach".to_string(),
                    parent:          None,
                    part_number:     None,
                    local_transform: NwnTransform {
                        translation:         [10.0, 0.0, 0.0],
                        rotation_axis_angle: [0.0, 1.0, 0.0, 0.0],
                        scale:               [1.0, 1.0, 1.0],
                    },
                    center:          None,
                    color:           None,
                    radius:          None,
                    alpha:           None,
                    wirecolor:       None,
                    light:           None,
                    emitter:         None,
                    reference:       None,
                    mesh:            None,
                }],
                meshes:            Vec::new(),
                materials:         Vec::new(),
                animations:        Vec::new(),
                diagnostics:       Vec::new(),
            },
            hidden_geometry_nodes: Vec::new(),
            attachments:           vec![NwnSceneAttachment {
                target_node_name: "attach".to_string(),
                model_name:       "child".to_string(),
                scene:            Box::new(child),
            }],
        };

        let mut encoded = Vec::new();
        write_composed_scene_obj(&mut encoded, &parent)
            .unwrap_or_else(|error| panic!("write obj: {error}"));
        let text = String::from_utf8(encoded).unwrap_or_else(|error| panic!("utf8: {error}"));
        assert!(text.contains("v 10 0 0"));
        assert!(text.contains("f 1 2 3"));
    }
}
