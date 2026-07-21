use std::{
    collections::BTreeMap,
    ffi::c_void,
    panic::{self, AssertUnwindSafe},
    ptr,
    sync::atomic::{AtomicPtr, Ordering},
};

use nwnrs_runtime::EventObjectId;

use super::super::{
    Engine, EngineThreadToken, EventSpec, abi::LoadModuleFinish, active_engine,
    hook::NativeHookSpec,
};
use crate::{bridge::BridgeInstallError, write_diagnostic};

const HOOK: &str = "module_load";
const EVENT_ID: i32 = 3002;

static ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

pub(super) fn append_hook_specs(engine: &Engine, hooks: &mut Vec<NativeHookSpec>) {
    if let Some(target) = engine.event_hook_target(HOOK) {
        hooks.push(NativeHookSpec::new(
            "CNWSModule::LoadModuleFinish events",
            target,
            replacement as LoadModuleFinish as *const () as usize,
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
    // SAFETY: this token is scoped to the synchronous native engine hook.
    let thread = unsafe { EngineThreadToken::new() };
    let (ran, frame) = engine.dispatch_event(
        &thread,
        EventSpec {
            name:     "module.load",
            id:       EVENT_ID,
            phase:    "before",
            controls: nwnrs_runtime::EventControls::default(),
        },
        EventObjectId::new(0),
        BTreeMap::new(),
    )?;
    if frame.skipped() || frame.result().is_some() {
        return Err(BridgeInstallError::new(
            "module.load accepted an unsupported control mutation",
        ));
    }
    Ok(ran)
}

fn call_original(module: *mut c_void) -> u32 {
    let original = ORIGINAL.load(Ordering::Acquire);
    if original.is_null() {
        return 0;
    }
    // SAFETY: Gum published the exact LoadModuleFinish trampoline.
    let original = unsafe { std::mem::transmute::<*mut c_void, LoadModuleFinish>(original) };
    original(module)
}
