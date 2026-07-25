pub(crate) mod abi;
mod address;
mod administration;
mod correlation;
mod event;
mod events;
pub(crate) mod hook;
mod pump;
mod server;
mod shim;
mod string;
pub(crate) mod thread;
mod vm;

use std::{collections::BTreeMap, ffi::c_void, sync::OnceLock};

use address::{NativeFunction, Resolver};
use administration::AdministrationEngine;
pub(crate) use correlation::CorrelationToken;
use event::{EventEngine, EventSubscriptionUpdate};
pub(crate) use event::{EventFrame, EventSpec};
use hook::{NativeHookSpec, install_native_hooks};
use nwnrs_runtime::{
    AdministrationCommand, BannedLists, EventCommand, EventObjectId, EventPayload, EventValue,
    HostCommandResult, RuntimeContext,
};
use server::ServerEngine;
pub(crate) use thread::EngineThreadToken;
use vm::VirtualMachineEngine;

use crate::bridge::BridgeInstallError;

static ACTIVE_ENGINE: OnceLock<Engine> = OnceLock::new();

pub(crate) fn set_active_engine(engine: Engine) -> Result<(), BridgeInstallError> {
    ACTIVE_ENGINE.set(engine).map_err(|_engine| {
        BridgeInstallError::new("NWScript bridge was initialized more than once")
    })
}

pub(crate) fn active_engine() -> Option<&'static Engine> {
    ACTIVE_ENGINE.get()
}

pub(crate) struct Engine {
    vm:             VirtualMachineEngine,
    server:         Option<ServerEngine>,
    administration: Option<AdministrationEngine>,
    event:          Option<EventEngine>,
}

impl Engine {
    pub(crate) fn resolve(
        module: *mut frida_gum_sys::GumModule,
        context: &RuntimeContext,
    ) -> Result<Self, BridgeInstallError> {
        shim::verify_native_shim()?;
        let resolver = Resolver::new(module)?;
        let layouts = &context.target.pack.layouts;
        let vm = VirtualMachineEngine::resolve(
            &resolver,
            &context.target.pack.bridge,
            &layouts.classes,
        )?;
        let server = context
            .target
            .pack
            .server_state
            .as_ref()
            .map(|target| {
                ServerEngine::resolve(
                    &resolver,
                    target,
                    &layouts.classes,
                    layouts.player_list.count_offset,
                )
            })
            .transpose()?;
        let event = context
            .target
            .pack
            .events
            .as_ref()
            .map(|target| {
                EventEngine::resolve(
                    &resolver,
                    target,
                    &layouts.classes,
                    context.target.pack.server_state.as_ref(),
                )
            })
            .transpose()?;
        let administration = context
            .target
            .pack
            .administration
            .as_ref()
            .map(|target| {
                AdministrationEngine::resolve(
                    &resolver,
                    target,
                    &context.target.pack.bridge,
                    &layouts.classes,
                )
            })
            .transpose()?;
        Ok(Self {
            vm,
            server,
            administration,
            event,
        })
    }

    pub(crate) fn hook_target(&self) -> usize {
        self.vm.hook_target()
    }

    pub(crate) fn administration_main_loop_hook_target(&self) -> Option<usize> {
        self.administration
            .as_ref()
            .map(AdministrationEngine::main_loop_hook_target)
    }

    pub(crate) fn event_hook_target(&self, name: &str) -> Option<usize> {
        self.event.as_ref()?.hook_target(name)
    }

    pub(crate) fn event_function_available(&self, name: &str) -> bool {
        self.event
            .as_ref()
            .is_some_and(|event| event.function_available(name))
    }

    pub(crate) fn event_function<Function: Copy>(
        &self,
        name: &str,
    ) -> Option<NativeFunction<Function>> {
        self.event.as_ref()?.function(name)
    }

    pub(crate) fn event_layout_offset(&self, name: &str) -> Option<usize> {
        self.event.as_ref()?.layout_offset(name)
    }

    pub(crate) fn event_server(
        &self,
        thread: &EngineThreadToken,
    ) -> Result<*mut c_void, BridgeInstallError> {
        self.event
            .as_ref()
            .ok_or_else(|| BridgeInstallError::new("target pack does not provide event targets"))?
            .server(thread)
    }

    pub(crate) fn event_game_object_id(
        &self,
        thread: &EngineThreadToken,
        object: *const c_void,
    ) -> Result<EventObjectId, BridgeInstallError> {
        self.event
            .as_ref()
            .ok_or_else(|| BridgeInstallError::new("target pack does not provide event targets"))?
            .game_object_id(thread, object)
    }

    pub(crate) fn dispatch_event(
        &self,
        thread: &EngineThreadToken,
        spec: EventSpec,
        target: EventObjectId,
        data: BTreeMap<String, EventValue>,
    ) -> Result<(bool, EventFrame), BridgeInstallError> {
        self.event
            .as_ref()
            .ok_or_else(|| BridgeInstallError::new("target pack does not provide event targets"))?
            .dispatch(thread, spec, target, data)
    }

    pub(crate) fn event_bootstrap_hook_specs(
        &self,
    ) -> Result<Vec<NativeHookSpec>, BridgeInstallError> {
        events::bootstrap_hook_specs(self)
    }

    pub(crate) fn event_should_dispatch(&self, spec: EventSpec) -> bool {
        self.event
            .as_ref()
            .is_some_and(|event| event.should_dispatch(spec))
    }

    pub(crate) fn begin_event_subscription_update(
        &self,
    ) -> Result<EventSubscriptionUpdate<'_>, BridgeInstallError> {
        self.event
            .as_ref()
            .ok_or_else(|| BridgeInstallError::new("target pack does not provide event targets"))?
            .begin_subscription_update()
    }

    pub(crate) fn commit_event_subscription_update(
        &self,
        subscriptions: EventSubscriptionUpdate<'_>,
    ) -> Result<(), BridgeInstallError> {
        let event = self
            .event
            .as_ref()
            .ok_or_else(|| BridgeInstallError::new("target pack does not provide event targets"))?;
        let required_hooks = event.pending_physical_hooks()?;
        if !required_hooks.is_empty() {
            let hooks: Vec<_> = events::hook_specs(self)?
                .into_iter()
                .filter(|hook| required_hooks.contains(hook.id()))
                .collect();
            let resolved_hooks = hooks
                .iter()
                .map(NativeHookSpec::id)
                .collect::<std::collections::BTreeSet<_>>();
            if resolved_hooks != required_hooks || resolved_hooks.len() != hooks.len() {
                return Err(BridgeInstallError::new(format!(
                    "event subscription plan resolved {} physical hooks for {} required hooks",
                    hooks.len(),
                    required_hooks.len()
                )));
            }
            install_native_hooks(&hooks)?;
            event.mark_hooks_installed(&required_hooks)?;
        }
        subscriptions.commit()
    }

    pub(crate) fn event_id_is_whitelisted(&self, name: &str, id: i32) -> bool {
        self.event
            .as_ref()
            .is_some_and(|event| event.id_is_whitelisted(name, id))
    }

    #[allow(dead_code)]
    pub(crate) fn begin_event_correlation<Value>(
        &self,
        owner: &'static str,
        key: u64,
        value: Value,
    ) -> Result<CorrelationToken, BridgeInstallError>
    where
        Value: Send + 'static,
    {
        self.event
            .as_ref()
            .ok_or_else(|| BridgeInstallError::new("target pack does not provide event targets"))?
            .begin_correlation(owner, key, value)
    }

    #[allow(dead_code)]
    pub(crate) fn finish_event_correlation<Value>(
        &self,
        owner: &'static str,
        key: u64,
        token: CorrelationToken,
    ) -> Result<Value, BridgeInstallError>
    where
        Value: Send + 'static,
    {
        self.event
            .as_ref()
            .ok_or_else(|| BridgeInstallError::new("target pack does not provide event targets"))?
            .finish_correlation(owner, key, token)
    }

    #[allow(dead_code)]
    pub(crate) fn enqueue_pump_event(
        &self,
        identity: &str,
        target: EventObjectId,
        data: BTreeMap<String, EventValue>,
    ) -> Result<u64, BridgeInstallError> {
        self.event
            .as_ref()
            .ok_or_else(|| BridgeInstallError::new("target pack does not provide event targets"))?
            .enqueue_pump_record(identity, target, data)
    }

    pub(crate) fn process_deferred_administration(
        &self,
        thread: &EngineThreadToken,
    ) -> Result<(), BridgeInstallError> {
        if let Some(event) = &self.event {
            event.process_pump(thread)?;
        }
        if let Some(administration) = &self.administration {
            administration.process_deferred(thread, self.server()?)?;
        }
        Ok(())
    }

    pub(crate) fn reset_event_lifecycle(&self) -> Result<(), BridgeInstallError> {
        if let Some(event) = &self.event {
            event.reset_lifecycle()?;
        }
        Ok(())
    }

    pub(crate) fn virtual_machine(
        &self,
        thread: &EngineThreadToken,
        commands: *mut c_void,
    ) -> Result<*mut c_void, BridgeInstallError> {
        self.vm.virtual_machine(thread, commands)
    }

    pub(crate) fn vm(&self) -> &VirtualMachineEngine {
        &self.vm
    }

    pub(crate) fn module_name(
        &self,
        thread: &EngineThreadToken,
    ) -> Result<Vec<u8>, BridgeInstallError> {
        self.server()?.module_name(thread)
    }

    pub(crate) fn player_count(
        &self,
        thread: &EngineThreadToken,
    ) -> Result<i32, BridgeInstallError> {
        self.server()?.player_count(thread)
    }

    pub(crate) fn max_players(
        &self,
        thread: &EngineThreadToken,
    ) -> Result<i32, BridgeInstallError> {
        self.server()?.max_players(thread)
    }

    pub(crate) fn udp_port(&self, thread: &EngineThreadToken) -> Result<i32, BridgeInstallError> {
        self.server()?.udp_port(thread)
    }

    pub(crate) fn server_name(
        &self,
        thread: &EngineThreadToken,
    ) -> Result<Vec<u8>, BridgeInstallError> {
        self.administration()?.server_name(thread, self.server()?)
    }

    pub(crate) fn player_password_is_set(
        &self,
        thread: &EngineThreadToken,
    ) -> Result<bool, BridgeInstallError> {
        self.administration()?
            .player_password_is_set(thread, self.server()?)
    }

    pub(crate) fn dm_password_is_set(
        &self,
        thread: &EngineThreadToken,
    ) -> Result<bool, BridgeInstallError> {
        self.administration()?
            .dm_password_is_set(thread, self.server()?)
    }

    pub(crate) fn min_level(&self, thread: &EngineThreadToken) -> Result<i32, BridgeInstallError> {
        self.administration()?.min_level(thread, self.server()?)
    }

    pub(crate) fn max_level(&self, thread: &EngineThreadToken) -> Result<i32, BridgeInstallError> {
        self.administration()?.max_level(thread, self.server()?)
    }

    pub(crate) fn play_option(
        &self,
        thread: &EngineThreadToken,
        option: i32,
    ) -> Result<i32, BridgeInstallError> {
        self.administration()?
            .play_option(thread, self.server()?, option)
    }

    pub(crate) fn debug_value(
        &self,
        thread: &EngineThreadToken,
        debug_type: i32,
    ) -> Result<i32, BridgeInstallError> {
        self.administration()?.debug_value(thread, debug_type)
    }

    pub(crate) fn banned_lists(
        &self,
        thread: &EngineThreadToken,
    ) -> Result<BannedLists, BridgeInstallError> {
        self.administration()?.banned_lists(thread, self.server()?)
    }

    pub(crate) fn execute_administration(
        &self,
        thread: &EngineThreadToken,
        command: &AdministrationCommand,
    ) -> Result<HostCommandResult, BridgeInstallError> {
        let result = self
            .administration()?
            .execute(thread, self.server()?, command)?;
        if matches!(command, AdministrationCommand::RequestShutdown)
            && let Some(event) = &self.event
        {
            event.shutdown()?;
        }
        Ok(result)
    }

    pub(crate) fn current_event(
        &self,
        thread: &EngineThreadToken,
    ) -> Result<Option<EventPayload>, BridgeInstallError> {
        self.event
            .as_ref()
            .map_or_else(|| Ok(None), |event| event.current_event(thread))
    }

    pub(crate) fn control_event(
        &self,
        thread: &EngineThreadToken,
        command: EventCommand,
    ) -> Result<(), BridgeInstallError> {
        let event = self
            .event
            .as_ref()
            .ok_or_else(|| BridgeInstallError::new("target pack does not provide event targets"))?;
        match command {
            EventCommand::Subscribe(identity) => event.record_subscription(&identity),
            EventCommand::ToggleIdWhitelist {
                name,
                enabled,
            } => event.toggle_id_whitelist(name, enabled),
            EventCommand::AddIdToWhitelist {
                name,
                id,
            } => event.add_id_to_whitelist(name, id),
            EventCommand::RemoveIdFromWhitelist {
                name,
                id,
            } => event.remove_id_from_whitelist(&name, id),
            command @ (EventCommand::Skip | EventCommand::SetResult(_)) => {
                event.control_event(thread, command)
            }
        }
    }

    fn server(&self) -> Result<&ServerEngine, BridgeInstallError> {
        self.server.as_ref().ok_or_else(|| {
            BridgeInstallError::new("target pack does not provide the server_state capability")
        })
    }

    fn administration(&self) -> Result<&AdministrationEngine, BridgeInstallError> {
        self.administration.as_ref().ok_or_else(|| {
            BridgeInstallError::new("target pack does not provide the administration capability")
        })
    }
}
