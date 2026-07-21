use std::{
    collections::BTreeMap,
    ffi::c_void,
    ptr,
    sync::atomic::{AtomicPtr, Ordering},
};

use nwnrs_runtime::EventValue;

use super::{
    super::{
        Engine, EventSpec,
        abi::{CancelTimingEvent, SendTimingEvent},
        hook::NativeHookSpec,
    },
    dispatch,
};
use crate::bridge::BridgeInstallError;

const SEND_HOOK: &str = "timing_bar_send";
const CANCEL_HOOK: &str = "timing_bar_cancel";

static SEND_ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static CANCEL_ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

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
    if let Some(target) = engine.event_hook_target(SEND_HOOK) {
        hooks.push(NativeHookSpec::new(
            "CNWSMessage::SendServerToPlayerGuiTimingEvent events",
            target,
            send_replacement as SendTimingEvent as *const () as usize,
            &SEND_ORIGINAL,
        ));
    }
    if let Some(target) = engine.event_hook_target(CANCEL_HOOK) {
        hooks.push(NativeHookSpec::new(
            "CNWSMessage::HandlePlayerToServerInputCancelGuiTimingEvent events",
            target,
            cancel_replacement as CancelTimingEvent as *const () as usize,
            &CANCEL_ORIGINAL,
        ));
    }
    Ok(())
}

extern "C" fn send_replacement(
    message: *mut c_void,
    player: *mut c_void,
    starting: i32,
    event_id: u8,
    duration: u32,
) -> i32 {
    let (name, data) = if starting != 0 {
        (
            "timing_bar.start",
            BTreeMap::from([
                ("duration".to_string(), EventValue::Unsigned(duration)),
                (
                    "event_id".to_string(),
                    EventValue::Integer(i32::from(event_id)),
                ),
            ]),
        )
    } else {
        ("timing_bar.stop", BTreeMap::new())
    };
    dispatch::player(player, EventSpec::catalog(name, "before"), data.clone());
    let result = call_send(message, player, starting, event_id, duration);
    dispatch::player(player, EventSpec::catalog(name, "after"), data);
    result
}

extern "C" fn cancel_replacement(message: *mut c_void, player: *mut c_void) -> i32 {
    dispatch::player(
        player,
        EventSpec::catalog("timing_bar.cancel", "before"),
        BTreeMap::new(),
    );
    let result = call_cancel(message, player);
    dispatch::player(
        player,
        EventSpec::catalog("timing_bar.cancel", "after"),
        BTreeMap::new(),
    );
    result
}

fn call_send(
    message: *mut c_void,
    player: *mut c_void,
    starting: i32,
    event_id: u8,
    duration: u32,
) -> i32 {
    let original = SEND_ORIGINAL.load(Ordering::Acquire);
    if original.is_null() {
        return 0;
    }
    // SAFETY: Gum published the SendTimingEvent trampoline for this hook.
    let original = unsafe { std::mem::transmute::<*mut c_void, SendTimingEvent>(original) };
    original(message, player, starting, event_id, duration)
}

fn call_cancel(message: *mut c_void, player: *mut c_void) -> i32 {
    let original = CANCEL_ORIGINAL.load(Ordering::Acquire);
    if original.is_null() {
        return 0;
    }
    // SAFETY: Gum published the CancelTimingEvent trampoline for this hook.
    let original = unsafe { std::mem::transmute::<*mut c_void, CancelTimingEvent>(original) };
    original(message, player)
}
