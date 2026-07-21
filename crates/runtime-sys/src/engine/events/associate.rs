use std::{
    collections::BTreeMap,
    ffi::c_void,
    panic::{self, AssertUnwindSafe},
    ptr,
    sync::atomic::{AtomicPtr, Ordering},
};

use nwnrs_runtime::{EventObjectId, EventValue};

use super::{
    super::{
        Engine, EventSpec,
        abi::{AddAssociate, FamiliarAction, GetAssociateId, RemoveAssociate},
        active_engine,
        hook::NativeHookSpec,
    },
    dispatch as event_dispatch,
};
use crate::{bridge::BridgeInstallError, write_diagnostic};

const ADD_HOOK: &str = "associate_add";
const REMOVE_HOOK: &str = "associate_remove";
const POSSESS_FAMILIAR_HOOK: &str = "associate_possess_familiar";
const UNPOSSESS_FAMILIAR_HOOK: &str = "associate_unpossess_familiar";
const GET_ID_FUNCTION: &str = "associate_get_id";

static ADD_ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static REMOVE_ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static POSSESS_FAMILIAR_ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static UNPOSSESS_FAMILIAR_ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

pub(super) fn append_hook_specs(
    engine: &Engine,
    hooks: &mut Vec<NativeHookSpec>,
) -> Result<(), BridgeInstallError> {
    if (engine.event_hook_target(POSSESS_FAMILIAR_HOOK).is_some()
        || engine.event_hook_target(UNPOSSESS_FAMILIAR_HOOK).is_some())
        && engine.event_function_target(GET_ID_FUNCTION).is_none()
    {
        return Err(BridgeInstallError::new(
            "familiar event hooks require events.functions.associate_get_id",
        ));
    }
    if let Some(target) = engine.event_hook_target(ADD_HOOK) {
        hooks.push(NativeHookSpec::new(
            "CNWSCreature::AddAssociate events",
            target,
            add_replacement as AddAssociate as *const () as usize,
            &ADD_ORIGINAL,
        ));
    }
    if let Some(target) = engine.event_hook_target(REMOVE_HOOK) {
        hooks.push(NativeHookSpec::new(
            "CNWSCreature::RemoveAssociate events",
            target,
            remove_replacement as RemoveAssociate as *const () as usize,
            &REMOVE_ORIGINAL,
        ));
    }
    if let Some(target) = engine.event_hook_target(POSSESS_FAMILIAR_HOOK) {
        hooks.push(NativeHookSpec::new(
            "CNWSCreature::PossessFamiliar events",
            target,
            possess_familiar_replacement as FamiliarAction as *const () as usize,
            &POSSESS_FAMILIAR_ORIGINAL,
        ));
    }
    if let Some(target) = engine.event_hook_target(UNPOSSESS_FAMILIAR_HOOK) {
        hooks.push(NativeHookSpec::new(
            "CNWSCreature::UnpossessFamiliar events",
            target,
            unpossess_familiar_replacement as FamiliarAction as *const () as usize,
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
        EventSpec::read_only("associate.add", "before"),
    );
    call_original_add(creature, associate, associate_type);
    emit_associate(
        creature,
        associate,
        Some(associate_type),
        EventSpec::read_only("associate.add", "after"),
    );
}

extern "C" fn remove_replacement(creature: *mut c_void, associate: u32) {
    emit_associate(
        creature,
        associate,
        None,
        EventSpec::read_only("associate.remove", "before"),
    );
    call_original_remove(creature, associate);
    emit_associate(
        creature,
        associate,
        None,
        EventSpec::read_only("associate.remove", "after"),
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
    original: &'static AtomicPtr<c_void>,
) {
    let familiar = familiar_id(creature);
    if !emit_familiar(creature, familiar, EventSpec::skippable(name, "before")) {
        call_original_familiar(original, creature);
    }
    emit_familiar(creature, familiar, EventSpec::read_only(name, "after"));
}

fn familiar_id(creature: *mut c_void) -> u32 {
    let result = panic::catch_unwind(AssertUnwindSafe(|| {
        let engine = active_engine()
            .ok_or_else(|| BridgeInstallError::new("event engine is not initialized"))?;
        let target = engine
            .event_function_target(GET_ID_FUNCTION)
            .ok_or_else(|| BridgeInstallError::new("associate_get_id is missing"))?;
        // SAFETY: the target pack binds this address to GetAssociateId.
        let get_id = unsafe { std::mem::transmute::<usize, GetAssociateId>(target) };
        Ok::<u32, BridgeInstallError>(get_id(creature, 3, 1))
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

fn emit_familiar(creature: *mut c_void, familiar: u32, spec: EventSpec) -> bool {
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
    spec: EventSpec,
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
    if let Some(frame) = dispatch(creature, spec, data)
        && (frame.skipped() || frame.result().is_some())
    {
        write_diagnostic(&format!(
            "read-only event {} {} accepted an unsupported control mutation",
            spec.name, spec.phase
        ));
    }
}

fn dispatch(
    creature: *mut c_void,
    spec: EventSpec,
    data: BTreeMap<String, EventValue>,
) -> Option<super::super::EventFrame> {
    let frame = event_dispatch::game_object(creature, spec, data);
    if let Some(frame) = &frame
        && frame.result().is_some()
    {
        write_diagnostic(&format!(
            "event {} {} accepted an unsupported result mutation",
            spec.name, spec.phase
        ));
    }
    frame
}

fn call_original_add(creature: *mut c_void, associate: u32, associate_type: u16) {
    let original = ADD_ORIGINAL.load(Ordering::Acquire);
    if !original.is_null() {
        // SAFETY: Gum published the AddAssociate trampoline with this ABI.
        let original = unsafe { std::mem::transmute::<*mut c_void, AddAssociate>(original) };
        original(creature, associate, associate_type);
    }
}

fn call_original_remove(creature: *mut c_void, associate: u32) {
    let original = REMOVE_ORIGINAL.load(Ordering::Acquire);
    if !original.is_null() {
        // SAFETY: Gum published the RemoveAssociate trampoline with this ABI.
        let original = unsafe { std::mem::transmute::<*mut c_void, RemoveAssociate>(original) };
        original(creature, associate);
    }
}

fn call_original_familiar(original: &AtomicPtr<c_void>, creature: *mut c_void) {
    let original = original.load(Ordering::Acquire);
    if !original.is_null() {
        // SAFETY: Gum published the familiar-action trampoline with this ABI.
        let original = unsafe { std::mem::transmute::<*mut c_void, FamiliarAction>(original) };
        original(creature);
    }
}
