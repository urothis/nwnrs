use std::error::Error;

use nwnrs_types::{
    mdl::{
        AsciiModel, AsciiPayloadKind, BinaryModel, BinaryToAsciiOptions, MODEL_RES_TYPE,
        lower_binary_model_to_ascii, lower_binary_model_to_ascii_with_options, parse_ascii_model,
        restore_compiled_model, write_ascii_model,
    },
    resman::CachePolicy,
};

use super::support::{demand_resource, require_game_resource, skip_if_game_resources_unavailable};

#[test]
fn fixture_parses_geometry_and_animation_structure() -> Result<(), Box<dyn Error>> {
    let ascii = match shipped_ascii_fixture() {
        Ok(ascii) => ascii,
        Err(error) => return skip_if_game_resources_unavailable(error),
    };

    assert_eq!(ascii.geometry_name, "a_ba_casts");
    assert_eq!(
        ascii
            .prefix_statement("newmodel")
            .and_then(|statement| statement.argument(0)),
        Some("a_ba_casts")
    );
    assert!(ascii.geometry_nodes().count() > 10);
    assert_eq!(ascii.animations.len(), 19);

    let conjure = ascii.animation("conjure1").unwrap_or_else(|| {
        panic!("missing conjure1 animation");
    });
    assert_eq!(
        conjure
            .statement("animroot")
            .and_then(|statement| statement.argument(0)),
        Some("rootdummy")
    );

    let rootdummy = conjure.node("rootdummy").unwrap_or_else(|| {
        panic!("missing rootdummy animation node");
    });
    let positionkey = rootdummy.statement("positionkey").unwrap_or_else(|| {
        panic!("missing rootdummy positionkey");
    });
    assert_eq!(positionkey.payload_kind, Some(AsciiPayloadKind::Counted));
    assert_eq!(positionkey.payload_rows.len(), 5);

    let eventful = ascii.animation("castout").unwrap_or_else(|| {
        panic!("missing castout animation");
    });
    assert_eq!(
        eventful
            .statement("event")
            .and_then(|statement| statement.argument(1)),
        Some("cast")
    );
    Ok(())
}

#[test]
fn canonical_write_roundtrips_through_parse() -> Result<(), Box<dyn Error>> {
    let parsed = match shipped_ascii_fixture() {
        Ok(ascii) => ascii,
        Err(error) => return skip_if_game_resources_unavailable(error),
    };

    let mut encoded = Vec::new();
    if let Err(error) = write_ascii_model(&mut encoded, &parsed) {
        panic!("write ascii model: {error}");
    }

    let reparsed = parse_ascii_model(&String::from_utf8_lossy(&encoded)).unwrap_or_else(|error| {
        panic!("reparse canonical text: {error}");
    });

    assert_eq!(reparsed, parsed);
    Ok(())
}

#[test]
fn converted_ascii_restores_original_compiled_model() -> Result<(), Box<dyn Error>> {
    let ascii = match shipped_reversible_ascii_fixture() {
        Ok(ascii) => ascii,
        Err(error) => return skip_if_game_resources_unavailable(error),
    };
    let compiled = restore_compiled_model(&ascii).unwrap_or_else(|error| {
        panic!("restore canonical ascii: {error}");
    });
    let parsed = compiled.parse_binary().unwrap_or_else(|error| {
        panic!("parse recompiled model: {error}");
    });

    assert_eq!(parsed.name(), "a_ba_casts");
    assert!(parsed.nodes().len() > 10);
    assert_eq!(parsed.animations().len(), 19);

    let mut edited = ascii;
    edited.geometry_name = "edited".to_string();
    let error = restore_compiled_model(&edited).unwrap_err();
    assert!(error.to_string().contains("edited ASCII"));
    Ok(())
}

fn shipped_ascii_fixture() -> Result<AsciiModel, Box<dyn Error>> {
    let res = require_game_resource(demand_resource("a_ba_casts", MODEL_RES_TYPE))?;
    let binary = BinaryModel::from_res(&res, CachePolicy::Use)?;
    Ok(lower_binary_model_to_ascii(&binary)?)
}

fn shipped_reversible_ascii_fixture() -> Result<AsciiModel, Box<dyn Error>> {
    let res = require_game_resource(demand_resource("a_ba_casts", MODEL_RES_TYPE))?;
    let binary = BinaryModel::from_res(&res, CachePolicy::Use)?;
    Ok(lower_binary_model_to_ascii_with_options(
        &binary,
        BinaryToAsciiOptions {
            embed_original_binary: true,
        },
    )?)
}
