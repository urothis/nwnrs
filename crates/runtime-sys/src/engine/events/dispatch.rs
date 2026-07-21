use std::{
    collections::BTreeMap,
    ffi::c_void,
    panic::{self, AssertUnwindSafe},
};

use nwnrs_runtime::{EventObjectId, EventValue};

use super::super::{EngineThreadToken, EventFrame, EventSpec, active_engine};
use crate::{bridge::BridgeInstallError, write_diagnostic};

pub(super) fn game_object(
    object: *const c_void,
    spec: EventSpec,
    data: BTreeMap<String, EventValue>,
) -> Option<EventFrame> {
    guarded(spec, || {
        let engine = active_engine()
            .ok_or_else(|| BridgeInstallError::new("event engine is not initialized"))?;
        // SAFETY: this token is scoped to the synchronous native engine hook.
        let thread = unsafe { EngineThreadToken::new() };
        let target = engine.event_game_object_id(&thread, object)?;
        dispatch(engine, &thread, target, spec, data)
    })
}

pub(super) fn player(
    player: *mut c_void,
    spec: EventSpec,
    data: BTreeMap<String, EventValue>,
) -> Option<EventFrame> {
    guarded(spec, || {
        let engine = active_engine()
            .ok_or_else(|| BridgeInstallError::new("event engine is not initialized"))?;
        let target = engine
            .event_function_target("player_get_game_object")
            .ok_or_else(|| BridgeInstallError::new("player_get_game_object is missing"))?;
        // SAFETY: the target pack binds this address to
        // CNWSPlayer::GetGameObject().
        let get_game_object =
            unsafe { std::mem::transmute::<usize, super::super::abi::GetPlayerGameObject>(target) };
        let object = get_game_object(player);
        // SAFETY: this token is scoped to the synchronous native engine hook.
        let thread = unsafe { EngineThreadToken::new() };
        let target = engine.event_game_object_id(&thread, object)?;
        dispatch(engine, &thread, target, spec, data)
    })
}

fn dispatch(
    engine: &super::super::Engine,
    thread: &EngineThreadToken,
    target: EventObjectId,
    spec: EventSpec,
    data: BTreeMap<String, EventValue>,
) -> Result<EventFrame, BridgeInstallError> {
    let (ran, frame) = engine.dispatch_event(thread, spec, target, data)?;
    if !ran {
        return Err(BridgeInstallError::new(format!(
            "generated event dispatcher did not run for {} {}",
            spec.name, spec.phase
        )));
    }
    Ok(frame)
}

fn guarded(
    spec: EventSpec,
    dispatch: impl FnOnce() -> Result<EventFrame, BridgeInstallError>,
) -> Option<EventFrame> {
    match panic::catch_unwind(AssertUnwindSafe(dispatch)) {
        Ok(Ok(frame)) => Some(frame),
        Ok(Err(error)) => {
            write_diagnostic(&error.to_string());
            None
        }
        Err(_) => {
            write_diagnostic(&format!(
                "{} {} event dispatch panicked",
                spec.name, spec.phase
            ));
            None
        }
    }
}
