#![allow(missing_docs)]

mod support;

use std::{error::Error, io::Cursor};

use nwnrs_types::{
    mdl::{
        BinaryModel, MODEL_RES_TYPE, Model, NodeKind, SemanticModel, lower_binary_model_to_ascii,
        parse_semantic_model, read_semantic_model, write_semantic_model,
    },
    resman::CachePolicy,
};
use support::{demand_resource, require_game_resource, skip_if_game_resources_unavailable};

#[test]
fn fixture_lowers_mesh_material_and_geometry() -> Result<(), Box<dyn Error>> {
    let model = match shipped_ascii_semantic_fixture() {
        Ok(model) => model,
        Err(error) => return skip_if_game_resources_unavailable(error),
    };
    assert_eq!(model.header.model_name, "a_ba_casts");
    let torso = model.node("torso_g").unwrap_or_else(|| {
        panic!("missing torso_g node");
    });
    assert_eq!(torso.kind, NodeKind::Trimesh);
    assert_eq!(torso.material.bitmap.as_deref(), Some("pmh0_chest001"));
    let torso_mesh = torso.mesh.as_ref().unwrap_or_else(|| {
        panic!("torso_g should have mesh data");
    });
    assert_eq!(torso_mesh.vertices.len(), 122);
    assert_eq!(torso_mesh.faces.len(), 70);
    assert_eq!(
        torso_mesh
            .uv_layers
            .first()
            .map(|layer| layer.coordinates.len()),
        Some(122)
    );
    Ok(())
}

#[test]
fn animated_fixture_lowers_headers_and_keyframes() -> Result<(), Box<dyn Error>> {
    let model = match shipped_ascii_semantic_fixture() {
        Ok(model) => model,
        Err(error) => return skip_if_game_resources_unavailable(error),
    };

    assert_eq!(model.animations.len(), 19);
    let conjure = model.animation("conjure1").unwrap_or_else(|| {
        panic!("missing conjure1 animation");
    });
    assert_eq!(conjure.length, Some(1.0));
    assert_eq!(conjure.transtime, Some(0.5));
    assert_eq!(conjure.animroot.as_deref(), Some("rootdummy"));

    let rootdummy = conjure.node("rootdummy").unwrap_or_else(|| {
        panic!("missing conjure1/rootdummy");
    });
    assert_eq!(rootdummy.position_keys.len(), 5);
    assert_eq!(rootdummy.orientation_keys.len(), 2);

    let castout = model.animation("castout").unwrap_or_else(|| {
        panic!("missing castout animation");
    });
    assert_eq!(
        castout.events.first().map(|event| event.name.as_str()),
        Some("cast")
    );
    Ok(())
}

#[test]
fn compiled_fixture_lowers_headers_and_animation_structure() -> Result<(), Box<dyn Error>> {
    let model = match shipped_compiled_semantic_fixture() {
        Ok(model) => model,
        Err(error) => return skip_if_game_resources_unavailable(error),
    };

    assert_eq!(model.header.model_name, "a_ba2");
    assert_eq!(model.header.supermodel.as_deref(), Some("a_ba"));
    assert_eq!(model.geometry_name, "a_ba2");
    assert_eq!(model.nodes.len(), 57);
    assert_eq!(model.animations.len(), 20);

    let torso = model.node("torso_g").unwrap_or_else(|| {
        panic!("missing compiled torso_g node");
    });
    assert_eq!(torso.parent.as_deref(), Some("rootdummy"));
    assert_eq!(torso.kind, NodeKind::Trimesh);
    assert!(torso.mesh.is_some());
    assert_eq!(torso.material.bitmap, None);
    assert!(model.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("suspicious bitmap value torso_g")
    }));
    assert!(
        model
            .nodes
            .iter()
            .any(|node| node.material.bitmap.as_deref() == Some("pmh0_pelvis001"))
    );
    assert!(model.nodes.iter().all(|node| {
        !matches!(
            node.material.bitmap.as_deref(),
            Some("torso_g" | "neck_g" | "head_g" | "material" | "Material")
        )
    }));

    let salute = model.animation("salute").unwrap_or_else(|| {
        panic!("missing compiled salute animation");
    });
    assert_eq!(salute.model_name, "a_ba2");
    assert_eq!(salute.length, Some(0.5));
    assert_eq!(salute.transtime, Some(0.4));
    assert_eq!(salute.animroot.as_deref(), Some("torso_g"));
    assert!(salute.node("rootdummy").is_some());
    Ok(())
}

#[test]
fn model_parse_semantic_lowers_raw_bytes() -> Result<(), Box<dyn Error>> {
    let bytes = match shipped_ascii_semantic_fixture_bytes() {
        Ok(bytes) => bytes,
        Err(error) => return skip_if_game_resources_unavailable(error),
    };
    let model = Model::new(bytes).parse_semantic().unwrap_or_else(|error| {
        panic!("parse semantic from model bytes: {error}");
    });
    assert!(model.node("torso_g").is_some());
    Ok(())
}

#[test]
fn semantic_writer_roundtrips_canonical_model() -> Result<(), Box<dyn Error>> {
    let model = match shipped_ascii_semantic_fixture() {
        Ok(model) => model,
        Err(error) => return skip_if_game_resources_unavailable(error),
    };

    let mut encoded = Vec::new();
    if let Err(error) = write_semantic_model(&mut encoded, &model) {
        panic!("write semantic model: {error}");
    }

    let mut cursor = Cursor::new(encoded);
    let reparsed = read_semantic_model(&mut cursor).unwrap_or_else(|error| {
        panic!("read rewritten semantic model: {error}");
    });
    assert_eq!(
        normalize_semantic_model(reparsed),
        normalize_semantic_model(model)
    );
    Ok(())
}

fn normalize_semantic_model(mut model: SemanticModel) -> SemanticModel {
    model.diagnostics.clear();
    model
}

fn shipped_ascii_semantic_fixture() -> Result<SemanticModel, Box<dyn Error>> {
    let res = require_game_resource(demand_resource("a_ba_casts", MODEL_RES_TYPE))?;
    let binary = BinaryModel::from_res(&res, CachePolicy::Use)?;
    let ascii = lower_binary_model_to_ascii(&binary)?;
    Ok(parse_semantic_model(&ascii.to_text())?)
}

fn shipped_ascii_semantic_fixture_bytes() -> Result<Vec<u8>, Box<dyn Error>> {
    let res = require_game_resource(demand_resource("a_ba_casts", MODEL_RES_TYPE))?;
    let binary = BinaryModel::from_res(&res, CachePolicy::Use)?;
    let ascii = lower_binary_model_to_ascii(&binary)?;
    Ok(ascii.to_text().into_bytes())
}

fn shipped_compiled_semantic_fixture() -> Result<SemanticModel, Box<dyn Error>> {
    let res = require_game_resource(demand_resource("a_ba2", MODEL_RES_TYPE))?;
    Ok(SemanticModel::from_auto_res(&res, CachePolicy::Use)?)
}
