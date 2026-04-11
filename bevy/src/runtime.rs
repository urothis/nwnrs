use std::{
    collections::{BTreeMap, BTreeSet},
    io::Cursor,
};

use bevy::{
    asset::{Assets, Handle},
    image::Image,
    mesh::Mesh,
    pbr::StandardMaterial,
};
use nwnrs_dds::prelude::DdsTexture;
use nwnrs_mdl::prelude::*;
use nwnrs_mtr::prelude::{MTR_RES_TYPE, read_mtr_from_res};
use nwnrs_plt::prelude::{PltLayer, PltRenderSpec, PltTexture, read_plt};
use nwnrs_resman::prelude::ResMan;
use nwnrs_resref::prelude::{ResRef, ResolvedResRef};
use nwnrs_tga::prelude::{TgaTexture, read_tga};
use nwnrs_txi::prelude::read_optional_txi_from_resman;

use crate::{
    NwnAppearanceOverrides, NwnBevyError, NwnModelAsset, NwnModelNodeAsset, NwnPrimitiveAsset,
    NwnTextureLoadReason, NwnUnresolvedTexture, apply_appearance_overrides,
    helper_surface_from_node, image_from_dds, image_from_plt, image_from_tga,
    light::build_model_light_asset,
    material_starts_visible, mesh_from_primitive, standard_material_from_nwn,
    tilefade_asset_from_primitive, transform_from_nwn,
    txi::{build_model_txi_asset, derive_txi_uv_to_local_horizontal},
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
    let mut cache = BTreeMap::new();
    let mut stack = BTreeSet::new();
    load_nwn_model_from_resman_with_overrides_cached(
        resman, model_name, overrides, images, meshes, materials, &mut cache, &mut stack,
    )
}

fn load_nwn_model_from_resman_with_overrides_cached(
    resman: &mut ResMan,
    model_name: &str,
    overrides: &NwnAppearanceOverrides,
    images: &mut Assets<Image>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    cache: &mut BTreeMap<String, NwnModelAsset>,
    stack: &mut BTreeSet<String>,
) -> Result<NwnModelAsset, NwnBevyError> {
    let cache_key = format!("{}::{overrides:?}", model_name.to_ascii_lowercase());
    if let Some(cached) = cache.get(&cache_key) {
        return Ok(cached.clone());
    }
    if !stack.insert(cache_key.clone()) {
        return Err(NwnBevyError::msg(format!(
            "detected recursive model reference involving {model_name}"
        )));
    }

    let resref = ResRef::new(model_name.to_string(), MODEL_RES_TYPE)
        .map_err(|error| NwnBevyError::msg(format!("invalid mdl resref {model_name}: {error}")))?;
    let res = resman
        .get(&resref)
        .ok_or_else(|| NwnBevyError::msg(format!("model not found in ResMan: {model_name}.mdl")))?;
    let scene = apply_appearance_overrides(&read_scene_model_auto_from_res(&res, true)?, overrides);

    let material_bindings = load_runtime_materials(&scene, resman, overrides, images, materials)?;
    let mesh_bindings = load_runtime_meshes(&scene, meshes)?;
    let nodes = build_runtime_node_assets(
        &scene,
        &mesh_bindings,
        &material_bindings,
        resman,
        overrides,
        images,
        meshes,
        materials,
        cache,
        stack,
    )?;
    let root_nodes = nodes
        .iter()
        .enumerate()
        .filter_map(|(index, node)| node.parent.is_none().then_some(index))
        .collect::<Vec<_>>();

    let model = NwnModelAsset {
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
    };
    stack.remove(&cache_key);
    cache.insert(cache_key, model.clone());
    Ok(model)
}

#[derive(Debug, Clone)]
struct RuntimeMaterialBinding {
    material:   Handle<StandardMaterial>,
    texture:    Option<Handle<Image>>,
    txi:        Option<crate::NwnModelTxiAsset>,
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
    overrides: &NwnAppearanceOverrides,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) -> Result<Vec<RuntimeMaterialBinding>, NwnBevyError> {
    let mut bindings = Vec::with_capacity(scene.materials.len());
    for (material_index, material) in scene.materials.iter().enumerate() {
        let texture_result = load_runtime_material_texture(
            scene,
            material_index,
            material,
            resman,
            overrides,
            images,
        )?;
        let texture_handle = texture_result.texture.clone();
        let material_handle = materials.add(standard_material_from_nwn(material, texture_handle));
        bindings.push(RuntimeMaterialBinding {
            material:   material_handle,
            texture:    texture_result.texture,
            txi:        texture_result.txi,
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
    overrides: &NwnAppearanceOverrides,
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
                    texture:    Some(images.add(image_from_runtime_resolved_texture(
                        &resolved, resman, overrides,
                    )?)),
                    txi:        load_runtime_material_txi(material, &resolved.resolved, resman)?,
                    unresolved: Vec::new(),
                });
            }
            SceneTextureResolution::Ignored => {
                return Ok(RuntimeMaterialTextureLoad::default());
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
                    texture:    Some(images.add(image_from_runtime_resolved_texture(
                        &resolved, resman, overrides,
                    )?)),
                    txi:        load_runtime_material_txi(material, &resolved.resolved, resman)?,
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
        txi:        None,
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

fn image_from_runtime_resolved_texture(
    resolved: &ResolvedTexture,
    resman: &mut ResMan,
    overrides: &NwnAppearanceOverrides,
) -> Result<Image, NwnBevyError> {
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
            image_from_plt_with_runtime_overrides(&read_plt(&mut cursor)?, resman, overrides)
        }
    }
}

#[derive(Debug, Clone)]
struct RuntimePaletteTexture {
    width:  u32,
    height: u32,
    rgba:   Vec<u8>,
}

fn image_from_plt_with_runtime_overrides(
    plt: &PltTexture,
    resman: &mut ResMan,
    overrides: &NwnAppearanceOverrides,
) -> Result<Image, NwnBevyError> {
    if overrides.plt_rows.is_empty() {
        return image_from_plt(plt);
    }

    let expected_pixels = plt.pixel_count()?;
    let mut rgba = Vec::with_capacity(expected_pixels.saturating_mul(4));
    let default_spec = PltRenderSpec::default();
    let mut palettes = BTreeMap::<u8, Option<RuntimePaletteTexture>>::new();

    for pixel in &plt.pixels {
        if let Some(row_index) = overrides.plt_row(pixel.layer_id)
            && let Some(palette) = runtime_palette_for_layer(resman, pixel.layer_id, &mut palettes)?
            && let Some(color) = palette_color_for_value(palette, row_index, pixel.value)
        {
            rgba.extend_from_slice(&color);
            continue;
        }

        let [r, g, b, a] = default_spec.color_for_layer_id(pixel.layer_id);
        let value = u16::from(pixel.value);
        rgba.push(scale_channel(r, value));
        rgba.push(scale_channel(g, value));
        rgba.push(scale_channel(b, value));
        rgba.push(scale_channel(a, value));
    }

    Ok(crate::image_from_rgba8(plt.width, plt.height, rgba))
}

fn runtime_palette_for_layer<'a>(
    resman: &mut ResMan,
    layer_id: u8,
    palettes: &'a mut BTreeMap<u8, Option<RuntimePaletteTexture>>,
) -> Result<Option<&'a RuntimePaletteTexture>, NwnBevyError> {
    if !palettes.contains_key(&layer_id) {
        let palette = palette_stem_for_layer_id(layer_id)
            .map(|stem| load_runtime_palette_texture(resman, stem))
            .transpose()?;
        palettes.insert(layer_id, palette);
    }
    Ok(palettes.get(&layer_id).and_then(|palette| palette.as_ref()))
}

fn load_runtime_palette_texture(
    resman: &mut ResMan,
    stem: &str,
) -> Result<RuntimePaletteTexture, NwnBevyError> {
    let resolved = ResolvedResRef::from_filename(&format!("{stem}.tga")).map_err(|error| {
        NwnBevyError::msg(format!("invalid palette resref {stem}.tga: {error}"))
    })?;
    let Some(res) = resman.get_resolved(&resolved) else {
        return Err(NwnBevyError::msg(format!(
            "palette not found in ResMan: {}",
            resolved.to_file()
        )));
    };
    let tga = read_tga_from_runtime_res(&res)?;
    let rgba = tga.decode_rgba8()?;
    Ok(RuntimePaletteTexture {
        width: u32::from(tga.width),
        height: u32::from(tga.height),
        rgba,
    })
}

fn read_tga_from_runtime_res(res: &nwnrs_resman::prelude::Res) -> Result<TgaTexture, NwnBevyError> {
    let bytes = res
        .read_all(true)
        .map_err(|error| NwnBevyError::msg(format!("read {}: {error}", res.resref())))?;
    let mut cursor = Cursor::new(bytes);
    Ok(read_tga(&mut cursor)?)
}

fn palette_stem_for_layer_id(layer_id: u8) -> Option<&'static str> {
    match PltLayer::from_id(layer_id)? {
        PltLayer::Skin => Some("pal_skin01"),
        PltLayer::Hair => Some("pal_hair01"),
        PltLayer::Metal1 => Some("pal_armor01"),
        PltLayer::Metal2 => Some("pal_armor02"),
        PltLayer::Cloth1 | PltLayer::Cloth2 => Some("pal_cloth01"),
        PltLayer::Leather1 | PltLayer::Leather2 => Some("pal_leath01"),
        PltLayer::Tattoo1 | PltLayer::Tattoo2 => Some("pal_tattoo01"),
    }
}

fn palette_color_for_value(
    palette: &RuntimePaletteTexture,
    row_index: u8,
    value: u8,
) -> Option<[u8; 4]> {
    let x = u32::from(value).min(palette.width.checked_sub(1)?);
    let y = u32::from(row_index).min(palette.height.checked_sub(1)?);
    let width = usize::try_from(palette.width).ok()?;
    let index = usize::try_from(y)
        .ok()?
        .checked_mul(width)?
        .checked_add(usize::try_from(x).ok()?)?
        .checked_mul(4)?;
    let rgba = palette.rgba.get(index..index + 4)?;
    Some([rgba[0], rgba[1], rgba[2], rgba[3]])
}

fn scale_channel(channel: u8, value: u16) -> u8 {
    ((u16::from(channel) * value + 127) / 255) as u8
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
    resman: &mut ResMan,
    overrides: &NwnAppearanceOverrides,
    images: &mut Assets<Image>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    cache: &mut BTreeMap<String, NwnModelAsset>,
    stack: &mut BTreeSet<String>,
) -> Result<Vec<NwnModelNodeAsset>, NwnBevyError> {
    let mut primitive_lookup = std::collections::BTreeMap::new();
    for binding in mesh_bindings {
        primitive_lookup.insert(
            (binding.scene_mesh_index, binding.primitive_index),
            binding.mesh.clone(),
        );
    }

    let mut nodes = Vec::with_capacity(scene.nodes.len());
    for (node_index, node) in scene.nodes.iter().enumerate() {
        let primitives = node
            .mesh
            .and_then(|mesh_index| scene.meshes.get(mesh_index).map(|mesh| (mesh_index, mesh)))
            .map_or_else(Vec::new, |(mesh_index, mesh)| {
                if !node_kind_has_visible_geometry(&node.kind) {
                    return Vec::new();
                }
                mesh.primitives
                    .iter()
                    .enumerate()
                    .filter_map(|(primitive_index, primitive)| {
                        let material_index = primitive.material?;
                        let material = scene.materials.get(material_index)?;
                        let mesh_handle = primitive_lookup
                            .get(&(mesh_index, primitive_index))
                            .cloned()?;
                        let material_handle = material_bindings
                            .get(material_index)
                            .map(|binding| binding.material.clone())?;
                        Some(NwnPrimitiveAsset {
                            label: format!("{}:{primitive_index}", mesh.name),
                            scene_primitive_index: primitive_index,
                            txi: material_bindings
                                .get(material_index)
                                .and_then(|binding| binding.txi.clone()),
                            txi_uv_to_local_horizontal: material_bindings
                                .get(material_index)
                                .and_then(|binding| binding.txi.as_ref())
                                .and_then(|_| {
                                    derive_txi_uv_to_local_horizontal(
                                        primitive,
                                        scene.coordinate_system,
                                    )
                                }),
                            mesh: mesh_handle,
                            material: material_handle,
                            tilefade: tilefade_asset_from_primitive(scene, material, primitive),
                            initially_visible: material_starts_visible(scene, material),
                            shadow_enabled: material.shadow_enabled,
                        })
                    })
                    .collect()
            });

        let mut references = Vec::new();
        if let Some(reference_model) = node
            .reference
            .as_ref()
            .and_then(|reference| reference.model.as_deref())
        {
            references.push(
                load_nwn_model_from_resman_with_overrides_cached(
                    resman,
                    reference_model,
                    overrides,
                    images,
                    meshes,
                    materials,
                    cache,
                    stack,
                )
                .map(|model| crate::NwnModelReferenceAsset {
                    model_name: reference_model.to_string(),
                    model:      Box::new(model),
                })?,
            );
        }

        nodes.push(NwnModelNodeAsset {
            name: node.name.clone(),
            kind: node.kind.clone(),
            parent: node.parent,
            transform: transform_from_nwn(&node.local_transform, scene.coordinate_system),
            light: build_model_light_asset(scene, node_index),
            references,
            helper_surface: helper_surface_from_node(scene, node),
            primitives,
        });
    }

    Ok(nodes)
}

fn node_kind_has_visible_geometry(kind: &NodeKind) -> bool {
    !matches!(kind, NodeKind::Aabb)
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
    txi:        Option<crate::NwnModelTxiAsset>,
    unresolved: Vec<NwnUnresolvedTexture>,
}

fn load_runtime_material_txi(
    material: &NwnMaterial,
    resolved: &ResolvedResRef,
    resman: &mut ResMan,
) -> Result<Option<crate::NwnModelTxiAsset>, NwnBevyError> {
    let Some(txi) = read_optional_txi_from_resman(resman, resolved.res_ref(), true)
        .map_err(|error| NwnBevyError::msg(format!("read {}.txi: {error}", resolved.res_ref())))?
    else {
        return Ok(None);
    };
    Ok(build_model_txi_asset(material, &txi))
}
