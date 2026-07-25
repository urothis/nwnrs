use std::{
    collections::BTreeMap,
    ffi::c_void,
    panic::{self, AssertUnwindSafe},
};

use nwnrs_runtime::{EventObjectId, EventValue};

use super::{
    super::{
        Engine, EventSpec,
        abi::{AddAssociate, FamiliarAction, GetAssociateId, RemoveAssociate},
        active_engine,
        hook::{NativeHookSpec, NativeOriginal},
    },
    dispatch as event_dispatch,
};
use crate::{bridge::BridgeInstallError, write_diagnostic};

const ADD_HOOK: &str = "associate_add";
const REMOVE_HOOK: &str = "associate_remove";
const POSSESS_FAMILIAR_HOOK: &str = "associate_possess_familiar";
const UNPOSSESS_FAMILIAR_HOOK: &str = "associate_unpossess_familiar";
const GET_ID_FUNCTION: &str = "associate_get_id";

static ADD_ORIGINAL: NativeOriginal<AddAssociate> = NativeOriginal::new();
static REMOVE_ORIGINAL: NativeOriginal<RemoveAssociate> = NativeOriginal::new();
static POSSESS_FAMILIAR_ORIGINAL: NativeOriginal<FamiliarAction> = NativeOriginal::new();
static UNPOSSESS_FAMILIAR_ORIGINAL: NativeOriginal<FamiliarAction> = NativeOriginal::new();

pub(super) fn append_hook_specs(
    engine: &Engine,
    hooks: &mut Vec<NativeHookSpec>,
) -> Result<(), BridgeInstallError> {
    if let Some(target) = engine.event_hook_target(ADD_HOOK) {
        hooks.push(NativeHookSpec::typed_event(
            ADD_HOOK,
            "CNWSCreature::AddAssociate events",
            target,
            add_replacement as AddAssociate,
            &ADD_ORIGINAL,
        ));
    }
    if let Some(target) = engine.event_hook_target(REMOVE_HOOK) {
        hooks.push(NativeHookSpec::typed_event(
            REMOVE_HOOK,
            "CNWSCreature::RemoveAssociate events",
            target,
            remove_replacement as RemoveAssociate,
            &REMOVE_ORIGINAL,
        ));
    }
    if let Some(target) = engine
        .event_function_available(GET_ID_FUNCTION)
        .then(|| engine.event_hook_target(POSSESS_FAMILIAR_HOOK))
        .flatten()
    {
        hooks.push(NativeHookSpec::typed_event(
            POSSESS_FAMILIAR_HOOK,
            "CNWSCreature::PossessFamiliar events",
            target,
            possess_familiar_replacement as FamiliarAction,
            &POSSESS_FAMILIAR_ORIGINAL,
        ));
    }
    if let Some(target) = engine
        .event_function_available(GET_ID_FUNCTION)
        .then(|| engine.event_hook_target(UNPOSSESS_FAMILIAR_HOOK))
        .flatten()
    {
        hooks.push(NativeHookSpec::typed_event(
            UNPOSSESS_FAMILIAR_HOOK,
            "CNWSCreature::UnpossessFamiliar events",
            target,
            unpossess_familiar_replacement as FamiliarAction,
            &UNPOSSESS_FAMILIAR_ORIGINAL,
        ));
    }
    Ok(())
}

extern "C" fn add_replacement(creature: *mut c_void, associate: u32, associate_type: u16) {
    emit_associate(
        creature,
        associate,
        Some(associate_type),
        EventSpec::catalog("associate.add", "before"),
    );
    call_original_add(creature, associate, associate_type);
    emit_associate(
        creature,
        associate,
        Some(associate_type),
        EventSpec::catalog("associate.add", "after"),
    );
}

extern "C" fn remove_replacement(creature: *mut c_void, associate: u32) {
    emit_associate(
        creature,
        associate,
        None,
        EventSpec::catalog("associate.remove", "before"),
    );
    call_original_remove(creature, associate);
    emit_associate(
        creature,
        associate,
        None,
        EventSpec::catalog("associate.remove", "after"),
    );
}

extern "C" fn possess_familiar_replacement(creature: *mut c_void) {
    emit_familiar_pair(
        creature,
        "associate.possess_familiar",
        &POSSESS_FAMILIAR_ORIGINAL,
    );
}

extern "C" fn unpossess_familiar_replacement(creature: *mut c_void) {
    emit_familiar_pair(
        creature,
        "associate.unpossess_familiar",
        &UNPOSSESS_FAMILIAR_ORIGINAL,
    );
}

fn emit_familiar_pair(
    creature: *mut c_void,
    name: &'static str,
    original: &'static NativeOriginal<FamiliarAction>,
) {
    let familiar = familiar_id(creature);
    if !emit_familiar(creature, familiar, EventSpec::catalog(name, "before")) {
        call_original_familiar(original, creature);
    }
    emit_familiar(creature, familiar, EventSpec::catalog(name, "after"));
}

fn familiar_id(creature: *mut c_void) -> u32 {
    let result = panic::catch_unwind(AssertUnwindSafe(|| {
        let engine = active_engine()
            .ok_or_else(|| BridgeInstallError::new("event engine is not initialized"))?;
        let get_id = engine
            .event_function::<GetAssociateId>(GET_ID_FUNCTION)
            .ok_or_else(|| BridgeInstallError::new("associate_get_id is missing"))?;
        Ok::<u32, BridgeInstallError>(get_id.get()(creature, 3, 1))
    }));
    match result {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => {
            write_diagnostic(&error.to_string());
            0x7f00_0000
        }
        Err(_) => {
            write_diagnostic("familiar ID lookup panicked");
            0x7f00_0000
        }
    }
}

fn emit_familiar(
    creature: *mut c_void,
    familiar: u32,
    spec: Result<EventSpec, BridgeInstallError>,
) -> bool {
    let data = BTreeMap::from([(
        "familiar".to_string(),
        EventValue::Object(EventObjectId::new(familiar)),
    )]);
    dispatch(creature, spec, data).is_some_and(|frame| frame.skipped())
}

fn emit_associate(
    creature: *mut c_void,
    associate: u32,
    associate_type: Option<u16>,
    spec: Result<EventSpec, BridgeInstallError>,
) {
    let mut data = BTreeMap::from([(
        "associate".to_string(),
        EventValue::Object(EventObjectId::new(associate)),
    )]);
    if let Some(associate_type) = associate_type {
        data.insert(
            "associate_type".to_string(),
            EventValue::Integer(i32::from(associate_type)),
        );
    }
    let _frame = dispatch(creature, spec, data);
}

fn dispatch(
    creature: *mut c_void,
    spec: Result<EventSpec, BridgeInstallError>,
    data: BTreeMap<String, EventValue>,
) -> Option<super::super::EventFrame> {
    event_dispatch::game_object(creature, spec, data)
}

fn call_original_add(creature: *mut c_void, associate: u32, associate_type: u16) {
    if let Some(original) = ADD_ORIGINAL.load() {
        original(creature, associate, associate_type);
    }
}

fn call_original_remove(creature: *mut c_void, associate: u32) {
    if let Some(original) = REMOVE_ORIGINAL.load() {
        original(creature, associate);
    }
}

fn call_original_familiar(original: &NativeOriginal<FamiliarAction>, creature: *mut c_void) {
    if let Some(original) = original.load() {
        original(creature);
    }
}
