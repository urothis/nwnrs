use std::{
    collections::BTreeMap,
    ffi::c_void,
    sync::{Mutex, MutexGuard},
};

use nwnrs_runtime::{EventCommand, EventControls, EventObjectId, EventPayload, EventTarget};

use super::{
    abi::{CExoString, RunScript},
    address::{FunctionTarget, GlobalStorage, HookTarget, NativeAddress, Resolver},
    thread::EngineThreadToken,
};
use crate::bridge::BridgeInstallError;

const MAX_EVENT_DEPTH: usize = 64;
const EVENT_DISPATCHER: &str = "_nwnrs_onload";

#[derive(Clone, Copy)]
pub(crate) struct EventSpec {
    pub(crate) name:     &'static str,
    pub(crate) id:       i32,
    pub(crate) phase:    &'static str,
    pub(crate) controls: EventControls,
}

impl EventSpec {
    pub(crate) const fn read_only(name: &'static str, phase: &'static str) -> Self {
        Self {
            name,
            id: -1,
            phase,
            controls: EventControls {
                skippable: false,
                result:    false,
            },
        }
    }

    pub(crate) const fn skippable(name: &'static str, phase: &'static str) -> Self {
        Self {
            name,
            id: -1,
            phase,
            controls: EventControls {
                skippable: true,
                result:    false,
            },
        }
    }
}

pub(crate) struct EventFrame {
    payload: EventPayload,
    skipped: bool,
    result:  Option<Vec<u8>>,
}

impl EventFrame {
    pub(crate) fn skipped(&self) -> bool {
        self.skipped
    }

    pub(crate) fn result(&self) -> Option<&[u8]> {
        self.result.as_deref()
    }
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
    virtual_machine_storage: NativeAddress<GlobalStorage>,
    run_script:              RunScript,
    hook_targets:            BTreeMap<String, NativeAddress<HookTarget>>,
    function_targets:        BTreeMap<String, NativeAddress<FunctionTarget>>,
    game_object_id_offset:   usize,
    frames:                  EventFrames,
}

impl EventEngine {
    pub(crate) fn resolve(
        resolver: &Resolver,
        target: &EventTarget,
    ) -> Result<Self, BridgeInstallError> {
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
        let mut hook_targets = BTreeMap::new();
        for (name, address) in &target.hooks {
            hook_targets.insert(
                name.clone(),
                resolver.resolve::<HookTarget>("events.hooks", name, address)?,
            );
        }
        let mut function_targets = BTreeMap::new();
        for (name, address) in &target.functions {
            function_targets.insert(
                name.clone(),
                resolver.resolve::<FunctionTarget>("events.functions", name, address)?,
            );
        }
        let game_object_id_offset =
            usize::try_from(target.game_object_id_offset).map_err(|_error| {
                BridgeInstallError::new("events.game_object_id_offset exceeds usize")
            })?;
        Ok(Self {
            virtual_machine_storage,
            run_script,
            hook_targets,
            function_targets,
            game_object_id_offset,
            frames: EventFrames::default(),
        })
    }

    pub(crate) fn hook_target(&self, name: &str) -> Option<usize> {
        self.hook_targets.get(name).map(NativeAddress::get)
    }

    pub(crate) fn function_target(&self, name: &str) -> Option<usize> {
        self.function_targets.get(name).map(NativeAddress::get)
    }

    pub(crate) fn game_object_id(
        &self,
        _thread: &EngineThreadToken,
        object: *const c_void,
    ) -> Result<EventObjectId, BridgeInstallError> {
        if object.is_null() {
            return Err(BridgeInstallError::new(
                "event hook received a null game object",
            ));
        }
        // SAFETY: the callback's ABI supplies a live CGameObject-derived
        // receiver and the exact target pack owns m_idSelf's byte offset.
        let value = unsafe {
            object
                .cast::<u8>()
                .add(self.game_object_id_offset)
                .cast::<u32>()
                .read_unaligned()
        };
        Ok(EventObjectId::new(value))
    }

    pub(crate) fn dispatch(
        &self,
        _thread: &EngineThreadToken,
        spec: EventSpec,
        target: EventObjectId,
        data: BTreeMap<String, nwnrs_runtime::EventValue>,
    ) -> Result<(bool, EventFrame), BridgeInstallError> {
        let payload = EventPayload {
            name: spec.name.to_string(),
            id: spec.id,
            script: EVENT_DISPATCHER.to_string(),
            phase: spec.phase.to_string(),
            depth: 0,
            target,
            controls: spec.controls,
            data,
        };
        let scope = self.frames.enter(payload)?;
        let vm = self.virtual_machine()?;
        let mut script_name = format!("{EVENT_DISPATCHER}\0").into_bytes();
        let string_length = u32::try_from(EVENT_DISPATCHER.len())
            .map_err(|_error| BridgeInstallError::new("event dispatcher resref exceeds u32"))?;
        let buffer_length = string_length
            .checked_add(1)
            .ok_or_else(|| BridgeInstallError::new("event dispatcher buffer length overflowed"))?;
        let mut script = CExoString {
            string: script_name.as_mut_ptr().cast(),
            string_length,
            buffer_length,
        };
        let ran = (self.run_script)(vm, &raw mut script, target.raw(), 1, 0) != 0;
        Ok((ran, scope.finish()?))
    }

    fn virtual_machine(&self) -> Result<*mut c_void, BridgeInstallError> {
        let storage = self.virtual_machine_storage.get() as *const *mut c_void;
        // SAFETY: the target pack identifies live global g_pVirtualMachine
        // storage and this read is synchronous on the engine thread.
        let vm = unsafe { storage.read() };
        if vm.is_null() {
            return Err(BridgeInstallError::new("g_pVirtualMachine was null"));
        }
        Ok(vm)
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
