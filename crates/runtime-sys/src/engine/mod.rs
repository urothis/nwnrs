pub(crate) mod abi;
mod address;
mod event;
mod server;
mod string;
pub(crate) mod thread;
mod vm;

use std::ffi::c_void;

use address::Resolver;
use event::EventEngine;
use nwnrs_runtime::{EventContext, RuntimeContext};
use server::ServerEngine;
pub(crate) use thread::EngineThreadToken;
use vm::VirtualMachineEngine;

use crate::bridge::BridgeInstallError;

pub(crate) struct Engine {
    vm:     VirtualMachineEngine,
    server: Option<ServerEngine>,
    event:  Option<EventEngine>,
}

impl Engine {
    pub(crate) fn resolve(
        module: *mut frida_gum_sys::GumModule,
        context: &RuntimeContext,
    ) -> Result<Self, BridgeInstallError> {
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
            .map(|_| EventEngine::from_layouts(&layouts.classes))
            .transpose()?;
        Ok(Self {
            vm,
            server,
            event,
        })
    }

    pub(crate) fn hook_target(&self) -> usize {
        self.vm.hook_target()
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

    pub(crate) fn event_context(
        &self,
        thread: &EngineThreadToken,
        vm: *mut c_void,
    ) -> Result<EventContext, BridgeInstallError> {
        self.event.as_ref().map_or_else(
            || Ok(EventContext::default()),
            |event| event.context(thread, vm),
        )
    }

    fn server(&self) -> Result<&ServerEngine, BridgeInstallError> {
        self.server.as_ref().ok_or_else(|| {
            BridgeInstallError::new("target pack does not provide the server_state capability")
        })
    }
}
