#![allow(missing_docs)]

mod support;

use std::{error::Error, io::Cursor};

use nwnrs_types::{
    mdl::{
        BinaryModel, MODEL_RES_TYPE, NwnScene, NwnTextureSlot, lower_binary_model_to_ascii,
        parse_scene_model, read_scene_model, write_scene_model,
    },
    resman::CachePolicy,
};
use support::{demand_resource, require_game_resource, skip_if_game_resources_unavailable};

#[test]
fn fixture_lowers_to_scene_mesh_and_material() -> Result<(), Box<dyn Error>> {
    let scene = match shipped_ascii_scene_fixture() {
        Ok(scene) => scene,
        Err(error) => return skip_if_game_resources_unavailable(error),
    };

    assert_eq!(scene.name, "a_ba_casts");
    let torso = scene.node("torso_g").unwrap_or_else(|| {
        panic!("missing scene node torso_g");
    });
    let mesh = torso
        .mesh
        .and_then(|mesh_index| scene.meshes.get(mesh_index))
        .unwrap_or_else(|| panic!("torso_g missing mesh reference"));
    assert_eq!(mesh.primitives.len(), 1);
    let primitive = mesh
        .primitives
        .first()
        .unwrap_or_else(|| panic!("missing primitive"));
    assert_eq!(primitive.positions.len(), 122);
    assert_eq!(primitive.faces.len(), 70);
    assert_eq!(
        primitive.uv_sets.first().map(|set| set.coordinates.len()),
        Some(122)
    );

    let material = primitive
        .material
        .and_then(|material_index| scene.materials.get(material_index))
        .unwrap_or_else(|| panic!("torso_g missing material reference"));
    assert!(
        material.textures.iter().any(
            |texture| texture.slot == NwnTextureSlot::Bitmap && texture.name == "pmh0_chest001"
        )
    );
    Ok(())
}

#[test]
fn animated_fixture_lowers_to_scene_tracks() -> Result<(), Box<dyn Error>> {
    let scene = match shipped_ascii_scene_fixture() {
        Ok(scene) => scene,
        Err(error) => return skip_if_game_resources_unavailable(error),
    };

    let conjure = scene.animation("conjure1").unwrap_or_else(|| {
        panic!("missing scene animation conjure1");
    });
    assert_eq!(conjure.length, 1.0);
    assert_eq!(conjure.transition_time, 0.5);
    assert_eq!(
        conjure
            .root_node
            .and_then(|index| scene.nodes.get(index))
            .map(|node| node.name.as_str()),
        Some("rootdummy")
    );

    let rootdummy = conjure.node_track("rootdummy").unwrap_or_else(|| {
        panic!("missing rootdummy animation track");
    });
    assert!(rootdummy.target_node.is_some());
    assert_eq!(rootdummy.transform.translation_keys.len(), 5);
    assert_eq!(rootdummy.transform.rotation_axis_angle_keys.len(), 2);
    Ok(())
}

#[test]
fn compiled_fixture_lowers_to_scene_graph_and_tracks() -> Result<(), Box<dyn Error>> {
    let scene = match shipped_compiled_scene_fixture() {
        Ok(scene) => scene,
        Err(error) => return skip_if_game_resources_unavailable(error),
    };

    assert_eq!(scene.name, "a_ba2");
    assert_eq!(scene.supermodel.as_deref(), Some("a_ba"));
    assert_eq!(scene.nodes.len(), 57);
    assert_eq!(scene.animations.len(), 20);

    let torso = scene.node("torso_g").unwrap_or_else(|| {
        panic!("missing compiled torso_g scene node");
    });
    let parent_name = torso
        .parent
        .and_then(|index| scene.nodes.get(index))
        .map(|node| node.name.as_str());
    assert_eq!(parent_name, Some("rootdummy"));
    assert!(torso.mesh.is_some());

    let salute = scene.animation("salute").unwrap_or_else(|| {
        panic!("missing compiled salute animation");
    });
    assert_eq!(salute.length, 0.5);
    assert_eq!(salute.transition_time, 0.4);
    assert_eq!(salute.root_name.as_deref(), Some("torso_g"));
    assert!(salute.node_track("rootdummy").is_some());
    Ok(())
}

#[test]
fn scene_writer_roundtrips_canonical_model() -> Result<(), Box<dyn Error>> {
    let scene = match shipped_ascii_scene_fixture() {
        Ok(scene) => scene,
        Err(error) => return skip_if_game_resources_unavailable(error),
    };

    let mut encoded = Vec::new();
    if let Err(error) = write_scene_model(&mut encoded, &scene) {
        panic!("write scene model: {error}");
    }

    let mut cursor = Cursor::new(encoded);
    let reparsed = read_scene_model(&mut cursor).unwrap_or_else(|error| {
        panic!("read rewritten scene model: {error}");
    });
    assert_eq!(reparsed.name, scene.name);
    assert_eq!(reparsed.nodes.len(), scene.nodes.len());
    assert_eq!(reparsed.meshes.len(), scene.meshes.len());
    assert_eq!(reparsed.materials.len(), scene.materials.len());
    assert_eq!(reparsed.animations.len(), scene.animations.len());
    assert_eq!(
        reparsed
            .animation("conjure1")
            .map(|animation| animation.root_name.as_deref()),
        scene
            .animation("conjure1")
            .map(|animation| animation.root_name.as_deref())
    );
    assert_eq!(
        reparsed
            .node("torso_g")
            .and_then(|node| node.mesh)
            .and_then(|mesh_index| reparsed.meshes.get(mesh_index))
            .and_then(|mesh| mesh.primitives.first())
            .map(|primitive| (primitive.positions.len(), primitive.faces.len())),
        Some((122, 70))
    );
    Ok(())
}

#[test]
fn scene_writer_rejects_invalid_parent_indices() -> Result<(), Box<dyn Error>> {
    let mut scene = writable_scene_fixture()?;
    scene
        .nodes
        .get_mut(1)
        .unwrap_or_else(|| panic!("writable scene fixture missing body node"))
        .parent = Some(usize::MAX);

    let error = write_scene_model(&mut Vec::new(), &scene).unwrap_err();
    assert!(error.to_string().contains("invalid parent index"));
    Ok(())
}

#[test]
fn scene_writer_rejects_invalid_material_indices() -> Result<(), Box<dyn Error>> {
    let mut scene = writable_scene_fixture()?;
    scene
        .meshes
        .get_mut(0)
        .and_then(|mesh| mesh.primitives.get_mut(0))
        .unwrap_or_else(|| panic!("writable scene fixture missing body primitive"))
        .material = Some(usize::MAX);

    let error = write_scene_model(&mut Vec::new(), &scene).unwrap_err();
    assert!(error.to_string().contains("invalid material index"));
    Ok(())
}

#[test]
fn scene_writer_rejects_invalid_animation_targets() -> Result<(), Box<dyn Error>> {
    let mut scene = writable_scene_fixture()?;
    scene
        .animations
        .get_mut(0)
        .and_then(|animation| animation.node_tracks.get_mut(0))
        .unwrap_or_else(|| panic!("writable scene fixture missing animation track"))
        .target_node = Some(usize::MAX);

    let error = write_scene_model(&mut Vec::new(), &scene).unwrap_err();
    assert!(error.to_string().contains("invalid target index"));
    Ok(())
}

#[test]
fn scene_writer_rejects_non_uniform_scale() -> Result<(), Box<dyn Error>> {
    let mut scene = writable_scene_fixture()?;
    scene
        .nodes
        .get_mut(1)
        .unwrap_or_else(|| panic!("writable scene fixture missing body node"))
        .local_transform
        .scale = [1.0, 2.0, 1.0];

    let error = write_scene_model(&mut Vec::new(), &scene).unwrap_err();
    assert!(error.to_string().contains("non-uniform scale"));
    Ok(())
}

fn shipped_ascii_scene_fixture() -> Result<NwnScene, Box<dyn Error>> {
    let res = require_game_resource(demand_resource("a_ba_casts", MODEL_RES_TYPE))?;
    let binary = BinaryModel::from_res(&res, CachePolicy::Use)?;
    let ascii = lower_binary_model_to_ascii(&binary)?;
    Ok(parse_scene_model(&ascii.to_text())?)
}

fn shipped_compiled_scene_fixture() -> Result<NwnScene, Box<dyn Error>> {
    let res = require_game_resource(demand_resource("a_ba2", MODEL_RES_TYPE))?;
    Ok(NwnScene::from_auto_res(&res, CachePolicy::Use)?)
}

fn writable_scene_fixture() -> Result<NwnScene, Box<dyn Error>> {
    Ok(parse_scene_model(
        "\
newmodel demo
setsupermodel demo null
classification character
setanimationscale 1
beginmodelgeom demo
node dummy demo
  parent NULL
endnode
node trimesh body
  parent demo
  bitmap body01
  verts 3
    0 0 0
    1 0 0
    0 1 0
  faces 1
    0 1 2  0  0 1 2  0
  tverts 3
    0 0 0
    1 0 0
    0 1 0
endnode
endmodelgeom demo
newanim idle demo
  length 1
  transtime 0.25
  animroot body
  node dummy body
    parent demo
    positionkey 1
      0 0 0 0
  endnode
doneanim idle demo
donemodel demo
",
    )?)
}
