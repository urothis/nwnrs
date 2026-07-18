use std::{error::Error, fmt};

use crate::RuntimeContext;

const NAMESPACE: &str = "NWNRS";
const MAX_LOG_MESSAGE_BYTES: usize = 64 * 1024;

/// Live read-only server values copied from the engine for one bridge call.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ServerState {
    /// Current module name as engine string bytes.
    pub module_name:  Vec<u8>,
    /// Number of players currently in the server player list.
    pub player_count: i32,
    /// Maximum number of player connections configured for the session.
    pub max_players:  i32,
}

/// One engine event-script invocation active on the current server thread.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EventContext {
    /// Stable semantic event name, such as `module.on_module_load`.
    pub name:        String,
    /// Engine `EVENT_SCRIPT_*` identifier, or `-1` outside an event.
    pub id:          i32,
    /// Event script resref copied from the engine.
    pub script_name: Vec<u8>,
    /// Current phase. The context-first API currently reports `running`.
    pub phase:       String,
    /// One-based VM script nesting depth, or zero outside an event.
    pub depth:       i32,
}

impl Default for EventContext {
    fn default() -> Self {
        Self {
            name:        String::new(),
            id:          -1,
            script_name: Vec::new(),
            phase:       String::new(),
            depth:       0,
        }
    }
}

/// A three-dimensional vector exchanged with NWScript.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vector {
    /// X coordinate.
    pub x: f32,
    /// Y coordinate.
    pub y: f32,
    /// Z coordinate.
    pub z: f32,
}

/// One value held by the native NWScript bridge.
#[derive(Clone, Debug, PartialEq)]
pub enum BridgeValue {
    /// Signed NWScript integer.
    Integer(i32),
    /// Single-precision NWScript float.
    Float(f32),
    /// Neverwinter Nights object identifier.
    Object(u32),
    /// Owned NWScript string bytes.
    String(Vec<u8>),
    /// Three-dimensional vector.
    Vector(Vector),
}

/// Severity attached to one log message emitted by NWScript.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScriptLogLevel {
    /// Highly detailed execution tracing.
    Trace,
    /// Diagnostic information useful while developing.
    Debug,
    /// Normal operational information.
    Info,
    /// A recoverable or suspicious condition.
    Warn,
    /// A failed operation requiring attention.
    Error,
}

impl TryFrom<i32> for ScriptLogLevel {
    type Error = BridgeError;

    fn try_from(value: i32) -> Result<Self, BridgeError> {
        match value {
            0 => Ok(Self::Trace),
            1 => Ok(Self::Debug),
            2 => Ok(Self::Info),
            3 => Ok(Self::Warn),
            4 => Ok(Self::Error),
            _ => Err(BridgeError::new(format!(
                "invalid NWNRS log level {value}; expected 0 through 4"
            ))),
        }
    }
}

/// One validated log record sent from NWScript to the native runtime.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScriptLog {
    /// Requested severity.
    pub level:   ScriptLogLevel,
    /// Original NWScript string bytes.
    pub message: Vec<u8>,
}

impl BridgeValue {
    fn kind(&self) -> &'static str {
        match self {
            Self::Integer(_) => "integer",
            Self::Float(_) => "float",
            Self::Object(_) => "object",
            Self::String(_) => "string",
            Self::Vector(_) => "vector",
        }
    }
}

/// An error produced while dispatching a call from NWScript.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BridgeError {
    message: String,
}

impl BridgeError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for BridgeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for BridgeError {}

/// Result returned by safe bridge operations.
pub type BridgeResult<T> = Result<T, BridgeError>;

/// Per-thread argument and return state for the NWScript bridge.
///
/// Values pushed by NWScript are consumed by the next call. A call replaces
/// all previous return values so stale data cannot leak between scripts.
#[derive(Debug, Default)]
pub struct ScriptBridge {
    arguments: Vec<BridgeValue>,
    returns:   Vec<BridgeValue>,
    logs:      Vec<ScriptLog>,
}

impl ScriptBridge {
    /// Pushes one argument for the next function call.
    pub fn push_argument(&mut self, value: BridgeValue) {
        self.arguments.push(value);
    }

    /// Dispatches one statically registered nwnrs function.
    ///
    /// # Errors
    ///
    /// Returns an error for an unknown namespace or function. Arguments and
    /// stale return values are discarded on both success and failure.
    pub fn call(
        &mut self,
        namespace: &str,
        function: &str,
        context: &RuntimeContext,
        server: &ServerState,
        event: &EventContext,
    ) -> BridgeResult<()> {
        self.returns.clear();
        self.logs.clear();
        let result = self.dispatch(namespace, function, context, server, event);
        self.arguments.clear();
        if result.is_err() {
            self.logs.clear();
        }
        result
    }

    /// Takes log records produced by the preceding successful call.
    #[must_use]
    pub fn take_logs(&mut self) -> Vec<ScriptLog> {
        std::mem::take(&mut self.logs)
    }

    /// Pops one return value of the requested type.
    ///
    /// # Errors
    ///
    /// Returns an error when no return value exists or its type differs.
    pub fn pop_integer(&mut self) -> BridgeResult<i32> {
        match self.pop_value("integer")? {
            BridgeValue::Integer(value) => Ok(value),
            _ => Err(BridgeError::new("return value changed while being popped")),
        }
    }

    /// Pops one floating-point return value.
    ///
    /// # Errors
    ///
    /// Returns an error when no return value exists or its type differs.
    pub fn pop_float(&mut self) -> BridgeResult<f32> {
        match self.pop_value("float")? {
            BridgeValue::Float(value) => Ok(value),
            _ => Err(BridgeError::new("return value changed while being popped")),
        }
    }

    /// Pops one object return value.
    ///
    /// # Errors
    ///
    /// Returns an error when no return value exists or its type differs.
    pub fn pop_object(&mut self) -> BridgeResult<u32> {
        match self.pop_value("object")? {
            BridgeValue::Object(value) => Ok(value),
            _ => Err(BridgeError::new("return value changed while being popped")),
        }
    }

    /// Pops one string return value.
    ///
    /// # Errors
    ///
    /// Returns an error when no return value exists or its type differs.
    pub fn pop_string(&mut self) -> BridgeResult<Vec<u8>> {
        match self.pop_value("string")? {
            BridgeValue::String(value) => Ok(value),
            _ => Err(BridgeError::new("return value changed while being popped")),
        }
    }

    /// Pops one vector return value.
    ///
    /// # Errors
    ///
    /// Returns an error when no return value exists or its type differs.
    pub fn pop_vector(&mut self) -> BridgeResult<Vector> {
        match self.pop_value("vector")? {
            BridgeValue::Vector(value) => Ok(value),
            _ => Err(BridgeError::new("return value changed while being popped")),
        }
    }

    fn dispatch(
        &mut self,
        namespace: &str,
        function: &str,
        context: &RuntimeContext,
        server: &ServerState,
        event: &EventContext,
    ) -> BridgeResult<()> {
        if namespace != NAMESPACE {
            return Err(BridgeError::new(format!(
                "unknown NWScript bridge namespace: {namespace}"
            )));
        }
        if function == "Log" {
            return self.dispatch_log();
        }
        if !self.arguments.is_empty() {
            return Err(BridgeError::new(format!(
                "{NAMESPACE}.{function} does not accept arguments"
            )));
        }

        let value = match function {
            "GetRuntimeVersion" => string_value(env!("CARGO_PKG_VERSION")),
            "GetServerBinarySha256" => string_value(&context.server.sha256.to_string()),
            "GetServerBuild" => string_value(
                context
                    .target
                    .pack
                    .server
                    .build
                    .as_deref()
                    .unwrap_or_default(),
            ),
            "GetServerPlatform" => string_value(&context.server.platform.to_string()),
            "GetServerOperatingSystem" => string_value(&context.server.platform.os.to_string()),
            "GetServerArchitecture" => {
                string_value(&context.server.platform.architecture.to_string())
            }
            "GetModuleName" => BridgeValue::String(server.module_name.clone()),
            "GetPlayerCount" => BridgeValue::Integer(server.player_count),
            "GetMaxPlayers" => BridgeValue::Integer(server.max_players),
            "GetIsInEvent" => BridgeValue::Integer(i32::from(event.depth > 0)),
            "GetCurrentEvent" => string_value(&event.name),
            "GetCurrentEventId" => BridgeValue::Integer(event.id),
            "GetCurrentEventScript" => BridgeValue::String(event.script_name.clone()),
            "GetCurrentEventPhase" => string_value(&event.phase),
            "GetCurrentEventDepth" => BridgeValue::Integer(event.depth),
            _ => {
                return Err(BridgeError::new(format!(
                    "unknown NWScript bridge function: {NAMESPACE}.{function}"
                )));
            }
        };
        self.returns.push(value);
        Ok(())
    }

    fn dispatch_log(&mut self) -> BridgeResult<()> {
        let message = match self.pop_argument("string")? {
            BridgeValue::String(value) => value,
            _ => return Err(BridgeError::new("log message changed while being popped")),
        };
        let level = match self.pop_argument("integer")? {
            BridgeValue::Integer(value) => ScriptLogLevel::try_from(value)?,
            _ => return Err(BridgeError::new("log level changed while being popped")),
        };
        if !self.arguments.is_empty() {
            return Err(BridgeError::new(format!(
                "{NAMESPACE}.Log received too many arguments"
            )));
        }
        if message.len() > MAX_LOG_MESSAGE_BYTES {
            return Err(BridgeError::new(format!(
                "{NAMESPACE}.Log message exceeds {MAX_LOG_MESSAGE_BYTES} bytes"
            )));
        }
        self.logs.push(ScriptLog {
            level,
            message,
        });
        Ok(())
    }

    fn pop_argument(&mut self, expected: &'static str) -> BridgeResult<BridgeValue> {
        let value = self
            .arguments
            .pop()
            .ok_or_else(|| BridgeError::new(format!("missing {expected} argument")))?;
        if value.kind() != expected {
            return Err(BridgeError::new(format!(
                "expected {expected} argument, found {}",
                value.kind()
            )));
        }
        Ok(value)
    }

    fn pop_value(&mut self, expected: &'static str) -> BridgeResult<BridgeValue> {
        let value = self
            .returns
            .pop()
            .ok_or_else(|| BridgeError::new(format!("no {expected} return value is available")))?;
        if value.kind() != expected {
            return Err(BridgeError::new(format!(
                "expected {expected} return value, found {}",
                value.kind()
            )));
        }
        Ok(value)
    }
}

fn string_value(value: &str) -> BridgeValue {
    BridgeValue::String(value.as_bytes().to_vec())
}

/// Returns the stable semantic name for an engine `EVENT_SCRIPT_*` identifier.
#[must_use]
pub fn event_name(id: i32) -> &'static str {
    match id {
        3000 => "module.on_heartbeat",
        3001 => "module.on_user_defined",
        3002 => "module.on_module_load",
        3003 => "module.on_module_start",
        3004 => "module.on_client_enter",
        3005 => "module.on_client_exit",
        3006 => "module.on_activate_item",
        3007 => "module.on_acquire_item",
        3008 => "module.on_lose_item",
        3009 => "module.on_player_death",
        3010 => "module.on_player_dying",
        3011 => "module.on_respawn_button_pressed",
        3012 => "module.on_player_rest",
        3013 => "module.on_player_level_up",
        3014 => "module.on_player_cancel_cutscene",
        3015 => "module.on_equip_item",
        3016 => "module.on_unequip_item",
        3017 => "module.on_player_chat",
        3018 => "module.on_player_target",
        4000 => "area.on_heartbeat",
        4001 => "area.on_user_defined",
        4002 => "area.on_enter",
        4003 => "area.on_exit",
        5000 => "creature.on_heartbeat",
        5001 => "creature.on_notice",
        5002 => "creature.on_spell_cast_at",
        5003 => "creature.on_melee_attacked",
        5004 => "creature.on_damaged",
        5005 => "creature.on_disturbed",
        5006 => "creature.on_end_combat_round",
        5007 => "creature.on_dialogue",
        5008 => "creature.on_spawn_in",
        5009 => "creature.on_rested",
        5010 => "creature.on_death",
        5011 => "creature.on_user_defined",
        5012 => "creature.on_blocked_by_door",
        7000 => "trigger.on_heartbeat",
        7001 => "trigger.on_object_enter",
        7002 => "trigger.on_object_exit",
        7003 => "trigger.on_user_defined",
        7004 => "trigger.on_trap_triggered",
        7005 => "trigger.on_disarmed",
        7006 => "trigger.on_clicked",
        9000 => "placeable.on_closed",
        9001 => "placeable.on_damaged",
        9002 => "placeable.on_death",
        9003 => "placeable.on_disarm",
        9004 => "placeable.on_heartbeat",
        9005 => "placeable.on_inventory_disturbed",
        9006 => "placeable.on_lock",
        9007 => "placeable.on_melee_attacked",
        9008 => "placeable.on_open",
        9009 => "placeable.on_spell_cast_at",
        9010 => "placeable.on_trap_triggered",
        9011 => "placeable.on_unlock",
        9012 => "placeable.on_used",
        9013 => "placeable.on_user_defined",
        9014 => "placeable.on_dialogue",
        9015 => "placeable.on_left_click",
        10000 => "door.on_open",
        10001 => "door.on_close",
        10002 => "door.on_damage",
        10003 => "door.on_death",
        10004 => "door.on_disarm",
        10005 => "door.on_heartbeat",
        10006 => "door.on_lock",
        10007 => "door.on_melee_attacked",
        10008 => "door.on_spell_cast_at",
        10009 => "door.on_trap_triggered",
        10010 => "door.on_unlock",
        10011 => "door.on_user_defined",
        10012 => "door.on_clicked",
        10013 => "door.on_dialogue",
        10014 => "door.on_fail_to_open",
        11000 => "area_of_effect.on_heartbeat",
        11001 => "area_of_effect.on_user_defined",
        11002 => "area_of_effect.on_object_enter",
        11003 => "area_of_effect.on_object_exit",
        13000 => "encounter.on_object_enter",
        13001 => "encounter.on_object_exit",
        13002 => "encounter.on_heartbeat",
        13003 => "encounter.on_exhausted",
        13004 => "encounter.on_user_defined",
        14000 => "store.on_open",
        14001 => "store.on_close",
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{
        BridgeValue, EventContext, ScriptBridge, ScriptLog, ScriptLogLevel, ServerState, Vector,
    };
    use crate::{
        Architecture, BinaryIdentity, BridgeTarget, EventTarget, FileSha256, OperatingSystem,
        Platform, RUNTIME_API_VERSION, RuntimeContext, SelectedTargetPack,
        TARGET_PACK_SCHEMA_VERSION, TargetAddress, TargetPack, TargetServer,
    };

    #[test]
    fn dispatches_server_identity_and_clears_failed_calls() -> Result<(), Box<dyn std::error::Error>>
    {
        let context = context();
        let server = ServerState {
            module_name:  b"fixture-module".to_vec(),
            player_count: 2,
            max_players:  64,
        };
        let event = EventContext {
            name:        "module.on_module_load".to_string(),
            id:          3002,
            script_name: b"nwnrs_init".to_vec(),
            phase:       "running".to_string(),
            depth:       1,
        };
        let mut bridge = ScriptBridge::default();

        bridge.call("NWNRS", "GetServerBuild", &context, &server, &event)?;
        assert_eq!(bridge.pop_string()?, b"fixture");
        bridge.call("NWNRS", "GetModuleName", &context, &server, &event)?;
        assert_eq!(bridge.pop_string()?, b"fixture-module");
        bridge.call("NWNRS", "GetPlayerCount", &context, &server, &event)?;
        assert_eq!(bridge.pop_integer()?, 2);
        bridge.call("NWNRS", "GetMaxPlayers", &context, &server, &event)?;
        assert_eq!(bridge.pop_integer()?, 64);
        bridge.call("NWNRS", "GetCurrentEvent", &context, &server, &event)?;
        assert_eq!(bridge.pop_string()?, b"module.on_module_load");
        bridge.call("NWNRS", "GetCurrentEventScript", &context, &server, &event)?;
        assert_eq!(bridge.pop_string()?, b"nwnrs_init");
        bridge.call("NWNRS", "GetCurrentEventDepth", &context, &server, &event)?;
        assert_eq!(bridge.pop_integer()?, 1);

        bridge.push_argument(BridgeValue::Integer(1));
        assert!(
            bridge
                .call("NWNRS", "GetRuntimeVersion", &context, &server, &event)
                .is_err()
        );
        bridge.call("NWNRS", "GetRuntimeVersion", &context, &server, &event)?;
        assert_eq!(bridge.pop_string()?, env!("CARGO_PKG_VERSION").as_bytes());
        Ok(())
    }

    #[test]
    fn preserves_scalar_bridge_value_types() -> Result<(), Box<dyn std::error::Error>> {
        let mut bridge = ScriptBridge {
            returns: vec![
                BridgeValue::Vector(Vector {
                    x: 1.0,
                    y: 2.0,
                    z: 3.0,
                }),
                BridgeValue::Object(7),
                BridgeValue::Float(2.5),
                BridgeValue::Integer(4),
            ],
            ..ScriptBridge::default()
        };
        assert_eq!(bridge.pop_integer()?, 4);
        assert_eq!(bridge.pop_float()?, 2.5);
        assert_eq!(bridge.pop_object()?, 7);
        assert_eq!(
            bridge.pop_vector()?,
            Vector {
                x: 1.0,
                y: 2.0,
                z: 3.0,
            }
        );
        Ok(())
    }

    #[test]
    fn validates_and_returns_nwscript_log_records() -> Result<(), Box<dyn std::error::Error>> {
        let context = context();
        let mut bridge = ScriptBridge::default();
        bridge.push_argument(BridgeValue::Integer(3));
        bridge.push_argument(BridgeValue::String(b"careful now".to_vec()));
        bridge.call(
            "NWNRS",
            "Log",
            &context,
            &ServerState::default(),
            &EventContext::default(),
        )?;
        assert_eq!(
            bridge.take_logs(),
            vec![ScriptLog {
                level:   ScriptLogLevel::Warn,
                message: b"careful now".to_vec(),
            }]
        );

        bridge.push_argument(BridgeValue::Integer(9));
        bridge.push_argument(BridgeValue::String(b"invalid".to_vec()));
        assert!(
            bridge
                .call(
                    "NWNRS",
                    "Log",
                    &context,
                    &ServerState::default(),
                    &EventContext::default(),
                )
                .is_err()
        );
        assert!(bridge.take_logs().is_empty());
        Ok(())
    }

    fn context() -> RuntimeContext {
        let platform = Platform {
            os:           OperatingSystem::Linux,
            architecture: Architecture::X86_64,
        };
        RuntimeContext {
            server:   BinaryIdentity {
                path: PathBuf::from("nwserver"),
                sha256: FileSha256([0; 32]),
                platform,
            },
            target:   SelectedTargetPack {
                path: PathBuf::from("target.toml"),
                pack: TargetPack {
                    schema_version: TARGET_PACK_SCHEMA_VERSION,
                    runtime_api:    RUNTIME_API_VERSION,
                    server:         TargetServer {
                        sha256: "0".repeat(64),
                        platform,
                        build: Some("fixture".to_string()),
                    },
                    bridge:         bridge_target(),
                    server_state:   server_state_target(),
                    events:         event_target(),
                },
            },
            required: true,
        }
    }

    fn bridge_target() -> BridgeTarget {
        let address = || TargetAddress::Offset {
            offset: 1
        };
        BridgeTarget {
            function_management:    address(),
            virtual_machine_offset: 0,
            stack_pop_integer:      address(),
            stack_push_integer:     address(),
            stack_pop_float:        address(),
            stack_push_float:       address(),
            stack_pop_object:       address(),
            stack_push_object:      address(),
            stack_pop_string:       address(),
            stack_push_string:      address(),
            stack_pop_vector:       address(),
            stack_push_vector:      address(),
            free_exo_string_buffer: address(),
        }
    }

    fn server_state_target() -> crate::ServerStateTarget {
        let address = || TargetAddress::Offset {
            offset: 1
        };
        crate::ServerStateTarget {
            app_manager:                    address(),
            server_exo_app_offset:          8,
            get_server_info:                address(),
            server_info_module_name_offset: 8,
            get_player_list:                address(),
            player_list_count_offset:       8,
            get_net_layer:                  address(),
            get_session_max_players:        address(),
        }
    }

    fn event_target() -> EventTarget {
        EventTarget {
            recursion_level_offset: 36,
            script_array_offset:    40,
            script_slot_count:      8,
            script_stride:          152,
            script_name_offset:     24,
            script_event_id_offset: 72,
        }
    }
}
