use std::{
    collections::BTreeMap,
    ffi::c_void,
    ptr,
    sync::atomic::{AtomicPtr, Ordering},
};

use super::{
    super::{Engine, EventSpec, abi::PlayerMessage, hook::NativeHookSpec},
    dispatch,
};
use crate::bridge::BridgeInstallError;

const HOOK: &str = "journal_message";
const QUEST_SCREEN_OPEN: u8 = 0x0a;
const QUEST_SCREEN_CLOSED: u8 = 0x0b;

static ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

pub(super) fn append_hook_specs(
    engine: &Engine,
    hooks: &mut Vec<NativeHookSpec>,
) -> Result<(), BridgeInstallError> {
    if engine
        .event_function_target("player_get_game_object")
        .is_none()
    {
        return Ok(());
    }
    if let Some(target) = engine.event_hook_target(HOOK) {
        hooks.push(NativeHookSpec::new(
            "CNWSMessage::HandlePlayerToServerJournalMessage events",
            target,
            replacement as PlayerMessage as *const () as usize,
            &ORIGINAL,
        ));
    }
    Ok(())
}

extern "C" fn replacement(message: *mut c_void, player: *mut c_void, minor: u8) -> i32 {
    let name = match minor {
        QUEST_SCREEN_OPEN => "journal.open",
        QUEST_SCREEN_CLOSED => "journal.close",
        _ => return call_original(message, player, minor),
    };
    dispatch::player(player, EventSpec::catalog(name, "before"), BTreeMap::new());
    let result = call_original(message, player, minor);
    dispatch::player(player, EventSpec::catalog(name, "after"), BTreeMap::new());
    result
}

fn call_original(message: *mut c_void, player: *mut c_void, minor: u8) -> i32 {
    let original = ORIGINAL.load(Ordering::Acquire);
    if original.is_null() {
        return 0;
    }
    // SAFETY: Gum published the PlayerMessage trampoline for this hook.
    let original = unsafe { std::mem::transmute::<*mut c_void, PlayerMessage>(original) };
    original(message, player, minor)
}
