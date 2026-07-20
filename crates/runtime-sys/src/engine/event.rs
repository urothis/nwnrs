use std::{
    collections::BTreeMap,
    ffi::c_void,
    sync::{Mutex, MutexGuard},
};

use nwnrs_runtime::{EventCommand, EventControls, EventObjectId, EventPayload, EventTarget};

use super::{
    abi::{CExoString, RunScript},
    address::{GlobalStorage, HookTarget, NativeAddress, Resolver},
    thread::EngineThreadToken,
};
use crate::bridge::BridgeInstallError;

const MAX_EVENT_DEPTH: usize = 64;
const MODULE_LOAD_EVENT_ID: i32 = 3002;
const MODULE_LOAD_DISPATCHER: &str = "_nwnrs_onload";

struct EventFrame {
    payload: EventPayload,
    skipped: bool,
    result:  Option<Vec<u8>>,
}

struct EventScope<'event> {
    frames: &'event Mutex<Vec<EventFrame>>,
    active: bool,
}

#[derive(Default)]
struct EventFrames {
    values: Mutex<Vec<EventFrame>>,
}

impl EventFrames {
    fn enter(&self, mut payload: EventPayload) -> Result<EventScope<'_>, BridgeInstallError> {
        let mut frames = lock_frames(&self.values)?;
        if frames.len() >= MAX_EVENT_DEPTH {
            return Err(BridgeInstallError::new(format!(
                "event nesting exceeds {MAX_EVENT_DEPTH} frames"
            )));
        }
        payload.depth = u32::try_from(frames.len())
            .ok()
            .and_then(|depth| depth.checked_add(1))
            .ok_or_else(|| BridgeInstallError::new("event nesting depth overflowed"))?;
        frames.push(EventFrame {
            payload,
            skipped: false,
            result: None,
        });
        drop(frames);
        Ok(EventScope {
            frames: &self.values,
            active: true,
        })
    }

    fn current(&self) -> Result<Option<EventPayload>, BridgeInstallError> {
        Ok(lock_frames(&self.values)?
            .last()
            .map(|frame| frame.payload.clone()))
    }

    fn control(&self, command: EventCommand) -> Result<(), BridgeInstallError> {
        let mut frames = lock_frames(&self.values)?;
        let frame = frames.last_mut().ok_or_else(|| {
            BridgeInstallError::new("event control requested outside nwnrs event dispatch")
        })?;
        match command {
            EventCommand::Skip if frame.payload.controls.skippable => frame.skipped = true,
            EventCommand::Skip => {
                return Err(BridgeInstallError::new(format!(
                    "event {} is not skippable",
                    frame.payload.name
                )));
            }
            EventCommand::SetResult(result) if frame.payload.controls.result => {
                frame.result = Some(result);
            }
            EventCommand::SetResult(_) => {
                return Err(BridgeInstallError::new(format!(
                    "event {} does not accept a result",
                    frame.payload.name
                )));
            }
        }
        Ok(())
    }
}

impl EventScope<'_> {
    fn finish(mut self) -> Result<EventFrame, BridgeInstallError> {
        let frame = lock_frames(self.frames)?.pop().ok_or_else(|| {
            BridgeInstallError::new("active event frame disappeared before dispatch completed")
        })?;
        self.active = false;
        Ok(frame)
    }
}

impl Drop for EventScope<'_> {
    fn drop(&mut self) {
        if self.active {
            let mut frames = self
                .frames
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let _discarded = frames.pop();
        }
    }
}

pub(crate) struct EventEngine {
    load_module_finish:      NativeAddress<HookTarget>,
    virtual_machine_storage: NativeAddress<GlobalStorage>,
    run_script:              RunScript,
    frames:                  EventFrames,
}

impl EventEngine {
    pub(crate) fn resolve(
        resolver: &Resolver,
        target: &EventTarget,
    ) -> Result<Self, BridgeInstallError> {
        let load_module_finish = resolver.resolve::<HookTarget>(
            "events",
            "load_module_finish",
            &target.load_module_finish,
        )?;
        let virtual_machine_storage = resolver.resolve::<GlobalStorage>(
            "events",
            "virtual_machine",
            &target.virtual_machine,
        )?;
        let run_script_address =
            resolver.resolve::<HookTarget>("events", "run_script", &target.run_script)?;
        // SAFETY: the exact target pack binds this address to
        // CVirtualMachine::RunScript for the selected server binary.
        let run_script =
            unsafe { std::mem::transmute::<usize, RunScript>(run_script_address.get()) };
        Ok(Self {
            load_module_finish,
            virtual_machine_storage,
            run_script,
            frames: EventFrames::default(),
        })
    }

    pub(crate) fn module_load_hook_target(&self) -> usize {
        self.load_module_finish.get()
    }

    pub(crate) fn run_module_onload(
        &self,
        _thread: &EngineThreadToken,
    ) -> Result<bool, BridgeInstallError> {
        let storage = self.virtual_machine_storage.get() as *const *mut c_void;
        // SAFETY: the target pack identifies live global g_pVirtualMachine
        // storage and this read is synchronous on the engine thread.
        let vm = unsafe { storage.read() };
        if vm.is_null() {
            return Err(BridgeInstallError::new(
                "g_pVirtualMachine was null at module-load completion",
            ));
        }

        let scope = self.frames.enter(EventPayload {
            name:     "module.on_module_load".to_string(),
            id:       MODULE_LOAD_EVENT_ID,
            script:   MODULE_LOAD_DISPATCHER.to_string(),
            phase:    "before".to_string(),
            depth:    0,
            target:   EventObjectId::new(0),
            controls: EventControls::default(),
            data:     BTreeMap::new(),
        })?;
        let mut script_name = format!("{MODULE_LOAD_DISPATCHER}\0").into_bytes();
        let string_length = u32::try_from(MODULE_LOAD_DISPATCHER.len()).map_err(|_error| {
            BridgeInstallError::new("module-load dispatcher resref exceeds u32")
        })?;
        let buffer_length = string_length.checked_add(1).ok_or_else(|| {
            BridgeInstallError::new("module-load dispatcher buffer length overflowed")
        })?;
        let mut script = CExoString {
            string: script_name.as_mut_ptr().cast(),
            string_length,
            buffer_length,
        };
        let ran = (self.run_script)(vm, &raw mut script, 0, 1, 0) != 0;
        let frame = scope.finish()?;
        if frame.skipped || frame.result.is_some() {
            return Err(BridgeInstallError::new(
                "module-load event accepted an unsupported control mutation",
            ));
        }
        Ok(ran)
    }

    pub(crate) fn current_event(
        &self,
        _thread: &EngineThreadToken,
    ) -> Result<Option<EventPayload>, BridgeInstallError> {
        self.frames.current()
    }

    pub(crate) fn control_event(
        &self,
        _thread: &EngineThreadToken,
        command: EventCommand,
    ) -> Result<(), BridgeInstallError> {
        self.frames.control(command)
    }
}

fn lock_frames(
    frames: &Mutex<Vec<EventFrame>>,
) -> Result<MutexGuard<'_, Vec<EventFrame>>, BridgeInstallError> {
    frames
        .lock()
        .map_err(|_error| BridgeInstallError::new("event frame lock is poisoned"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(name: &str, controls: EventControls) -> EventPayload {
        EventPayload {
            name: name.to_string(),
            id: -1,
            script: "_nwnrs_event".to_string(),
            phase: "before".to_string(),
            depth: 0,
            target: EventObjectId::new(0x0102_0304),
            controls,
            data: BTreeMap::new(),
        }
    }

    #[test]
    fn nested_frames_restore_parent_state() -> Result<(), BridgeInstallError> {
        let frames = EventFrames::default();
        let outer = frames.enter(payload(
            "fixture.outer",
            EventControls {
                skippable: true,
                result:    true,
            },
        ))?;
        assert_eq!(frames.current()?.map(|event| event.depth), Some(1));
        frames.control(EventCommand::SetResult(b"{\"outer\":true}".to_vec()))?;

        let inner = frames.enter(payload(
            "fixture.inner",
            EventControls {
                skippable: true,
                result:    false,
            },
        ))?;
        assert_eq!(frames.current()?.map(|event| event.depth), Some(2));
        frames.control(EventCommand::Skip)?;
        let inner = inner.finish()?;
        assert!(inner.skipped);
        assert!(inner.result.is_none());

        assert_eq!(
            frames.current()?.map(|event| event.name),
            Some("fixture.outer".to_string())
        );
        let outer = outer.finish()?;
        assert!(!outer.skipped);
        assert_eq!(outer.result, Some(b"{\"outer\":true}".to_vec()));
        assert!(frames.current()?.is_none());
        Ok(())
    }

    #[test]
    fn scope_drop_removes_frame_and_controls_are_schema_checked() -> Result<(), BridgeInstallError>
    {
        let frames = EventFrames::default();
        {
            let _scope = frames.enter(payload("fixture.read_only", EventControls::default()))?;
            let error = frames
                .control(EventCommand::Skip)
                .expect_err("read-only event must reject skip");
            assert!(error.to_string().contains("not skippable"));
            let error = frames
                .control(EventCommand::SetResult(b"null".to_vec()))
                .expect_err("read-only event must reject results");
            assert!(error.to_string().contains("does not accept a result"));
        }
        assert!(frames.current()?.is_none());
        Ok(())
    }
}
