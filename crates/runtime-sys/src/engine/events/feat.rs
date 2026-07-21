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
        abi::{EngineVector, UseFeat},
        hook::NativeHookSpec,
    },
    dispatch,
};

const USE_HOOK: &str = "feat_use";

static USE_ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

pub(super) fn append_hook_specs(engine: &Engine, hooks: &mut Vec<NativeHookSpec>) {
    if let Some(target) = engine.event_hook_target(USE_HOOK) {
        hooks.push(NativeHookSpec::new(
            "CNWSCreature::UseFeat events",
            target,
            use_replacement as UseFeat as *const () as usize,
            &USE_ORIGINAL,
        ));
    }
}

extern "C" fn use_replacement(
    creature: *mut c_void,
    feat: u16,
    subfeat: u16,
    target: u32,
    area: u32,
    position: *const EngineVector,
) -> i32 {
    let event_position = if position.is_null() {
        EventVector {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }
    } else {
        // SAFETY: UseFeat supplies this vector for the synchronous call.
        let position = unsafe { *position };
        EventVector {
            x: position.x,
            y: position.y,
            z: position.z,
        }
    };
    let data = BTreeMap::from([
        (
            "area".to_string(),
            EventValue::Object(EventObjectId::new(area)),
        ),
        ("feat".to_string(), EventValue::Integer(i32::from(feat))),
        ("position".to_string(), EventValue::Vector(event_position)),
        (
            "subfeat".to_string(),
            EventValue::Integer(i32::from(subfeat)),
        ),
        (
            "target".to_string(),
            EventValue::Object(EventObjectId::new(target)),
        ),
    ]);
    let skipped = dispatch::game_object(
        creature,
        EventSpec::skippable("feat.use", "before"),
        data.clone(),
    )
    .is_some_and(|frame| frame.skipped());
    let result = if skipped {
        0
    } else {
        call_original(creature, feat, subfeat, target, area, position)
    };
    let mut after = data;
    after.insert("action_result".to_string(), EventValue::Integer(result));
    dispatch::game_object(creature, EventSpec::read_only("feat.use", "after"), after);
    result
}

fn call_original(
    creature: *mut c_void,
    feat: u16,
    subfeat: u16,
    target: u32,
    area: u32,
    position: *const EngineVector,
) -> i32 {
    let original = USE_ORIGINAL.load(Ordering::Acquire);
    if original.is_null() {
        return 0;
    }
    // SAFETY: Gum published the UseFeat trampoline with this exact ABI.
    let original = unsafe { std::mem::transmute::<*mut c_void, UseFeat>(original) };
    original(creature, feat, subfeat, target, area, position)
}
