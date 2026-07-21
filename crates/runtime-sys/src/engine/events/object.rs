use std::{
    collections::BTreeMap,
    ffi::c_void,
    ptr,
    sync::atomic::{AtomicPtr, Ordering},
};

use nwnrs_runtime::{EventObjectId, EventValue, EventVector};

use super::{
    super::{
        Engine, EventSpec,
        abi::{
            BroadcastSafeProjectile, CloseInventory, EngineVector, ObjectAction, OpenInventory,
            UnlockObjectAction,
        },
        hook::NativeHookSpec,
    },
    dispatch,
};
use crate::write_diagnostic;

const LOCK_HOOK: &str = "object_lock";
const UNLOCK_HOOK: &str = "object_unlock";
const USE_HOOK: &str = "object_use";
const PLACEABLE_OPEN_HOOK: &str = "placeable_open";
const PLACEABLE_CLOSE_HOOK: &str = "placeable_close";
const BROADCAST_SAFE_PROJECTILE_HOOK: &str = "object_broadcast_safe_projectile";

static LOCK_ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static UNLOCK_ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static USE_ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static PLACEABLE_OPEN_ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static PLACEABLE_CLOSE_ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static BROADCAST_SAFE_PROJECTILE_ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

pub(super) fn append_hook_specs(engine: &Engine, hooks: &mut Vec<NativeHookSpec>) {
    append(
        engine,
        hooks,
        LOCK_HOOK,
        "CNWSObject::AddLockObjectAction events",
        lock_replacement as ObjectAction as *const () as usize,
        &LOCK_ORIGINAL,
    );
    append(
        engine,
        hooks,
        UNLOCK_HOOK,
        "CNWSObject::AddUnlockObjectAction events",
        unlock_replacement as UnlockObjectAction as *const () as usize,
        &UNLOCK_ORIGINAL,
    );
    append(
        engine,
        hooks,
        USE_HOOK,
        "CNWSObject::AddUseObjectAction events",
        use_replacement as ObjectAction as *const () as usize,
        &USE_ORIGINAL,
    );
    append(
        engine,
        hooks,
        PLACEABLE_OPEN_HOOK,
        "CNWSPlaceable::OpenInventory events",
        placeable_open_replacement as OpenInventory as *const () as usize,
        &PLACEABLE_OPEN_ORIGINAL,
    );
    append(
        engine,
        hooks,
        PLACEABLE_CLOSE_HOOK,
        "CNWSPlaceable::CloseInventory events",
        placeable_close_replacement as CloseInventory as *const () as usize,
        &PLACEABLE_CLOSE_ORIGINAL,
    );
    append(
        engine,
        hooks,
        BROADCAST_SAFE_PROJECTILE_HOOK,
        "CNWSObject::BroadcastSafeProjectile events",
        broadcast_safe_projectile_replacement as BroadcastSafeProjectile as *const () as usize,
        &BROADCAST_SAFE_PROJECTILE_ORIGINAL,
    );
}

fn append(
    engine: &Engine,
    hooks: &mut Vec<NativeHookSpec>,
    key: &str,
    name: &'static str,
    replacement: usize,
    original: &'static AtomicPtr<c_void>,
) {
    if let Some(target) = engine.event_hook_target(key) {
        hooks.push(NativeHookSpec::new(name, target, replacement, original));
    }
}

extern "C" fn lock_replacement(object: *mut c_void, door: u32) -> i32 {
    object_action(
        object,
        "object.lock",
        BTreeMap::from([object_value("door", door)]),
        &LOCK_ORIGINAL,
        door,
    )
}

extern "C" fn use_replacement(object: *mut c_void, used: u32) -> i32 {
    object_action(
        object,
        "object.use",
        BTreeMap::from([object_value("object", used)]),
        &USE_ORIGINAL,
        used,
    )
}

fn object_action(
    object: *mut c_void,
    name: &'static str,
    data: BTreeMap<String, EventValue>,
    original: &'static AtomicPtr<c_void>,
    argument: u32,
) -> i32 {
    let skipped = emit(object, EventSpec::skippable(name, "before"), data.clone())
        .is_some_and(|frame| frame.skipped());
    let result = if skipped {
        0
    } else {
        call_object_action(original, object, argument)
    };
    let mut after = data;
    after.insert("action_result".to_string(), EventValue::Integer(result));
    emit(object, EventSpec::read_only(name, "after"), after);
    result
}

extern "C" fn unlock_replacement(
    object: *mut c_void,
    door: u32,
    thieves_tool: u32,
    active_property_index: i32,
) -> i32 {
    let data = BTreeMap::from([
        (
            "active_property_index".to_string(),
            EventValue::Integer(active_property_index),
        ),
        object_value("door", door),
        object_value("thieves_tool", thieves_tool),
    ]);
    let skipped = emit(
        object,
        EventSpec::skippable("object.unlock", "before"),
        data.clone(),
    )
    .is_some_and(|frame| frame.skipped());
    let result = if skipped {
        0
    } else {
        call_unlock(object, door, thieves_tool, active_property_index)
    };
    let mut after = data;
    after.insert("action_result".to_string(), EventValue::Integer(result));
    emit(
        object,
        EventSpec::read_only("object.unlock", "after"),
        after,
    );
    result
}

extern "C" fn placeable_open_replacement(placeable: *mut c_void, opener: u32) {
    let data = BTreeMap::from([object_value("object", opener)]);
    let skipped = emit(
        placeable,
        EventSpec::skippable("placeable.open", "before"),
        data.clone(),
    )
    .is_some_and(|frame| frame.skipped());
    if !skipped {
        call_open(placeable, opener);
    }
    let mut after = data;
    after.insert("before_skipped".to_string(), EventValue::Boolean(skipped));
    emit(
        placeable,
        EventSpec::read_only("placeable.open", "after"),
        after,
    );
}

extern "C" fn placeable_close_replacement(placeable: *mut c_void, closer: u32, update_player: i32) {
    let data = BTreeMap::from([object_value("object", closer)]);
    emit(
        placeable,
        EventSpec::read_only("placeable.close", "before"),
        data.clone(),
    );
    call_close(placeable, closer, update_player);
    emit(
        placeable,
        EventSpec::read_only("placeable.close", "after"),
        data,
    );
}

#[allow(clippy::too_many_arguments)]
extern "C" fn broadcast_safe_projectile_replacement(
    object: *mut c_void,
    originator: u32,
    target: u32,
    originator_position: EngineVector,
    target_position: EngineVector,
    delta: u32,
    projectile_type: u8,
    spell_id: u32,
    attack_result: u8,
    projectile_path_type: u8,
) {
    let data = BTreeMap::from([
        object_value("originator", originator),
        object_value("target", target),
        (
            "originator_position".to_string(),
            EventValue::Vector(event_vector(originator_position)),
        ),
        (
            "target_position".to_string(),
            EventValue::Vector(event_vector(target_position)),
        ),
        ("delta".to_string(), EventValue::Unsigned(delta)),
        (
            "projectile_type".to_string(),
            EventValue::Integer(i32::from(projectile_type)),
        ),
        ("spell_id".to_string(), EventValue::Unsigned(spell_id)),
        (
            "attack_result".to_string(),
            EventValue::Integer(i32::from(attack_result)),
        ),
        (
            "projectile_path_type".to_string(),
            EventValue::Integer(i32::from(projectile_path_type)),
        ),
    ]);
    let skipped = emit(
        object,
        EventSpec::skippable("object.broadcast_safe_projectile", "before"),
        data.clone(),
    )
    .is_some_and(|frame| frame.skipped());
    if !skipped {
        call_broadcast_safe_projectile(
            object,
            originator,
            target,
            originator_position,
            target_position,
            delta,
            projectile_type,
            spell_id,
            attack_result,
            projectile_path_type,
        );
    }
    emit(
        object,
        EventSpec::read_only("object.broadcast_safe_projectile", "after"),
        data,
    );
}

fn event_vector(value: EngineVector) -> EventVector {
    EventVector {
        x: value.x,
        y: value.y,
        z: value.z,
    }
}

fn emit(
    object: *mut c_void,
    spec: EventSpec,
    data: BTreeMap<String, EventValue>,
) -> Option<super::super::EventFrame> {
    let frame = dispatch::game_object(object, spec, data);
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

fn object_value(name: &str, value: u32) -> (String, EventValue) {
    (
        name.to_string(),
        EventValue::Object(EventObjectId::new(value)),
    )
}

fn call_object_action(original: &AtomicPtr<c_void>, object: *mut c_void, argument: u32) -> i32 {
    let original = original.load(Ordering::Acquire);
    if original.is_null() {
        return 0;
    }
    // SAFETY: Gum published an ObjectAction trampoline for this hook.
    let original = unsafe { std::mem::transmute::<*mut c_void, ObjectAction>(original) };
    original(object, argument)
}

fn call_unlock(object: *mut c_void, door: u32, thieves_tool: u32, index: i32) -> i32 {
    let original = UNLOCK_ORIGINAL.load(Ordering::Acquire);
    if original.is_null() {
        return 0;
    }
    // SAFETY: Gum published the UnlockObjectAction trampoline.
    let original = unsafe { std::mem::transmute::<*mut c_void, UnlockObjectAction>(original) };
    original(object, door, thieves_tool, index)
}

#[allow(clippy::too_many_arguments)]
fn call_broadcast_safe_projectile(
    object: *mut c_void,
    originator: u32,
    target: u32,
    originator_position: EngineVector,
    target_position: EngineVector,
    delta: u32,
    projectile_type: u8,
    spell_id: u32,
    attack_result: u8,
    projectile_path_type: u8,
) {
    let original = BROADCAST_SAFE_PROJECTILE_ORIGINAL.load(Ordering::Acquire);
    if original.is_null() {
        return;
    }
    // SAFETY: Gum published the BroadcastSafeProjectile trampoline.
    let original = unsafe { std::mem::transmute::<*mut c_void, BroadcastSafeProjectile>(original) };
    original(
        object,
        originator,
        target,
        originator_position,
        target_position,
        delta,
        projectile_type,
        spell_id,
        attack_result,
        projectile_path_type,
    );
}

fn call_open(placeable: *mut c_void, opener: u32) {
    let original = PLACEABLE_OPEN_ORIGINAL.load(Ordering::Acquire);
    if !original.is_null() {
        // SAFETY: Gum published the OpenInventory trampoline.
        let original = unsafe { std::mem::transmute::<*mut c_void, OpenInventory>(original) };
        original(placeable, opener);
    }
}

fn call_close(placeable: *mut c_void, closer: u32, update_player: i32) {
    let original = PLACEABLE_CLOSE_ORIGINAL.load(Ordering::Acquire);
    if !original.is_null() {
        // SAFETY: Gum published the CloseInventory trampoline.
        let original = unsafe { std::mem::transmute::<*mut c_void, CloseInventory>(original) };
        original(placeable, closer, update_player);
    }
}
