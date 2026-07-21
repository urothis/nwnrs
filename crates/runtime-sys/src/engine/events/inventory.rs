use std::{
    collections::BTreeMap,
    ffi::c_void,
    ptr,
    sync::atomic::{AtomicPtr, Ordering},
};

use nwnrs_runtime::EventValue;

use super::{
    super::{Engine, EventSpec, abi::ModifyGold, hook::NativeHookSpec},
    dispatch,
};

const ADD_GOLD_HOOK: &str = "inventory_add_gold";
const REMOVE_GOLD_HOOK: &str = "inventory_remove_gold";

static ADD_GOLD_ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static REMOVE_GOLD_ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

pub(super) fn append_hook_specs(engine: &Engine, hooks: &mut Vec<NativeHookSpec>) {
    append(
        engine,
        hooks,
        ADD_GOLD_HOOK,
        "CNWSCreature::AddGold events",
        add_gold_replacement as ModifyGold as *const () as usize,
        &ADD_GOLD_ORIGINAL,
    );
    append(
        engine,
        hooks,
        REMOVE_GOLD_HOOK,
        "CNWSCreature::RemoveGold events",
        remove_gold_replacement as ModifyGold as *const () as usize,
        &REMOVE_GOLD_ORIGINAL,
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

extern "C" fn add_gold_replacement(creature: *mut c_void, gold: i32, feedback: i32) {
    modify_gold(
        creature,
        gold,
        feedback,
        "inventory.add_gold",
        &ADD_GOLD_ORIGINAL,
    );
}

extern "C" fn remove_gold_replacement(creature: *mut c_void, gold: i32, feedback: i32) {
    modify_gold(
        creature,
        gold,
        feedback,
        "inventory.remove_gold",
        &REMOVE_GOLD_ORIGINAL,
    );
}

fn modify_gold(
    creature: *mut c_void,
    gold: i32,
    feedback: i32,
    name: &'static str,
    original: &'static AtomicPtr<c_void>,
) {
    let data = BTreeMap::from([("gold".to_string(), EventValue::Integer(gold))]);
    let skipped =
        dispatch::game_object(creature, EventSpec::skippable(name, "before"), data.clone())
            .is_some_and(|frame| frame.skipped());
    if !skipped {
        call_original(original, creature, gold, feedback);
    }
    dispatch::game_object(creature, EventSpec::read_only(name, "after"), data);
}

fn call_original(original: &AtomicPtr<c_void>, creature: *mut c_void, gold: i32, feedback: i32) {
    let original = original.load(Ordering::Acquire);
    if !original.is_null() {
        // SAFETY: Gum published the ModifyGold trampoline for this hook.
        let original = unsafe { std::mem::transmute::<*mut c_void, ModifyGold>(original) };
        original(creature, gold, feedback);
    }
}
