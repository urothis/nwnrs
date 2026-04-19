use std::collections::BTreeSet;

use crate::{
    ModelError, ModelResult, NwnAnimMeshTrack, NwnAnimation, NwnComposedScene,
    NwnNodeAnimationTrack, NwnScene, NwnTransform, NwnVec2Sample, NwnVec3Sample, Vec3Key, Vec4Key,
    bake_scene_pose_with_bind_pose,
};

/// Returns animation names in source order.
#[must_use]
pub fn scene_animation_names(scene: &NwnScene) -> Vec<String> {
    scene
        .animations
        .iter()
        .map(|animation| animation.name.clone())
        .collect()
}

/// Returns unique animation names from one composed scene tree in stable sorted
/// order.
#[must_use]
pub fn composed_scene_animation_names(scene: &NwnComposedScene) -> Vec<String> {
    let mut names = BTreeSet::new();
    collect_composed_scene_animation_names(scene, &mut names);
    names.into_iter().collect()
}

/// Returns the requested animation, case-insensitively.
#[must_use]
pub fn find_scene_animation<'a>(scene: &'a NwnScene, name: &str) -> Option<&'a NwnAnimation> {
    scene.animation(name)
}

/// Returns the default animation selection policy used when no explicit name is
/// supplied: prefer `default`, otherwise the only animation if there is exactly
/// one.
#[must_use]
pub fn default_scene_animation(scene: &NwnScene) -> Option<&NwnAnimation> {
    if let Some(default_animation) = scene.animation("default") {
        return Some(default_animation);
    }

    if scene.animations.len() == 1 {
        return scene.animations.first();
    }

    None
}

/// Samples one scene at `time` seconds on the named animation and returns a
/// frozen scene snapshot.
pub fn sample_scene_animation(
    scene: &NwnScene,
    animation_name: &str,
    time: f32,
) -> ModelResult<NwnScene> {
    let animation = find_scene_animation(scene, animation_name)
        .ok_or_else(|| invalid_animation_error(animation_name, &scene_animation_names(scene)))?;
    sample_scene_with_animation(scene, animation, time)
}

/// Samples one scene at `time` seconds using the default animation selection
/// policy.
pub fn sample_scene_default_animation(scene: &NwnScene, time: f32) -> ModelResult<NwnScene> {
    let animation = default_scene_animation(scene).ok_or_else(|| {
        ModelError::msg(format!(
            "no default animation could be selected; available animations: {}",
            format_animation_names(&scene_animation_names(scene))
        ))
    })?;
    sample_scene_with_animation(scene, animation, time)
}

/// Samples one composed scene tree at `time` seconds on the named animation.
/// Child scenes that do not contain the animation remain in their base pose.
pub fn sample_composed_scene_animation(
    scene: &NwnComposedScene,
    animation_name: &str,
    time: f32,
) -> ModelResult<NwnComposedScene> {
    let available = composed_scene_animation_names(scene);
    if !available
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(animation_name))
    {
        return Err(invalid_animation_error(animation_name, &available));
    }
    sample_composed_scene_with_animation(scene, Some(animation_name), time)
}

/// Samples one composed scene tree at `time` seconds using the root scene's
/// default animation selection policy.
pub fn sample_composed_scene_default_animation(
    scene: &NwnComposedScene,
    time: f32,
) -> ModelResult<NwnComposedScene> {
    let animation = default_scene_animation(&scene.scene).ok_or_else(|| {
        ModelError::msg(format!(
            "no default animation could be selected; available animations: {}",
            format_animation_names(&composed_scene_animation_names(scene))
        ))
    })?;
    sample_composed_scene_with_animation(scene, Some(animation.name.as_str()), time)
}

fn collect_composed_scene_animation_names(scene: &NwnComposedScene, names: &mut BTreeSet<String>) {
    for animation in &scene.scene.animations {
        names.insert(animation.name.clone());
    }
    for attachment in &scene.attachments {
        collect_composed_scene_animation_names(&attachment.scene, names);
    }
}

fn sample_composed_scene_with_animation(
    scene: &NwnComposedScene,
    animation_name: Option<&str>,
    time: f32,
) -> ModelResult<NwnComposedScene> {
    let sampled_scene = animation_name
        .and_then(|name| find_scene_animation(&scene.scene, name))
        .map_or_else(
            || Ok(scene.scene.clone()),
            |animation| sample_scene_with_animation(&scene.scene, animation, time),
        );
    let attachments = scene
        .attachments
        .iter()
        .map(|attachment| {
            Ok(crate::NwnSceneAttachment {
                target_node_name: attachment.target_node_name.clone(),
                model_name:       attachment.model_name.clone(),
                scene:            Box::new(sample_composed_scene_with_animation(
                    &attachment.scene,
                    animation_name,
                    time,
                )?),
            })
        })
        .collect::<ModelResult<Vec<_>>>()?;
    Ok(NwnComposedScene {
        model_name: scene.model_name.clone(),
        scene: sampled_scene?,
        hidden_geometry_nodes: scene.hidden_geometry_nodes.clone(),
        attachments,
    })
}

fn sample_scene_with_animation(
    scene: &NwnScene,
    animation: &NwnAnimation,
    time: f32,
) -> ModelResult<NwnScene> {
    let mut sampled = scene.clone();
    let sampled_time = normalize_animation_time(animation.length, time);

    for track in &animation.node_tracks {
        let Some(node_index) = resolve_track_node_index(&sampled, track) else {
            continue;
        };
        let Some(node) = sampled.nodes.get_mut(node_index) else {
            continue;
        };
        node.local_transform =
            sample_transform_track(&track.transform, &node.local_transform, sampled_time);

        if let Some(animmesh) = track.animmesh.as_ref()
            && let Some(mesh_index) = node.mesh
            && let Some(mesh) = sampled.meshes.get_mut(mesh_index)
        {
            for primitive in &mut mesh.primitives {
                apply_animmesh_track(primitive, animmesh, animation.length, sampled_time);
            }
        }
    }

    bake_scene_pose_with_bind_pose(scene, &sampled)
}

fn resolve_track_node_index(scene: &NwnScene, track: &NwnNodeAnimationTrack) -> Option<usize> {
    track.target_node.or_else(|| {
        scene
            .nodes
            .iter()
            .position(|node| node.name.eq_ignore_ascii_case(track.target_name.as_str()))
    })
}

fn sample_transform_track(
    track: &crate::NwnTransformTrack,
    fallback: &NwnTransform,
    time: f32,
) -> NwnTransform {
    NwnTransform {
        translation:         sample_vec3_keys(&track.translation_keys, time, fallback.translation),
        rotation_axis_angle: sample_rotation_axis_angle_keys(
            &track.rotation_axis_angle_keys,
            time,
            fallback.rotation_axis_angle,
        ),
        scale:               sample_vec3_keys(&track.scale_keys, time, fallback.scale),
    }
}

fn apply_animmesh_track(
    primitive: &mut crate::NwnPrimitive,
    animmesh: &NwnAnimMeshTrack,
    animation_length: f32,
    time: f32,
) {
    if !animmesh.vertex_samples.is_empty() {
        primitive.positions = sample_vec3_frames(
            &animmesh.vertex_samples,
            animmesh.sample_period,
            animation_length,
            &primitive.positions,
            time,
        );
    }

    if !animmesh.uv_samples.is_empty()
        && let Some(uv_set) = primitive.uv_sets.first_mut()
    {
        uv_set.coordinates = sample_vec2_frames(
            &animmesh.uv_samples,
            animmesh.sample_period,
            animation_length,
            &uv_set.coordinates,
            time,
        );
    }
}

fn normalize_animation_time(length: f32, time: f32) -> f32 {
    if length > 0.0 {
        time.rem_euclid(length)
    } else {
        time.max(0.0)
    }
}

fn sample_vec3_keys(keys: &[Vec3Key], time: f32, fallback: [f32; 3]) -> [f32; 3] {
    let Some(first) = keys.first() else {
        return fallback;
    };
    if keys.len() == 1 || time <= first.time {
        return first.value;
    }
    let last = keys.last().unwrap_or(first);
    if time >= last.time {
        return last.value;
    }

    for window in keys.windows(2) {
        let [start, end] = window else {
            continue;
        };
        if time <= end.time {
            let duration = (end.time - start.time).max(f32::EPSILON);
            let factor = ((time - start.time) / duration).clamp(0.0, 1.0);
            return [
                start.value[0] + (end.value[0] - start.value[0]) * factor,
                start.value[1] + (end.value[1] - start.value[1]) * factor,
                start.value[2] + (end.value[2] - start.value[2]) * factor,
            ];
        }
    }

    last.value
}

fn sample_rotation_axis_angle_keys(keys: &[Vec4Key], time: f32, fallback: [f32; 4]) -> [f32; 4] {
    let Some(first) = keys.first() else {
        return fallback;
    };
    if keys.len() == 1 || time <= first.time {
        return first.value;
    }
    let last = keys.last().unwrap_or(first);
    if time >= last.time {
        return last.value;
    }

    for window in keys.windows(2) {
        let [start, end] = window else {
            continue;
        };
        if time <= end.time {
            let duration = (end.time - start.time).max(f32::EPSILON);
            let factor = ((time - start.time) / duration).clamp(0.0, 1.0);
            return Quat::from_axis_angle(start.value)
                .slerp(Quat::from_axis_angle(end.value), factor)
                .to_axis_angle();
        }
    }

    last.value
}

fn sample_vec3_frames(
    samples: &[NwnVec3Sample],
    sample_period: Option<f32>,
    animation_length: f32,
    fallback: &[[f32; 3]],
    elapsed: f32,
) -> Vec<[f32; 3]> {
    let Some((current, next, factor)) =
        sample_frame_indices(samples.len(), sample_period, animation_length, elapsed)
    else {
        return fallback.to_vec();
    };
    if current == next {
        return samples
            .get(current)
            .map_or_else(|| fallback.to_vec(), |sample| sample.values.clone());
    }

    let Some(current_sample) = samples.get(current) else {
        return fallback.to_vec();
    };
    let Some(next_sample) = samples.get(next) else {
        return current_sample.values.clone();
    };

    current_sample
        .values
        .iter()
        .zip(next_sample.values.iter())
        .map(|(start, end)| {
            [
                start[0] + (end[0] - start[0]) * factor,
                start[1] + (end[1] - start[1]) * factor,
                start[2] + (end[2] - start[2]) * factor,
            ]
        })
        .collect()
}

fn sample_vec2_frames(
    samples: &[NwnVec2Sample],
    sample_period: Option<f32>,
    animation_length: f32,
    fallback: &[[f32; 2]],
    elapsed: f32,
) -> Vec<[f32; 2]> {
    let Some((current, next, factor)) =
        sample_frame_indices(samples.len(), sample_period, animation_length, elapsed)
    else {
        return fallback.to_vec();
    };
    if current == next {
        return samples
            .get(current)
            .map_or_else(|| fallback.to_vec(), |sample| sample.values.clone());
    }

    let Some(current_sample) = samples.get(current) else {
        return fallback.to_vec();
    };
    let Some(next_sample) = samples.get(next) else {
        return current_sample.values.clone();
    };

    current_sample
        .values
        .iter()
        .zip(next_sample.values.iter())
        .map(|(start, end)| {
            [
                start[0] + (end[0] - start[0]) * factor,
                start[1] + (end[1] - start[1]) * factor,
            ]
        })
        .collect()
}

fn sample_frame_indices(
    sample_count: usize,
    sample_period: Option<f32>,
    animation_length: f32,
    elapsed: f32,
) -> Option<(usize, usize, f32)> {
    match sample_count {
        0 => None,
        1 => Some((0, 0, 0.0)),
        _ => {
            let phase_time = normalize_animation_time(animation_length, elapsed);
            let derived_period = sample_period
                .filter(|period| *period > f32::EPSILON)
                .unwrap_or_else(|| (animation_length / sample_count as f32).max(f32::EPSILON));
            let cycle_duration = derived_period * sample_count as f32;
            let cycle_time = if cycle_duration > f32::EPSILON {
                phase_time.rem_euclid(cycle_duration)
            } else {
                0.0
            };
            let mut current = 0_usize;
            let mut next_boundary = derived_period;
            while current + 1 < sample_count && cycle_time >= next_boundary {
                current += 1;
                next_boundary += derived_period;
            }
            let next = (current + 1) % sample_count;
            let current_start = current as f32 * derived_period;
            let factor = ((cycle_time - current_start) / derived_period).clamp(0.0, 1.0);
            Some((current, next, factor))
        }
    }
}

fn invalid_animation_error(name: &str, available: &[String]) -> ModelError {
    ModelError::msg(format!(
        "animation {name:?} not found; available animations: {}",
        format_animation_names(available)
    ))
}

fn format_animation_names(names: &[String]) -> String {
    if names.is_empty() {
        "none".to_string()
    } else {
        names.join(", ")
    }
}

#[derive(Debug, Clone, Copy)]
struct Quat {
    x: f32,
    y: f32,
    z: f32,
    w: f32,
}

impl Quat {
    fn from_axis_angle(value: [f32; 4]) -> Self {
        let [axis_x, axis_y, axis_z, angle] = value;
        if angle.abs() < f32::EPSILON {
            return Self::identity();
        }
        let [axis_x, axis_y, axis_z] =
            normalize_vec3([axis_x, axis_y, axis_z]).unwrap_or([0.0, 1.0, 0.0]);
        let half = angle * 0.5;
        let sin = half.sin();
        let cos = half.cos();
        Self {
            x: axis_x * sin,
            y: axis_y * sin,
            z: axis_z * sin,
            w: cos,
        }
    }

    fn identity() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: 1.0,
        }
    }

    fn normalized(self) -> Self {
        let length = (self.x * self.x + self.y * self.y + self.z * self.z + self.w * self.w).sqrt();
        if length <= f32::EPSILON {
            return Self::identity();
        }
        let inv = length.recip();
        Self {
            x: self.x * inv,
            y: self.y * inv,
            z: self.z * inv,
            w: self.w * inv,
        }
    }

    fn dot(self, rhs: Self) -> f32 {
        self.x * rhs.x + self.y * rhs.y + self.z * rhs.z + self.w * rhs.w
    }

    fn slerp(self, rhs: Self, factor: f32) -> Self {
        let mut end = rhs;
        let mut cos_theta = self.dot(rhs);
        if cos_theta < 0.0 {
            end = Self {
                x: -rhs.x,
                y: -rhs.y,
                z: -rhs.z,
                w: -rhs.w,
            };
            cos_theta = -cos_theta;
        }

        if cos_theta > 0.9995 {
            return Self {
                x: self.x + (end.x - self.x) * factor,
                y: self.y + (end.y - self.y) * factor,
                z: self.z + (end.z - self.z) * factor,
                w: self.w + (end.w - self.w) * factor,
            }
            .normalized();
        }

        let theta = cos_theta.acos();
        let sin_theta = theta.sin().max(f32::EPSILON);
        let weight_start = ((1.0 - factor) * theta).sin() / sin_theta;
        let weight_end = (factor * theta).sin() / sin_theta;
        Self {
            x: self.x * weight_start + end.x * weight_end,
            y: self.y * weight_start + end.y * weight_end,
            z: self.z * weight_start + end.z * weight_end,
            w: self.w * weight_start + end.w * weight_end,
        }
        .normalized()
    }

    fn to_axis_angle(self) -> [f32; 4] {
        let normalized = self.normalized();
        let angle = 2.0 * normalized.w.clamp(-1.0, 1.0).acos();
        let sin_half = (1.0 - normalized.w * normalized.w).sqrt();
        if sin_half <= f32::EPSILON {
            [0.0, 1.0, 0.0, 0.0]
        } else {
            [
                normalized.x / sin_half,
                normalized.y / sin_half,
                normalized.z / sin_half,
                angle,
            ]
        }
    }
}

fn normalize_vec3(vector: [f32; 3]) -> Option<[f32; 3]> {
    let length_squared = vector.iter().map(|value| value * value).sum::<f32>();
    if length_squared <= f32::EPSILON {
        return None;
    }
    let inv = length_squared.sqrt().recip();
    Some([vector[0] * inv, vector[1] * inv, vector[2] * inv])
}

#[allow(clippy::panic)]
#[cfg(test)]
mod tests {
    use crate::{
        AnimationEvent, NodeKind, NwnAnimMeshTrack, NwnAnimation, NwnComposedScene,
        NwnCoordinateSystem, NwnFace, NwnMaterial, NwnMaterialTrack, NwnMesh,
        NwnNodeAnimationTrack, NwnPrimitive, NwnScene, NwnSceneAttachment, NwnSceneNode,
        NwnTextureRef, NwnTextureSlot, NwnTransform, NwnTransformTrack, NwnUvSet, NwnVec2Sample,
        NwnVec3Sample, ScalarKey, Vec3Key, Vec4Key, sample_composed_scene_animation,
        sample_scene_animation, scene_animation_names,
    };

    #[test]
    fn lists_scene_animation_names_in_order() {
        let scene = scene_with_animation("idle");
        assert_eq!(scene_animation_names(&scene), vec!["idle".to_string()]);
    }

    #[test]
    fn samples_transform_keys_at_time_zero() {
        let scene = scene_with_animation("default");
        let sampled = sample_scene_animation(&scene, "default", 0.0)
            .unwrap_or_else(|error| panic!("sample scene: {error}"));
        let node = sampled
            .nodes
            .first()
            .unwrap_or_else(|| panic!("sampled scene missing node"));
        assert_eq!(node.local_transform.translation, [5.0, 0.0, 0.0]);
    }

    #[test]
    fn samples_animmesh_positions() {
        let scene = scene_with_animation("default");
        let sampled = sample_scene_animation(&scene, "default", 0.5)
            .unwrap_or_else(|error| panic!("sample scene: {error}"));
        let position = sampled
            .meshes
            .first()
            .and_then(|mesh| mesh.primitives.first())
            .and_then(|primitive| primitive.positions.first())
            .unwrap_or_else(|| panic!("sampled scene missing primitive position"));
        assert_eq!(*position, [6.0, 0.0, 0.0]);
    }

    #[test]
    fn samples_skinned_mesh_positions() {
        let scene = skinned_scene_with_animation();
        let sampled = sample_scene_animation(&scene, "move", 1.0)
            .unwrap_or_else(|error| panic!("sample scene: {error}"));
        let position = sampled
            .meshes
            .first()
            .and_then(|mesh| mesh.primitives.first())
            .and_then(|primitive| primitive.positions.first())
            .unwrap_or_else(|| panic!("sampled skinned scene missing primitive position"));
        assert_eq!(*position, [2.0, 0.0, 0.0]);
    }

    #[test]
    fn composed_sampling_updates_attachment_with_matching_animation() {
        let child = NwnComposedScene {
            model_name:            "child".to_string(),
            scene:                 scene_with_animation("default"),
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

        let sampled = sample_composed_scene_animation(&parent, "default", 0.0)
            .unwrap_or_else(|error| panic!("sample composed: {error}"));
        let node = sampled
            .attachments
            .first()
            .map(|attachment| &attachment.scene.scene)
            .and_then(|scene| scene.nodes.first())
            .unwrap_or_else(|| panic!("sampled composed scene missing attachment node"));
        assert_eq!(node.local_transform.translation, [5.0, 0.0, 0.0]);
    }

    fn scene_with_animation(name: &str) -> NwnScene {
        NwnScene {
            name:              "demo".to_string(),
            supermodel:        None,
            classification:    None,
            animation_scale:   None,
            coordinate_system: NwnCoordinateSystem::AuroraSource,
            nodes:             vec![NwnSceneNode {
                kind:            NodeKind::Trimesh,
                node_type:       "trimesh".to_string(),
                name:            "mesh".to_string(),
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
                name:        "mesh".to_string(),
                source_node: 0,
                primitives:  vec![NwnPrimitive {
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
                    material:        Some(0),
                }],
            }],
            materials:         vec![NwnMaterial {
                source_node:       0,
                render_enabled:    true,
                shadow_enabled:    true,
                beaming:           0,
                inherit_color:     0,
                tilefade:          0,
                rotate_texture:    0,
                transparency_hint: 0,
                shininess:         0.0,
                alpha:             1.0,
                ambient:           [0.0, 0.0, 0.0],
                diffuse:           [1.0, 1.0, 1.0],
                specular:          [0.0, 0.0, 0.0],
                self_illum_color:  [0.0, 0.0, 0.0],
                material_name:     None,
                render_hint:       None,
                helper_bitmap:     None,
                textures:          vec![NwnTextureRef {
                    slot: NwnTextureSlot::Bitmap,
                    name: "null".to_string(),
                }],
            }],
            animations:        vec![NwnAnimation {
                name:            name.to_string(),
                model_name:      "demo".to_string(),
                length:          1.0,
                transition_time: 0.0,
                root_name:       None,
                root_node:       None,
                events:          Vec::<AnimationEvent>::new(),
                node_tracks:     vec![NwnNodeAnimationTrack {
                    target_name: "mesh".to_string(),
                    target_node: Some(0),
                    kind:        NodeKind::Trimesh,
                    transform:   NwnTransformTrack {
                        translation_keys:         vec![
                            Vec3Key {
                                time:  0.0,
                                value: [5.0, 0.0, 0.0],
                            },
                            Vec3Key {
                                time:  1.0,
                                value: [7.0, 0.0, 0.0],
                            },
                        ],
                        rotation_axis_angle_keys: vec![Vec4Key {
                            time:  0.0,
                            value: [0.0, 1.0, 0.0, 0.0],
                        }],
                        scale_keys:               vec![Vec3Key {
                            time:  0.0,
                            value: [1.0, 1.0, 1.0],
                        }],
                    },
                    material:    NwnMaterialTrack {
                        color_keys:            Vec::new(),
                        radius_keys:           Vec::<ScalarKey>::new(),
                        alpha_keys:            Vec::<ScalarKey>::new(),
                        self_illum_color_keys: Vec::new(),
                    },
                    animmesh:    Some(NwnAnimMeshTrack {
                        sample_period:  Some(1.0),
                        face_overrides: Vec::new(),
                        vertex_samples: vec![
                            NwnVec3Sample {
                                values: vec![[5.0, 0.0, 0.0], [6.0, 0.0, 0.0], [5.0, 1.0, 0.0]],
                            },
                            NwnVec3Sample {
                                values: vec![[7.0, 0.0, 0.0], [8.0, 0.0, 0.0], [7.0, 1.0, 0.0]],
                            },
                        ],
                        uv_samples:     vec![NwnVec2Sample {
                            values: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
                        }],
                    }),
                }],
            }],
            diagnostics:       Vec::new(),
        }
    }

    fn skinned_scene_with_animation() -> NwnScene {
        NwnScene {
            name:              "skin_demo".to_string(),
            supermodel:        None,
            classification:    None,
            animation_scale:   None,
            coordinate_system: NwnCoordinateSystem::AuroraSource,
            nodes:             vec![
                NwnSceneNode {
                    kind:            NodeKind::Dummy,
                    node_type:       "dummy".to_string(),
                    name:            "root".to_string(),
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
                    mesh:            None,
                },
                NwnSceneNode {
                    kind:            NodeKind::Dummy,
                    node_type:       "dummy".to_string(),
                    name:            "bone".to_string(),
                    parent:          Some(0),
                    part_number:     None,
                    local_transform: NwnTransform {
                        translation:         [1.0, 0.0, 0.0],
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
                },
                NwnSceneNode {
                    kind:            NodeKind::Skin,
                    node_type:       "skin".to_string(),
                    name:            "skin".to_string(),
                    parent:          Some(0),
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
                },
            ],
            meshes:            vec![NwnMesh {
                name:        "skin".to_string(),
                source_node: 2,
                primitives:  vec![NwnPrimitive {
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
                    weight_rows:     vec![vec![crate::NwnSkinWeight {
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
            animations:        vec![NwnAnimation {
                name:            "move".to_string(),
                model_name:      "skin_demo".to_string(),
                length:          1.0,
                transition_time: 0.0,
                root_name:       None,
                root_node:       None,
                events:          Vec::new(),
                node_tracks:     vec![NwnNodeAnimationTrack {
                    target_name: "bone".to_string(),
                    target_node: Some(1),
                    kind:        NodeKind::Dummy,
                    transform:   NwnTransformTrack {
                        translation_keys:         vec![Vec3Key {
                            time:  1.0,
                            value: [2.0, 0.0, 0.0],
                        }],
                        rotation_axis_angle_keys: Vec::new(),
                        scale_keys:               Vec::new(),
                    },
                    material:    NwnMaterialTrack {
                        color_keys:            Vec::new(),
                        radius_keys:           Vec::new(),
                        alpha_keys:            Vec::new(),
                        self_illum_color_keys: Vec::new(),
                    },
                    animmesh:    None,
                }],
            }],
            diagnostics:       Vec::new(),
        }
    }
}
