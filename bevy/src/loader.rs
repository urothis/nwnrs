use std::{collections::BTreeMap, io::Cursor};

use bevy::{
    asset::{AssetLoader, AssetPath, Handle, LoadContext, io::Reader},
    image::Image,
    mesh::Mesh,
    pbr::StandardMaterial,
    prelude::TypePath,
};
use nwnrs_dds::prelude::DdsTexture;
use nwnrs_mdl::prelude::*;
use nwnrs_mtr::prelude::{MTR_RES_TYPE, read_mtr, read_mtr_from_res};
use nwnrs_plt::prelude::read_plt;
use nwnrs_resref::prelude::{ResRef, ResolvedResRef};
use nwnrs_tga::prelude::read_tga;
use serde::{Deserialize, Serialize};

use crate::{
    NwnBevyError, NwnModelAsset, NwnModelNodeAsset, NwnPrimitiveAsset, NwnTextureLoadReason,
    NwnUnresolvedTexture, image_from_dds, image_from_plt, image_from_tga,
    install_state::shared_resman, mesh_from_primitive, standard_material_from_nwn,
    transform_from_nwn,
};

/// Loader for NWN `mdl` model assets.
#[derive(Default, Debug, Clone, TypePath)]
pub struct NwnMdlAssetLoader;

/// Settings for [`NwnMdlAssetLoader`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NwnMdlAssetLoaderSettings;

impl AssetLoader for NwnMdlAssetLoader {
    type Asset = NwnModelAsset;
    type Error = NwnBevyError;
    type Settings = NwnMdlAssetLoaderSettings;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &Self::Settings,
        load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;

        let mut cursor = Cursor::new(bytes);
        let scene = read_scene_model_auto(&mut cursor)?;

        let material_bindings = load_materials(load_context, &scene).await?;
        let mesh_bindings = load_meshes(load_context, &scene)?;
        let nodes = build_node_assets(&scene, &mesh_bindings, &material_bindings);
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

    fn extensions(&self) -> &[&str] {
        &["mdl"]
    }
}

#[derive(Debug, Clone)]
struct MaterialBinding {
    material:   Handle<StandardMaterial>,
    texture:    Option<Handle<Image>>,
    unresolved: Vec<NwnUnresolvedTexture>,
}

#[derive(Debug, Clone)]
struct MeshBinding {
    scene_mesh_index: usize,
    primitive_index:  usize,
    mesh:             Handle<Mesh>,
}

async fn load_materials(
    load_context: &mut LoadContext<'_>,
    scene: &NwnScene,
) -> Result<Vec<MaterialBinding>, NwnBevyError> {
    let mut bindings = Vec::with_capacity(scene.materials.len());
    for (material_index, material) in scene.materials.iter().enumerate() {
        let texture_result = load_material_texture(load_context, scene, material_index).await?;
        let texture_handle = texture_result.texture.clone();
        let material_handle = load_context
            .labeled_asset_scope(format!("material/{material_index}"), |_labeled| {
                Ok::<_, NwnBevyError>(standard_material_from_nwn(material, texture_handle))
            })?;
        bindings.push(MaterialBinding {
            material:   material_handle,
            texture:    texture_result.texture,
            unresolved: texture_result.unresolved,
        });
    }

    Ok(bindings)
}

async fn load_material_texture(
    load_context: &mut LoadContext<'_>,
    scene: &NwnScene,
    material_index: usize,
) -> Result<MaterialTextureLoad, NwnBevyError> {
    let material = scene
        .materials
        .get(material_index)
        .ok_or_else(|| NwnBevyError::msg(format!("material {material_index} is out of range")))?;

    let texture = material
        .textures
        .iter()
        .find(|texture| matches!(texture.slot, NwnTextureSlot::Bitmap));
    if texture.is_none() && material.material_name.is_none() {
        return Ok(MaterialTextureLoad::default());
    }

    let mut attempted = Vec::new();
    match load_material_texture_from_resman(load_context, material_index, material, texture)? {
        InstallTextureLoad::Loaded(texture_load) => return Ok(texture_load),
        InstallTextureLoad::Missing(candidates) => attempted.extend(candidates),
        InstallTextureLoad::Unavailable => {}
    }

    if let Some(texture) = texture {
        match load_asset_texture_candidate(load_context, material_index, texture.name.as_str())
            .await?
        {
            Some(texture_load) => return Ok(texture_load),
            None => attempted.extend(texture_candidates(texture.name.as_str())),
        }
    }

    let mtr_names = mtr_candidate_names(material, texture);
    for mtr_name in &mtr_names {
        let mtr_filename = format!("{mtr_name}.mtr");
        let asset_path = resolve_relative_asset_path(load_context, &mtr_filename)?;
        attempted.push(asset_path.to_string());
        let Ok(bytes) = load_context.read_asset_bytes(asset_path.clone()).await else {
            continue;
        };
        let mut cursor = Cursor::new(bytes);
        let mtr = read_mtr(&mut cursor)?;
        if let Some(texture_name) = mtr.texture0() {
            match load_asset_texture_candidate(load_context, material_index, texture_name).await? {
                Some(texture_load) => return Ok(texture_load),
                None => attempted.extend(texture_candidates(texture_name)),
            }
        }
    }

    Ok(MaterialTextureLoad {
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

fn load_material_texture_from_resman(
    load_context: &mut LoadContext<'_>,
    material_index: usize,
    material: &NwnMaterial,
    texture: Option<&NwnTextureRef>,
) -> Result<InstallTextureLoad, NwnBevyError> {
    let Some(shared_resman) = shared_resman() else {
        return Ok(InstallTextureLoad::Unavailable);
    };

    let mut resman = match shared_resman.lock() {
        Ok(resman) => resman,
        Err(error) => error.into_inner(),
    };
    let mut attempted = Vec::new();

    if let Some(texture) = texture {
        match resolve_texture_ref(texture, &mut resman, &texture_resolver_options()) {
            Ok(resolved) => {
                drop(resman);
                let image = image_from_resolved_texture(&resolved)?;
                let handle = load_context.labeled_asset_scope(
                    format!("texture/material_{material_index}"),
                    |_labeled| Ok::<_, NwnBevyError>(image.clone()),
                )?;
                return Ok(InstallTextureLoad::Loaded(MaterialTextureLoad {
                    texture:    Some(handle),
                    unresolved: Vec::new(),
                }));
            }
            Err(missing) => attempted.extend(
                missing
                    .attempted
                    .into_iter()
                    .map(|candidate| candidate.to_file()),
            ),
        }
    }

    for mtr_name in mtr_candidate_names(material, texture) {
        let Some(mtr_rr) = ResRef::new(mtr_name.clone(), MTR_RES_TYPE).ok() else {
            continue;
        };
        let mtr_filename = ResolvedResRef::new(mtr_name.clone(), MTR_RES_TYPE)
            .map(|resolved| resolved.to_file())
            .unwrap_or_else(|_| format!("{mtr_name}.mtr"));
        attempted.push(mtr_filename);
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
        match resolve_texture_ref(&texture_ref, &mut resman, &texture_resolver_options()) {
            Ok(resolved) => {
                drop(resman);
                let image = image_from_resolved_texture(&resolved)?;
                let handle = load_context.labeled_asset_scope(
                    format!("texture/material_{material_index}"),
                    |_labeled| Ok::<_, NwnBevyError>(image.clone()),
                )?;
                return Ok(InstallTextureLoad::Loaded(MaterialTextureLoad {
                    texture:    Some(handle),
                    unresolved: Vec::new(),
                }));
            }
            Err(missing) => attempted.extend(
                missing
                    .attempted
                    .into_iter()
                    .map(|candidate| candidate.to_file()),
            ),
        }
    }

    drop(resman);
    Ok(InstallTextureLoad::Missing(attempted))
}

async fn load_asset_texture_candidate(
    load_context: &mut LoadContext<'_>,
    material_index: usize,
    texture_name: &str,
) -> Result<Option<MaterialTextureLoad>, NwnBevyError> {
    for candidate in texture_candidates(texture_name) {
        let asset_path = resolve_relative_asset_path(load_context, &candidate)?;
        let Ok(bytes) = load_context.read_asset_bytes(asset_path).await else {
            continue;
        };
        let image = decode_texture_bytes(&candidate, &bytes)?;
        let label = format!("texture/material_{material_index}");
        let handle = load_context
            .labeled_asset_scope(label, |_labeled| Ok::<_, NwnBevyError>(image.clone()))?;
        return Ok(Some(MaterialTextureLoad {
            texture:    Some(handle),
            unresolved: Vec::new(),
        }));
    }
    Ok(None)
}

fn mtr_candidate_names(material: &NwnMaterial, bitmap: Option<&NwnTextureRef>) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(material_name) = material.material_name.as_deref()
        && is_mtr_candidate(material_name)
    {
        names.push(material_name.to_string());
    }
    if let Some(bitmap) = bitmap
        && is_mtr_candidate(bitmap.name.as_str())
        && !names
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(bitmap.name.as_str()))
    {
        names.push(bitmap.name.clone());
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

fn texture_resolver_options() -> TextureResolverOptions {
    TextureResolverOptions {
        fallback_order: vec![
            TextureResourceKind::Dds,
            TextureResourceKind::Tga,
            TextureResourceKind::Plt,
        ],
    }
}

fn image_from_resolved_texture(resolved: &ResolvedTexture) -> Result<Image, NwnBevyError> {
    let bytes = resolved
        .resource
        .read_all(true)
        .map_err(|error| NwnBevyError::msg(format!("read {}: {error}", resolved.resolved)))?;

    match resolved.kind {
        TextureResourceKind::Dds => decode_texture_bytes(&resolved.resolved.to_file(), &bytes),
        TextureResourceKind::Tga => decode_texture_bytes(&resolved.resolved.to_file(), &bytes),
        TextureResourceKind::Plt => decode_texture_bytes(&resolved.resolved.to_file(), &bytes),
    }
}

fn decode_texture_bytes(name: &str, bytes: &[u8]) -> Result<Image, NwnBevyError> {
    if name.to_ascii_lowercase().ends_with(".dds") {
        image_from_dds(&DdsTexture::read_from_texture_bytes(bytes)?)
    } else if name.to_ascii_lowercase().ends_with(".tga") {
        let mut cursor = Cursor::new(bytes);
        image_from_tga(&read_tga(&mut cursor)?)
    } else if name.to_ascii_lowercase().ends_with(".plt") {
        let mut cursor = Cursor::new(bytes);
        image_from_plt(&read_plt(&mut cursor)?)
    } else {
        Err(NwnBevyError::msg(format!(
            "unsupported texture format: {name}"
        )))
    }
}

fn load_meshes(
    load_context: &mut LoadContext<'_>,
    scene: &NwnScene,
) -> Result<Vec<MeshBinding>, NwnBevyError> {
    let mut bindings = Vec::new();
    for (scene_mesh_index, mesh) in scene.meshes.iter().enumerate() {
        for (primitive_index, primitive) in mesh.primitives.iter().enumerate() {
            let label = format!("mesh/{scene_mesh_index}/{primitive_index}");
            let bevy_mesh = mesh_from_primitive(primitive)?;
            let mesh_handle = load_context
                .labeled_asset_scope(label, |_labeled| Ok::<_, NwnBevyError>(bevy_mesh))?;
            bindings.push(MeshBinding {
                scene_mesh_index,
                primitive_index,
                mesh: mesh_handle,
            });
        }
    }

    Ok(bindings)
}

fn build_node_assets(
    scene: &NwnScene,
    mesh_bindings: &[MeshBinding],
    material_bindings: &[MaterialBinding],
) -> Vec<NwnModelNodeAsset> {
    let mut primitive_lookup = BTreeMap::new();
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
                                label:    format!("{}:{primitive_index}", mesh.name),
                                mesh:     mesh_handle,
                                material: material_handle,
                            })
                        })
                        .collect()
                });

            NwnModelNodeAsset {
                name: node.name.clone(),
                kind: node.kind.clone(),
                parent: node.parent,
                transform: transform_from_nwn(&node.local_transform),
                primitives,
            }
        })
        .collect()
}

fn resolve_relative_asset_path(
    load_context: &LoadContext<'_>,
    candidate: &str,
) -> Result<AssetPath<'static>, NwnBevyError> {
    let base = load_context
        .path()
        .parent()
        .unwrap_or_else(|| load_context.path().clone_owned());
    base.resolve(candidate)
        .map_err(|error| NwnBevyError::msg(format!("invalid asset path {candidate}: {error}")))
}

fn texture_candidates(name: &str) -> Vec<String> {
    match texture_extension(name) {
        Some("dds") | Some("tga") | Some("plt") => vec![name.to_string()],
        Some(_) => vec![name.to_string()],
        None => vec![
            format!("{name}.dds"),
            format!("{name}.tga"),
            format!("{name}.plt"),
        ],
    }
}

fn texture_extension(name: &str) -> Option<&str> {
    let extension = std::path::Path::new(name).extension()?.to_str()?;
    Some(extension)
}

#[cfg(test)]
fn is_explicit_plt(name: &str) -> bool {
    matches!(texture_extension(name), Some(extension) if extension.eq_ignore_ascii_case("plt"))
}

#[derive(Debug, Default)]
struct MaterialTextureLoad {
    texture:    Option<Handle<Image>>,
    unresolved: Vec<NwnUnresolvedTexture>,
}

#[derive(Debug)]
enum InstallTextureLoad {
    Loaded(MaterialTextureLoad),
    Missing(Vec<String>),
    Unavailable,
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use nwnrs_mdl::prelude::{
        NwnMaterial, NwnTextureRef, NwnTextureSlot, TextureResourceKind, resolve_texture_ref,
    };
    use nwnrs_resman::{ResContainer, ResMan};
    use nwnrs_resmemfile::prelude::read_resmemfile;
    use nwnrs_resref::ResolvedResRef;

    use super::{is_explicit_plt, texture_candidates, texture_resolver_options};
    use crate::install_state::{clear_shared_resman, set_shared_resman};

    #[test]
    fn bare_texture_names_try_dds_then_tga_then_plt() {
        assert_eq!(
            texture_candidates("stone"),
            vec![
                "stone.dds".to_string(),
                "stone.tga".to_string(),
                "stone.plt".to_string()
            ]
        );
    }

    #[test]
    fn explicit_plt_is_detected() {
        assert!(is_explicit_plt("cloak_001.plt"));
        assert!(!is_explicit_plt("cloak_001.dds"));
    }

    #[test]
    fn install_texture_resolution_prefers_dds_over_tga() {
        let texture = NwnTextureRef {
            slot: NwnTextureSlot::Bitmap,
            name: "stone".to_string(),
        };
        let shared = Arc::new(Mutex::new(build_manager(&[
            ("tga", "stone.tga", b"tga"),
            ("dds", "stone.dds", b"dds"),
        ])));
        set_shared_resman(Arc::clone(&shared));

        let resolved = {
            let mut manager = match shared.lock() {
                Ok(manager) => manager,
                Err(error) => error.into_inner(),
            };
            let resolved = resolve_texture_ref(&texture, &mut manager, &texture_resolver_options());
            assert!(
                resolved.is_ok(),
                "resolve install texture failed: {:?}",
                resolved.err()
            );
            match resolved {
                Ok(resolved) => resolved,
                Err(_) => return,
            }
        };

        clear_shared_resman();
        assert_eq!(resolved.resolved.to_file(), "stone.dds");
    }

    #[test]
    fn install_texture_resolution_considers_plt_after_dds_and_tga() {
        let texture = NwnTextureRef {
            slot: NwnTextureSlot::Bitmap,
            name: "cloak_001".to_string(),
        };
        let mut manager = build_manager(&[("plt", "cloak_001.plt", b"plt")]);

        let resolved = resolve_texture_ref(&texture, &mut manager, &texture_resolver_options());
        assert!(
            resolved.is_ok(),
            "resolve bare plt texture failed: {:?}",
            resolved.err()
        );
        let resolved = match resolved {
            Ok(resolved) => resolved,
            Err(_) => return,
        };

        assert_eq!(resolved.kind, TextureResourceKind::Plt);
        assert_eq!(resolved.resolved.to_file(), "cloak_001.plt");
    }

    #[test]
    fn mtr_candidates_prefer_material_name_then_bitmap() {
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
            diffuse:           [1.0, 1.0, 1.0],
            specular:          [0.0, 0.0, 0.0],
            self_illum_color:  [0.0, 0.0, 0.0],
            material_name:     Some("Stone".to_string()),
            render_hint:       None,
            textures:          vec![NwnTextureRef {
                slot: NwnTextureSlot::Bitmap,
                name: "weaponstex".to_string(),
            }],
        };

        assert_eq!(
            super::mtr_candidate_names(&material, material.textures.first()),
            vec!["Stone".to_string(), "weaponstex".to_string()]
        );
    }

    fn build_manager(entries: &[(&str, &str, &[u8])]) -> ResMan {
        let mut manager = ResMan::new(1);
        for (label, filename, bytes) in entries {
            let resref = ResolvedResRef::from_filename(filename);
            assert!(resref.is_ok(), "resolved {filename}: {:?}", resref.err());
            let resref = match resref {
                Ok(resref) => resref,
                Err(_) => continue,
            };
            let container = read_resmemfile((*label).to_string(), resref.into(), bytes.to_vec());
            assert!(
                container.is_ok(),
                "resmem {filename}: {:?}",
                container.err()
            );
            let container = match container {
                Ok(container) => container,
                Err(_) => continue,
            };
            manager.add(Arc::new(container) as Arc<dyn ResContainer>);
        }
        manager
    }
}
