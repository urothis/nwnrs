use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::c_void,
    sync::{Mutex, MutexGuard},
    thread::ThreadId,
};

use arc_swap::ArcSwap;
use nwnrs_runtime::{
    EngineClassLayouts, EventCommand, EventControls, EventKey, EventObjectId, EventPayload,
    EventReentrancyPolicy, EventResultKind, EventSourceKind, EventSubscriptionMask, EventTarget,
    ServerStateTarget, event_definition, keyed_event_definition, keyed_runtime_event_definition,
};

use super::{
    abi::{CExoString, RunScript},
    address::{FunctionTarget, GlobalStorage, HookTarget, NativeAddress, NativeFunction, Resolver},
    correlation::{CorrelationOwner, CorrelationToken},
    pump::{DEFAULT_EVENT_PUMP_DRAIN_LIMIT, EventPump, EventPumpRecord},
    thread::EngineThreadToken,
};
use crate::bridge::BridgeInstallError;

const MAX_EVENT_DEPTH: usize = 64;
const EVENT_DISPATCHER: &str = "_nwnrs_onload";
type EventIdWhitelists = BTreeMap<String, BTreeSet<i32>>;

#[derive(Clone, Copy)]
pub(crate) struct EventSpec {
    pub(crate) key:        EventKey,
    pub(crate) name:       &'static str,
    pub(crate) id:         i32,
    pub(crate) phase:      &'static str,
    pub(crate) controls:   EventControls,
    pub(crate) result:     EventResultKind,
    pub(crate) reentrancy: EventReentrancyPolicy,
}

impl EventSpec {
    pub(crate) fn catalog(
        name: &'static str,
        phase: &'static str,
    ) -> Result<Self, BridgeInstallError> {
        let (key, definition) = keyed_runtime_event_definition(name, phase).ok_or_else(|| {
            BridgeInstallError::new(format!("event {name} {phase} is absent from the catalog"))
        })?;
        Ok(Self {
            key,
            name,
            id: -1,
            phase,
            controls: EventControls {
                skippable: definition.skippable,
                result:    definition.result_kind != EventResultKind::None,
            },
            result: definition.result_kind,
            reentrancy: definition.reentrancy,
        })
    }

    pub(crate) const fn with_id(mut self, id: i32) -> Self {
        self.id = id;
        self
    }

    fn identity(identity: &str) -> Result<Self, BridgeInstallError> {
        let (key, definition) = keyed_event_definition(identity).ok_or_else(|| {
            BridgeInstallError::new(format!("event {identity} is absent from the catalog"))
        })?;
        Ok(Self {
            key,
            name: definition.name,
            id: -1,
            phase: definition.phase,
            controls: EventControls {
                skippable: definition.skippable,
                result:    definition.result_kind != EventResultKind::None,
            },
            result: definition.result_kind,
            reentrancy: definition.reentrancy,
        })
    }
}

pub(crate) struct EventFrame {
    payload:     EventPayload,
    key:         EventKey,
    skipped:     bool,
    result_kind: EventResultKind,
    result:      Option<EventResultValue>,
}

#[derive(Clone, Debug, PartialEq)]
enum EventResultValue {
    Boolean(bool),
    Unsigned(u32),
    Signed(i32),
    Float(f32),
    String(String),
    ObjectId(u32),
}

impl EventFrame {
    pub(crate) fn skipped(&self) -> bool {
        self.skipped
    }

    pub(crate) fn has_result(&self) -> bool {
        self.result.is_some()
    }

    pub(crate) fn boolean_result(&self) -> Option<bool> {
        match &self.result {
            Some(EventResultValue::Boolean(value)) => Some(*value),
            _ => None,
        }
    }

    pub(crate) fn unsigned_result(&self) -> Option<u32> {
        match &self.result {
            Some(EventResultValue::Unsigned(value)) => Some(*value),
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn signed_result(&self) -> Option<i32> {
        match &self.result {
            Some(EventResultValue::Signed(value)) => Some(*value),
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn float_result(&self) -> Option<f32> {
        match &self.result {
            Some(EventResultValue::Float(value)) => Some(*value),
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn string_result(&self) -> Option<&str> {
        match &self.result {
            Some(EventResultValue::String(value)) => Some(value),
            _ => None,
        }
    }

    pub(crate) fn object_result(&self) -> Option<u32> {
        match &self.result {
            Some(EventResultValue::ObjectId(value)) => Some(*value),
            _ => None,
        }
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
    fn enter(
        &self,
        mut payload: EventPayload,
        key: EventKey,
        result_kind: EventResultKind,
    ) -> Result<EventScope<'_>, BridgeInstallError> {
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
            key,
            skipped: false,
            result_kind,
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

    fn contains(&self, key: EventKey) -> bool {
        self.values
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .any(|frame| frame.key == key)
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
                frame.result = Some(parse_event_result(frame.result_kind, &result)?);
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
    active:                BTreeSet<String>,
    pending:               Option<BTreeSet<String>>,
    active_id_whitelists:  EventIdWhitelists,
    pending_id_whitelists: Option<EventIdWhitelists>,
}

struct PublishedEventConfiguration {
    subscriptions: EventSubscriptionMask,
    id_whitelists: EventIdWhitelists,
}

impl PublishedEventConfiguration {
    fn empty() -> Self {
        Self {
            subscriptions: EventSubscriptionMask::empty(),
            id_whitelists: BTreeMap::new(),
        }
    }

    fn is_subscribed(&self, key: EventKey) -> bool {
        self.subscriptions.contains(key)
    }

    fn id_is_whitelisted(&self, name: &str, id: i32) -> bool {
        self.id_whitelists
            .get(name)
            .is_none_or(|whitelist| whitelist.contains(&id))
    }
}

impl SubscriptionState {
    fn begin(&mut self) -> Result<(), BridgeInstallError> {
        if self.pending.is_some() || self.pending_id_whitelists.is_some() {
            return Err(BridgeInstallError::new(
                "event subscription update is already active",
            ));
        }
        self.pending = Some(BTreeSet::new());
        self.pending_id_whitelists = Some(BTreeMap::new());
        Ok(())
    }

    fn record(
        &mut self,
        identity: &str,
        forced_id_whitelist: Option<&str>,
    ) -> Result<(), BridgeInstallError> {
        self.pending
            .as_mut()
            .ok_or_else(|| BridgeInstallError::new("event subscription update is not active"))?
            .insert(identity.to_string());
        if let Some(name) = forced_id_whitelist {
            self.pending_id_whitelists
                .as_mut()
                .ok_or_else(|| BridgeInstallError::new("event filter update is not active"))?
                .entry(name.to_string())
                .or_default();
        }
        Ok(())
    }

    fn commit(&mut self) -> Result<(), BridgeInstallError> {
        let pending = self.pending.take().ok_or_else(|| {
            BridgeInstallError::new("event subscription update disappeared before commit")
        })?;
        let pending_id_whitelists = self.pending_id_whitelists.take().ok_or_else(|| {
            BridgeInstallError::new("event filter update disappeared before commit")
        })?;
        self.active = pending;
        self.active_id_whitelists = pending_id_whitelists;
        Ok(())
    }

    fn abort(&mut self) {
        self.pending = None;
        self.pending_id_whitelists = None;
    }

    fn id_whitelists_mut(&mut self) -> &mut EventIdWhitelists {
        self.pending_id_whitelists
            .as_mut()
            .unwrap_or(&mut self.active_id_whitelists)
    }

    fn configuration_if_active(&self) -> Option<(&BTreeSet<String>, &EventIdWhitelists)> {
        self.pending
            .is_none()
            .then_some((&self.active, &self.active_id_whitelists))
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
        let (active, id_whitelists) = subscriptions.configuration_if_active().ok_or_else(|| {
            BridgeInstallError::new("event subscription update remained active after commit")
        })?;
        let published = self
            .engine
            .build_published_configuration(active, id_whitelists)?;
        drop(subscriptions);
        self.engine.published.store(published.into());
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
    virtual_machine_storage: NativeAddress<GlobalStorage>,
    run_script: RunScript,
    hook_targets: BTreeMap<String, NativeAddress<HookTarget>>,
    function_targets: BTreeMap<String, NativeAddress<FunctionTarget>>,
    game_object_id_offset: usize,
    event_layout_offsets: BTreeMap<&'static str, usize>,
    app_manager_storage: Option<NativeAddress<GlobalStorage>>,
    app_manager_server_offset: usize,
    frames: EventFrames,
    correlations: CorrelationOwner,
    pump: EventPump,
    subscriptions: Mutex<SubscriptionState>,
    published: ArcSwap<PublishedEventConfiguration>,
    installed_physical_hooks: Mutex<BTreeSet<&'static str>>,
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
        let run_script = resolver
            .resolve::<FunctionTarget>("events", "run_script", &target.run_script)?
            .function::<RunScript>()
            .get();
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
        let installed_physical_hooks = target
            .hooks
            .get("module_load")
            .map_or_else(std::collections::BTreeSet::new, |_target| {
                std::collections::BTreeSet::from(["module_load"])
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
            correlations: CorrelationOwner::default(),
            pump: EventPump::default(),
            subscriptions: Mutex::new(SubscriptionState::default()),
            published: ArcSwap::from_pointee(PublishedEventConfiguration::empty()),
            installed_physical_hooks: Mutex::new(installed_physical_hooks),
        })
    }

    pub(crate) fn hook_target(&self, name: &str) -> Option<usize> {
        self.hook_targets.get(name).map(NativeAddress::get)
    }

    pub(crate) fn function_available(&self, name: &str) -> bool {
        self.function_targets.contains_key(name)
    }

    pub(crate) fn function<Function: Copy>(&self, name: &str) -> Option<NativeFunction<Function>> {
        self.function_targets.get(name).map(NativeAddress::function)
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

    pub(crate) fn is_subscribed(&self, key: EventKey) -> bool {
        self.published.load().is_subscribed(key)
    }

    pub(crate) fn should_dispatch(&self, spec: EventSpec) -> bool {
        self.is_subscribed(spec.key)
            && match spec.reentrancy {
                EventReentrancyPolicy::Allow => true,
                EventReentrancyPolicy::SuppressSameRoute => !self.frames.contains(spec.key),
            }
    }

    #[allow(dead_code)]
    pub(crate) fn begin_correlation<Value>(
        &self,
        owner: &'static str,
        key: u64,
        value: Value,
    ) -> Result<CorrelationToken, BridgeInstallError>
    where
        Value: Send + 'static,
    {
        self.correlations.begin(owner, key, value)
    }

    #[allow(dead_code)]
    pub(crate) fn finish_correlation<Value>(
        &self,
        owner: &'static str,
        key: u64,
        token: CorrelationToken,
    ) -> Result<Value, BridgeInstallError>
    where
        Value: Send + 'static,
    {
        self.correlations.finish(owner, key, token)
    }

    #[allow(dead_code)]
    pub(crate) fn enqueue_pump_record(
        &self,
        identity: &str,
        target: EventObjectId,
        data: BTreeMap<String, nwnrs_runtime::EventValue>,
    ) -> Result<u64, BridgeInstallError> {
        let definition = event_definition(identity)
            .ok_or_else(|| BridgeInstallError::new("unknown asynchronous event identity"))?;
        if definition.source_kind != EventSourceKind::EventPump {
            return Err(BridgeInstallError::new(format!(
                "event {identity} is not produced by the asynchronous event pump"
            )));
        }
        self.pump.enqueue(identity, target, data)
    }

    pub(crate) fn process_pump(
        &self,
        thread: &EngineThreadToken,
    ) -> Result<(), BridgeInstallError> {
        let records = self.pump.drain(DEFAULT_EVENT_PUMP_DRAIN_LIMIT)?;
        let mut first_error = None;
        for record in records {
            if let Err(error) = self.dispatch_pump_record(thread, record)
                && first_error.is_none()
            {
                first_error = Some(error);
            }
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    fn dispatch_pump_record(
        &self,
        thread: &EngineThreadToken,
        record: EventPumpRecord,
    ) -> Result<(), BridgeInstallError> {
        let definition = event_definition(&record.identity).ok_or_else(|| {
            BridgeInstallError::new(format!(
                "queued event {} disappeared from the catalog",
                record.identity
            ))
        })?;
        if definition.source_kind != EventSourceKind::EventPump {
            return Err(BridgeInstallError::new(format!(
                "queued event {} is not an event-pump route",
                record.identity
            )));
        }
        let spec = EventSpec::identity(&record.identity)?;
        if !self.should_dispatch(spec) {
            return Ok(());
        }
        let (ran, _frame) = self.dispatch(thread, spec, record.target, record.data)?;
        if !ran {
            return Err(BridgeInstallError::new(format!(
                "generated event dispatcher did not run for queued event {} sequence {}",
                record.identity, record.sequence
            )));
        }
        Ok(())
    }

    pub(crate) fn reset_lifecycle(&self) -> Result<(), BridgeInstallError> {
        let _discarded_correlations = self.correlations.clear()?;
        let _discarded_records = self.pump.reset()?;
        Ok(())
    }

    pub(crate) fn shutdown(&self) -> Result<(), BridgeInstallError> {
        let _discarded_correlations = self.correlations.clear()?;
        let _discarded_records = self.pump.close()?;
        Ok(())
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

    pub(crate) fn pending_physical_hooks(
        &self,
    ) -> Result<BTreeSet<&'static str>, BridgeInstallError> {
        let subscriptions = self
            .subscriptions
            .lock()
            .map_err(|_error| BridgeInstallError::new("event subscription lock is poisoned"))?;
        let pending = subscriptions
            .pending
            .as_ref()
            .ok_or_else(|| BridgeInstallError::new("event subscription update is not active"))?;
        let installed = self
            .installed_physical_hooks
            .lock()
            .map_err(|_error| BridgeInstallError::new("installed event hook lock is poisoned"))?;
        let mut required = BTreeSet::new();
        for identity in pending {
            let event = event_definition(identity)
                .ok_or_else(|| BridgeInstallError::new("unknown event subscription"))?;
            self.hook_target(event.hook).ok_or_else(|| {
                BridgeInstallError::new(format!("event hook {} is unavailable", event.hook))
            })?;
            if !installed.contains(event.hook) {
                required.insert(event.hook);
            }
        }
        Ok(required)
    }

    pub(crate) fn record_subscription(&self, identity: &str) -> Result<(), BridgeInstallError> {
        let forced_id_whitelist = event_definition(identity)
            .and_then(|definition| definition.filter_policy.required_whitelist());
        let mut subscriptions = self
            .subscriptions
            .lock()
            .map_err(|_error| BridgeInstallError::new("event subscription lock is poisoned"))?;
        subscriptions.record(identity, forced_id_whitelist)
    }

    fn build_published_configuration(
        &self,
        active: &BTreeSet<String>,
        id_whitelists: &EventIdWhitelists,
    ) -> Result<PublishedEventConfiguration, BridgeInstallError> {
        let mut subscriptions = EventSubscriptionMask::empty();
        for identity in active {
            let (key, _event) = keyed_event_definition(identity).ok_or_else(|| {
                BridgeInstallError::new(format!(
                    "event subscription {identity} disappeared from the catalog"
                ))
            })?;
            if !subscriptions.insert(key) {
                return Err(BridgeInstallError::new(
                    "event subscription key exceeds the bitset",
                ));
            }
        }
        Ok(PublishedEventConfiguration {
            subscriptions,
            id_whitelists: id_whitelists.clone(),
        })
    }

    fn publish_active_configuration(
        &self,
        subscriptions: &SubscriptionState,
    ) -> Result<(), BridgeInstallError> {
        if let Some((active, id_whitelists)) = subscriptions.configuration_if_active() {
            self.published.store(
                self.build_published_configuration(active, id_whitelists)?
                    .into(),
            );
        }
        Ok(())
    }

    pub(crate) fn mark_hooks_installed(
        &self,
        hooks: &BTreeSet<&'static str>,
    ) -> Result<(), BridgeInstallError> {
        self.installed_physical_hooks
            .lock()
            .map_err(|_error| BridgeInstallError::new("installed event hook lock is poisoned"))?
            .extend(hooks);
        Ok(())
    }

    pub(crate) fn toggle_id_whitelist(
        &self,
        name: String,
        enabled: bool,
    ) -> Result<(), BridgeInstallError> {
        let mut subscriptions = self
            .subscriptions
            .lock()
            .map_err(|_error| BridgeInstallError::new("event ID whitelist lock is poisoned"))?;
        let whitelists = subscriptions.id_whitelists_mut();
        if enabled {
            whitelists.entry(name).or_default();
        } else {
            whitelists.remove(&name);
        }
        self.publish_active_configuration(&subscriptions)
    }

    pub(crate) fn add_id_to_whitelist(
        &self,
        name: String,
        id: i32,
    ) -> Result<(), BridgeInstallError> {
        let mut subscriptions = self
            .subscriptions
            .lock()
            .map_err(|_error| BridgeInstallError::new("event ID whitelist lock is poisoned"))?;
        if let Some(whitelist) = subscriptions.id_whitelists_mut().get_mut(&name) {
            whitelist.insert(id);
        }
        self.publish_active_configuration(&subscriptions)
    }

    pub(crate) fn remove_id_from_whitelist(
        &self,
        name: &str,
        id: i32,
    ) -> Result<(), BridgeInstallError> {
        let mut subscriptions = self
            .subscriptions
            .lock()
            .map_err(|_error| BridgeInstallError::new("event ID whitelist lock is poisoned"))?;
        if let Some(whitelist) = subscriptions.id_whitelists_mut().get_mut(name) {
            whitelist.remove(&id);
        }
        self.publish_active_configuration(&subscriptions)
    }

    pub(crate) fn id_is_whitelisted(&self, name: &str, id: i32) -> bool {
        self.published.load().id_is_whitelisted(name, id)
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
        let scope = self.frames.enter(payload, spec.key, spec.result)?;
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

fn parse_event_result(
    kind: EventResultKind,
    result: &[u8],
) -> Result<EventResultValue, BridgeInstallError> {
    match kind {
        EventResultKind::None => Err(BridgeInstallError::new(
            "event result has no declared result type",
        )),
        EventResultKind::Boolean => serde_json::from_slice(result)
            .map(EventResultValue::Boolean)
            .map_err(|_error| BridgeInstallError::new("event result is not a JSON boolean")),
        EventResultKind::Unsigned => serde_json::from_slice(result)
            .map(EventResultValue::Unsigned)
            .map_err(|_error| BridgeInstallError::new("event result is not a JSON u32")),
        EventResultKind::Signed => serde_json::from_slice(result)
            .map(EventResultValue::Signed)
            .map_err(|_error| BridgeInstallError::new("event result is not a JSON i32")),
        EventResultKind::Float => {
            let value: f32 = serde_json::from_slice(result)
                .map_err(|_error| BridgeInstallError::new("event result is not a JSON f32"))?;
            if !value.is_finite() {
                return Err(BridgeInstallError::new(
                    "event result is not a finite JSON f32",
                ));
            }
            Ok(EventResultValue::Float(value))
        }
        EventResultKind::String => {
            let value: String = serde_json::from_slice(result)
                .map_err(|_error| BridgeInstallError::new("event result is not a JSON string"))?;
            if value.len() > 65_535 || value.contains('\0') {
                return Err(BridgeInstallError::new(
                    "event result is not a bounded engine string",
                ));
            }
            Ok(EventResultValue::String(value))
        }
        EventResultKind::ObjectId => {
            let value: String = serde_json::from_slice(result).map_err(|_error| {
                BridgeInstallError::new("event result is not a JSON object-ID string")
            })?;
            if value.len() != 8 {
                return Err(BridgeInstallError::new(
                    "event result object ID must contain eight hexadecimal digits",
                ));
            }
            u32::from_str_radix(&value, 16)
                .map(EventResultValue::ObjectId)
                .map_err(|_error| {
                    BridgeInstallError::new("event result object ID is not hexadecimal")
                })
        }
    }
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

    fn event_key(identity: &str) -> EventKey {
        keyed_event_definition(identity)
            .expect("fixture event identity exists")
            .0
    }

    #[test]
    fn nested_frames_restore_parent_state() -> Result<(), BridgeInstallError> {
        let frames = EventFrames::default();
        let outer = frames.enter(
            payload(
                "fixture.outer",
                EventControls {
                    skippable: true,
                    result:    true,
                },
            ),
            event_key("module_load"),
            EventResultKind::Boolean,
        )?;
        assert_eq!(frames.current()?.map(|event| event.depth), Some(1));
        assert!(frames.contains(event_key("module_load")));
        assert!(!frames.contains(event_key("associate_add_before")));
        frames.control(EventCommand::SetResult(b"true".to_vec()))?;

        let inner = frames.enter(
            payload(
                "fixture.inner",
                EventControls {
                    skippable: true,
                    result:    false,
                },
            ),
            event_key("associate_add_before"),
            EventResultKind::None,
        )?;
        assert_eq!(frames.current()?.map(|event| event.depth), Some(2));
        assert!(frames.contains(event_key("associate_add_before")));
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
        assert_eq!(outer.boolean_result(), Some(true));
        assert!(frames.current()?.is_none());
        assert!(!frames.contains(event_key("module_load")));
        Ok(())
    }

    #[test]
    fn scope_drop_removes_frame_and_controls_are_schema_checked() -> Result<(), BridgeInstallError>
    {
        let frames = EventFrames::default();
        {
            let _scope = frames.enter(
                payload("fixture.read_only", EventControls::default()),
                event_key("module_load"),
                EventResultKind::None,
            )?;
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
    fn event_results_are_decoded_once_to_the_declared_type() -> Result<(), BridgeInstallError> {
        assert_eq!(
            parse_event_result(EventResultKind::Boolean, b"true")?,
            EventResultValue::Boolean(true)
        );
        assert_eq!(
            parse_event_result(EventResultKind::Unsigned, b"4294967295")?,
            EventResultValue::Unsigned(u32::MAX)
        );
        assert_eq!(
            parse_event_result(EventResultKind::Signed, b"-2147483648")?,
            EventResultValue::Signed(i32::MIN)
        );
        assert_eq!(
            parse_event_result(EventResultKind::Float, b"1.25")?,
            EventResultValue::Float(1.25)
        );
        assert_eq!(
            parse_event_result(EventResultKind::String, br#""denied""#)?,
            EventResultValue::String("denied".to_string())
        );
        assert_eq!(
            parse_event_result(EventResultKind::ObjectId, b"\"0102aBcd\"")?,
            EventResultValue::ObjectId(0x0102_abcd)
        );
        assert!(parse_event_result(EventResultKind::Boolean, b"1").is_err());
        assert!(parse_event_result(EventResultKind::ObjectId, b"\"123\"").is_err());
        Ok(())
    }

    #[test]
    fn subscription_updates_commit_or_preserve_the_previous_set() -> Result<(), BridgeInstallError>
    {
        let mut subscriptions = SubscriptionState::default();
        subscriptions.active.insert("old".to_string());
        subscriptions
            .active_id_whitelists
            .insert("old.filter".to_string(), BTreeSet::from([1]));
        subscriptions.begin()?;
        subscriptions.record("new", Some("new.filter"))?;
        subscriptions
            .id_whitelists_mut()
            .get_mut("new.filter")
            .expect("forced filter")
            .insert(2);
        subscriptions.abort();
        assert_eq!(subscriptions.active, BTreeSet::from(["old".to_string()]));
        assert_eq!(
            subscriptions.active_id_whitelists,
            BTreeMap::from([("old.filter".to_string(), BTreeSet::from([1]))])
        );

        subscriptions.begin()?;
        subscriptions.record("new", Some("new.filter"))?;
        subscriptions
            .id_whitelists_mut()
            .get_mut("new.filter")
            .expect("forced filter")
            .insert(2);
        subscriptions.commit()?;
        assert_eq!(subscriptions.active, BTreeSet::from(["new".to_string()]));
        assert_eq!(
            subscriptions.active_id_whitelists,
            BTreeMap::from([("new.filter".to_string(), BTreeSet::from([2]))])
        );
        Ok(())
    }

    #[test]
    fn event_frames_reject_a_second_native_thread() -> Result<(), BridgeInstallError> {
        let frames = Arc::new(EventFrames::default());
        let scope = frames.enter(
            payload("fixture.owner", EventControls::default()),
            event_key("module_load"),
            EventResultKind::None,
        )?;
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
