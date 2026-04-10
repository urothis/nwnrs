use std::{collections::BTreeMap, io::Cursor};

use bevy::{
    asset::{Assets, Handle},
    image::Image,
    mesh::Mesh,
    pbr::StandardMaterial,
};
use nwnrs_dds::prelude::DdsTexture;
use nwnrs_mdl::prelude::*;
use nwnrs_mtr::prelude::{MTR_RES_TYPE, read_mtr_from_res};
use nwnrs_plt::prelude::read_plt;
use nwnrs_resman::prelude::ResMan;
use nwnrs_resref::prelude::ResRef;
use nwnrs_tga::prelude::read_tga;

use crate::{
    NwnAppearanceOverrides, NwnBevyError, NwnModelAsset, NwnModelNodeAsset, NwnPrimitiveAsset,
    NwnTextureLoadReason, NwnUnresolvedTexture, apply_appearance_overrides, image_from_dds,
    image_from_plt, image_from_tga, mesh_from_primitive, standard_material_from_nwn,
    transform_from_nwn,
};

/// Loads one NWN model from `resman` and converts it into Bevy-side assets.
pub fn load_nwn_model_from_resman(
    resman: &mut ResMan,
    model_name: &str,
    images: &mut Assets<Image>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) -> Result<NwnModelAsset, NwnBevyError> {
    load_nwn_model_from_resman_with_overrides(
        resman,
        model_name,
        &NwnAppearanceOverrides::default(),
        images,
        meshes,
        materials,
    )
}

/// Loads one NWN model from `resman` and applies explicit appearance/part
/// overrides before converting it into Bevy-side assets.
pub fn load_nwn_model_from_resman_with_overrides(
    resman: &mut ResMan,
    model_name: &str,
    overrides: &NwnAppearanceOverrides,
    images: &mut Assets<Image>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) -> Result<NwnModelAsset, NwnBevyError> {
    let resref = ResRef::new(model_name.to_string(), MODEL_RES_TYPE)
        .map_err(|error| NwnBevyError::msg(format!("invalid mdl resref {model_name}: {error}")))?;
    let res = resman
        .get(&resref)
        .ok_or_else(|| NwnBevyError::msg(format!("model not found in ResMan: {model_name}.mdl")))?;
    let scene = apply_appearance_overrides(&read_scene_model_auto_from_res(&res, true)?, overrides);

    let material_bindings = load_runtime_materials(&scene, resman, images, materials)?;
    let mesh_bindings = load_runtime_meshes(&scene, meshes)?;
    let nodes = build_runtime_node_assets(&scene, &mesh_bindings, &material_bindings);
    let root_nodes = nodes
        .iter()
        .enumerate()
        .filter_map(|(index, node)| node.parent.is_none().then_some(index))
        .collect::<Vec<_>>();

    Ok(NwnModelAsset {
        scene,
        nodes,
        root_nodes,
        materials: material_bindings
            .iter()
            .map(|binding| binding.material.clone())
            .collect(),
        meshes: mesh_bindings
            .iter()
            .map(|binding| binding.mesh.clone())
            .collect(),
        textures: material_bindings
            .iter()
            .filter_map(|binding| binding.texture.clone())
            .collect(),
        unresolved: material_bindings
            .into_iter()
            .flat_map(|binding| binding.unresolved)
            .collect(),
    })
}

#[derive(Debug, Clone)]
struct RuntimeMaterialBinding {
    material:   Handle<StandardMaterial>,
    texture:    Option<Handle<Image>>,
    unresolved: Vec<NwnUnresolvedTexture>,
}

#[derive(Debug, Clone)]
struct RuntimeMeshBinding {
    scene_mesh_index: usize,
    primitive_index:  usize,
    mesh:             Handle<Mesh>,
}

fn load_runtime_materials(
    scene: &NwnScene,
    resman: &mut ResMan,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) -> Result<Vec<RuntimeMaterialBinding>, NwnBevyError> {
    let mut bindings = Vec::with_capacity(scene.materials.len());
    for (material_index, material) in scene.materials.iter().enumerate() {
        let texture_result =
            load_runtime_material_texture(scene, material_index, material, resman, images)?;
        let texture_handle = texture_result.texture.clone();
        let material_handle = materials.add(standard_material_from_nwn(material, texture_handle));
        bindings.push(RuntimeMaterialBinding {
            material:   material_handle,
            texture:    texture_result.texture,
            unresolved: texture_result.unresolved,
        });
    }

    Ok(bindings)
}

fn load_runtime_material_texture(
    scene: &NwnScene,
    material_index: usize,
    material: &NwnMaterial,
    resman: &mut ResMan,
    images: &mut Assets<Image>,
) -> Result<RuntimeMaterialTextureLoad, NwnBevyError> {
    let texture = material
        .textures
        .iter()
        .find(|texture| matches!(texture.slot, NwnTextureSlot::Bitmap));
    if texture.is_none() && material.material_name.is_none() {
        return Ok(RuntimeMaterialTextureLoad::default());
    }

    let mut attempted = Vec::new();
    let texture_names = texture
        .map(|texture| scene_texture_resolution_names(scene, material, texture))
        .unwrap_or_default();
    let mtr_names = mtr_candidate_names(material, &texture_names);

    if let Some(texture) = texture {
        match resolve_scene_texture_ref_with_policy(
            scene,
            material,
            texture,
            resman,
            &runtime_texture_resolver_options(),
        ) {
            SceneTextureResolution::Resolved(resolved) => {
                return Ok(RuntimeMaterialTextureLoad {
                    texture:    Some(images.add(image_from_runtime_resolved_texture(&resolved)?)),
                    unresolved: Vec::new(),
                });
            }
            SceneTextureResolution::Ignored => {
                attempted.extend(deferred_attempts(&texture_names, &mtr_names));
            }
            SceneTextureResolution::Missing(missing) => attempted.extend(
                missing
                    .attempted
                    .into_iter()
                    .map(|candidate| candidate.to_file()),
            ),
        }
    }

    for mtr_name in mtr_names {
        let Some(mtr_rr) = ResRef::new(mtr_name.clone(), MTR_RES_TYPE).ok() else {
            continue;
        };
        attempted.push(format!("{mtr_name}.mtr"));
        let Some(mtr_res) = resman.get(&mtr_rr) else {
            continue;
        };
        let mtr = read_mtr_from_res(&mtr_res, true)?;
        let Some(texture_name) = mtr.texture0() else {
            continue;
        };
        let texture_ref = NwnTextureRef {
            slot: NwnTextureSlot::Bitmap,
            name: texture_name.to_string(),
        };
        match resolve_texture_ref(&texture_ref, resman, &runtime_texture_resolver_options()) {
            Ok(resolved) => {
                return Ok(RuntimeMaterialTextureLoad {
                    texture:    Some(images.add(image_from_runtime_resolved_texture(&resolved)?)),
                    unresolved: Vec::new(),
                });
            }
            Err(missing) => attempted.extend(
                missing
                    .attempted
                    .into_iter()
                    .map(|candidate| candidate.to_file()),
            ),
        }
    }

    Ok(RuntimeMaterialTextureLoad {
        texture:    None,
        unresolved: vec![NwnUnresolvedTexture {
            material_index,
            slot: texture
                .map(|texture| texture.slot.clone())
                .unwrap_or(NwnTextureSlot::Bitmap),
            name: texture
                .map(|texture| texture.name.clone())
                .or_else(|| material.material_name.clone())
                .unwrap_or_default(),
            attempted,
            reason: NwnTextureLoadReason::Missing,
        }],
    })
}

fn image_from_runtime_resolved_texture(resolved: &ResolvedTexture) -> Result<Image, NwnBevyError> {
    let bytes = resolved
        .resource
        .read_all(true)
        .map_err(|error| NwnBevyError::msg(format!("read {}: {error}", resolved.resolved)))?;
    match resolved.kind {
        TextureResourceKind::Dds => image_from_dds(&DdsTexture::read_from_texture_bytes(&bytes)?),
        TextureResourceKind::Tga => {
            let mut cursor = Cursor::new(bytes);
            image_from_tga(&read_tga(&mut cursor)?)
        }
        TextureResourceKind::Plt => {
            let mut cursor = Cursor::new(bytes);
            image_from_plt(&read_plt(&mut cursor)?)
        }
    }
}

fn mtr_candidate_names(material: &NwnMaterial, bitmap_names: &[String]) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(material_name) = material.material_name.as_deref()
        && is_mtr_candidate(material_name)
    {
        names.push(material_name.to_string());
    }
    for bitmap_name in bitmap_names {
        if is_mtr_candidate(bitmap_name)
            && !names
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(bitmap_name.as_str()))
        {
            names.push(bitmap_name.clone());
        }
    }
    names
}

fn is_mtr_candidate(name: &str) -> bool {
    let trimmed = name.trim();
    !trimmed.is_empty()
        && !trimmed.eq_ignore_ascii_case("null")
        && std::path::Path::new(trimmed).extension().is_none()
        && !trimmed.contains('/')
        && !trimmed.contains('\\')
}

fn deferred_attempts(texture_names: &[String], mtr_names: &[String]) -> Vec<String> {
    let mut attempted = BTreeMap::<String, ()>::new();
    for texture_name in texture_names {
        for candidate in [
            format!("{texture_name}.dds"),
            format!("{texture_name}.tga"),
            format!("{texture_name}.plt"),
            format!("{texture_name}.mdl"),
        ] {
            attempted.insert(candidate, ());
        }
    }
    for mtr_name in mtr_names {
        attempted.insert(format!("{mtr_name}.mtr"), ());
    }
    attempted.into_keys().collect()
}

fn load_runtime_meshes(
    scene: &NwnScene,
    meshes: &mut Assets<Mesh>,
) -> Result<Vec<RuntimeMeshBinding>, NwnBevyError> {
    let mut bindings = Vec::new();
    for (scene_mesh_index, mesh) in scene.meshes.iter().enumerate() {
        for (primitive_index, primitive) in mesh.primitives.iter().enumerate() {
            let mesh_handle = meshes.add(mesh_from_primitive(primitive, scene.coordinate_system)?);
            bindings.push(RuntimeMeshBinding {
                scene_mesh_index,
                primitive_index,
                mesh: mesh_handle,
            });
        }
    }

    Ok(bindings)
}

fn build_runtime_node_assets(
    scene: &NwnScene,
    mesh_bindings: &[RuntimeMeshBinding],
    material_bindings: &[RuntimeMaterialBinding],
) -> Vec<NwnModelNodeAsset> {
    let mut primitive_lookup = std::collections::BTreeMap::new();
    for binding in mesh_bindings {
        primitive_lookup.insert(
            (binding.scene_mesh_index, binding.primitive_index),
            binding.mesh.clone(),
        );
    }

    scene
        .nodes
        .iter()
        .map(|node| {
            let primitives = node
                .mesh
                .and_then(|mesh_index| scene.meshes.get(mesh_index).map(|mesh| (mesh_index, mesh)))
                .map_or_else(Vec::new, |(mesh_index, mesh)| {
                    mesh.primitives
                        .iter()
                        .enumerate()
                        .filter_map(|(primitive_index, primitive)| {
                            let material_index = primitive.material?;
                            let material = scene.materials.get(material_index)?;
                            if !material.render_enabled {
                                return None;
                            }
                            let mesh_handle = primitive_lookup
                                .get(&(mesh_index, primitive_index))
                                .cloned()?;
                            let material_handle = material_bindings
                                .get(material_index)
                                .map(|binding| binding.material.clone())?;
                            Some(NwnPrimitiveAsset {
                                label:          format!("{}:{primitive_index}", mesh.name),
                                mesh:           mesh_handle,
                                material:       material_handle,
                                shadow_enabled: material.shadow_enabled,
                            })
                        })
                        .collect()
                });

            NwnModelNodeAsset {
                name: node.name.clone(),
                kind: node.kind.clone(),
                parent: node.parent,
                transform: transform_from_nwn(&node.local_transform, scene.coordinate_system),
                primitives,
            }
        })
        .collect()
}

fn runtime_texture_resolver_options() -> TextureResolverOptions {
    TextureResolverOptions {
        fallback_order: vec![
            TextureResourceKind::Dds,
            TextureResourceKind::Tga,
            TextureResourceKind::Plt,
        ],
    }
}

#[derive(Debug, Default)]
struct RuntimeMaterialTextureLoad {
    texture:    Option<Handle<Image>>,
    unresolved: Vec<NwnUnresolvedTexture>,
}
