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
        abi::{EngineVector, UseSkill},
        hook::NativeHookSpec,
    },
    dispatch,
};

const USE_HOOK: &str = "skill_use";

static USE_ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

pub(super) fn append_hook_specs(engine: &Engine, hooks: &mut Vec<NativeHookSpec>) {
    if let Some(target) = engine.event_hook_target(USE_HOOK) {
        hooks.push(NativeHookSpec::new(
            "CNWSCreature::UseSkill events",
            target,
            use_replacement as UseSkill as *const () as usize,
            &USE_ORIGINAL,
        ));
    }
}

#[allow(clippy::too_many_arguments)]
extern "C" fn use_replacement(
    creature: *mut c_void,
    skill: u8,
    subskill: u8,
    target: u32,
    position: EngineVector,
    area: u32,
    used_item: u32,
    active_property_index: i32,
) -> i32 {
    let data = BTreeMap::from([
        ("skill".to_string(), EventValue::Integer(i32::from(skill))),
        (
            "subskill".to_string(),
            EventValue::Integer(i32::from(subskill)),
        ),
        (
            "target".to_string(),
            EventValue::Object(EventObjectId::new(target)),
        ),
        (
            "position".to_string(),
            EventValue::Vector(EventVector {
                x: position.x,
                y: position.y,
                z: position.z,
            }),
        ),
        (
            "area".to_string(),
            EventValue::Object(EventObjectId::new(area)),
        ),
        (
            "used_item".to_string(),
            EventValue::Object(EventObjectId::new(used_item)),
        ),
        (
            "active_property_index".to_string(),
            EventValue::Integer(active_property_index),
        ),
    ]);
    let skipped = dispatch::game_object(
        creature,
        EventSpec::catalog("skill.use", "before"),
        data.clone(),
    )
    .is_some_and(|frame| frame.skipped());
    let result = if skipped {
        0
    } else {
        call_original(
            creature,
            skill,
            subskill,
            target,
            position,
            area,
            used_item,
            active_property_index,
        )
    };
    let mut after = data;
    after.insert("action_result".to_string(), EventValue::Integer(result));
    dispatch::game_object(creature, EventSpec::catalog("skill.use", "after"), after);
    result
}

#[allow(clippy::too_many_arguments)]
fn call_original(
    creature: *mut c_void,
    skill: u8,
    subskill: u8,
    target: u32,
    position: EngineVector,
    area: u32,
    used_item: u32,
    active_property_index: i32,
) -> i32 {
    let original = USE_ORIGINAL.load(Ordering::Acquire);
    if original.is_null() {
        return 0;
    }
    // SAFETY: Gum published the UseSkill trampoline with this exact ABI.
    let original = unsafe { std::mem::transmute::<*mut c_void, UseSkill>(original) };
    original(
        creature,
        skill,
        subskill,
        target,
        position,
        area,
        used_item,
        active_property_index,
    )
}
