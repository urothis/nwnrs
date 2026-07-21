use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::c_void,
    sync::{Mutex, MutexGuard},
    thread::ThreadId,
};

use nwnrs_runtime::{
    EngineClassLayouts, EventCommand, EventControls, EventObjectId, EventPayload, EventResultKind,
    EventTarget, ServerStateTarget, event_definition, runtime_event_definition,
};

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
    pub(crate) fn catalog(
        name: &'static str,
        phase: &'static str,
    ) -> Result<Self, BridgeInstallError> {
        let definition = runtime_event_definition(name, phase).ok_or_else(|| {
            BridgeInstallError::new(format!("event {name} {phase} is absent from the catalog"))
        })?;
        Ok(Self {
            name,
            id: -1,
            phase,
            controls: EventControls {
                skippable: definition.skippable,
                result:    definition.result_kind != EventResultKind::None,
            },
        })
    }

    pub(crate) const fn with_id(mut self, id: i32) -> Self {
        self.id = id;
        self
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
    owner:  Mutex<Option<ThreadId>>,
}

impl EventFrames {
    fn enter(&self, mut payload: EventPayload) -> Result<EventScope<'_>, BridgeInstallError> {
        self.validate_thread()?;
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
        self.validate_thread()?;
        Ok(lock_frames(&self.values)?
            .last()
            .map(|frame| frame.payload.clone()))
    }

    fn control(&self, command: EventCommand) -> Result<(), BridgeInstallError> {
        self.validate_thread()?;
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
            EventCommand::Subscribe(_)
            | EventCommand::ToggleIdWhitelist {
                ..
            }
            | EventCommand::AddIdToWhitelist {
                ..
            }
            | EventCommand::RemoveIdFromWhitelist {
                ..
            } => {
                return Err(BridgeInstallError::new(
                    "event engine administration was routed to an active frame",
                ));
            }
        }
        Ok(())
    }

    fn validate_thread(&self) -> Result<(), BridgeInstallError> {
        let current = std::thread::current().id();
        let mut owner = self
            .owner
            .lock()
            .map_err(|_error| BridgeInstallError::new("event thread lock is poisoned"))?;
        match *owner {
            Some(owner) if owner != current => Err(BridgeInstallError::new(
                "event dispatch entered from a thread other than the NWServer event thread",
            )),
            Some(_) => Ok(()),
            None => {
                *owner = Some(current);
                Ok(())
            }
        }
    }
}

#[derive(Default)]
struct SubscriptionState {
    active:  BTreeSet<String>,
    pending: Option<BTreeSet<String>>,
}

impl SubscriptionState {
    fn begin(&mut self) -> Result<(), BridgeInstallError> {
        if self.pending.is_some() {
            return Err(BridgeInstallError::new(
                "event subscription update is already active",
            ));
        }
        self.pending = Some(BTreeSet::new());
        Ok(())
    }

    fn record(&mut self, identity: &str) -> Result<(), BridgeInstallError> {
        self.pending
            .as_mut()
            .ok_or_else(|| BridgeInstallError::new("event subscription update is not active"))?
            .insert(identity.to_string());
        Ok(())
    }

    fn commit(&mut self) -> Result<(), BridgeInstallError> {
        let pending = self.pending.take().ok_or_else(|| {
            BridgeInstallError::new("event subscription update disappeared before commit")
        })?;
        self.active = pending;
        Ok(())
    }

    fn abort(&mut self) {
        self.pending = None;
    }
}

pub(crate) struct EventSubscriptionUpdate<'event> {
    engine: &'event EventEngine,
    active: bool,
}

impl EventSubscriptionUpdate<'_> {
    pub(crate) fn commit(mut self) -> Result<(), BridgeInstallError> {
        let mut subscriptions = self
            .engine
            .subscriptions
            .lock()
            .map_err(|_error| BridgeInstallError::new("event subscription lock is poisoned"))?;
        subscriptions.commit()?;
        self.active = false;
        Ok(())
    }
}

impl Drop for EventSubscriptionUpdate<'_> {
    fn drop(&mut self) {
        if self.active {
            self.engine
                .subscriptions
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .abort();
        }
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
    virtual_machine_storage:   NativeAddress<GlobalStorage>,
    run_script:                RunScript,
    hook_targets:              BTreeMap<String, NativeAddress<HookTarget>>,
    function_targets:          BTreeMap<String, NativeAddress<FunctionTarget>>,
    game_object_id_offset:     usize,
    event_layout_offsets:      BTreeMap<&'static str, usize>,
    app_manager_storage:       Option<NativeAddress<GlobalStorage>>,
    app_manager_server_offset: usize,
    frames:                    EventFrames,
    subscriptions:             Mutex<SubscriptionState>,
    installed_hook_targets:    Mutex<BTreeSet<usize>>,
    id_whitelists:             Mutex<BTreeMap<String, BTreeSet<i32>>>,
}

impl EventEngine {
    pub(crate) fn resolve(
        resolver: &Resolver,
        target: &EventTarget,
        layouts: &EngineClassLayouts,
        server_state: Option<&ServerStateTarget>,
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
        let mut resolved_hook_targets = BTreeMap::new();
        for (name, address) in &target.hooks {
            let resolved = resolver.resolve::<HookTarget>("events.hooks", name, address)?;
            if let Some(existing) = resolved_hook_targets.insert(resolved.get(), name) {
                return Err(BridgeInstallError::new(format!(
                    "event hooks {existing} and {name} resolve to the same physical address"
                )));
            }
            hook_targets.insert(name.clone(), resolved);
        }
        let mut function_targets = BTreeMap::new();
        for (name, address) in &target.functions {
            function_targets.insert(
                name.clone(),
                resolver.resolve::<FunctionTarget>("events.functions", name, address)?,
            );
        }
        let game_object_id_offset =
            usize::try_from(layouts.game_object_id_offset).map_err(|_error| {
                BridgeInstallError::new("layouts.classes.game_object_id_offset exceeds usize")
            })?;
        let mut event_layout_offsets = BTreeMap::new();
        event_layout_offsets.insert("game_object_id", game_object_id_offset);
        event_layout_offsets.insert(
            "player_id",
            usize::try_from(layouts.player_id_offset).map_err(|_error| {
                BridgeInstallError::new("layouts.classes.player_id_offset exceeds usize")
            })?,
        );
        for (name, offset) in [
            ("game_object_type", layouts.game_object_type_offset),
            (
                "item_repository_parent",
                layouts.item_repository_parent_offset,
            ),
            (
                "creature_stats_base_creature",
                layouts.creature_stats_base_creature_offset,
            ),
            (
                "creature_stats_experience",
                layouts.creature_stats_experience_offset,
            ),
            ("item_base_item", layouts.item_base_item_offset),
            ("item_possessor", layouts.item_possessor_offset),
            ("message_read_buffer", layouts.message_read_buffer_offset),
            (
                "message_read_buffer_size",
                layouts.message_read_buffer_size_offset,
            ),
            (
                "message_read_buffer_position",
                layouts.message_read_buffer_position_offset,
            ),
            (
                "message_read_fragments_size",
                layouts.message_read_fragments_size_offset,
            ),
            (
                "message_read_fragments_position",
                layouts.message_read_fragments_position_offset,
            ),
            (
                "message_current_read_bit",
                layouts.message_current_read_bit_offset,
            ),
            (
                "message_last_byte_bits",
                layouts.message_last_byte_bits_offset,
            ),
            ("player_object_id", layouts.player_object_id_offset),
            ("player_inventory_gui", layouts.player_inventory_gui_offset),
            (
                "player_other_inventory_gui",
                layouts.player_other_inventory_gui_offset,
            ),
            (
                "inventory_gui_selected_panel",
                layouts.inventory_gui_selected_panel_offset,
            ),
        ] {
            if let Some(offset) = offset {
                event_layout_offsets.insert(
                    name,
                    usize::try_from(offset).map_err(|_error| {
                        BridgeInstallError::new(format!(
                            "layouts.classes.{name}_offset exceeds usize"
                        ))
                    })?,
                );
            }
        }
        let app_manager_storage = server_state
            .map(|server| {
                resolver.resolve::<GlobalStorage>(
                    "server_state",
                    "app_manager",
                    &server.app_manager,
                )
            })
            .transpose()?;
        let app_manager_server_offset = usize::try_from(layouts.app_manager_server_offset)
            .map_err(|_error| {
                BridgeInstallError::new("layouts.classes.app_manager_server_offset exceeds usize")
            })?;
        let installed_hook_targets = target
            .hooks
            .get("module_load")
            .and_then(|_| hook_targets.get("module_load"))
            .map_or_else(std::collections::BTreeSet::new, |target| {
                std::collections::BTreeSet::from([target.get()])
            });
        Ok(Self {
            virtual_machine_storage,
            run_script,
            hook_targets,
            function_targets,
            game_object_id_offset,
            event_layout_offsets,
            app_manager_storage,
            app_manager_server_offset,
            frames: EventFrames::default(),
            subscriptions: Mutex::new(SubscriptionState::default()),
            installed_hook_targets: Mutex::new(installed_hook_targets),
            id_whitelists: Mutex::new(BTreeMap::new()),
        })
    }

    pub(crate) fn hook_target(&self, name: &str) -> Option<usize> {
        self.hook_targets.get(name).map(NativeAddress::get)
    }

    pub(crate) fn function_target(&self, name: &str) -> Option<usize> {
        self.function_targets.get(name).map(NativeAddress::get)
    }

    pub(crate) fn layout_offset(&self, name: &str) -> Option<usize> {
        self.event_layout_offsets.get(name).copied()
    }

    pub(crate) fn server(
        &self,
        _thread: &EngineThreadToken,
    ) -> Result<*mut c_void, BridgeInstallError> {
        let storage = self
            .app_manager_storage
            .as_ref()
            .ok_or_else(|| BridgeInstallError::new("event requires server_state.app_manager"))?;
        // SAFETY: the exact target pack binds this storage to g_pAppManager.
        let app_manager = unsafe { (storage.get() as *const *mut c_void).read() };
        if app_manager.is_null() {
            return Err(BridgeInstallError::new("g_pAppManager was null"));
        }
        // SAFETY: the compiler-derived layout binds this field to
        // CAppManager::m_pServerExoApp.
        let server = unsafe {
            app_manager
                .cast::<u8>()
                .add(self.app_manager_server_offset)
                .cast::<*mut c_void>()
                .read_unaligned()
        };
        if server.is_null() {
            return Err(BridgeInstallError::new(
                "CAppManager::m_pServerExoApp was null",
            ));
        }
        Ok(server)
    }

    pub(crate) fn is_subscribed(&self, name: &str, phase: &str) -> bool {
        let subscriptions = self
            .subscriptions
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        nwnrs_runtime::runtime_event_definition(name, phase)
            .is_some_and(|event| subscriptions.active.contains(event.identity))
    }

    pub(crate) fn begin_subscription_update(
        &self,
    ) -> Result<EventSubscriptionUpdate<'_>, BridgeInstallError> {
        let mut subscriptions = self
            .subscriptions
            .lock()
            .map_err(|_error| BridgeInstallError::new("event subscription lock is poisoned"))?;
        subscriptions.begin()?;
        Ok(EventSubscriptionUpdate {
            engine: self,
            active: true,
        })
    }

    pub(crate) fn subscription_target(
        &self,
        identity: &str,
    ) -> Result<Option<usize>, BridgeInstallError> {
        let event = event_definition(identity)
            .ok_or_else(|| BridgeInstallError::new("unknown event subscription"))?;
        let target = self.hook_target(event.hook).ok_or_else(|| {
            BridgeInstallError::new(format!("event hook {} is unavailable", event.hook))
        })?;
        if self
            .installed_hook_targets
            .lock()
            .map_err(|_error| BridgeInstallError::new("installed event hook lock is poisoned"))?
            .contains(&target)
        {
            Ok(None)
        } else {
            Ok(Some(target))
        }
    }

    pub(crate) fn record_subscription(&self, identity: &str) -> Result<(), BridgeInstallError> {
        if let Some(whitelist) =
            event_definition(identity).and_then(|definition| definition.forced_id_whitelist)
        {
            self.id_whitelists
                .lock()
                .map_err(|_error| BridgeInstallError::new("event ID whitelist lock is poisoned"))?
                .entry(whitelist.to_string())
                .or_default();
        }
        let mut subscriptions = self
            .subscriptions
            .lock()
            .map_err(|_error| BridgeInstallError::new("event subscription lock is poisoned"))?;
        subscriptions.record(identity)
    }

    pub(crate) fn mark_hook_installed(&self, target: usize) -> Result<(), BridgeInstallError> {
        self.installed_hook_targets
            .lock()
            .map_err(|_error| BridgeInstallError::new("installed event hook lock is poisoned"))?
            .insert(target);
        Ok(())
    }

    pub(crate) fn toggle_id_whitelist(
        &self,
        name: String,
        enabled: bool,
    ) -> Result<(), BridgeInstallError> {
        let mut whitelists = self
            .id_whitelists
            .lock()
            .map_err(|_error| BridgeInstallError::new("event ID whitelist lock is poisoned"))?;
        if enabled {
            whitelists.entry(name).or_default();
        } else {
            whitelists.remove(&name);
        }
        Ok(())
    }

    pub(crate) fn add_id_to_whitelist(
        &self,
        name: String,
        id: i32,
    ) -> Result<(), BridgeInstallError> {
        if let Some(whitelist) = self
            .id_whitelists
            .lock()
            .map_err(|_error| BridgeInstallError::new("event ID whitelist lock is poisoned"))?
            .get_mut(&name)
        {
            whitelist.insert(id);
        }
        Ok(())
    }

    pub(crate) fn remove_id_from_whitelist(
        &self,
        name: &str,
        id: i32,
    ) -> Result<(), BridgeInstallError> {
        if let Some(whitelist) = self
            .id_whitelists
            .lock()
            .map_err(|_error| BridgeInstallError::new("event ID whitelist lock is poisoned"))?
            .get_mut(name)
        {
            whitelist.remove(&id);
        }
        Ok(())
    }

    pub(crate) fn id_is_whitelisted(&self, name: &str, id: i32) -> bool {
        self.id_whitelists
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(name)
            .is_none_or(|whitelist| whitelist.contains(&id))
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
    use std::sync::Arc;

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

    #[test]
    fn catalog_is_authoritative_for_event_controls() -> Result<(), BridgeInstallError> {
        let item_use = EventSpec::catalog("item.use", "before")?;
        assert!(item_use.controls.skippable);
        assert!(item_use.controls.result);
        let placeable_close = EventSpec::catalog("placeable.close", "before")?;
        assert!(!placeable_close.controls.skippable);
        assert!(!placeable_close.controls.result);
        assert!(EventSpec::catalog("missing.event", "before").is_err());
        Ok(())
    }

    #[test]
    fn subscription_updates_commit_or_preserve_the_previous_set() -> Result<(), BridgeInstallError>
    {
        let mut subscriptions = SubscriptionState::default();
        subscriptions.active.insert("old".to_string());
        subscriptions.begin()?;
        subscriptions.record("new")?;
        subscriptions.abort();
        assert_eq!(subscriptions.active, BTreeSet::from(["old".to_string()]));

        subscriptions.begin()?;
        subscriptions.record("new")?;
        subscriptions.commit()?;
        assert_eq!(subscriptions.active, BTreeSet::from(["new".to_string()]));
        Ok(())
    }

    #[test]
    fn event_frames_reject_a_second_native_thread() -> Result<(), BridgeInstallError> {
        let frames = Arc::new(EventFrames::default());
        let scope = frames.enter(payload("fixture.owner", EventControls::default()))?;
        let other = Arc::clone(&frames);
        let error = std::thread::spawn(move || other.current())
            .join()
            .map_err(|_panic| BridgeInstallError::new("event thread test panicked"))?
            .expect_err("second event thread must be rejected");
        assert!(
            error
                .to_string()
                .contains("other than the NWServer event thread")
        );
        let _frame = scope.finish()?;
        Ok(())
    }
}
