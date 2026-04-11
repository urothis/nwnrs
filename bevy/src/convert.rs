use bevy::{
    asset::{Handle, RenderAssetUsages},
    image::{Image, ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    mesh::{Indices, Mesh, PrimitiveTopology},
    pbr::StandardMaterial,
    prelude::{AlphaMode, Color, Vec3},
    render::render_resource::{Extent3d, Face, TextureDimension, TextureFormat},
    transform::components::Transform,
};
use nwnrs_dds::prelude::*;
use nwnrs_mdl::prelude::*;
use nwnrs_plt::prelude::*;
use nwnrs_tga::prelude::*;

use crate::{
    NwnBevyError, NwnModelHelperSurfaceAsset, NwnModelTileFadeAsset, tilefade_default_visibility,
};

/// Returns whether the given scene material is used by renderable geometry
/// that can actually sample a primary bitmap texture.
pub fn material_requires_bitmap_resolution(scene: &NwnScene, material_index: usize) -> bool {
    scene.nodes.iter().any(|node| {
        if matches!(node.kind, NodeKind::Aabb) {
            return false;
        }
        let Some(mesh_index) = node.mesh else {
            return false;
        };
        let Some(mesh) = scene.meshes.get(mesh_index) else {
            return false;
        };
        mesh.primitives.iter().any(|primitive| {
            primitive.material == Some(material_index)
                && primitive
                    .uv_sets
                    .first()
                    .is_some_and(|set| !set.coordinates.is_empty())
        })
    })
}

/// Returns whether a primitive using this material should start visible in
/// Bevy even if the authored material disables rendering.
pub fn material_starts_visible(scene: &NwnScene, material: &NwnMaterial) -> bool {
    if matches!(scene.classification, Some(ModelClassification::Tile)) && material.tilefade > 0 {
        tilefade_default_visibility(material.tilefade, material.render_enabled)
    } else {
        material.render_enabled
    }
}

/// Computes primitive bounds in local Bevy space after NWN coordinate
/// conversion.
pub fn primitive_bounds_from_nwn(
    primitive: &NwnPrimitive,
    coordinate_system: NwnCoordinateSystem,
) -> (Vec3, Vec3) {
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for position in &primitive.positions {
        let point = Vec3::from_array(position_from_nwn(*position, coordinate_system));
        min = min.min(point);
        max = max.max(point);
    }

    if !min.is_finite() || !max.is_finite() {
        return (Vec3::ZERO, Vec3::ZERO);
    }

    ((min + max) * 0.5, (max - min) * 0.5)
}

/// Returns helper-surface metadata for one helper node, when present.
pub fn helper_surface_from_node(
    scene: &NwnScene,
    node: &NwnSceneNode,
) -> Option<NwnModelHelperSurfaceAsset> {
    if !matches!(node.kind, NodeKind::Aabb) {
        return None;
    }

    let mesh = scene.meshes.get(node.mesh?)?;
    let mut bitmaps = Vec::new();
    let mut surface_labels = Vec::new();
    let mut texture_names = Vec::new();

    for primitive in &mesh.primitives {
        if let Some(material_index) = primitive.material
            && let Some(bitmap) = scene
                .materials
                .get(material_index)
                .and_then(|material| material.helper_bitmap.as_ref())
            && !bitmaps.iter().any(|entry| entry == bitmap)
        {
            bitmaps.push(bitmap.clone());
        }
        for label in &primitive.surface_labels {
            if !surface_labels.iter().any(|entry| entry == label) {
                surface_labels.push(label.clone());
            }
        }
        for name in &primitive.texture_names {
            if !texture_names.iter().any(|entry| entry == name) {
                texture_names.push(name.clone());
            }
        }
    }

    if bitmaps.is_empty() && surface_labels.is_empty() && texture_names.is_empty() {
        None
    } else {
        Some(NwnModelHelperSurfaceAsset {
            bitmaps,
            surface_labels,
            texture_names,
        })
    }
}

/// Returns tilefade metadata for one primitive when the material uses tilefade
/// behavior on a tile model.
pub fn tilefade_asset_from_primitive(
    scene: &NwnScene,
    material: &NwnMaterial,
    primitive: &NwnPrimitive,
) -> Option<NwnModelTileFadeAsset> {
    if !matches!(scene.classification, Some(ModelClassification::Tile)) || material.tilefade <= 0 {
        return None;
    }
    let (local_center, local_half_extents) =
        primitive_bounds_from_nwn(primitive, scene.coordinate_system);
    Some(NwnModelTileFadeAsset {
        mode: material.tilefade,
        authored_visible: material.render_enabled,
        local_center,
        local_half_extents,
    })
}

/// Converts one NWN DDS payload into a Bevy `Image`.
pub fn image_from_dds(dds: &DdsTexture) -> Result<Image, NwnBevyError> {
    let rgba = dds.decode_rgba8()?;
    Ok(image_from_rgba8(dds.width, dds.height, rgba))
}

/// Converts one NWN TGA payload into a Bevy `Image`.
pub fn image_from_tga(tga: &TgaTexture) -> Result<Image, NwnBevyError> {
    let rgba = tga.decode_rgba8()?;
    Ok(image_from_rgba8(
        u32::from(tga.width),
        u32::from(tga.height),
        rgba,
    ))
}

/// Converts one NWN PLT payload into a Bevy `Image` using the default render
/// spec.
pub fn image_from_plt(plt: &PltTexture) -> Result<Image, NwnBevyError> {
    let rgba = plt.render_rgba8(&PltRenderSpec::default())?;
    Ok(image_from_rgba8(plt.width, plt.height, rgba))
}

/// Converts one NWN static primitive into a Bevy `Mesh`.
pub fn mesh_from_primitive(
    primitive: &NwnPrimitive,
    coordinate_system: NwnCoordinateSystem,
) -> Result<Mesh, NwnBevyError> {
    let mut positions = Vec::with_capacity(primitive.faces.len() * 3);
    let mut normals = Vec::with_capacity(primitive.faces.len() * 3);
    let mut uvs = Vec::with_capacity(primitive.faces.len() * 3);
    let mut indices = Vec::with_capacity(primitive.faces.len() * 3);
    let primary_uv_set = primitive.uv_sets.first();
    let source_positions = primitive
        .positions
        .iter()
        .copied()
        .map(|position| position_from_nwn(position, coordinate_system))
        .collect::<Vec<_>>();

    for face in &primitive.faces {
        let face_normal = compute_face_normal(face, &source_positions);
        for corner in 0..3 {
            let position_index_raw = *face.vertex_indices.get(corner).ok_or_else(|| {
                NwnBevyError::msg(format!("mesh corner {corner} is out of range"))
            })?;
            let position_index = usize::try_from(position_index_raw).map_err(|error| {
                NwnBevyError::msg(format!("mesh position index conversion failed: {error}"))
            })?;
            let position = source_positions
                .get(position_index)
                .copied()
                .ok_or_else(|| {
                    NwnBevyError::msg(format!(
                        "mesh position index {} is out of range",
                        position_index_raw
                    ))
                })?;
            positions.push(position);

            if let Some(normal) = primitive.normals.get(position_index).copied() {
                normals.push(direction_from_nwn(normal, coordinate_system));
            } else {
                normals.push(face_normal);
            }

            let uv = primary_uv_set
                .and_then(|set| {
                    face.uv_indices
                        .get(corner)
                        .copied()
                        .and_then(|raw_index| usize::try_from(raw_index).ok())
                        .and_then(|uv_index| set.coordinates.get(uv_index).copied())
                })
                .unwrap_or([0.0, 0.0]);
            uvs.push(uv);

            indices.push(u32::try_from(indices.len()).map_err(|error| {
                NwnBevyError::msg(format!("mesh index conversion failed: {error}"))
            })?);
        }
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    Ok(mesh)
}

/// Converts one NWN material into a basic Bevy `StandardMaterial`.
pub fn standard_material_from_nwn(
    material: &NwnMaterial,
    base_color_texture: Option<Handle<Image>>,
) -> StandardMaterial {
    let specular_average =
        (material.specular[0] + material.specular[1] + material.specular[2]) / 3.0;
    let shininess = material.shininess.clamp(0.0, 128.0);
    let roughness = (1.0 - (shininess / 128.0)).clamp(0.08, 1.0);
    let tilefade = material.tilefade > 0;
    StandardMaterial {
        base_color: Color::srgba(
            material.diffuse[0],
            material.diffuse[1],
            material.diffuse[2],
            material.alpha,
        ),
        base_color_texture,
        emissive: Color::srgb(
            material.self_illum_color[0],
            material.self_illum_color[1],
            material.self_illum_color[2],
        )
        .into(),
        metallic: 0.0,
        perceptual_roughness: roughness,
        reflectance: (specular_average * 0.5).clamp(0.0, 0.5),
        alpha_mode: if material.alpha < 0.999 {
            AlphaMode::Blend
        } else {
            AlphaMode::Opaque
        },
        cull_mode: (!tilefade).then_some(Face::Back),
        ..Default::default()
    }
}

/// Converts one NWN local transform into a Bevy transform.
pub fn transform_from_nwn(
    transform: &NwnTransform,
    coordinate_system: NwnCoordinateSystem,
) -> Transform {
    let [axis_x, axis_y, axis_z, angle] = transform.rotation_axis_angle;
    let [axis_x, axis_y, axis_z] = direction_from_nwn([axis_x, axis_y, axis_z], coordinate_system);
    let rotation = if angle.abs() < f32::EPSILON {
        bevy::math::Quat::IDENTITY
    } else {
        let axis = bevy::math::Vec3::new(axis_x, axis_y, axis_z);
        let normalized = axis.try_normalize().unwrap_or(bevy::math::Vec3::Y);
        bevy::math::Quat::from_axis_angle(normalized, angle)
    };

    Transform {
        translation: bevy::math::Vec3::from_array(position_from_nwn(
            transform.translation,
            coordinate_system,
        )),
        rotation,
        scale: bevy::math::Vec3::new(transform.scale[0], transform.scale[1], transform.scale[2]),
    }
}

pub(crate) fn position_from_nwn(
    position: [f32; 3],
    coordinate_system: NwnCoordinateSystem,
) -> [f32; 3] {
    match coordinate_system {
        // Aurora source-space matches Blender-style coordinates: X right,
        // Y forward, Z up. Bevy's 3D world conventions are Y up with forward
        // along -Z, so rotate the horizontal plane from XY onto XZ.
        NwnCoordinateSystem::AuroraSource => [position[0], position[2], -position[1]],
    }
}

fn direction_from_nwn(direction: [f32; 3], coordinate_system: NwnCoordinateSystem) -> [f32; 3] {
    match coordinate_system {
        NwnCoordinateSystem::AuroraSource => [direction[0], direction[2], -direction[1]],
    }
}

pub(crate) fn image_from_rgba8(width: u32, height: u32, rgba: Vec<u8>) -> Image {
    let mip_chain = build_rgba8_mip_chain(width, height, rgba);
    let mut image = Image::new_uninit(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.texture_descriptor.mip_level_count = mip_level_count(width, height);
    image.data = Some(mip_chain);
    image.sampler = ImageSampler::Descriptor(nwn_texture_sampler());
    image
}

fn nwn_texture_sampler() -> ImageSamplerDescriptor {
    let mut sampler = ImageSamplerDescriptor::default();
    sampler.address_mode_u = ImageAddressMode::Repeat;
    sampler.address_mode_v = ImageAddressMode::Repeat;
    sampler.address_mode_w = ImageAddressMode::Repeat;
    sampler.mag_filter = ImageFilterMode::Linear;
    sampler.min_filter = ImageFilterMode::Linear;
    sampler.mipmap_filter = ImageFilterMode::Linear;
    sampler.anisotropy_clamp = 8;
    sampler
}

fn mip_level_count(width: u32, height: u32) -> u32 {
    let largest_dimension = width.max(height).max(1);
    u32::BITS - largest_dimension.leading_zeros()
}

fn build_rgba8_mip_chain(width: u32, height: u32, base_level: Vec<u8>) -> Vec<u8> {
    let mut output = Vec::new();
    let mut current_width = width.max(1);
    let mut current_height = height.max(1);
    let mut current_level = base_level;

    loop {
        output.extend_from_slice(&current_level);
        if current_width == 1 && current_height == 1 {
            break;
        }

        let next_width = (current_width / 2).max(1);
        let next_height = (current_height / 2).max(1);
        current_level = downsample_rgba8_box(
            &current_level,
            current_width as usize,
            current_height as usize,
            next_width as usize,
            next_height as usize,
        );
        current_width = next_width;
        current_height = next_height;
    }

    output
}

fn downsample_rgba8_box(
    source: &[u8],
    source_width: usize,
    source_height: usize,
    target_width: usize,
    target_height: usize,
) -> Vec<u8> {
    let mut output = vec![0_u8; target_width * target_height * 4];

    for y in 0..target_height {
        for x in 0..target_width {
            let source_x0 = x * 2;
            let source_y0 = y * 2;
            let source_x1 = (source_x0 + 1).min(source_width - 1);
            let source_y1 = (source_y0 + 1).min(source_height - 1);

            let mut rgba_sum = [0_u32; 4];
            let mut sample_count = 0_u32;
            for source_y in [source_y0, source_y1] {
                for source_x in [source_x0, source_x1] {
                    let index = (source_y * source_width + source_x) * 4;
                    rgba_sum[0] += u32::from(source[index]);
                    rgba_sum[1] += u32::from(source[index + 1]);
                    rgba_sum[2] += u32::from(source[index + 2]);
                    rgba_sum[3] += u32::from(source[index + 3]);
                    sample_count += 1;
                }
            }

            let index = (y * target_width + x) * 4;
            output[index] = (rgba_sum[0] / sample_count) as u8;
            output[index + 1] = (rgba_sum[1] / sample_count) as u8;
            output[index + 2] = (rgba_sum[2] / sample_count) as u8;
            output[index + 3] = (rgba_sum[3] / sample_count) as u8;
        }
    }

    output
}

fn compute_face_normal(face: &NwnFace, positions: &[[f32; 3]]) -> [f32; 3] {
    let a = positions
        .get(usize::try_from(face.vertex_indices[0]).unwrap_or(usize::MAX))
        .copied()
        .unwrap_or([0.0, 0.0, 0.0]);
    let b = positions
        .get(usize::try_from(face.vertex_indices[1]).unwrap_or(usize::MAX))
        .copied()
        .unwrap_or([0.0, 0.0, 0.0]);
    let c = positions
        .get(usize::try_from(face.vertex_indices[2]).unwrap_or(usize::MAX))
        .copied()
        .unwrap_or([0.0, 0.0, 0.0]);

    let edge_ab = bevy::math::Vec3::new(b[0] - a[0], b[1] - a[1], b[2] - a[2]);
    let edge_ac = bevy::math::Vec3::new(c[0] - a[0], c[1] - a[1], c[2] - a[2]);
    edge_ab
        .cross(edge_ac)
        .try_normalize()
        .unwrap_or(bevy::math::Vec3::Y)
        .to_array()
}

#[cfg(test)]
mod tests {
    use bevy::asset::Handle;
    use nwnrs_mdl::prelude::{
        ModelClassification, NwnCoordinateSystem, NwnFace, NwnMaterial, NwnPrimitive, NwnScene,
        NwnTextureRef, NwnTextureSlot, NwnTransform, NwnUvSet,
    };

    use super::{
        image_from_rgba8, material_starts_visible, mesh_from_primitive, mip_level_count,
        standard_material_from_nwn, transform_from_nwn,
    };

    #[test]
    fn builds_basic_triangle_mesh() {
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
            normals:         vec![],
            tangents:        vec![],
            color_rows:      vec![],
            weight_rows:     vec![],
            constraint_rows: vec![],
            surface_labels:  vec![],
            texture_names:   vec![],
            material:        Some(0),
        };

        let mesh = mesh_from_primitive(&primitive, NwnCoordinateSystem::AuroraSource);
        assert!(mesh.is_ok(), "build mesh failed: {mesh:?}");
        if let Ok(mesh) = mesh {
            assert_eq!(
                mesh.primitive_topology(),
                bevy::mesh::PrimitiveTopology::TriangleList
            );
        }
    }

    #[test]
    fn rgba_images_get_repeat_sampler_and_mips() {
        let pixels = vec![255_u8; 4 * 4 * 4];
        let image = image_from_rgba8(4, 4, pixels);
        assert_eq!(
            image.texture_descriptor.mip_level_count,
            mip_level_count(4, 4)
        );

        match image.sampler {
            bevy::image::ImageSampler::Descriptor(descriptor) => {
                assert_eq!(
                    descriptor.address_mode_u,
                    bevy::image::ImageAddressMode::Repeat
                );
                assert_eq!(
                    descriptor.address_mode_v,
                    bevy::image::ImageAddressMode::Repeat
                );
                assert_eq!(
                    descriptor.mipmap_filter,
                    bevy::image::ImageFilterMode::Linear
                );
                assert_eq!(descriptor.anisotropy_clamp, 8);
            }
            bevy::image::ImageSampler::Default => panic!("expected custom sampler"),
        }

        let expected_bytes = ((4 * 4) + (2 * 2) + (1 * 1)) * 4;
        assert_eq!(
            image.data.unwrap_or_default().len(),
            expected_bytes as usize
        );
    }

    #[test]
    fn maps_nwn_material_alpha_to_blend_mode() {
        let material = NwnMaterial {
            source_node:       0,
            render_enabled:    true,
            shadow_enabled:    true,
            beaming:           0,
            inherit_color:     0,
            tilefade:          0,
            rotate_texture:    0,
            transparency_hint: 0,
            shininess:         1.0,
            alpha:             0.5,
            ambient:           [1.0, 1.0, 1.0],
            diffuse:           [1.0, 0.5, 0.25],
            specular:          [0.0, 0.0, 0.0],
            self_illum_color:  [0.0, 0.0, 0.0],
            material_name:     None,
            render_hint:       None,
            helper_bitmap:     None,
            textures:          vec![NwnTextureRef {
                slot: NwnTextureSlot::Bitmap,
                name: "demo".to_string(),
            }],
        };

        let bevy_material = standard_material_from_nwn(&material, Some(Handle::default()));
        assert!(matches!(
            bevy_material.alpha_mode,
            bevy::prelude::AlphaMode::Blend
        ));
    }

    #[test]
    fn maps_zero_specular_material_to_non_shiny_surface() {
        let material = NwnMaterial {
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
            ambient:           [1.0, 1.0, 1.0],
            diffuse:           [0.4, 0.7, 0.3],
            specular:          [0.0, 0.0, 0.0],
            self_illum_color:  [0.0, 0.0, 0.0],
            material_name:     None,
            render_hint:       None,
            helper_bitmap:     None,
            textures:          Vec::new(),
        };

        let bevy_material = standard_material_from_nwn(&material, None);
        assert_eq!(bevy_material.metallic, 0.0);
        assert!(bevy_material.perceptual_roughness >= 0.95);
        assert_eq!(bevy_material.reflectance, 0.0);
    }

    #[test]
    fn tilefade_materials_are_double_sided() {
        let material = NwnMaterial {
            source_node:       0,
            render_enabled:    true,
            shadow_enabled:    true,
            beaming:           0,
            inherit_color:     0,
            tilefade:          1,
            rotate_texture:    0,
            transparency_hint: 0,
            shininess:         0.0,
            alpha:             1.0,
            ambient:           [1.0, 1.0, 1.0],
            diffuse:           [1.0, 1.0, 1.0],
            specular:          [0.0, 0.0, 0.0],
            self_illum_color:  [0.0, 0.0, 0.0],
            material_name:     None,
            render_hint:       None,
            helper_bitmap:     None,
            textures:          vec![NwnTextureRef {
                slot: NwnTextureSlot::Bitmap,
                name: "TCN01_roof11".to_string(),
            }],
        };

        let bevy_material = standard_material_from_nwn(&material, Some(Handle::default()));
        assert_eq!(bevy_material.cull_mode, None);
    }

    #[test]
    fn hidden_tilefade_materials_start_visible_for_tile_models() {
        let scene = NwnScene {
            name:              "tile".to_string(),
            supermodel:        None,
            classification:    Some(ModelClassification::Tile),
            animation_scale:   None,
            coordinate_system: NwnCoordinateSystem::AuroraSource,
            nodes:             Vec::new(),
            meshes:            Vec::new(),
            materials:         Vec::new(),
            animations:        Vec::new(),
            diagnostics:       Vec::new(),
        };
        let material = NwnMaterial {
            source_node:       0,
            render_enabled:    false,
            shadow_enabled:    true,
            beaming:           0,
            inherit_color:     0,
            tilefade:          1,
            rotate_texture:    0,
            transparency_hint: 0,
            shininess:         0.0,
            alpha:             1.0,
            ambient:           [1.0, 1.0, 1.0],
            diffuse:           [1.0, 1.0, 1.0],
            specular:          [0.0, 0.0, 0.0],
            self_illum_color:  [0.0, 0.0, 0.0],
            material_name:     None,
            render_hint:       None,
            helper_bitmap:     None,
            textures:          Vec::new(),
        };

        assert!(material_starts_visible(&scene, &material));
    }

    #[test]
    fn hidden_non_tilefade_materials_stay_hidden() {
        let scene = NwnScene {
            name:              "item".to_string(),
            supermodel:        None,
            classification:    Some(ModelClassification::Item),
            animation_scale:   None,
            coordinate_system: NwnCoordinateSystem::AuroraSource,
            nodes:             Vec::new(),
            meshes:            Vec::new(),
            materials:         Vec::new(),
            animations:        Vec::new(),
            diagnostics:       Vec::new(),
        };
        let material = NwnMaterial {
            source_node:       0,
            render_enabled:    false,
            shadow_enabled:    true,
            beaming:           0,
            inherit_color:     0,
            tilefade:          1,
            rotate_texture:    0,
            transparency_hint: 0,
            shininess:         0.0,
            alpha:             1.0,
            ambient:           [1.0, 1.0, 1.0],
            diffuse:           [1.0, 1.0, 1.0],
            specular:          [0.0, 0.0, 0.0],
            self_illum_color:  [0.0, 0.0, 0.0],
            material_name:     None,
            render_hint:       None,
            helper_bitmap:     None,
            textures:          Vec::new(),
        };

        assert!(!material_starts_visible(&scene, &material));
    }

    #[test]
    fn converts_axis_angle_transform_without_panicking() {
        let transform = NwnTransform {
            translation:         [1.0, 2.0, 3.0],
            rotation_axis_angle: [0.0, 0.0, 1.0, core::f32::consts::FRAC_PI_2],
            scale:               [1.0, 2.0, 3.0],
        };

        let bevy_transform = transform_from_nwn(&transform, NwnCoordinateSystem::AuroraSource);
        assert_eq!(bevy_transform.translation.to_array(), [1.0, 3.0, -2.0]);
        assert_eq!(bevy_transform.scale.to_array(), [1.0, 2.0, 3.0]);
    }
}
