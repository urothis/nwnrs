use std::{error::Error, io::Cursor};

use nwnrs_types::{
    mdl::{
        BinaryModel, MODEL_RES_TYPE, ModelEncoding, ParsedModel, detect_model_encoding,
        lower_binary_model_to_ascii, parse_binary_model_bytes, parse_model_bytes,
        read_parsed_model, write_ascii_model, write_original_binary_model, write_parsed_model,
    },
    resman::CachePolicy,
};

use super::support::{demand_resource, require_game_resource, skip_if_game_resources_unavailable};

const FILE_HEADER_SIZE: usize = 12;

#[test]
fn detects_ascii_fixture_encoding() -> Result<(), Box<dyn Error>> {
    let bytes = match shipped_ascii_fixture_bytes() {
        Ok(bytes) => bytes,
        Err(error) => return skip_if_game_resources_unavailable(error),
    };
    assert_eq!(detect_model_encoding(&bytes), ModelEncoding::Ascii);
    Ok(())
}

#[test]
fn automatic_ascii_io_preserves_non_utf8_bytes() -> Result<(), Box<dyn Error>> {
    let bytes = b"# author \xff\nnewmodel demo\nbeginmodelgeom demo\nnode dummy demo\n  parent \
                  NULL\nendnode\nendmodelgeom demo\ndonemodel demo\n";
    let parsed = parse_model_bytes(bytes)?;
    assert!(matches!(parsed, ParsedModel::Ascii(_)));

    let mut encoded = Vec::new();
    write_parsed_model(&mut encoded, &parsed)?;
    assert!(encoded.windows(9).any(|window| window == b"author \xff\n"));
    Ok(())
}

#[test]
fn detects_compiled_fixture_encoding() -> Result<(), Box<dyn Error>> {
    let bytes = match shipped_compiled_fixture_bytes() {
        Ok(bytes) => bytes,
        Err(error) => return skip_if_game_resources_unavailable(error),
    };
    assert_eq!(detect_model_encoding(&bytes), ModelEncoding::Compiled);
    Ok(())
}

#[test]
fn parses_compiled_fixture_header_and_summary() -> Result<(), Box<dyn Error>> {
    let model = match shipped_compiled_fixture() {
        Ok(model) => model,
        Err(error) => return skip_if_game_resources_unavailable(error),
    };

    assert_eq!(model.name(), "a_ba2");
    assert_eq!(model.node_count_hint(), 222);
    assert_eq!(model.nodes().len(), 57);
    assert_eq!(model.animations().len(), 20);
    assert_eq!(model.header().binary_id, 0);
    assert_eq!(model.header().raw_data_offset, 760_200);
    assert_eq!(model.header().raw_data_size, 77_606);
    assert!(model.node("torso_g").is_some());
    assert!(model.animation("salute").is_some());
    assert_eq!(
        model
            .animation("salute")
            .map(|animation| animation.nodes.len()),
        Some(36)
    );
    assert_eq!(
        model
            .animation("castout")
            .map(|animation| animation.nodes.len()),
        Some(55)
    );
    Ok(())
}

#[test]
fn auto_parsing_dispatches_to_compiled_model() -> Result<(), Box<dyn Error>> {
    let bytes = match shipped_compiled_fixture_bytes() {
        Ok(bytes) => bytes,
        Err(error) => return skip_if_game_resources_unavailable(error),
    };

    let parsed = parse_model_bytes(&bytes).unwrap_or_else(|error| {
        panic!("parse compiled bytes: {error}");
    });
    match parsed {
        ParsedModel::Compiled(model) => {
            assert_eq!(model.name(), "a_ba2");
        }
        ParsedModel::Ascii(_ascii) => panic!("expected compiled model"),
    }
    Ok(())
}

#[test]
fn malformed_animation_pointer_becomes_diagnostic() -> Result<(), Box<dyn Error>> {
    let mut bytes = match shipped_compiled_fixture_bytes() {
        Ok(bytes) => bytes,
        Err(error) => return skip_if_game_resources_unavailable(error),
    };
    let animation_pointer_offset = FILE_HEADER_SIZE + 232;
    let replacement = u32::MAX.to_le_bytes();
    let target = bytes
        .get_mut(animation_pointer_offset..animation_pointer_offset + 4)
        .unwrap_or_else(|| panic!("compiled fixture missing first animation pointer"));
    target.copy_from_slice(&replacement);

    let model = parse_binary_model_bytes(&bytes).unwrap_or_else(|error| {
        panic!("parse corrupted compiled bytes: {error}");
    });

    assert!(
        model
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.message.contains("failed to parse animation"))
    );
    Ok(())
}

#[test]
fn binary_writer_roundtrips_exact_bytes() -> Result<(), Box<dyn Error>> {
    let bytes = match shipped_compiled_fixture_bytes() {
        Ok(bytes) => bytes,
        Err(error) => return skip_if_game_resources_unavailable(error),
    };
    let model = parse_binary_model_bytes(&bytes).unwrap_or_else(|error| {
        panic!("parse compiled bytes: {error}");
    });

    let mut encoded = Vec::new();
    if let Err(error) = write_original_binary_model(&mut encoded, &model) {
        panic!("write compiled model: {error}");
    }

    assert_eq!(encoded, bytes);
    Ok(())
}

#[test]
fn parsed_compiled_writer_roundtrips_exact_bytes() -> Result<(), Box<dyn Error>> {
    let bytes = match shipped_compiled_fixture_bytes() {
        Ok(bytes) => bytes,
        Err(error) => return skip_if_game_resources_unavailable(error),
    };
    let parsed = parse_model_bytes(&bytes).unwrap_or_else(|error| {
        panic!("parse compiled model bytes: {error}");
    });

    let mut encoded = Vec::new();
    if let Err(error) = write_parsed_model(&mut encoded, &parsed) {
        panic!("write parsed compiled model: {error}");
    }

    assert_eq!(encoded, bytes);
    Ok(())
}

#[test]
fn parsed_ascii_writer_roundtrips_canonically() -> Result<(), Box<dyn Error>> {
    let bytes = match shipped_ascii_fixture_bytes() {
        Ok(bytes) => bytes,
        Err(error) => return skip_if_game_resources_unavailable(error),
    };
    let mut reader = Cursor::new(bytes);
    let parsed = read_parsed_model(&mut reader).unwrap_or_else(|error| {
        panic!("read parsed ascii model: {error}");
    });

    let mut encoded = Vec::new();
    if let Err(error) = write_parsed_model(&mut encoded, &parsed) {
        panic!("write parsed ascii model: {error}");
    }

    let reparsed = parse_model_bytes(&encoded).unwrap_or_else(|error| {
        panic!("reparse encoded ascii model: {error}");
    });
    assert_eq!(reparsed, parsed);
    Ok(())
}

fn shipped_compiled_fixture() -> Result<BinaryModel, Box<dyn Error>> {
    let res = require_game_resource(demand_resource("a_ba2", MODEL_RES_TYPE))?;
    Ok(BinaryModel::from_res(&res, CachePolicy::Use)?)
}

fn shipped_compiled_fixture_bytes() -> Result<Vec<u8>, Box<dyn Error>> {
    Ok(shipped_compiled_fixture()?.original_bytes().to_vec())
}

fn shipped_ascii_fixture_bytes() -> Result<Vec<u8>, Box<dyn Error>> {
    let res = require_game_resource(demand_resource("a_ba_casts", MODEL_RES_TYPE))?;
    let binary = BinaryModel::from_res(&res, CachePolicy::Use)?;
    let ascii = lower_binary_model_to_ascii(&binary)?;
    let mut bytes = Vec::new();
    write_ascii_model(&mut bytes, &ascii)?;
    Ok(bytes)
}
