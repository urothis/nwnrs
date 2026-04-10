use bevy::{
    asset::{Handle, RenderAssetUsages},
    image::Image,
    mesh::{Indices, Mesh, PrimitiveTopology},
    pbr::StandardMaterial,
    prelude::{AlphaMode, Color},
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    transform::components::Transform,
};
use nwnrs_dds::prelude::*;
use nwnrs_mdl::prelude::*;
use nwnrs_plt::prelude::*;
use nwnrs_tga::prelude::*;

use crate::NwnBevyError;

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
pub fn mesh_from_primitive(primitive: &NwnPrimitive) -> Result<Mesh, NwnBevyError> {
    let mut positions = Vec::with_capacity(primitive.faces.len() * 3);
    let mut normals = Vec::with_capacity(primitive.faces.len() * 3);
    let mut uvs = Vec::with_capacity(primitive.faces.len() * 3);
    let mut indices = Vec::with_capacity(primitive.faces.len() * 3);
    let primary_uv_set = primitive.uv_sets.first();

    for face in &primitive.faces {
        let face_normal = compute_face_normal(face, &primitive.positions);
        for corner in 0..3 {
            let position_index_raw = *face.vertex_indices.get(corner).ok_or_else(|| {
                NwnBevyError::msg(format!("mesh corner {corner} is out of range"))
            })?;
            let position_index = usize::try_from(position_index_raw).map_err(|error| {
                NwnBevyError::msg(format!("mesh position index conversion failed: {error}"))
            })?;
            let position = primitive
                .positions
                .get(position_index)
                .copied()
                .ok_or_else(|| {
                    NwnBevyError::msg(format!(
                        "mesh position index {} is out of range",
                        position_index_raw
                    ))
                })?;
            positions.push(position);

            if let Some(normal) = primitive.normals.get(position_index) {
                normals.push(*normal);
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
    StandardMaterial {
        base_color: Color::srgba(
            material.diffuse[0],
            material.diffuse[1],
            material.diffuse[2],
            material.alpha,
        ),
        base_color_texture,
        alpha_mode: if material.alpha < 0.999 {
            AlphaMode::Blend
        } else {
            AlphaMode::Opaque
        },
        ..Default::default()
    }
}

/// Converts one NWN local transform into a Bevy transform.
pub fn transform_from_nwn(transform: &NwnTransform) -> Transform {
    let [axis_x, axis_y, axis_z, angle] = transform.rotation_axis_angle;
    let rotation = if angle.abs() < f32::EPSILON {
        bevy::math::Quat::IDENTITY
    } else {
        let axis = bevy::math::Vec3::new(axis_x, axis_y, axis_z);
        let normalized = axis.try_normalize().unwrap_or(bevy::math::Vec3::Y);
        bevy::math::Quat::from_axis_angle(normalized, angle)
    };

    Transform {
        translation: bevy::math::Vec3::new(
            transform.translation[0],
            transform.translation[1],
            transform.translation[2],
        ),
        rotation,
        scale: bevy::math::Vec3::new(transform.scale[0], transform.scale[1], transform.scale[2]),
    }
}

fn image_from_rgba8(width: u32, height: u32, rgba: Vec<u8>) -> Image {
    Image::new(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        rgba,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
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
        NwnFace, NwnMaterial, NwnPrimitive, NwnTextureRef, NwnTextureSlot, NwnTransform, NwnUvSet,
    };

    use super::{mesh_from_primitive, standard_material_from_nwn, transform_from_nwn};

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

        let mesh = mesh_from_primitive(&primitive);
        assert!(mesh.is_ok(), "build mesh failed: {mesh:?}");
        if let Ok(mesh) = mesh {
            assert_eq!(
                mesh.primitive_topology(),
                bevy::mesh::PrimitiveTopology::TriangleList
            );
        }
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
    fn converts_axis_angle_transform_without_panicking() {
        let transform = NwnTransform {
            translation:         [1.0, 2.0, 3.0],
            rotation_axis_angle: [0.0, 0.0, 1.0, core::f32::consts::FRAC_PI_2],
            scale:               [1.0, 2.0, 3.0],
        };

        let bevy_transform = transform_from_nwn(&transform);
        assert_eq!(bevy_transform.translation.to_array(), [1.0, 2.0, 3.0]);
        assert_eq!(bevy_transform.scale.to_array(), [1.0, 2.0, 3.0]);
    }
}
