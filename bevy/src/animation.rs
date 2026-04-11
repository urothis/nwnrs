use bevy::{
    asset::{Assets, Handle},
    math::{Affine2, Mat2},
    mesh::{Mesh, Mesh3d},
    pbr::{MeshMaterial3d, StandardMaterial},
    prelude::{
        AlphaMode, Color, Component, GlobalTransform, Quat, Query, Res, ResMut, Time, Transform,
        Vec2, Vec3,
    },
};
use nwnrs_mdl::prelude::*;

use crate::{
    NwnAreaWind, NwnModelTxiAsset, NwnModelTxiProcedureAsset, NwnPrimitiveAsset, position_from_nwn,
    transform_from_nwn,
};

#[derive(Component, Debug, Clone)]
pub(crate) struct NwnAnimatedTransform {
    base_transform:    NwnTransform,
    coordinate_system: NwnCoordinateSystem,
    animation_length:  f32,
    transform_track:   NwnTransformTrack,
}

#[derive(Debug, Clone)]
struct NwnAnimatedMaterialBinding {
    handle:                Handle<StandardMaterial>,
    base_color:            [f32; 3],
    base_alpha:            f32,
    base_self_illum_color: [f32; 3],
}

#[derive(Component, Debug, Clone)]
pub(crate) struct NwnAnimatedMaterial {
    animation_length: f32,
    material_track:   NwnMaterialTrack,
    bindings:         Vec<NwnAnimatedMaterialBinding>,
}

#[derive(Component, Debug, Clone)]
pub(crate) struct NwnAnimatedTxiMaterial {
    base_uv_transform:      Affine2,
    base_material:          Handle<StandardMaterial>,
    instance_material:      Option<Handle<StandardMaterial>>,
    txi:                    NwnModelTxiAsset,
    uv_to_local_horizontal: Option<Affine2>,
}

#[derive(Component, Debug, Clone)]
pub(crate) struct NwnAnimatedMesh {
    animation_length:  f32,
    sample_period:     Option<f32>,
    coordinate_system: NwnCoordinateSystem,
    base_mesh:         Handle<Mesh>,
    instance_mesh:     Option<Handle<Mesh>>,
    base_positions:    Vec<[f32; 3]>,
    base_uvs:          Vec<[f32; 2]>,
    position_indices:  Vec<usize>,
    uv_indices:        Vec<Option<usize>>,
    vertex_samples:    Vec<NwnVec3Sample>,
    uv_samples:        Vec<NwnVec2Sample>,
}

pub(crate) fn attach_model_animation_components(
    commands: &mut bevy::prelude::Commands<'_, '_>,
    model: &crate::NwnModelAsset,
    node_entities: &[bevy::prelude::Entity],
    primitive_entities: &[Vec<bevy::prelude::Entity>],
    selected_animation: Option<&str>,
) {
    let Some(animation) = select_model_animation(&model.scene, selected_animation) else {
        return;
    };
    if animation.length <= 0.0 {
        return;
    }

    for (node_index, node) in model.scene.nodes.iter().enumerate() {
        let Some(track) = animation_track_for_node(animation, node_index, node.name.as_str())
        else {
            continue;
        };
        let Some(&entity) = node_entities.get(node_index) else {
            continue;
        };
        let mut entity = commands.entity(entity);
        if has_transform_animation(track) {
            entity.insert(NwnAnimatedTransform {
                base_transform:    node.local_transform.clone(),
                coordinate_system: model.scene.coordinate_system,
                animation_length:  animation.length,
                transform_track:   track.transform.clone(),
            });
        }
        if let Some(animated_material) =
            build_animated_material(model, node_index, track, animation.length)
        {
            entity.insert(animated_material);
        }
        attach_animated_meshes(
            commands,
            model,
            node_index,
            track,
            animation.length,
            primitive_entities,
        );
    }
}

pub(crate) fn animate_nwn_model_transforms(
    time: Res<'_, Time>,
    mut animated_nodes: Query<'_, '_, (&NwnAnimatedTransform, &mut Transform)>,
) {
    let elapsed = time.elapsed_secs();
    for (animated, mut transform) in &mut animated_nodes {
        *transform = sample_animated_transform(animated, elapsed);
    }
}

pub(crate) fn animate_nwn_model_materials(
    time: Res<'_, Time>,
    mut materials: ResMut<'_, Assets<StandardMaterial>>,
    animated_nodes: Query<'_, '_, &NwnAnimatedMaterial>,
) {
    let elapsed = time.elapsed_secs();
    for animated in &animated_nodes {
        apply_animated_material(animated, &mut materials, elapsed);
    }
}

pub(crate) fn animate_nwn_txi_materials(
    time: Res<'_, Time>,
    wind: Res<'_, NwnAreaWind>,
    mut materials: ResMut<'_, Assets<StandardMaterial>>,
    mut animated_primitives: Query<
        '_,
        '_,
        (
            &GlobalTransform,
            &mut MeshMaterial3d<StandardMaterial>,
            &mut NwnAnimatedTxiMaterial,
        ),
    >,
) {
    let elapsed = time.elapsed_secs();
    for (global_transform, mut material_handle, mut animated) in &mut animated_primitives {
        let Some(instance_handle) =
            ensure_instance_material(&mut material_handle, &mut animated, &mut materials)
        else {
            continue;
        };
        let Some(material) = materials.get_mut(&instance_handle) else {
            continue;
        };
        apply_txi_material(&animated, global_transform, material, elapsed, *wind);
    }
}

pub(crate) fn animate_nwn_model_meshes(
    time: Res<'_, Time>,
    mut meshes: ResMut<'_, Assets<Mesh>>,
    mut animated_meshes: Query<'_, '_, (&mut Mesh3d, &mut NwnAnimatedMesh)>,
) {
    let elapsed = time.elapsed_secs();
    for (mut mesh3d, mut animated) in &mut animated_meshes {
        let Some(mesh_handle) = ensure_instance_mesh(&mut mesh3d, &mut animated, &mut meshes)
        else {
            continue;
        };
        let Some(mesh) = meshes.get_mut(&mesh_handle) else {
            continue;
        };
        apply_animated_mesh(&animated, mesh, elapsed);
    }
}

fn select_model_animation<'a>(
    scene: &'a NwnScene,
    selected_animation: Option<&str>,
) -> Option<&'a NwnAnimation> {
    if let Some(name) = selected_animation {
        return scene.animation(name);
    }

    if let Some(default_animation) = scene.animation("default") {
        return Some(default_animation);
    }

    if scene.animations.len() == 1 {
        return scene.animations.first();
    }

    None
}

fn animation_track_for_node<'a>(
    animation: &'a NwnAnimation,
    node_index: usize,
    node_name: &str,
) -> Option<&'a NwnNodeAnimationTrack> {
    animation.node_tracks.iter().find(|track| {
        track.target_node == Some(node_index)
            || (track.target_node.is_none() && track.target_name.eq_ignore_ascii_case(node_name))
    })
}

fn has_transform_animation(track: &NwnNodeAnimationTrack) -> bool {
    !track.transform.translation_keys.is_empty()
        || !track.transform.rotation_axis_angle_keys.is_empty()
        || !track.transform.scale_keys.is_empty()
}

fn has_material_animation(track: &NwnNodeAnimationTrack) -> bool {
    !track.material.color_keys.is_empty()
        || !track.material.alpha_keys.is_empty()
        || !track.material.self_illum_color_keys.is_empty()
}

fn build_animated_material(
    model: &crate::NwnModelAsset,
    node_index: usize,
    track: &NwnNodeAnimationTrack,
    animation_length: f32,
) -> Option<NwnAnimatedMaterial> {
    if !has_material_animation(track) {
        return None;
    }

    let scene_node = model.scene.nodes.get(node_index)?;
    let mesh_index = scene_node.mesh?;
    let mesh = model.scene.meshes.get(mesh_index)?;
    let node_assets = model.nodes.get(node_index)?;
    let mut bindings = Vec::new();

    for (primitive_asset, primitive) in node_assets.primitives.iter().zip(mesh.primitives.iter()) {
        let Some(material_index) = primitive.material else {
            continue;
        };
        let Some(source_material) = model.scene.materials.get(material_index) else {
            continue;
        };
        bindings.push(NwnAnimatedMaterialBinding {
            handle:                primitive_asset.material.clone(),
            base_color:            source_material.diffuse,
            base_alpha:            source_material.alpha,
            base_self_illum_color: source_material.self_illum_color,
        });
    }

    if bindings.is_empty() {
        None
    } else {
        Some(NwnAnimatedMaterial {
            animation_length,
            material_track: track.material.clone(),
            bindings,
        })
    }
}

fn attach_animated_meshes(
    commands: &mut bevy::prelude::Commands<'_, '_>,
    model: &crate::NwnModelAsset,
    node_index: usize,
    track: &NwnNodeAnimationTrack,
    animation_length: f32,
    primitive_entities: &[Vec<bevy::prelude::Entity>],
) {
    let Some(animmesh) = track.animmesh.as_ref() else {
        return;
    };
    if animmesh.vertex_samples.is_empty() && animmesh.uv_samples.is_empty() {
        return;
    }

    let Some(scene_node) = model.scene.nodes.get(node_index) else {
        return;
    };
    let Some(mesh_index) = scene_node.mesh else {
        return;
    };
    let Some(scene_mesh) = model.scene.meshes.get(mesh_index) else {
        return;
    };
    let Some(node_asset) = model.nodes.get(node_index) else {
        return;
    };
    let Some(node_primitives) = primitive_entities.get(node_index) else {
        return;
    };

    for (primitive_asset, &primitive_entity) in
        node_asset.primitives.iter().zip(node_primitives.iter())
    {
        let Some(scene_primitive) = scene_mesh
            .primitives
            .get(primitive_asset.scene_primitive_index)
        else {
            continue;
        };
        let Some(animated_mesh) = build_animated_mesh(
            primitive_asset,
            scene_primitive,
            animmesh,
            model.scene.coordinate_system,
            animation_length,
        ) else {
            continue;
        };
        commands.entity(primitive_entity).insert(animated_mesh);
    }
}

fn build_animated_mesh(
    primitive_asset: &NwnPrimitiveAsset,
    primitive: &NwnPrimitive,
    animmesh: &NwnAnimMeshTrack,
    coordinate_system: NwnCoordinateSystem,
    animation_length: f32,
) -> Option<NwnAnimatedMesh> {
    let effective_faces = if animmesh.face_overrides.is_empty() {
        primitive.faces.as_slice()
    } else {
        animmesh.face_overrides.as_slice()
    };
    if effective_faces.is_empty() {
        return None;
    }

    let mut position_indices = Vec::with_capacity(effective_faces.len() * 3);
    let mut uv_indices = Vec::with_capacity(effective_faces.len() * 3);
    for face in effective_faces {
        for corner in 0..3 {
            let position_index = usize::try_from(*face.vertex_indices.get(corner)?).ok()?;
            position_indices.push(position_index);
            let uv_index = face
                .uv_indices
                .get(corner)
                .copied()
                .and_then(|raw| usize::try_from(raw).ok());
            uv_indices.push(uv_index);
        }
    }

    let base_uvs = primitive
        .uv_sets
        .first()
        .map(|set| set.coordinates.clone())
        .unwrap_or_default();

    Some(NwnAnimatedMesh {
        animation_length,
        sample_period: animmesh.sample_period,
        coordinate_system,
        base_mesh: primitive_asset.mesh.clone(),
        instance_mesh: None,
        base_positions: primitive.positions.clone(),
        base_uvs,
        position_indices,
        uv_indices,
        vertex_samples: animmesh.vertex_samples.clone(),
        uv_samples: animmesh.uv_samples.clone(),
    })
}

pub(crate) fn animated_txi_material_component(
    primitive: &NwnPrimitiveAsset,
) -> Option<NwnAnimatedTxiMaterial> {
    let txi = primitive.txi.as_ref()?;
    if txi.procedure.is_none() {
        return None;
    }

    Some(NwnAnimatedTxiMaterial {
        base_uv_transform:      Affine2::IDENTITY,
        base_material:          Handle::<StandardMaterial>::default(),
        instance_material:      None,
        txi:                    txi.clone(),
        uv_to_local_horizontal: primitive.txi_uv_to_local_horizontal,
    })
}

fn sample_animated_transform(animated: &NwnAnimatedTransform, elapsed: f32) -> Transform {
    let sample_time = if animated.animation_length > 0.0 {
        elapsed.rem_euclid(animated.animation_length)
    } else {
        0.0
    };
    let translation = sample_vec3_keys(
        &animated.transform_track.translation_keys,
        sample_time,
        animated.base_transform.translation,
    );
    let scale = sample_vec3_keys(
        &animated.transform_track.scale_keys,
        sample_time,
        animated.base_transform.scale,
    );
    let rotation = sample_rotation_keys(
        &animated.transform_track.rotation_axis_angle_keys,
        sample_time,
        animated.base_transform.rotation_axis_angle,
        animated.coordinate_system,
    );

    let mut transform = transform_from_nwn(
        &NwnTransform {
            translation,
            rotation_axis_angle: animated.base_transform.rotation_axis_angle,
            scale,
        },
        animated.coordinate_system,
    );
    transform.rotation = rotation;
    transform
}

fn apply_animated_material(
    animated: &NwnAnimatedMaterial,
    materials: &mut Assets<StandardMaterial>,
    elapsed: f32,
) {
    let sample_time = if animated.animation_length > 0.0 {
        elapsed.rem_euclid(animated.animation_length)
    } else {
        0.0
    };

    for binding in &animated.bindings {
        let Some(material) = materials.get_mut(&binding.handle) else {
            continue;
        };
        let color = sample_vec3_keys(
            &animated.material_track.color_keys,
            sample_time,
            binding.base_color,
        );
        let alpha = sample_scalar_keys(
            &animated.material_track.alpha_keys,
            sample_time,
            binding.base_alpha,
        )
        .clamp(0.0, 1.0);
        let self_illum_color = sample_vec3_keys(
            &animated.material_track.self_illum_color_keys,
            sample_time,
            binding.base_self_illum_color,
        );
        material.base_color = Color::srgba(color[0], color[1], color[2], alpha);
        material.emissive = Color::srgb(
            self_illum_color[0],
            self_illum_color[1],
            self_illum_color[2],
        )
        .into();
        material.alpha_mode = if alpha < 0.999 {
            AlphaMode::Blend
        } else {
            AlphaMode::Opaque
        };
    }
}

fn ensure_instance_material(
    material_handle: &mut MeshMaterial3d<StandardMaterial>,
    animated: &mut NwnAnimatedTxiMaterial,
    materials: &mut Assets<StandardMaterial>,
) -> Option<Handle<StandardMaterial>> {
    if animated.base_material == Handle::<StandardMaterial>::default() {
        animated.base_material = material_handle.0.clone();
    }
    if let Some(handle) = animated.instance_material.clone() {
        return Some(handle);
    }

    let source_material = materials.get(&animated.base_material)?.clone();
    animated.base_uv_transform = source_material.uv_transform;
    let handle = materials.add(source_material);
    material_handle.0 = handle.clone();
    animated.instance_material = Some(handle.clone());
    Some(handle)
}

fn apply_txi_material(
    animated: &NwnAnimatedTxiMaterial,
    global_transform: &GlobalTransform,
    material: &mut StandardMaterial,
    elapsed: f32,
    wind: NwnAreaWind,
) {
    let static_uv_transform = txi_static_uv_transform(animated, global_transform);
    material.uv_transform = animated.base_uv_transform
        * txi_uv_transform(&animated.txi, elapsed, wind)
        * static_uv_transform;
}

fn txi_uv_transform(txi: &NwnModelTxiAsset, elapsed: f32, wind: NwnAreaWind) -> Affine2 {
    match &txi.procedure {
        Some(NwnModelTxiProcedureAsset::Arturo {
            channel_scale,
            channel_translate,
            arturo_width,
            arturo_height,
            default_width,
            default_height,
            speed,
            ..
        }) => {
            let scale = if channel_scale.len() >= 4 {
                Vec2::new(1.0 + channel_scale[2], 1.0 + channel_scale[3])
            } else if channel_scale.len() >= 2 {
                Vec2::new(1.0 + channel_scale[0], 1.0 + channel_scale[1])
            } else {
                Vec2::ONE
            };
            let velocity = if channel_translate.len() >= 4 {
                Vec2::new(channel_translate[2], channel_translate[3])
            } else if channel_translate.len() >= 2 {
                Vec2::new(channel_translate[0], channel_translate[1])
            } else {
                Vec2::ZERO
            };
            let dimensions = Vec2::new(
                arturo_width.or(*default_width).unwrap_or(64).max(1) as f32,
                arturo_height.or(*default_height).unwrap_or(64).max(1) as f32,
            );
            let canonical_uv_to_world_horizontal = Mat2::from_cols(Vec2::X, -Vec2::Y);
            let canonical_world_to_uv = canonical_uv_to_world_horizontal.inverse();
            let wind_aligned_velocity = if wind.magnitude > 0.0 {
                canonical_world_to_uv
                    * wind
                        .direction
                        .try_normalize()
                        .unwrap_or(Vec2::new(1.0, -1.0).normalize())
                    * velocity.length()
                    * wind.magnitude
            } else {
                velocity
            };
            let offset =
                (wind_aligned_velocity * elapsed * speed.unwrap_or(1.0) / dimensions).fract();
            Affine2::from_scale(scale) * Affine2::from_translation(offset)
        }
        None => Affine2::IDENTITY,
    }
}

fn txi_static_uv_transform(
    animated: &NwnAnimatedTxiMaterial,
    global_transform: &GlobalTransform,
) -> Affine2 {
    let Some(uv_to_local_horizontal) = animated.uv_to_local_horizontal else {
        return Affine2::IDENTITY;
    };
    let canonical_uv_to_world_horizontal = Mat2::from_cols(Vec2::X, -Vec2::Y);
    let canonical_world_to_uv = canonical_uv_to_world_horizontal.inverse();
    let Some(world_from_local) = world_from_local_horizontal(global_transform) else {
        return Affine2::IDENTITY;
    };
    Affine2::from_mat2(canonical_world_to_uv) * world_from_local * uv_to_local_horizontal
}

fn world_from_local_horizontal(global_transform: &GlobalTransform) -> Option<Affine2> {
    let transform = global_transform.compute_transform();
    let world_x = transform.rotation * (Vec3::X * transform.scale.x);
    let world_z = transform.rotation * (Vec3::Z * transform.scale.z);
    let basis = Mat2::from_cols(
        Vec2::new(world_x.x, world_x.z),
        Vec2::new(world_z.x, world_z.z),
    );
    if basis.determinant().abs() <= f32::EPSILON {
        return None;
    }

    Some(Affine2::from_mat2_translation(
        basis,
        Vec2::new(transform.translation.x, transform.translation.z),
    ))
}

fn ensure_instance_mesh(
    mesh3d: &mut Mesh3d,
    animated: &mut NwnAnimatedMesh,
    meshes: &mut Assets<Mesh>,
) -> Option<Handle<Mesh>> {
    if let Some(handle) = animated.instance_mesh.clone() {
        return Some(handle);
    }

    let source_mesh = meshes.get(&animated.base_mesh)?.clone();
    let handle = meshes.add(source_mesh);
    mesh3d.0 = handle.clone();
    animated.instance_mesh = Some(handle.clone());
    Some(handle)
}

fn apply_animated_mesh(animated: &NwnAnimatedMesh, mesh: &mut Mesh, elapsed: f32) {
    let positions = sample_mesh_positions(animated, elapsed);
    let normals = compute_triangle_normals(&positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);

    if !animated.uv_indices.is_empty() {
        let uvs = sample_mesh_uvs(animated, elapsed);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    }
}

fn sample_mesh_positions(animated: &NwnAnimatedMesh, elapsed: f32) -> Vec<[f32; 3]> {
    let sample = sample_vec3_frames(
        &animated.vertex_samples,
        animated.sample_period,
        animated.animation_length,
        &animated.base_positions,
        elapsed,
    );
    animated
        .position_indices
        .iter()
        .map(|&index| {
            let position = sample.get(index).copied().unwrap_or([0.0, 0.0, 0.0]);
            position_from_nwn(position, animated.coordinate_system)
        })
        .collect()
}

fn sample_mesh_uvs(animated: &NwnAnimatedMesh, elapsed: f32) -> Vec<[f32; 2]> {
    let sample = sample_vec2_frames(
        &animated.uv_samples,
        animated.sample_period,
        animated.animation_length,
        &animated.base_uvs,
        elapsed,
    );
    animated
        .uv_indices
        .iter()
        .map(|index| {
            index
                .and_then(|index| sample.get(index).copied())
                .unwrap_or([0.0, 0.0])
        })
        .collect()
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
            .map(|sample| sample.values.clone())
            .unwrap_or_else(|| fallback.to_vec());
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
            .map(|sample| sample.values.clone())
            .unwrap_or_else(|| fallback.to_vec());
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
            let phase_time = if animation_length > 0.0 {
                elapsed.rem_euclid(animation_length)
            } else {
                elapsed.max(0.0)
            };
            let derived_period = sample_period
                .filter(|period| *period > f32::EPSILON)
                .unwrap_or_else(|| (animation_length / sample_count as f32).max(f32::EPSILON));
            let cycle_duration = derived_period * sample_count as f32;
            let cycle_time = if cycle_duration > f32::EPSILON {
                phase_time.rem_euclid(cycle_duration)
            } else {
                0.0
            };
            let current = (cycle_time / derived_period).floor() as usize % sample_count;
            let next = (current + 1) % sample_count;
            let current_start = current as f32 * derived_period;
            let factor = ((cycle_time - current_start) / derived_period).clamp(0.0, 1.0);
            Some((current, next, factor))
        }
    }
}

fn compute_triangle_normals(positions: &[[f32; 3]]) -> Vec<[f32; 3]> {
    let mut normals = Vec::with_capacity(positions.len());
    for triangle in positions.chunks(3) {
        let normal = if triangle.len() == 3 {
            let a = bevy::math::Vec3::from_array(triangle[0]);
            let b = bevy::math::Vec3::from_array(triangle[1]);
            let c = bevy::math::Vec3::from_array(triangle[2]);
            (b - a)
                .cross(c - a)
                .try_normalize()
                .unwrap_or(bevy::math::Vec3::Y)
                .to_array()
        } else {
            bevy::math::Vec3::Y.to_array()
        };
        normals.extend(std::iter::repeat_n(normal, triangle.len()));
    }
    normals
}

fn sample_scalar_keys(keys: &[ScalarKey], time: f32, fallback: f32) -> f32 {
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
            return start.value + (end.value - start.value) * factor;
        }
    }

    last.value
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

fn sample_rotation_keys(
    keys: &[Vec4Key],
    time: f32,
    fallback: [f32; 4],
    coordinate_system: NwnCoordinateSystem,
) -> Quat {
    let Some(first) = keys.first() else {
        return quat_from_nwn_axis_angle(fallback, coordinate_system);
    };
    if keys.len() == 1 || time <= first.time {
        return quat_from_nwn_axis_angle(first.value, coordinate_system);
    }
    let last = keys.last().unwrap_or(first);
    if time >= last.time {
        return quat_from_nwn_axis_angle(last.value, coordinate_system);
    }

    for window in keys.windows(2) {
        let [start, end] = window else {
            continue;
        };
        if time <= end.time {
            let duration = (end.time - start.time).max(f32::EPSILON);
            let factor = ((time - start.time) / duration).clamp(0.0, 1.0);
            let start_quat = quat_from_nwn_axis_angle(start.value, coordinate_system);
            let end_quat = quat_from_nwn_axis_angle(end.value, coordinate_system);
            return start_quat.slerp(end_quat, factor);
        }
    }

    quat_from_nwn_axis_angle(last.value, coordinate_system)
}

fn quat_from_nwn_axis_angle(
    rotation_axis_angle: [f32; 4],
    coordinate_system: NwnCoordinateSystem,
) -> Quat {
    transform_from_nwn(
        &NwnTransform {
            translation: [0.0, 0.0, 0.0],
            rotation_axis_angle,
            scale: [1.0, 1.0, 1.0],
        },
        coordinate_system,
    )
    .rotation
}

#[cfg(test)]
mod tests {
    use bevy::{
        asset::{Assets, Handle},
        color::LinearRgba,
        math::{Affine2, Mat2},
        mesh::Mesh,
        pbr::StandardMaterial,
        prelude::{AlphaMode, Color, EulerRot, GlobalTransform, Quat, Transform, Vec2, Vec3},
    };
    use nwnrs_mdl::prelude::{
        AnimationEvent, ModelClassification, NodeKind, NwnAnimation, NwnCoordinateSystem,
        NwnMaterialTrack, NwnScene, NwnSceneNode, NwnTransform, NwnTransformTrack, NwnVec2Sample,
        NwnVec3Sample, ScalarKey, Vec3Key, Vec4Key,
    };

    use super::{
        NwnAnimatedMaterial, NwnAnimatedMaterialBinding, NwnAnimatedMesh, NwnAnimatedTransform,
        NwnAnimatedTxiMaterial, apply_animated_material, sample_animated_transform,
        sample_mesh_positions, sample_mesh_uvs, select_model_animation, txi_static_uv_transform,
        txi_uv_transform,
    };
    use crate::{NwnAreaWind, NwnModelTxiAsset, NwnModelTxiProcedureAsset};

    #[test]
    fn select_model_animation_prefers_default_then_single_animation() {
        let default_scene =
            scene_with_animations(vec![animation("idle", 1.0), animation("default", 2.0)]);
        assert_eq!(
            select_model_animation(&default_scene, None).map(|animation| animation.name.as_str()),
            Some("default")
        );

        let single_scene = scene_with_animations(vec![animation("idle", 1.0)]);
        assert_eq!(
            select_model_animation(&single_scene, None).map(|animation| animation.name.as_str()),
            Some("idle")
        );

        let multi_scene =
            scene_with_animations(vec![animation("idle", 1.0), animation("walk", 1.0)]);
        assert!(select_model_animation(&multi_scene, None).is_none());
        assert_eq!(
            select_model_animation(&multi_scene, Some("walk"))
                .map(|animation| animation.name.as_str()),
            Some("walk")
        );
    }

    #[test]
    fn sample_animated_transform_loops_translation_rotation_and_scale() {
        let animated = NwnAnimatedTransform {
            base_transform:    NwnTransform {
                translation:         [0.0, 0.0, 0.0],
                rotation_axis_angle: [0.0, 0.0, 1.0, 0.0],
                scale:               [1.0, 1.0, 1.0],
            },
            coordinate_system: NwnCoordinateSystem::AuroraSource,
            animation_length:  1.0,
            transform_track:   NwnTransformTrack {
                translation_keys:         vec![
                    Vec3Key {
                        time:  0.0,
                        value: [0.0, 0.0, 0.0],
                    },
                    Vec3Key {
                        time:  1.0,
                        value: [0.0, 2.0, 0.0],
                    },
                ],
                rotation_axis_angle_keys: vec![
                    Vec4Key {
                        time:  0.0,
                        value: [0.0, 0.0, 1.0, 0.0],
                    },
                    Vec4Key {
                        time:  1.0,
                        value: [0.0, 0.0, 1.0, std::f32::consts::PI],
                    },
                ],
                scale_keys:               vec![
                    Vec3Key {
                        time:  0.0,
                        value: [1.0, 1.0, 1.0],
                    },
                    Vec3Key {
                        time:  1.0,
                        value: [2.0, 2.0, 2.0],
                    },
                ],
            },
        };

        let transform = sample_animated_transform(&animated, 1.5);
        assert!((transform.translation.z + 1.0).abs() < 0.001);
        assert!((transform.scale.x - 1.5).abs() < 0.001);
        let forward = transform.rotation * Vec3::NEG_Z;
        assert!(forward.z.abs() < 0.01);
        assert!((forward.x.abs() - 1.0).abs() < 0.01);
    }

    #[test]
    fn apply_animated_material_loops_color_alpha_and_self_illumination() {
        let mut materials = Assets::<StandardMaterial>::default();
        let handle = materials.add(StandardMaterial {
            base_color: Color::srgba(0.2, 0.4, 0.6, 1.0),
            emissive: Color::BLACK.into(),
            alpha_mode: AlphaMode::Opaque,
            ..Default::default()
        });
        let animated = NwnAnimatedMaterial {
            animation_length: 1.0,
            material_track:   NwnMaterialTrack {
                color_keys:            vec![
                    Vec3Key {
                        time:  0.0,
                        value: [0.2, 0.4, 0.6],
                    },
                    Vec3Key {
                        time:  1.0,
                        value: [0.8, 0.2, 0.4],
                    },
                ],
                radius_keys:           Vec::new(),
                alpha_keys:            vec![
                    ScalarKey {
                        time:  0.0,
                        value: 1.0,
                    },
                    ScalarKey {
                        time:  1.0,
                        value: 0.2,
                    },
                ],
                self_illum_color_keys: vec![
                    Vec3Key {
                        time:  0.0,
                        value: [0.0, 0.0, 0.0],
                    },
                    Vec3Key {
                        time:  1.0,
                        value: [0.6, 0.3, 0.1],
                    },
                ],
            },
            bindings:         vec![NwnAnimatedMaterialBinding {
                handle:                handle.clone(),
                base_color:            [0.2, 0.4, 0.6],
                base_alpha:            1.0,
                base_self_illum_color: [0.0, 0.0, 0.0],
            }],
        };

        apply_animated_material(&animated, &mut materials, 1.5);

        let material = materials
            .get(&handle)
            .unwrap_or_else(|| panic!("missing material"));
        let color = material.base_color.to_srgba();
        assert!((color.red - 0.5).abs() < 0.01);
        assert!((color.green - 0.3).abs() < 0.01);
        assert!((color.blue - 0.5).abs() < 0.01);
        assert!((color.alpha - 0.6).abs() < 0.01);
        assert!(matches!(material.alpha_mode, AlphaMode::Blend));
        let expected_emissive: LinearRgba = Color::srgb(0.3, 0.15, 0.05).into();
        assert!((material.emissive.red - expected_emissive.red).abs() < 0.01);
        assert!((material.emissive.green - expected_emissive.green).abs() < 0.01);
        assert!((material.emissive.blue - expected_emissive.blue).abs() < 0.01);
    }

    #[test]
    fn sample_animmesh_loops_positions_and_uvs() {
        let animated = NwnAnimatedMesh {
            animation_length:  2.0,
            sample_period:     Some(1.0),
            coordinate_system: NwnCoordinateSystem::AuroraSource,
            base_mesh:         Handle::<Mesh>::default(),
            instance_mesh:     None,
            base_positions:    vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            base_uvs:          vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
            position_indices:  vec![0, 1, 2],
            uv_indices:        vec![Some(0), Some(1), Some(2)],
            vertex_samples:    vec![
                NwnVec3Sample {
                    values: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                },
                NwnVec3Sample {
                    values: vec![[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 2.0, 0.0]],
                },
            ],
            uv_samples:        vec![
                NwnVec2Sample {
                    values: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
                },
                NwnVec2Sample {
                    values: vec![[0.5, 0.0], [1.0, 0.5], [0.5, 1.0]],
                },
            ],
        };

        let positions = sample_mesh_positions(&animated, 0.5);
        assert_eq!(positions.len(), 3);
        assert!((positions[1][0] - 1.5).abs() < 0.001);
        assert!((positions[2][2] + 1.5).abs() < 0.001);

        let uvs = sample_mesh_uvs(&animated, 0.5);
        assert_eq!(uvs.len(), 3);
        assert!((uvs[0][0] - 0.25).abs() < 0.001);
        assert!((uvs[1][1] - 0.25).abs() < 0.001);
    }

    #[test]
    fn arturo_txi_produces_wrapped_uv_translation() {
        let txi = NwnModelTxiAsset {
            rotate_texture:      1,
            bump_map_texture:    Some("shinywater".to_string()),
            bumpy_shiny_texture: Some("ttr01__env".to_string()),
            procedure:           Some(NwnModelTxiProcedureAsset::Arturo {
                channel_scale:        vec![0.0, 0.0, 0.0, 0.0],
                channel_translate:    vec![0.0, 0.0, 1.0, 0.5],
                distort:              Some(1),
                arturo_width:         Some(32),
                arturo_height:        Some(32),
                distortion_amplitude: Some(6.0),
                speed:                Some(20.0),
                default_height:       Some(64),
                default_width:        Some(64),
                alpha_mean:           Some(0.999),
            }),
        };

        let transform = txi_uv_transform(&txi, 1.0, NwnAreaWind::default());
        assert!((transform.translation.x - 0.625).abs() < 0.001);
        assert!((transform.translation.y - 0.3125).abs() < 0.001);
    }

    #[test]
    fn arturo_txi_scroll_is_independent_of_tile_rotation() {
        let txi = NwnModelTxiAsset {
            rotate_texture:      1,
            bump_map_texture:    Some("shinywater".to_string()),
            bumpy_shiny_texture: Some("ttr01__env".to_string()),
            procedure:           Some(NwnModelTxiProcedureAsset::Arturo {
                channel_scale:        vec![0.0, 0.0, 0.0, 0.0],
                channel_translate:    vec![0.0, 0.0, 1.0, 0.5],
                distort:              Some(1),
                arturo_width:         Some(32),
                arturo_height:        Some(32),
                distortion_amplitude: Some(6.0),
                speed:                Some(20.0),
                default_height:       Some(64),
                default_width:        Some(64),
                alpha_mean:           Some(0.999),
            }),
        };
        let transform = txi_uv_transform(&txi, 1.0, NwnAreaWind::default());
        assert!((transform.translation.x - 0.625).abs() < 0.001);
        assert!((transform.translation.y - 0.3125).abs() < 0.001);
    }

    #[test]
    fn arturo_txi_wind_overrides_raw_txi_direction() {
        let txi = NwnModelTxiAsset {
            rotate_texture:      1,
            bump_map_texture:    Some("shinywater".to_string()),
            bumpy_shiny_texture: Some("ttr01__env".to_string()),
            procedure:           Some(NwnModelTxiProcedureAsset::Arturo {
                channel_scale:        vec![0.0, 0.0, 0.0, 0.0],
                channel_translate:    vec![0.0, 0.0, 1.0, 0.5],
                distort:              Some(1),
                arturo_width:         Some(32),
                arturo_height:        Some(32),
                distortion_amplitude: Some(6.0),
                speed:                Some(20.0),
                default_height:       Some(64),
                default_width:        Some(64),
                alpha_mean:           Some(0.999),
            }),
        };
        let transform = txi_uv_transform(
            &txi,
            1.0,
            NwnAreaWind {
                direction: Vec2::new(1.0, 0.0),
                magnitude: 2.0,
            },
        );
        assert!((transform.translation.x - 0.3975).abs() < 0.001);
        assert!(transform.translation.y.abs() < 0.001);
    }

    #[test]
    fn rotate_texture_unrotates_uvs_by_world_yaw() {
        let animated = NwnAnimatedTxiMaterial {
            base_uv_transform:      Affine2::IDENTITY,
            base_material:          Handle::<StandardMaterial>::default(),
            instance_material:      None,
            txi:                    NwnModelTxiAsset {
                rotate_texture:      1,
                bump_map_texture:    None,
                bumpy_shiny_texture: None,
                procedure:           Some(NwnModelTxiProcedureAsset::Arturo {
                    channel_scale:        vec![],
                    channel_translate:    vec![],
                    distort:              None,
                    arturo_width:         None,
                    arturo_height:        None,
                    distortion_amplitude: None,
                    speed:                None,
                    default_height:       None,
                    default_width:        None,
                    alpha_mean:           None,
                }),
            },
            uv_to_local_horizontal: Some(Affine2::from_mat2(Mat2::from_cols(Vec2::X, -Vec2::Y))),
        };
        let rotated = GlobalTransform::from(Transform::from_rotation(Quat::from_rotation_y(
            std::f32::consts::FRAC_PI_2,
        )));

        let transform = txi_static_uv_transform(&animated, &rotated);
        let mapped_u = transform.matrix2 * Vec2::X;
        let mapped_v = transform.matrix2 * Vec2::Y;
        assert!((mapped_u.x - 0.0).abs() < 0.001);
        assert!((mapped_u.y - 1.0).abs() < 0.001);
        assert!((mapped_v.x + 1.0).abs() < 0.001);
        assert!((mapped_v.y - 0.0).abs() < 0.001);
    }

    #[test]
    fn rotate_texture_static_transform_preserves_world_phase_from_uv_origin() {
        let animated = NwnAnimatedTxiMaterial {
            base_uv_transform:      Affine2::IDENTITY,
            base_material:          Handle::<StandardMaterial>::default(),
            instance_material:      None,
            txi:                    NwnModelTxiAsset {
                rotate_texture:      1,
                bump_map_texture:    None,
                bumpy_shiny_texture: None,
                procedure:           Some(NwnModelTxiProcedureAsset::Arturo {
                    channel_scale:        vec![],
                    channel_translate:    vec![],
                    distort:              None,
                    arturo_width:         None,
                    arturo_height:        None,
                    distortion_amplitude: None,
                    speed:                None,
                    default_height:       None,
                    default_width:        None,
                    alpha_mean:           None,
                }),
            },
            uv_to_local_horizontal: Some(Affine2::from_mat2_translation(
                Mat2::from_cols(Vec2::X, -Vec2::Y),
                Vec2::new(2.0, 3.0),
            )),
        };
        let translated = GlobalTransform::from(Transform::from_translation(bevy::math::Vec3::new(
            10.0, 0.0, 20.0,
        )));

        let transform = txi_static_uv_transform(&animated, &translated);
        assert!((transform.translation.x - 12.0).abs() < 0.001);
        assert!((transform.translation.y + 23.0).abs() < 0.001);
    }

    #[test]
    fn rotate_texture_static_transform_preserves_horizontal_mirroring() {
        let animated = NwnAnimatedTxiMaterial {
            base_uv_transform:      Affine2::IDENTITY,
            base_material:          Handle::<StandardMaterial>::default(),
            instance_material:      None,
            txi:                    NwnModelTxiAsset {
                rotate_texture:      1,
                bump_map_texture:    None,
                bumpy_shiny_texture: None,
                procedure:           Some(NwnModelTxiProcedureAsset::Arturo {
                    channel_scale:        vec![],
                    channel_translate:    vec![],
                    distort:              None,
                    arturo_width:         None,
                    arturo_height:        None,
                    distortion_amplitude: None,
                    speed:                None,
                    default_height:       None,
                    default_width:        None,
                    alpha_mean:           None,
                }),
            },
            uv_to_local_horizontal: Some(Affine2::from_mat2(Mat2::from_cols(Vec2::X, -Vec2::Y))),
        };
        let mirrored = GlobalTransform::from(Transform {
            translation: Vec3::ZERO,
            rotation:    Quat::IDENTITY,
            scale:       Vec3::new(-1.0, 1.0, 1.0),
        });

        let transform = txi_static_uv_transform(&animated, &mirrored);
        let mapped_u = transform.matrix2 * Vec2::X;
        let mapped_v = transform.matrix2 * Vec2::Y;
        assert!((mapped_u.x + 1.0).abs() < 0.001);
        assert!(mapped_u.y.abs() < 0.001);
        assert!(mapped_v.x.abs() < 0.001);
        assert!((mapped_v.y - 1.0).abs() < 0.001);
    }

    #[test]
    fn rotate_texture_static_transform_uses_full_horizontal_basis() {
        let animated = NwnAnimatedTxiMaterial {
            base_uv_transform:      Affine2::IDENTITY,
            base_material:          Handle::<StandardMaterial>::default(),
            instance_material:      None,
            txi:                    NwnModelTxiAsset {
                rotate_texture:      1,
                bump_map_texture:    None,
                bumpy_shiny_texture: None,
                procedure:           Some(NwnModelTxiProcedureAsset::Arturo {
                    channel_scale:        vec![],
                    channel_translate:    vec![],
                    distort:              None,
                    arturo_width:         None,
                    arturo_height:        None,
                    distortion_amplitude: None,
                    speed:                None,
                    default_height:       None,
                    default_width:        None,
                    alpha_mean:           None,
                }),
            },
            uv_to_local_horizontal: Some(Affine2::from_mat2(Mat2::from_cols(Vec2::X, -Vec2::Y))),
        };
        let world = Transform {
            translation: Vec3::new(8.0, 3.0, -5.0),
            rotation:    Quat::from_euler(EulerRot::XYZ, 0.45, std::f32::consts::FRAC_PI_2, 0.0),
            scale:       Vec3::new(2.0, 1.0, 0.5),
        };
        let global = GlobalTransform::from(world);

        let transform = txi_static_uv_transform(&animated, &global);
        let canonical_world_to_uv = Mat2::from_cols(Vec2::X, -Vec2::Y).inverse();
        let world_x = world.rotation * (Vec3::X * world.scale.x);
        let world_z = world.rotation * (Vec3::Z * world.scale.z);
        let expected_basis = canonical_world_to_uv
            * Mat2::from_cols(
                Vec2::new(world_x.x, world_x.z),
                Vec2::new(world_z.x, world_z.z),
            )
            * Mat2::from_cols(Vec2::X, -Vec2::Y);
        let expected_translation =
            canonical_world_to_uv * Vec2::new(world.translation.x, world.translation.z);
        let mapped_u = transform.matrix2 * Vec2::X;
        let mapped_v = transform.matrix2 * Vec2::Y;
        let expected_u = expected_basis * Vec2::X;
        let expected_v = expected_basis * Vec2::Y;
        assert!((mapped_u.x - expected_u.x).abs() < 0.001);
        assert!((mapped_u.y - expected_u.y).abs() < 0.001);
        assert!((mapped_v.x - expected_v.x).abs() < 0.001);
        assert!((mapped_v.y - expected_v.y).abs() < 0.001);
        assert!((transform.translation.x - expected_translation.x).abs() < 0.001);
        assert!((transform.translation.y - expected_translation.y).abs() < 0.001);
    }

    #[test]
    fn procedure_txi_unrotates_uvs_even_without_rotate_texture_flag() {
        let animated = NwnAnimatedTxiMaterial {
            base_uv_transform:      Affine2::IDENTITY,
            base_material:          Handle::<StandardMaterial>::default(),
            instance_material:      None,
            txi:                    NwnModelTxiAsset {
                rotate_texture:      0,
                bump_map_texture:    None,
                bumpy_shiny_texture: None,
                procedure:           Some(NwnModelTxiProcedureAsset::Arturo {
                    channel_scale:        vec![],
                    channel_translate:    vec![],
                    distort:              None,
                    arturo_width:         None,
                    arturo_height:        None,
                    distortion_amplitude: None,
                    speed:                None,
                    default_height:       None,
                    default_width:        None,
                    alpha_mean:           None,
                }),
            },
            uv_to_local_horizontal: Some(Affine2::from_mat2(Mat2::from_cols(Vec2::X, -Vec2::Y))),
        };
        let rotated = GlobalTransform::from(Transform::from_rotation(Quat::from_rotation_y(
            std::f32::consts::FRAC_PI_2,
        )));

        let transform = txi_static_uv_transform(&animated, &rotated);
        let mapped_u = transform.matrix2 * Vec2::X;
        let mapped_v = transform.matrix2 * Vec2::Y;
        assert!((mapped_u.x - 0.0).abs() < 0.001);
        assert!((mapped_u.y - 1.0).abs() < 0.001);
        assert!((mapped_v.x + 1.0).abs() < 0.001);
        assert!((mapped_v.y - 0.0).abs() < 0.001);
    }

    fn scene_with_animations(animations: Vec<NwnAnimation>) -> NwnScene {
        NwnScene {
            name: "anim".to_string(),
            supermodel: None,
            classification: Some(ModelClassification::Item),
            animation_scale: Some(1.0),
            coordinate_system: NwnCoordinateSystem::AuroraSource,
            nodes: vec![NwnSceneNode {
                kind:            NodeKind::Dummy,
                node_type:       "dummy".to_string(),
                name:            "Node".to_string(),
                parent:          None,
                part_number:     None,
                local_transform: NwnTransform {
                    translation:         [0.0, 0.0, 0.0],
                    rotation_axis_angle: [0.0, 0.0, 1.0, 0.0],
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
            meshes: Vec::new(),
            materials: Vec::new(),
            animations,
            diagnostics: Vec::new(),
        }
    }

    fn animation(name: &str, length: f32) -> NwnAnimation {
        NwnAnimation {
            name: name.to_string(),
            model_name: "anim".to_string(),
            length,
            transition_time: 0.0,
            root_name: None,
            root_node: None,
            events: Vec::<AnimationEvent>::new(),
            node_tracks: vec![nwnrs_mdl::prelude::NwnNodeAnimationTrack {
                target_name: "Node".to_string(),
                target_node: Some(0),
                kind:        NodeKind::Dummy,
                transform:   NwnTransformTrack {
                    translation_keys:         Vec::new(),
                    rotation_axis_angle_keys: Vec::new(),
                    scale_keys:               Vec::new(),
                },
                material:    nwnrs_mdl::prelude::NwnMaterialTrack {
                    color_keys:            Vec::new(),
                    radius_keys:           Vec::<ScalarKey>::new(),
                    alpha_keys:            Vec::<ScalarKey>::new(),
                    self_illum_color_keys: Vec::new(),
                },
                animmesh:    None,
            }],
        }
    }
}
