//! Safe runtime-host adapter backed by live NWServer engine access.

use nwnrs_runtime::{
    AdministrationCommand, BridgeError, BridgeErrorCode, BridgeResult, EventCommand,
    HostCommandResult, HostQuery, HostValue, RuntimeHost,
};

use crate::{
    bridge::BridgeInstallError,
    engine::{Engine, EngineThreadToken},
};

/// Binds one safe dispatcher call to the current native VM callback.
pub(crate) struct NativeRuntimeHost<'engine, 'thread> {
    engine: &'engine Engine,
    thread: &'thread EngineThreadToken,
}

impl<'engine, 'thread> NativeRuntimeHost<'engine, 'thread> {
    pub(crate) const fn new(engine: &'engine Engine, thread: &'thread EngineThreadToken) -> Self {
        Self {
            engine,
            thread,
        }
    }
}

impl RuntimeHost for NativeRuntimeHost<'_, '_> {
    fn query(&mut self, query: HostQuery) -> BridgeResult<HostValue> {
        let value = match query {
            HostQuery::ModuleName => {
                HostValue::String(self.engine.module_name(self.thread).map_err(engine_error)?)
            }
            HostQuery::PlayerCount => HostValue::Integer(
                self.engine
                    .player_count(self.thread)
                    .map_err(engine_error)?,
            ),
            HostQuery::MaxPlayers => {
                HostValue::Integer(self.engine.max_players(self.thread).map_err(engine_error)?)
            }
            HostQuery::UdpPort => {
                HostValue::Integer(self.engine.udp_port(self.thread).map_err(engine_error)?)
            }
            HostQuery::ServerName => {
                HostValue::String(self.engine.server_name(self.thread).map_err(engine_error)?)
            }
            HostQuery::PlayerPasswordIsSet => HostValue::Boolean(
                self.engine
                    .player_password_is_set(self.thread)
                    .map_err(engine_error)?,
            ),
            HostQuery::DmPasswordIsSet => HostValue::Boolean(
                self.engine
                    .dm_password_is_set(self.thread)
                    .map_err(engine_error)?,
            ),
            HostQuery::MinLevel => {
                HostValue::Integer(self.engine.min_level(self.thread).map_err(engine_error)?)
            }
            HostQuery::MaxLevel => {
                HostValue::Integer(self.engine.max_level(self.thread).map_err(engine_error)?)
            }
            HostQuery::PlayOption(option) => HostValue::Integer(
                self.engine
                    .play_option(self.thread, option)
                    .map_err(engine_error)?,
            ),
            HostQuery::DebugValue(debug_type) => HostValue::Integer(
                self.engine
                    .debug_value(self.thread, debug_type)
                    .map_err(engine_error)?,
            ),
            HostQuery::BannedLists => HostValue::BannedLists(
                self.engine
                    .banned_lists(self.thread)
                    .map_err(engine_error)?,
            ),
            HostQuery::CurrentEvent => HostValue::CurrentEvent(
                self.engine
                    .current_event(self.thread)
                    .map_err(engine_error)?,
            ),
        };
        Ok(value)
    }

    fn execute(&mut self, command: AdministrationCommand) -> BridgeResult<HostCommandResult> {
        self.engine
            .execute_administration(self.thread, &command)
            .map_err(engine_error)
    }

    fn control_event(&mut self, command: EventCommand) -> BridgeResult<()> {
        self.engine
            .control_event(self.thread, command)
            .map_err(engine_error)
    }
}

fn engine_error(error: BridgeInstallError) -> BridgeError {
    BridgeError::new(BridgeErrorCode::Engine, error.to_string())
}
