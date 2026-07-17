use std::collections::{BTreeMap, BTreeSet};

use crate::mdl::{
    ModelError, ModelResult, NwnComposedScene, NwnCoordinateSystem, NwnScene, NwnTransform,
};

/// Bakes skinned meshes in one scene using the scene's current pose.
///
/// # Errors
///
/// Returns [`ModelError`] if the scene pose cannot be baked.
pub fn bake_scene_pose(scene: &NwnScene) -> ModelResult<NwnScene> {
    bake_scene_pose_with_bind_pose(scene, scene)
}

/// Bakes skinned meshes in one composed scene tree using each scene's current
/// pose.
///
/// # Errors
///
/// Returns [`ModelError`] if any scene pose cannot be baked.
pub fn bake_composed_scene_pose(scene: &NwnComposedScene) -> ModelResult<NwnComposedScene> {
    Ok(NwnComposedScene {
        model_name:            scene.model_name.clone(),
        scene:                 bake_scene_pose(&scene.scene)?,
        hidden_geometry_nodes: scene.hidden_geometry_nodes.clone(),
        attachments:           scene
            .attachments
            .iter()
            .map(|attachment| {
                Ok(crate::mdl::NwnSceneAttachment {
                    target_node_name: attachment.target_node_name.clone(),
                    model_name:       attachment.model_name.clone(),
                    scene:            Box::new(bake_composed_scene_pose(&attachment.scene)?),
                })
            })
            .collect::<ModelResult<Vec<_>>>()?,
    })
}

pub(crate) fn bake_scene_pose_with_bind_pose(
    bind_scene: &NwnScene,
    posed_scene: &NwnScene,
) -> ModelResult<NwnScene> {
    if bind_scene.nodes.len() != posed_scene.nodes.len() {
        return Err(ModelError::msg(format!(
            "bind scene {} and posed scene {} have different node counts",
            bind_scene.name, posed_scene.name
        )));
    }
    if bind_scene.meshes.len() != posed_scene.meshes.len() {
        return Err(ModelError::msg(format!(
            "bind scene {} and posed scene {} have different mesh counts",
            bind_scene.name, posed_scene.name
        )));
    }

    let bind_worlds = scene_world_transforms(bind_scene, Mat4::identity());
    let posed_worlds = scene_world_transforms(posed_scene, Mat4::identity());
    let bind_name_to_index = scene_node_name_indices(bind_scene);
    let posed_name_to_index = scene_node_name_indices(posed_scene);
    let mut baked = posed_scene.clone();

    for (mesh_index, mesh) in baked.meshes.iter_mut().enumerate() {
        let Some(bind_mesh) = bind_scene.meshes.get(mesh_index) else {
            continue;
        };
        if bind_mesh.primitives.len() != mesh.primitives.len() {
            continue;
        }

        let mesh_bind_world = *bind_worlds.get(bind_mesh.source_node).ok_or_else(|| {
            ModelError::msg(format!(
                "bind mesh source node {} is out of range",
                bind_mesh.source_node
            ))
        })?;
        let mesh_current_world = *posed_worlds.get(mesh.source_node).ok_or_else(|| {
            ModelError::msg(format!(
                "mesh source node {} is out of range",
                mesh.source_node
            ))
        })?;
        let mesh_current_inverse = mesh_current_world.inverse_affine().ok_or_else(|| {
            ModelError::msg(format!(
                "mesh {} uses a non-invertible current transform",
                mesh.name
            ))
        })?;

        let bone_names = collect_skin_bone_names(mesh);
        let bone_local_matrices = bone_names
            .into_iter()
            .filter_map(|bone| {
                let bind_index = bind_name_to_index.get(bone.as_str()).copied()?;
                let posed_index = posed_name_to_index.get(bone.as_str()).copied()?;
                let bind_inverse = bind_worlds.get(bind_index)?.inverse_affine()?;
                let posed_world = *posed_worlds.get(posed_index)?;
                Some((
                    bone,
                    mesh_current_inverse
                        .mul(posed_world)
                        .mul(bind_inverse)
                        .mul(mesh_bind_world),
                ))
            })
            .collect::<BTreeMap<_, _>>();

        for (primitive_index, primitive) in mesh.primitives.iter_mut().enumerate() {
            let Some(bind_primitive) = bind_mesh.primitives.get(primitive_index) else {
                continue;
            };
            if bind_primitive.positions.len() != primitive.weight_rows.len() {
                continue;
            }

            for (vertex_index, bind_position) in bind_primitive.positions.iter().enumerate() {
                let Some(weights) = primitive.weight_rows.get(vertex_index) else {
                    continue;
                };
                let Some(position) = weighted_point(bind_position, weights, &bone_local_matrices)
                else {
                    continue;
                };
                if let Some(output) = primitive.positions.get_mut(vertex_index) {
                    *output = position;
                }
            }

            if bind_primitive.normals.len() != primitive.weight_rows.len() {
                continue;
            }

            for (normal_index, bind_normal) in bind_primitive.normals.iter().enumerate() {
                let Some(weights) = primitive.weight_rows.get(normal_index) else {
                    continue;
                };
                let Some(normal) = weighted_vector(bind_normal, weights, &bone_local_matrices)
                else {
                    continue;
                };
                if let Some(output) = primitive.normals.get_mut(normal_index) {
                    *output = normalize(normal).unwrap_or(*output);
                }
            }
        }
    }

    Ok(baked)
}

fn collect_skin_bone_names(mesh: &crate::mdl::NwnMesh) -> BTreeSet<String> {
    mesh.primitives
        .iter()
        .flat_map(|primitive| primitive.weight_rows.iter())
        .flat_map(|row| row.iter())
        .map(|weight| weight.bone.to_ascii_lowercase())
        .collect()
}

fn weighted_point(
    bind_position: &[f32; 3],
    weights: &[crate::mdl::NwnSkinWeight],
    bone_local_matrices: &BTreeMap<String, Mat4>,
) -> Option<[f32; 3]> {
    let mut total_weight = 0.0_f32;
    let mut accum = [0.0_f32; 3];

    for weight in weights {
        if weight.weight <= 0.0 {
            continue;
        }
        let Some(transform) = bone_local_matrices.get(&weight.bone.to_ascii_lowercase()) else {
            continue;
        };
        let transformed = transform.transform_point(*bind_position);
        accum[0] += transformed[0] * weight.weight;
        accum[1] += transformed[1] * weight.weight;
        accum[2] += transformed[2] * weight.weight;
        total_weight += weight.weight;
    }

    if total_weight <= f32::EPSILON {
        None
    } else {
        let inv = total_weight.recip();
        Some([accum[0] * inv, accum[1] * inv, accum[2] * inv])
    }
}

fn weighted_vector(
    bind_vector: &[f32; 3],
    weights: &[crate::mdl::NwnSkinWeight],
    bone_local_matrices: &BTreeMap<String, Mat4>,
) -> Option<[f32; 3]> {
    let mut total_weight = 0.0_f32;
    let mut accum = [0.0_f32; 3];

    for weight in weights {
        if weight.weight <= 0.0 {
            continue;
        }
        let Some(transform) = bone_local_matrices.get(&weight.bone.to_ascii_lowercase()) else {
            continue;
        };
        let transformed = transform.transform_vector(*bind_vector);
        accum[0] += transformed[0] * weight.weight;
        accum[1] += transformed[1] * weight.weight;
        accum[2] += transformed[2] * weight.weight;
        total_weight += weight.weight;
    }

    if total_weight <= f32::EPSILON {
        None
    } else {
        let inv = total_weight.recip();
        Some([accum[0] * inv, accum[1] * inv, accum[2] * inv])
    }
}

fn scene_node_name_indices(scene: &NwnScene) -> BTreeMap<String, usize> {
    scene
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.name.to_ascii_lowercase(), index))
        .collect()
}

pub(crate) fn scene_world_transforms(scene: &NwnScene, parent_world: Mat4) -> Vec<Mat4> {
    let mut worlds = Vec::with_capacity(scene.nodes.len());
    for node in &scene.nodes {
        let local = mat4_from_nwn_transform(&node.local_transform, scene.coordinate_system);
        let world = node
            .parent
            .and_then(|parent| worlds.get(parent).copied())
            .unwrap_or(parent_world)
            .mul(local);
        worlds.push(world);
    }
    worlds
}

fn position_from_nwn(position: [f32; 3], coordinate_system: NwnCoordinateSystem) -> [f32; 3] {
    match coordinate_system {
        NwnCoordinateSystem::AuroraSource => position,
    }
}

fn direction_from_nwn(direction: [f32; 3], coordinate_system: NwnCoordinateSystem) -> [f32; 3] {
    match coordinate_system {
        NwnCoordinateSystem::AuroraSource => direction,
    }
}

pub(crate) fn mat4_from_nwn_transform(
    transform: &NwnTransform,
    coordinate_system: NwnCoordinateSystem,
) -> Mat4 {
    let translation = position_from_nwn(transform.translation, coordinate_system);
    let axis = direction_from_nwn(
        [
            transform.rotation_axis_angle[0],
            transform.rotation_axis_angle[1],
            transform.rotation_axis_angle[2],
        ],
        coordinate_system,
    );
    let rotation = rotation_from_axis_angle(axis, transform.rotation_axis_angle[3]);
    let scale = Mat4::scale(transform.scale);
    Mat4::translation(translation).mul(rotation).mul(scale)
}

fn rotation_from_axis_angle(axis: [f32; 3], angle: f32) -> Mat4 {
    if angle.abs() < f32::EPSILON {
        return Mat4::identity();
    }

    let [x, y, z] = normalize(axis).unwrap_or([0.0, 1.0, 0.0]);
    let cos = angle.cos();
    let sin = angle.sin();
    let one_minus = 1.0 - cos;

    Mat4 {
        cols: [
            [
                cos + x * x * one_minus,
                y * x * one_minus + z * sin,
                z * x * one_minus - y * sin,
                0.0,
            ],
            [
                x * y * one_minus - z * sin,
                cos + y * y * one_minus,
                z * y * one_minus + x * sin,
                0.0,
            ],
            [
                x * z * one_minus + y * sin,
                y * z * one_minus - x * sin,
                cos + z * z * one_minus,
                0.0,
            ],
            [0.0, 0.0, 0.0, 1.0],
        ],
    }
}

fn normalize(vector: [f32; 3]) -> Option<[f32; 3]> {
    let length_squared = vector.iter().map(|value| value * value).sum::<f32>();
    if length_squared <= f32::EPSILON {
        return None;
    }
    let inv = length_squared.sqrt().recip();
    Some([vector[0] * inv, vector[1] * inv, vector[2] * inv])
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Mat4 {
    cols: [[f32; 4]; 4],
}

impl Mat4 {
    pub(crate) fn identity() -> Self {
        Self {
            cols: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    fn translation(offset: [f32; 3]) -> Self {
        let mut matrix = Self::identity();
        matrix.cols[3][0] = offset[0];
        matrix.cols[3][1] = offset[1];
        matrix.cols[3][2] = offset[2];
        matrix
    }

    fn scale(scale: [f32; 3]) -> Self {
        Self {
            cols: [
                [scale[0], 0.0, 0.0, 0.0],
                [0.0, scale[1], 0.0, 0.0],
                [0.0, 0.0, scale[2], 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    pub(crate) fn mul(self, rhs: Self) -> Self {
        let [lhs0, lhs1, lhs2, lhs3] = self.cols;
        let [rhs0, rhs1, rhs2, rhs3] = rhs.cols;

        let out = [
            mul_mat4_column(lhs0, lhs1, lhs2, lhs3, rhs0),
            mul_mat4_column(lhs0, lhs1, lhs2, lhs3, rhs1),
            mul_mat4_column(lhs0, lhs1, lhs2, lhs3, rhs2),
            mul_mat4_column(lhs0, lhs1, lhs2, lhs3, rhs3),
        ];
        Self {
            cols: out
        }
    }

    pub(crate) fn transform_point(self, point: [f32; 3]) -> [f32; 3] {
        [
            self.cols[0][0] * point[0]
                + self.cols[1][0] * point[1]
                + self.cols[2][0] * point[2]
                + self.cols[3][0],
            self.cols[0][1] * point[0]
                + self.cols[1][1] * point[1]
                + self.cols[2][1] * point[2]
                + self.cols[3][1],
            self.cols[0][2] * point[0]
                + self.cols[1][2] * point[1]
                + self.cols[2][2] * point[2]
                + self.cols[3][2],
        ]
    }

    fn transform_vector(self, vector: [f32; 3]) -> [f32; 3] {
        [
            self.cols[0][0] * vector[0] + self.cols[1][0] * vector[1] + self.cols[2][0] * vector[2],
            self.cols[0][1] * vector[0] + self.cols[1][1] * vector[1] + self.cols[2][1] * vector[2],
            self.cols[0][2] * vector[0] + self.cols[1][2] * vector[1] + self.cols[2][2] * vector[2],
        ]
    }

    fn inverse_affine(self) -> Option<Self> {
        let m00 = self.cols[0][0];
        let m10 = self.cols[1][0];
        let m20 = self.cols[2][0];
        let m01 = self.cols[0][1];
        let m11 = self.cols[1][1];
        let m21 = self.cols[2][1];
        let m02 = self.cols[0][2];
        let m12 = self.cols[1][2];
        let m22 = self.cols[2][2];

        let cofactor00 = m11 * m22 - m21 * m12;
        let cofactor01 = -(m01 * m22 - m21 * m02);
        let cofactor02 = m01 * m12 - m11 * m02;
        let cofactor10 = -(m10 * m22 - m20 * m12);
        let cofactor11 = m00 * m22 - m20 * m02;
        let cofactor12 = -(m00 * m12 - m10 * m02);
        let cofactor20 = m10 * m21 - m20 * m11;
        let cofactor21 = -(m00 * m21 - m20 * m01);
        let cofactor22 = m00 * m11 - m10 * m01;

        let determinant = m00 * cofactor00 + m10 * cofactor01 + m20 * cofactor02;
        if determinant.abs() <= f32::EPSILON {
            return None;
        }
        let inv_det = determinant.recip();

        let inverse = Self {
            cols: [
                [
                    cofactor00 * inv_det,
                    cofactor01 * inv_det,
                    cofactor02 * inv_det,
                    0.0,
                ],
                [
                    cofactor10 * inv_det,
                    cofactor11 * inv_det,
                    cofactor12 * inv_det,
                    0.0,
                ],
                [
                    cofactor20 * inv_det,
                    cofactor21 * inv_det,
                    cofactor22 * inv_det,
                    0.0,
                ],
                [0.0, 0.0, 0.0, 1.0],
            ],
        };
        let translation = [self.cols[3][0], self.cols[3][1], self.cols[3][2]];
        let inverse_translation =
            inverse.transform_vector([-translation[0], -translation[1], -translation[2]]);

        Some(Self {
            cols: [
                inverse.cols[0],
                inverse.cols[1],
                inverse.cols[2],
                [
                    inverse_translation[0],
                    inverse_translation[1],
                    inverse_translation[2],
                    1.0,
                ],
            ],
        })
    }
}

fn mul_mat4_column(
    lhs0: [f32; 4],
    lhs1: [f32; 4],
    lhs2: [f32; 4],
    lhs3: [f32; 4],
    rhs: [f32; 4],
) -> [f32; 4] {
    [
        lhs0[0] * rhs[0] + lhs1[0] * rhs[1] + lhs2[0] * rhs[2] + lhs3[0] * rhs[3],
        lhs0[1] * rhs[0] + lhs1[1] * rhs[1] + lhs2[1] * rhs[2] + lhs3[1] * rhs[3],
        lhs0[2] * rhs[0] + lhs1[2] * rhs[1] + lhs2[2] * rhs[2] + lhs3[2] * rhs[3],
        lhs0[3] * rhs[0] + lhs1[3] * rhs[1] + lhs2[3] * rhs[2] + lhs3[3] * rhs[3],
    ]
}

#[cfg(test)]
mod tests {
    use super::{bake_scene_pose, bake_scene_pose_with_bind_pose};
    use crate::mdl::{
        NodeKind, NwnCoordinateSystem, NwnFace, NwnMesh, NwnPrimitive, NwnScene, NwnSceneNode,
        NwnSkinWeight, NwnTransform, NwnUvSet, Vec3Key,
    };

    #[test]
    fn bake_scene_pose_with_bind_pose_applies_skin_weights() {
        let bind = skinned_fixture_scene();
        let mut posed = bind.clone();
        let node = posed
            .nodes
            .get_mut(1)
            .unwrap_or_else(|| panic!("posed scene missing animated bone"));
        node.local_transform.translation = [2.0, 0.0, 0.0];

        let baked = bake_scene_pose_with_bind_pose(&bind, &posed)
            .unwrap_or_else(|error| panic!("bake pose: {error}"));

        let position = baked
            .meshes
            .first()
            .and_then(|mesh| mesh.primitives.first())
            .and_then(|primitive| primitive.positions.first())
            .unwrap_or_else(|| panic!("baked scene missing skinned vertex"));
        assert_eq!(*position, [2.0, 0.0, 0.0]);
    }

    #[test]
    fn bake_scene_pose_is_stable_for_bind_pose() {
        let bind = skinned_fixture_scene();
        let baked = bake_scene_pose(&bind).unwrap_or_else(|error| panic!("bake scene: {error}"));
        let position = baked
            .meshes
            .first()
            .and_then(|mesh| mesh.primitives.first())
            .and_then(|primitive| primitive.positions.first())
            .unwrap_or_else(|| panic!("baked bind scene missing skinned vertex"));
        assert_eq!(*position, [1.0, 0.0, 0.0]);
    }

    fn skinned_fixture_scene() -> NwnScene {
        NwnScene {
            name:              "skin_demo".to_string(),
            supermodel:        None,
            classification:    None,
            animation_scale:   None,
            ignore_fog:        None,
            coordinate_system: NwnCoordinateSystem::AuroraSource,
            nodes:             vec![
                NwnSceneNode {
                    kind:               NodeKind::Dummy,
                    node_type:          "dummy".to_string(),
                    name:               "root".to_string(),
                    parent:             None,
                    part_number:        None,
                    local_transform:    NwnTransform {
                        translation:         [0.0, 0.0, 0.0],
                        rotation_axis_angle: [0.0, 1.0, 0.0, 0.0],
                        scale:               [1.0, 1.0, 1.0],
                    },
                    center:             None,
                    color:              None,
                    radius:             None,
                    alpha:              None,
                    wirecolor:          None,
                    light:              None,
                    emitter:            None,
                    dangly:             None,
                    reference:          None,
                    mesh:               None,
                    opaque_controllers: Vec::new(),
                },
                NwnSceneNode {
                    kind:               NodeKind::Dummy,
                    node_type:          "dummy".to_string(),
                    name:               "bone".to_string(),
                    parent:             Some(0),
                    part_number:        None,
                    local_transform:    NwnTransform {
                        translation:         [1.0, 0.0, 0.0],
                        rotation_axis_angle: [0.0, 1.0, 0.0, 0.0],
                        scale:               [1.0, 1.0, 1.0],
                    },
                    center:             None,
                    color:              None,
                    radius:             None,
                    alpha:              None,
                    wirecolor:          None,
                    light:              None,
                    emitter:            None,
                    dangly:             None,
                    reference:          None,
                    mesh:               None,
                    opaque_controllers: Vec::new(),
                },
                NwnSceneNode {
                    kind:               NodeKind::Skin,
                    node_type:          "skin".to_string(),
                    name:               "wing".to_string(),
                    parent:             Some(0),
                    part_number:        None,
                    local_transform:    NwnTransform {
                        translation:         [0.0, 0.0, 0.0],
                        rotation_axis_angle: [0.0, 1.0, 0.0, 0.0],
                        scale:               [1.0, 1.0, 1.0],
                    },
                    center:             None,
                    color:              None,
                    radius:             None,
                    alpha:              None,
                    wirecolor:          None,
                    light:              None,
                    emitter:            None,
                    dangly:             None,
                    reference:          None,
                    mesh:               Some(0),
                    opaque_controllers: Vec::new(),
                },
            ],
            meshes:            vec![NwnMesh {
                name:        "wing".to_string(),
                source_node: 2,
                primitives:  vec![NwnPrimitive {
                    sample_period:   None,
                    positions:       vec![[1.0, 0.0, 0.0]],
                    faces:           vec![NwnFace {
                        vertex_indices: [0, 0, 0],
                        group:          0,
                        uv_indices:     [0, 0, 0],
                        material_index: 0,
                    }],
                    uv_sets:         vec![NwnUvSet {
                        index:       0,
                        coordinates: vec![[0.0, 0.0]],
                    }],
                    normals:         vec![[1.0, 0.0, 0.0]],
                    tangents:        Vec::new(),
                    color_rows:      Vec::new(),
                    weight_rows:     vec![vec![NwnSkinWeight {
                        bone:   "bone".to_string(),
                        weight: 1.0,
                    }]],
                    constraint_rows: Vec::new(),
                    surface_labels:  Vec::new(),
                    texture_names:   Vec::new(),
                    material:        None,
                }],
            }],
            materials:         Vec::new(),
            animations:        vec![crate::mdl::NwnAnimation {
                name:            "move".to_string(),
                model_name:      "skin_demo".to_string(),
                length:          1.0,
                transition_time: 0.0,
                root_name:       None,
                root_node:       None,
                events:          Vec::new(),
                node_tracks:     vec![crate::mdl::NwnNodeAnimationTrack {
                    target_name:        "bone".to_string(),
                    target_node:        Some(1),
                    kind:               NodeKind::Dummy,
                    transform:          crate::mdl::NwnTransformTrack {
                        translation_keys:         vec![Vec3Key {
                            time:  1.0,
                            value: [2.0, 0.0, 0.0],
                        }],
                        rotation_axis_angle_keys: Vec::new(),
                        scale_keys:               Vec::new(),
                    },
                    material:           crate::mdl::NwnMaterialTrack {
                        color_keys:                 Vec::new(),
                        radius_keys:                Vec::new(),
                        alpha_keys:                 Vec::new(),
                        self_illum_color_keys:      Vec::new(),
                        multiplier_keys:            Vec::new(),
                        shadow_radius_keys:         Vec::new(),
                        vertical_displacement_keys: Vec::new(),
                    },
                    effects:            crate::mdl::NwnEffectTrack {
                        emitter_controllers: Vec::new(),
                        dangly:              None,
                    },
                    animmesh:           None,
                    bezier_controllers: Vec::new(),
                    opaque_controllers: Vec::new(),
                }],
            }],
            diagnostics:       Vec::new(),
        }
    }
}
