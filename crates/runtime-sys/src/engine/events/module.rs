use std::{
    collections::BTreeMap,
    ffi::c_void,
    panic::{self, AssertUnwindSafe},
};

use nwnrs_runtime::EventObjectId;

use super::super::{
    Engine, EngineThreadToken, EventSpec,
    abi::LoadModuleFinish,
    active_engine,
    hook::{NativeHookSpec, NativeOriginal},
};
use crate::{bridge::BridgeInstallError, write_diagnostic};

const HOOK: &str = "module_load";
const EVENT_ID: i32 = 3002;

static ORIGINAL: NativeOriginal<LoadModuleFinish> = NativeOriginal::new();

pub(super) fn append_hook_specs(engine: &Engine, hooks: &mut Vec<NativeHookSpec>) {
    if let Some(target) = engine.event_hook_target(HOOK) {
        hooks.push(NativeHookSpec::typed_event(
            HOOK,
            "CNWSModule::LoadModuleFinish events",
            target,
            replacement as LoadModuleFinish,
            &ORIGINAL,
        ));
    }
}

extern "C" fn replacement(module: *mut c_void) -> u32 {
    let dispatch = panic::catch_unwind(AssertUnwindSafe(dispatch));
    match dispatch {
        Ok(Ok(true)) => {}
        Ok(Ok(false)) => write_diagnostic("generated _nwnrs_onload script did not run"),
        Ok(Err(error)) => write_diagnostic(&error.to_string()),
        Err(_) => write_diagnostic("module.load event dispatch panicked"),
    }
    call_original(module)
}

fn dispatch() -> Result<bool, BridgeInstallError> {
    let engine = active_engine()
        .ok_or_else(|| BridgeInstallError::new("event engine is not initialized"))?;
    engine.reset_event_lifecycle()?;
    let subscriptions = engine.begin_event_subscription_update()?;
    // SAFETY: this token is scoped to the synchronous native engine hook.
    let thread = unsafe { EngineThreadToken::new() };
    let (ran, frame) = engine.dispatch_event(
        &thread,
        EventSpec::catalog("module.load", "before")?.with_id(EVENT_ID),
        EventObjectId::new(0),
        BTreeMap::new(),
    )?;
    if frame.skipped() || frame.has_result() {
        return Err(BridgeInstallError::new(
            "module.load accepted an unsupported control mutation",
        ));
    }
    if ran {
        engine.commit_event_subscription_update(subscriptions)?;
    }
    Ok(ran)
}

fn call_original(module: *mut c_void) -> u32 {
    let Some(original) = ORIGINAL.load() else {
        return 0;
    };
    original(module)
}
