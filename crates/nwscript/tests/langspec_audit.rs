#![allow(missing_docs)]

use std::{error::Error, io};

use nwnrs_nwscript::prelude::*;

mod support;

use support::{load_nss_bytes, skip_if_game_resources_unavailable, test_error};

type TestResult = Result<(), Box<dyn Error>>;

fn find_function<'a>(spec: &'a LangSpec, name: &str) -> Result<&'a BuiltinFunction, io::Error> {
    spec.functions
        .iter()
        .find(|function| function.name == name)
        .ok_or_else(|| test_error(format!("missing builtin {name}")))
}

#[test]
fn pinned_nwscript_langspec_matches_expected_builtin_shape() -> TestResult {
    let source = match load_nss_bytes("nwscript.nss") {
        Ok(source) => source,
        Err(error) => return skip_if_game_resources_unavailable(error),
    };
    let spec = parse_langspec_bytes("nwscript.nss", &source)?;

    assert_eq!(spec.engine_num_structures, 8);
    assert_eq!(
        spec.engine_structures,
        vec![
            "effect".to_string(),
            "event".to_string(),
            "location".to_string(),
            "talent".to_string(),
            "itemproperty".to_string(),
            "sqlquery".to_string(),
            "cassowary".to_string(),
            "json".to_string(),
        ]
    );

    let object_type_invalid = spec
        .constants
        .iter()
        .find(|constant| constant.name == "OBJECT_TYPE_INVALID");
    assert_eq!(
        object_type_invalid.map(|constant| constant.value.clone()),
        Some(BuiltinValue::Int(32767))
    );

    let damage_type_custom19 = spec
        .constants
        .iter()
        .find(|constant| constant.name == "DAMAGE_TYPE_CUSTOM19");
    assert_eq!(
        damage_type_custom19.map(|constant| constant.value.clone()),
        Some(BuiltinValue::Int(i32::MIN))
    );

    let effect_damage = find_function(&spec, "EffectDamage")?;
    assert_eq!(
        effect_damage.return_type,
        BuiltinType::EngineStructure("effect".to_string())
    );
    assert_eq!(effect_damage.parameters.len(), 3);

    let get_first_object_in_area = find_function(&spec, "GetFirstObjectInArea")?;
    assert_eq!(
        get_first_object_in_area
            .parameters
            .first()
            .and_then(|param| param.default.clone()),
        Some(BuiltinValue::ObjectInvalid)
    );

    let speak_one_liner = find_function(&spec, "SpeakOneLinerConversation")?;
    assert_eq!(
        speak_one_liner
            .parameters
            .get(1)
            .and_then(|param| param.default.clone()),
        Some(BuiltinValue::ObjectId(32767))
    );

    let json_object = find_function(&spec, "JsonObject")?;
    assert_eq!(
        json_object.return_type,
        BuiltinType::EngineStructure("json".to_string())
    );
    assert!(json_object.parameters.is_empty());

    let get_starting_location = find_function(&spec, "GetStartingLocation")?;
    assert_eq!(
        get_starting_location.return_type,
        BuiltinType::EngineStructure("location".to_string())
    );
    Ok(())
}

#[test]
fn langspec_regression_cases_continue_to_parse() -> TestResult {
    let large_int = br#"
        #define ENGINE_NUM_STRUCTURES 0
        int DAMAGE_TYPE_CUSTOM19 = 2147483648;
    "#;
    let large_int_spec = parse_langspec_bytes("langspec_large_int.nss", large_int)?;
    let constant = large_int_spec
        .constants
        .iter()
        .find(|constant| constant.name == "DAMAGE_TYPE_CUSTOM19");
    assert_eq!(
        constant.map(|constant| constant.value.clone()),
        Some(BuiltinValue::Int(i32::MIN))
    );

    let object_id_default = br#"
        #define ENGINE_NUM_STRUCTURES 0
        int OBJECT_TYPE_INVALID = 32767;
        void SpeakOneLinerConversation(int nLine, object oSpeaker = OBJECT_TYPE_INVALID);
    "#;
    let object_id_spec = parse_langspec_bytes("langspec_object_id_default.nss", object_id_default)?;
    let function = find_function(&object_id_spec, "SpeakOneLinerConversation")?;
    assert_eq!(
        function
            .parameters
            .get(1)
            .and_then(|param| param.default.clone()),
        Some(BuiltinValue::ObjectId(32767))
    );

    Ok(())
}
