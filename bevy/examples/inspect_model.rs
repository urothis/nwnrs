//! Prints authored animation and material diagnostics for one NWN model.

use bevy::{asset::Assets, image::Image, mesh::Mesh, pbr::StandardMaterial};
use nwnrs_bevy::load_nwn_model_from_resman;
use nwnrs_game::prelude::{find_nwnrs_root, find_user_root, new_default_resman};
use nwnrs_mdl::prelude::{MODEL_RES_TYPE, read_scene_model_auto_from_res};
use nwnrs_resref::prelude::{ResRef, ResolvedResRef};

fn main() {
    let model_name = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "ttr01_k01_01".to_string());

    let root = find_nwnrs_root("").unwrap_or_else(|error| {
        panic!("resolve NWN root: {error}");
    });
    let user_root = find_user_root("").unwrap_or_else(|error| {
        panic!("resolve NWN user root: {error}");
    });
    let mut resman = new_default_resman(
        &root,
        &user_root,
        "english",
        0,
        true,
        false,
        &[],
        &[],
        &[],
        &[],
    )
    .unwrap_or_else(|error| {
        panic!("build default resman: {error}");
    });

    let resref = ResRef::new(model_name.clone(), MODEL_RES_TYPE).unwrap_or_else(|error| {
        panic!("invalid mdl resref {model_name}: {error}");
    });
    let res = resman.get(&resref).unwrap_or_else(|| {
        panic!("model not found in resman: {model_name}.mdl");
    });
    let scene = read_scene_model_auto_from_res(&res, true).unwrap_or_else(|error| {
        panic!("read scene {model_name}: {error}");
    });
    let mut images = Assets::<Image>::default();
    let mut meshes = Assets::<Mesh>::default();
    let mut materials = Assets::<StandardMaterial>::default();
    let runtime_model = load_nwn_model_from_resman(
        &mut resman,
        &model_name,
        &mut images,
        &mut meshes,
        &mut materials,
    )
    .unwrap_or_else(|error| {
        panic!("load runtime model {model_name}: {error}");
    });

    println!("model={}", scene.name);
    println!("nodes={}", scene.nodes.len());
    for (index, node) in scene.nodes.iter().enumerate() {
        println!(
            "  node index={} name={} kind={:?} parent={:?} mesh={:?} reference_model={}",
            index,
            node.name,
            node.kind,
            node.parent,
            node.mesh,
            node.reference
                .as_ref()
                .and_then(|reference| reference.model.as_deref())
                .unwrap_or("")
        );
    }
    println!("animations={}", scene.animations.len());
    for animation in &scene.animations {
        println!(
            "  animation name={} length={}",
            animation.name, animation.length
        );
        for track in &animation.node_tracks {
            let animmesh = track.animmesh.as_ref();
            let has_transform = !track.transform.translation_keys.is_empty()
                || !track.transform.rotation_axis_angle_keys.is_empty()
                || !track.transform.scale_keys.is_empty();
            let has_material = !track.material.color_keys.is_empty()
                || !track.material.alpha_keys.is_empty()
                || !track.material.radius_keys.is_empty()
                || !track.material.self_illum_color_keys.is_empty();
            println!(
                "    track target={} kind={:?} transform={} material={} animmesh_vertices={} \
                 animmesh_uvs={} sample_period={:?}",
                track.target_name,
                track.kind,
                has_transform,
                has_material,
                animmesh
                    .map(|track| track.vertex_samples.len())
                    .unwrap_or(0),
                animmesh.map(|track| track.uv_samples.len()).unwrap_or(0),
                animmesh.and_then(|track| track.sample_period),
            );
            if has_material {
                println!(
                    "      color_keys={:?} alpha_keys={:?} self_illum_keys={:?}",
                    track.material.color_keys,
                    track.material.alpha_keys,
                    track.material.self_illum_color_keys
                );
            }
        }
    }

    println!("materials={}", scene.materials.len());
    for (index, material) in scene.materials.iter().enumerate() {
        println!(
            "  material index={} source_node={} source_name={} render_enabled={} \
             rotate_texture={} tilefade={} alpha={} render_hint={:?} material_name={:?} \
             helper_bitmap={:?}",
            index,
            material.source_node,
            scene
                .nodes
                .get(material.source_node)
                .map(|node| node.name.as_str())
                .unwrap_or(""),
            material.render_enabled,
            material.rotate_texture,
            material.tilefade,
            material.alpha,
            material.render_hint,
            material.material_name,
            material.helper_bitmap,
        );
        for texture in &material.textures {
            println!("    texture slot={:?} name={}", texture.slot, texture.name);
            inspect_sidecar(&mut resman, &texture.name, "mtr");
            inspect_sidecar(&mut resman, &texture.name, "txi");
        }
        if material.textures.iter().any(|texture| {
            let lower = texture.name.to_ascii_lowercase();
            lower.contains("water")
        }) {
            inspect_water_primitives(&scene, index, material);
        }
        inspect_material_primitives(&scene, index);
    }

    println!("runtime_unresolved={}", runtime_model.unresolved.len());
    for unresolved in &runtime_model.unresolved {
        println!(
            "  unresolved material_index={} slot={:?} name={} reason={:?}",
            unresolved.material_index, unresolved.slot, unresolved.name, unresolved.reason
        );
        if !unresolved.attempted.is_empty() {
            println!(
                "    attempted={}",
                unresolved
                    .attempted
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    }
}

fn inspect_material_primitives(scene: &nwnrs_mdl::prelude::NwnScene, material_index: usize) {
    for node in &scene.nodes {
        let Some(mesh_index) = node.mesh else {
            continue;
        };
        let Some(mesh) = scene.meshes.get(mesh_index) else {
            continue;
        };
        for (primitive_index, primitive) in mesh.primitives.iter().enumerate() {
            if primitive.material != Some(material_index) {
                continue;
            }
            println!(
                "    primitive node={} kind={:?} mesh={} primitive_index={} positions={} faces={} \
                 uv_sets={}",
                node.name,
                node.kind,
                mesh.name,
                primitive_index,
                primitive.positions.len(),
                primitive.faces.len(),
                primitive.uv_sets.len()
            );
        }
    }
}

fn inspect_sidecar(resman: &mut nwnrs_resman::prelude::ResMan, stem: &str, extension: &str) {
    let filename = format!("{stem}.{extension}");
    let resolved = match ResolvedResRef::from_filename(&filename) {
        Ok(resolved) => resolved,
        Err(_) => return,
    };
    let rr: ResRef = resolved.into();
    let Some(res) = resman.get(&rr) else {
        return;
    };
    println!("      found sidecar {}", filename);
    if extension.eq_ignore_ascii_case("txi") {
        let bytes = res.read_all(true).unwrap_or_else(|error| {
            panic!("read {filename}: {error}");
        });
        let text = String::from_utf8_lossy(&bytes);
        for line in text.lines().take(32) {
            println!("        {}", line);
        }
    }
}

fn inspect_water_primitives(
    scene: &nwnrs_mdl::prelude::NwnScene,
    material_index: usize,
    material: &nwnrs_mdl::prelude::NwnMaterial,
) {
    for (node_index, node) in scene.nodes.iter().enumerate() {
        let Some(mesh_index) = node.mesh else {
            continue;
        };
        let Some(mesh) = scene.meshes.get(mesh_index) else {
            continue;
        };
        for (primitive_index, primitive) in mesh.primitives.iter().enumerate() {
            if primitive.material != Some(material_index) {
                continue;
            }
            println!(
                "    water-like primitive texture={} node={} kind={:?} node_index={} parent={:?} \
                 translation={:?} rotation_axis_angle={:?} scale={:?}",
                material
                    .textures
                    .first()
                    .map(|texture| texture.name.as_str())
                    .unwrap_or(""),
                node.name,
                node.kind,
                node_index,
                node.parent,
                node.local_transform.translation,
                node.local_transform.rotation_axis_angle,
                node.local_transform.scale,
            );
            println!(
                "      mesh={} primitive_index={} positions={} faces={} uv_sets={}",
                mesh.name,
                primitive_index,
                primitive.positions.len(),
                primitive.faces.len(),
                primitive.uv_sets.len()
            );
            for (index, position) in primitive.positions.iter().take(8).enumerate() {
                println!("      pos[{index}]={:?}", position);
            }
            if let Some(uv_set) = primitive.uv_sets.first() {
                for (index, uv) in uv_set.coordinates.iter().take(8).enumerate() {
                    println!("      uv[{index}]={:?}", uv);
                }
            }
            for (face_index, face) in primitive.faces.iter().take(4).enumerate() {
                println!(
                    "      face[{face_index}] verts={:?} uvs={:?}",
                    face.vertex_indices, face.uv_indices
                );
            }
        }
    }
}
