use std::{collections::BTreeMap, ffi::c_void};

use super::{
    super::{
        Engine, EventSpec,
        abi::PlayerMessage,
        hook::{NativeHookSpec, NativeOriginal},
    },
    dispatch,
};
use crate::bridge::BridgeInstallError;

const HOOK: &str = "journal_message";
const QUEST_SCREEN_OPEN: u8 = 0x0a;
const QUEST_SCREEN_CLOSED: u8 = 0x0b;

static ORIGINAL: NativeOriginal<PlayerMessage> = NativeOriginal::new();

pub(super) fn append_hook_specs(
    engine: &Engine,
    hooks: &mut Vec<NativeHookSpec>,
) -> Result<(), BridgeInstallError> {
    if !engine.event_function_available("player_get_game_object") {
        return Ok(());
    }
    if let Some(target) = engine.event_hook_target(HOOK) {
        hooks.push(NativeHookSpec::typed_event(
            HOOK,
            "CNWSMessage::HandlePlayerToServerJournalMessage events",
            target,
            replacement as PlayerMessage,
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
    let Some(original) = ORIGINAL.load() else {
        return 0;
    };
    original(message, player, minor)
}
