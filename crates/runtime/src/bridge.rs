use std::{error::Error, fmt};

use crate::{Capability, RUNTIME_API_VERSION, RuntimeContext};

const NAMESPACE: &str = "NWNRS";
const MAX_LOG_MESSAGE_BYTES: usize = 64 * 1024;

/// Live read-only server values copied from the engine for one bridge call.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ServerSnapshot {
    /// Current module name as engine string bytes.
    pub module_name:  Vec<u8>,
    /// Number of players currently in the server player list.
    pub player_count: i32,
    /// Maximum number of player connections configured for the session.
    pub max_players:  i32,
    /// Active UDP listening port reported by the engine network layer.
    pub udp_port:     i32,
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
            _ => Err(BridgeError::new(
                BridgeErrorCode::InvalidArgument,
                format!("invalid NWNRS log level {value}; expected 0 through 4"),
            )),
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

/// Stable error codes exposed to NWScript.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(i32)]
pub enum BridgeErrorCode {
    /// No bridge error has been recorded.
    None = 0,
    /// The requested namespace is not registered.
    UnknownNamespace = 1,
    /// The requested function is not registered.
    UnknownFunction = 2,
    /// An argument or return value was missing or had the wrong type.
    InvalidArgument = 3,
    /// The exact target pack does not provide the required capability.
    MissingCapability = 4,
    /// A validated native engine operation failed.
    Engine = 5,
    /// A script attempted to reenter the per-thread bridge state.
    Reentrant = 6,
}

impl BridgeErrorCode {
    /// Returns the stable integer representation used by NWScript.
    #[must_use]
    pub const fn value(self) -> i32 {
        match self {
            Self::None => 0,
            Self::UnknownNamespace => 1,
            Self::UnknownFunction => 2,
            Self::InvalidArgument => 3,
            Self::MissingCapability => 4,
            Self::Engine => 5,
            Self::Reentrant => 6,
        }
    }
}

/// One statically registered function in the stable NWScript API.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BridgeFunction {
    /// Returns the integer bridge API version.
    GetApiVersion,
    /// Returns a named target-pack capability version.
    GetCapabilityVersion,
    /// Checks whether a named capability satisfies a minimum version.
    HasCapability,
    /// Returns the most recent bridge error code on this thread.
    GetLastErrorCode,
    /// Returns the most recent bridge error message on this thread.
    GetLastErrorMessage,
    /// Emits one structured runtime log record.
    Log,
    /// Returns the runtime crate version.
    GetRuntimeVersion,
    /// Returns the exact server binary SHA-256.
    GetServerBinarySha256,
    /// Returns the server build label.
    GetServerBuild,
    /// Returns the combined server platform.
    GetServerPlatform,
    /// Returns the server operating system.
    GetServerOperatingSystem,
    /// Returns the server architecture.
    GetServerArchitecture,
    /// Returns the active module name.
    GetModuleName,
    /// Returns the active player count.
    GetPlayerCount,
    /// Returns the configured maximum player count.
    GetMaxPlayers,
    /// Returns the active server UDP listening port.
    GetServerPort,
    /// Reports whether an engine event is active.
    GetIsInEvent,
    /// Returns the semantic current event name.
    GetCurrentEvent,
    /// Returns the current engine event identifier.
    GetCurrentEventId,
    /// Returns the current event script resref.
    GetCurrentEventScript,
    /// Returns the current event phase.
    GetCurrentEventPhase,
    /// Returns the current event nesting depth.
    GetCurrentEventDepth,
}

impl BridgeFunction {
    /// Complete registry in stable declaration order.
    ///
    /// ```
    /// assert!(nwnrs_runtime::BridgeFunction::ALL
    ///     .contains(&nwnrs_runtime::BridgeFunction::Log));
    /// ```
    pub const ALL: [Self; 22] = [
        Self::GetApiVersion,
        Self::GetCapabilityVersion,
        Self::HasCapability,
        Self::GetLastErrorCode,
        Self::GetLastErrorMessage,
        Self::Log,
        Self::GetRuntimeVersion,
        Self::GetServerBinarySha256,
        Self::GetServerBuild,
        Self::GetServerPlatform,
        Self::GetServerOperatingSystem,
        Self::GetServerArchitecture,
        Self::GetModuleName,
        Self::GetPlayerCount,
        Self::GetMaxPlayers,
        Self::GetServerPort,
        Self::GetIsInEvent,
        Self::GetCurrentEvent,
        Self::GetCurrentEventId,
        Self::GetCurrentEventScript,
        Self::GetCurrentEventPhase,
        Self::GetCurrentEventDepth,
    ];

    /// Returns the exact public function name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::GetApiVersion => "GetApiVersion",
            Self::GetCapabilityVersion => "GetCapabilityVersion",
            Self::HasCapability => "HasCapability",
            Self::GetLastErrorCode => "GetLastErrorCode",
            Self::GetLastErrorMessage => "GetLastErrorMessage",
            Self::Log => "Log",
            Self::GetRuntimeVersion => "GetRuntimeVersion",
            Self::GetServerBinarySha256 => "GetServerBinarySha256",
            Self::GetServerBuild => "GetServerBuild",
            Self::GetServerPlatform => "GetServerPlatform",
            Self::GetServerOperatingSystem => "GetServerOperatingSystem",
            Self::GetServerArchitecture => "GetServerArchitecture",
            Self::GetModuleName => "GetModuleName",
            Self::GetPlayerCount => "GetPlayerCount",
            Self::GetMaxPlayers => "GetMaxPlayers",
            Self::GetServerPort => "GetServerPort",
            Self::GetIsInEvent => "GetIsInEvent",
            Self::GetCurrentEvent => "GetCurrentEvent",
            Self::GetCurrentEventId => "GetCurrentEventId",
            Self::GetCurrentEventScript => "GetCurrentEventScript",
            Self::GetCurrentEventPhase => "GetCurrentEventPhase",
            Self::GetCurrentEventDepth => "GetCurrentEventDepth",
        }
    }

    /// Parses an exact, case-sensitive public function name.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "GetApiVersion" => Some(Self::GetApiVersion),
            "GetCapabilityVersion" => Some(Self::GetCapabilityVersion),
            "HasCapability" => Some(Self::HasCapability),
            "GetLastErrorCode" => Some(Self::GetLastErrorCode),
            "GetLastErrorMessage" => Some(Self::GetLastErrorMessage),
            "Log" => Some(Self::Log),
            "GetRuntimeVersion" => Some(Self::GetRuntimeVersion),
            "GetServerBinarySha256" => Some(Self::GetServerBinarySha256),
            "GetServerBuild" => Some(Self::GetServerBuild),
            "GetServerPlatform" => Some(Self::GetServerPlatform),
            "GetServerOperatingSystem" => Some(Self::GetServerOperatingSystem),
            "GetServerArchitecture" => Some(Self::GetServerArchitecture),
            "GetModuleName" => Some(Self::GetModuleName),
            "GetPlayerCount" => Some(Self::GetPlayerCount),
            "GetMaxPlayers" => Some(Self::GetMaxPlayers),
            "GetServerPort" => Some(Self::GetServerPort),
            "GetIsInEvent" => Some(Self::GetIsInEvent),
            "GetCurrentEvent" => Some(Self::GetCurrentEvent),
            "GetCurrentEventId" => Some(Self::GetCurrentEventId),
            "GetCurrentEventScript" => Some(Self::GetCurrentEventScript),
            "GetCurrentEventPhase" => Some(Self::GetCurrentEventPhase),
            "GetCurrentEventDepth" => Some(Self::GetCurrentEventDepth),
            _ => None,
        }
    }

    /// Returns the target-pack capability required by this function.
    #[must_use]
    pub const fn required_capability(self) -> Option<Capability> {
        match self {
            Self::GetModuleName
            | Self::GetPlayerCount
            | Self::GetMaxPlayers
            | Self::GetServerPort => Some(Capability::ServerState),
            Self::GetIsInEvent
            | Self::GetCurrentEvent
            | Self::GetCurrentEventId
            | Self::GetCurrentEventScript
            | Self::GetCurrentEventPhase
            | Self::GetCurrentEventDepth => Some(Capability::EventContext),
            _ => None,
        }
    }

    fn preserves_last_error(self) -> bool {
        matches!(self, Self::GetLastErrorCode | Self::GetLastErrorMessage)
    }
}

/// An error produced while dispatching a call from NWScript.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BridgeError {
    code:    BridgeErrorCode,
    message: String,
}

impl BridgeError {
    /// Creates a bridge error with a stable public code.
    #[must_use]
    pub fn new(code: BridgeErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// Returns the stable error code.
    #[must_use]
    pub const fn code(&self) -> BridgeErrorCode {
        self.code
    }

    /// Returns the diagnostic message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
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
    arguments:  Vec<BridgeValue>,
    returns:    Vec<BridgeValue>,
    logs:       Vec<ScriptLog>,
    last_error: Option<BridgeError>,
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
        server: &ServerSnapshot,
        event: &EventContext,
    ) -> BridgeResult<()> {
        self.returns.clear();
        self.logs.clear();
        let parsed = BridgeFunction::from_name(function);
        if !parsed.is_some_and(BridgeFunction::preserves_last_error) {
            self.last_error = None;
        }
        let result = self.dispatch(namespace, function, parsed, context, server, event);
        self.arguments.clear();
        if let Err(error) = &result {
            self.logs.clear();
            self.last_error = Some(error.clone());
        }
        result
    }

    /// Records a native engine failure for NWScript to inspect.
    pub fn record_external_error(&mut self, error: BridgeError) {
        self.returns.clear();
        self.logs.clear();
        self.arguments.clear();
        self.last_error = Some(error);
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
            _ => Err(invalid_argument("return value changed while being popped")),
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
            _ => Err(invalid_argument("return value changed while being popped")),
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
            _ => Err(invalid_argument("return value changed while being popped")),
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
            _ => Err(invalid_argument("return value changed while being popped")),
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
            _ => Err(invalid_argument("return value changed while being popped")),
        }
    }

    fn dispatch(
        &mut self,
        namespace: &str,
        function: &str,
        parsed: Option<BridgeFunction>,
        context: &RuntimeContext,
        server: &ServerSnapshot,
        event: &EventContext,
    ) -> BridgeResult<()> {
        if namespace != NAMESPACE {
            return Err(BridgeError::new(
                BridgeErrorCode::UnknownNamespace,
                format!("unknown NWScript bridge namespace: {namespace}"),
            ));
        }
        let function = parsed.ok_or_else(|| {
            BridgeError::new(
                BridgeErrorCode::UnknownFunction,
                format!("unknown NWScript bridge function: {NAMESPACE}.{function}"),
            )
        })?;
        if let Some(capability) = function.required_capability()
            && context.target.pack.capability_version(capability) == 0
        {
            return Err(BridgeError::new(
                BridgeErrorCode::MissingCapability,
                format!(
                    "target pack does not provide the {} capability",
                    capability.name()
                ),
            ));
        }
        if function == BridgeFunction::Log {
            return self.dispatch_log();
        }
        let value = match function {
            BridgeFunction::GetCapabilityVersion => {
                let capability = self.pop_capability()?;
                BridgeValue::Integer(capability_version(context, capability)?)
            }
            BridgeFunction::HasCapability => {
                let capability = self.pop_capability()?;
                let minimum = match self.pop_argument("integer")? {
                    BridgeValue::Integer(value) if value >= 0 => {
                        u32::try_from(value).map_err(|_error| {
                            invalid_argument("minimum capability version exceeds u32")
                        })?
                    }
                    BridgeValue::Integer(value) => {
                        return Err(invalid_argument(format!(
                            "minimum capability version cannot be {value}"
                        )));
                    }
                    _ => return Err(invalid_argument("minimum capability version changed type")),
                };
                BridgeValue::Integer(i32::from(
                    context.target.pack.capability_version(capability) >= minimum,
                ))
            }
            BridgeFunction::GetLastErrorCode => BridgeValue::Integer(
                self.last_error
                    .as_ref()
                    .map_or(BridgeErrorCode::None.value(), |error| error.code().value()),
            ),
            BridgeFunction::GetLastErrorMessage => BridgeValue::String(
                self.last_error
                    .as_ref()
                    .map_or_else(Vec::new, |error| error.message().as_bytes().to_vec()),
            ),
            BridgeFunction::GetApiVersion => {
                BridgeValue::Integer(i32::try_from(RUNTIME_API_VERSION).map_err(|_error| {
                    invalid_argument("runtime API version exceeds NWScript integer range")
                })?)
            }
            BridgeFunction::GetRuntimeVersion => string_value(env!("CARGO_PKG_VERSION")),
            BridgeFunction::GetServerBinarySha256 => {
                string_value(&context.server.sha256.to_string())
            }
            BridgeFunction::GetServerBuild => string_value(
                context
                    .target
                    .pack
                    .server
                    .build
                    .as_deref()
                    .unwrap_or_default(),
            ),
            BridgeFunction::GetServerPlatform => string_value(&context.server.platform.to_string()),
            BridgeFunction::GetServerOperatingSystem => {
                string_value(&context.server.platform.os.to_string())
            }
            BridgeFunction::GetServerArchitecture => {
                string_value(&context.server.platform.architecture.to_string())
            }
            BridgeFunction::GetModuleName => BridgeValue::String(server.module_name.clone()),
            BridgeFunction::GetPlayerCount => BridgeValue::Integer(server.player_count),
            BridgeFunction::GetMaxPlayers => BridgeValue::Integer(server.max_players),
            BridgeFunction::GetServerPort => BridgeValue::Integer(server.udp_port),
            BridgeFunction::GetIsInEvent => BridgeValue::Integer(i32::from(event.depth > 0)),
            BridgeFunction::GetCurrentEvent => string_value(&event.name),
            BridgeFunction::GetCurrentEventId => BridgeValue::Integer(event.id),
            BridgeFunction::GetCurrentEventScript => BridgeValue::String(event.script_name.clone()),
            BridgeFunction::GetCurrentEventPhase => string_value(&event.phase),
            BridgeFunction::GetCurrentEventDepth => BridgeValue::Integer(event.depth),
            BridgeFunction::Log => unreachable!("logging dispatched before value matching"),
        };
        if !self.arguments.is_empty() {
            return Err(invalid_argument(format!(
                "{NAMESPACE}.{} received too many arguments",
                function.name()
            )));
        }
        self.returns.push(value);
        Ok(())
    }

    fn dispatch_log(&mut self) -> BridgeResult<()> {
        let message = match self.pop_argument("string")? {
            BridgeValue::String(value) => value,
            _ => return Err(invalid_argument("log message changed while being popped")),
        };
        let level = match self.pop_argument("integer")? {
            BridgeValue::Integer(value) => ScriptLogLevel::try_from(value)?,
            _ => return Err(invalid_argument("log level changed while being popped")),
        };
        if !self.arguments.is_empty() {
            return Err(invalid_argument(format!(
                "{NAMESPACE}.Log received too many arguments"
            )));
        }
        if message.len() > MAX_LOG_MESSAGE_BYTES {
            return Err(invalid_argument(format!(
                "{NAMESPACE}.Log message exceeds {MAX_LOG_MESSAGE_BYTES} bytes"
            )));
        }
        self.logs.push(ScriptLog {
            level,
            message,
        });
        Ok(())
    }

    fn pop_capability(&mut self) -> BridgeResult<Capability> {
        let name = match self.pop_argument("string")? {
            BridgeValue::String(value) => String::from_utf8(value)
                .map_err(|_error| invalid_argument("capability name is not UTF-8"))?,
            _ => return Err(invalid_argument("capability name changed type")),
        };
        Capability::from_name(&name)
            .ok_or_else(|| invalid_argument(format!("unknown target-pack capability: {name}")))
    }

    fn pop_argument(&mut self, expected: &'static str) -> BridgeResult<BridgeValue> {
        let value = self
            .arguments
            .pop()
            .ok_or_else(|| invalid_argument(format!("missing {expected} argument")))?;
        if value.kind() != expected {
            return Err(invalid_argument(format!(
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
            .ok_or_else(|| invalid_argument(format!("no {expected} return value is available")))?;
        if value.kind() != expected {
            return Err(invalid_argument(format!(
                "expected {expected} return value, found {}",
                value.kind()
            )));
        }
        Ok(value)
    }
}

fn invalid_argument(message: impl Into<String>) -> BridgeError {
    BridgeError::new(BridgeErrorCode::InvalidArgument, message)
}

fn capability_version(context: &RuntimeContext, capability: Capability) -> BridgeResult<i32> {
    i32::try_from(context.target.pack.capability_version(capability)).map_err(|_error| {
        invalid_argument(format!(
            "{} capability version exceeds NWScript integer range",
            capability.name()
        ))
    })
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
        BridgeErrorCode, BridgeValue, EventContext, ScriptBridge, ScriptLog, ScriptLogLevel,
        ServerSnapshot, Vector,
    };
    use crate::{
        AbiLayouts, Architecture, BinaryIdentity, BridgeTarget, CExoStringLayout,
        EVENT_CONTEXT_CAPABILITY_VERSION, EngineClassLayouts, EventTarget, FileSha256,
        NWSCRIPT_BRIDGE_CAPABILITY_VERSION, OperatingSystem, Platform, PlayerListLayout,
        RUNTIME_API_VERSION, RuntimeContext, SERVER_STATE_CAPABILITY_VERSION, SelectedTargetPack,
        TARGET_PACK_SCHEMA_VERSION, TargetAddress, TargetPack, TargetServer, TargetSource,
        VectorLayout,
    };

    #[test]
    fn dispatches_server_identity_and_clears_failed_calls() -> Result<(), Box<dyn std::error::Error>>
    {
        let context = context();
        let server = ServerSnapshot {
            module_name:  b"fixture-module".to_vec(),
            player_count: 2,
            max_players:  64,
            udp_port:     5121,
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
        bridge.call("NWNRS", "GetServerPort", &context, &server, &event)?;
        assert_eq!(bridge.pop_integer()?, 5121);
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
            &ServerSnapshot::default(),
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
                    &ServerSnapshot::default(),
                    &EventContext::default(),
                )
                .is_err()
        );
        assert!(bridge.take_logs().is_empty());
        Ok(())
    }

    #[test]
    fn exposes_versions_capabilities_and_stable_errors() -> Result<(), Box<dyn std::error::Error>> {
        let mut context = context();
        let mut bridge = ScriptBridge::default();
        let server = ServerSnapshot::default();
        let event = EventContext::default();

        bridge.call("NWNRS", "GetApiVersion", &context, &server, &event)?;
        assert_eq!(bridge.pop_integer()?, 1);

        bridge.push_argument(BridgeValue::String(b"server_state".to_vec()));
        bridge.call("NWNRS", "GetCapabilityVersion", &context, &server, &event)?;
        assert_eq!(bridge.pop_integer()?, 1);

        let error = bridge
            .call("NWNRS", "NotRegistered", &context, &server, &event)
            .expect_err("unknown function must fail");
        assert_eq!(error.code(), BridgeErrorCode::UnknownFunction);
        bridge.call("NWNRS", "GetLastErrorCode", &context, &server, &event)?;
        assert_eq!(
            bridge.pop_integer()?,
            BridgeErrorCode::UnknownFunction.value()
        );
        bridge.call("NWNRS", "GetLastErrorMessage", &context, &server, &event)?;
        assert!(String::from_utf8(bridge.pop_string()?)?.contains("NotRegistered"));

        context.target.pack.server_state = None;
        let error = bridge
            .call("NWNRS", "GetModuleName", &context, &server, &event)
            .expect_err("missing optional capability must fail");
        assert_eq!(error.code(), BridgeErrorCode::MissingCapability);
        Ok(())
    }

    #[test]
    fn public_nwscript_header_matches_the_rust_contract() {
        let header = include_str!("../../../module/nwnrs.nss");
        assert!(header.contains(&format!(
            "const int NWNRS_API_VERSION = {RUNTIME_API_VERSION};"
        )));
        for function in super::BridgeFunction::ALL {
            assert_eq!(
                super::BridgeFunction::from_name(function.name()),
                Some(function)
            );
            assert!(header.contains(function.name()));
        }
        for (name, version) in [
            ("NWSCRIPT_BRIDGE", NWSCRIPT_BRIDGE_CAPABILITY_VERSION),
            ("SERVER_STATE", SERVER_STATE_CAPABILITY_VERSION),
            ("EVENT_CONTEXT", EVENT_CONTEXT_CAPABILITY_VERSION),
        ] {
            assert_eq!(version, 1);
            assert!(header.contains(&format!("NWNRS_CAPABILITY_{name}")));
        }
        for (name, code) in [
            ("NONE", BridgeErrorCode::None),
            ("UNKNOWN_NAMESPACE", BridgeErrorCode::UnknownNamespace),
            ("UNKNOWN_FUNCTION", BridgeErrorCode::UnknownFunction),
            ("INVALID_ARGUMENT", BridgeErrorCode::InvalidArgument),
            ("MISSING_CAPABILITY", BridgeErrorCode::MissingCapability),
            ("ENGINE", BridgeErrorCode::Engine),
            ("REENTRANT", BridgeErrorCode::Reentrant),
        ] {
            assert!(header.contains(&format!("NWNRS_ERROR_{name} = {};", code.value())));
        }
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
                    source:         source(),
                    layouts:        layouts(),
                    bridge:         bridge_target(),
                    server_state:   Some(server_state_target()),
                    events:         Some(event_target()),
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
            version:                NWSCRIPT_BRIDGE_CAPABILITY_VERSION,
            function_management:    address(),
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
            version:                 SERVER_STATE_CAPABILITY_VERSION,
            app_manager:             address(),
            get_server_info:         address(),
            get_player_list:         address(),
            get_net_layer:           address(),
            get_session_max_players: address(),
            get_udp_port:            address(),
        }
    }

    fn event_target() -> EventTarget {
        EventTarget {
            version: EVENT_CONTEXT_CAPABILITY_VERSION,
        }
    }

    fn source() -> TargetSource {
        TargetSource {
            unified_commit: "3d4c4e13c6bf01b032ffe90534fc4a19eb036c03".to_string(),
            nwn_build:      8193,
            nwn_revision:   37,
            nwn_postfix:    17,
        }
    }

    fn layouts() -> AbiLayouts {
        AbiLayouts {
            c_exo_string: CExoStringLayout {
                size:                 16,
                alignment:            8,
                string_offset:        0,
                string_length_offset: 8,
                buffer_length_offset: 12,
            },
            player_list:  PlayerListLayout {
                size:            16,
                alignment:       8,
                elements_offset: 0,
                count_offset:    8,
                capacity_offset: 12,
            },
            vector:       VectorLayout {
                size:      12,
                alignment: 4,
                x_offset:  0,
                y_offset:  4,
                z_offset:  8,
            },
            classes:      EngineClassLayouts {
                command_implementer_vm_offset: 0,
                app_manager_server_offset:     8,
                server_info_module_offset:     8,
                vm_recursion_level_offset:     36,
                vm_script_array_offset:        40,
                vm_script_slot_count:          8,
                vm_script_size:                152,
                vm_script_alignment:           8,
                vm_script_name_offset:         24,
                vm_script_event_id_offset:     72,
            },
        }
    }
}
