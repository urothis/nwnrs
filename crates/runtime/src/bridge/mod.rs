use crate::{Capability, RUNTIME_API_VERSION, RuntimeContext};

mod error;
mod host;
mod value;

pub use error::{BridgeError, BridgeErrorCode, BridgeResult};
pub use host::{
    AdministrationCommand, BannedLists, EventCommand, EventControls, EventLocation, EventObjectId,
    EventPayload, EventValue, EventVector, HostCommandResult, HostQuery, HostValue, RuntimeHost,
};
pub use value::{BridgeValue, ScriptLog, ScriptLogLevel, Vector};

const NAMESPACE: &str = "NWNRS";
const MAX_LOG_MESSAGE_BYTES: usize = 64 * 1024;
const MAX_ADMIN_STRING_BYTES: usize = 64 * 1024;
const MAX_EVENT_JSON_BYTES: usize = 64 * 1024;
const OBJECT_INVALID: u32 = 0x7f00_0000;

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
    /// Returns the advertised server name.
    GetServerName,
    /// Changes the advertised server name.
    SetServerName,
    /// Changes the active module name.
    SetModuleName,
    /// Reports whether a player password is configured.
    GetIsPlayerPasswordSet,
    /// Changes the player password.
    SetPlayerPassword,
    /// Clears the player password.
    ClearPlayerPassword,
    /// Reports whether a DM password is configured.
    GetIsDmPasswordSet,
    /// Changes the DM password.
    SetDmPassword,
    /// Clears the DM password.
    ClearDmPassword,
    /// Returns the minimum permitted character level.
    GetMinLevel,
    /// Changes the minimum permitted character level.
    SetMinLevel,
    /// Returns the maximum permitted character level.
    GetMaxLevel,
    /// Changes the maximum permitted character level.
    SetMaxLevel,
    /// Returns one active play option.
    GetPlayOption,
    /// Changes one active play option.
    SetPlayOption,
    /// Returns one engine debug toggle.
    GetDebugValue,
    /// Changes one engine debug toggle.
    SetDebugValue,
    /// Requests graceful server shutdown.
    RequestShutdown,
    /// Returns all three ban lists as JSON.
    GetBannedList,
    /// Adds an IP address to the ban list.
    AddBannedIp,
    /// Removes an IP address from the ban list.
    RemoveBannedIp,
    /// Adds a public CD key to the ban list.
    AddBannedCdKey,
    /// Removes a public CD key from the ban list.
    RemoveBannedCdKey,
    /// Adds a player account name to the ban list.
    AddBannedPlayerName,
    /// Removes a player account name from the ban list.
    RemoveBannedPlayerName,
    /// Reloads engine rules tables.
    ReloadRules,
    /// Disconnects a player and removes their active server-vault character.
    DeletePlayerCharacter,
    /// Deletes one stored player TURD by account and character name.
    DeleteTurd,
    /// Reports whether an engine event is active.
    GetIsInEvent,
    /// Returns the complete current event as serialized JSON.
    GetCurrentEvent,
    /// Skips the current event when its schema permits it.
    SkipCurrentEvent,
    /// Sets the current event's JSON result when its schema permits it.
    SetCurrentEventResult,
}

impl BridgeFunction {
    /// Complete registry in stable declaration order.
    ///
    /// ```
    /// assert!(nwnrs_runtime::BridgeFunction::ALL
    ///     .contains(&nwnrs_runtime::BridgeFunction::Log));
    /// ```
    pub const ALL: [Self; 48] = [
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
        Self::GetServerName,
        Self::SetServerName,
        Self::SetModuleName,
        Self::GetIsPlayerPasswordSet,
        Self::SetPlayerPassword,
        Self::ClearPlayerPassword,
        Self::GetIsDmPasswordSet,
        Self::SetDmPassword,
        Self::ClearDmPassword,
        Self::GetMinLevel,
        Self::SetMinLevel,
        Self::GetMaxLevel,
        Self::SetMaxLevel,
        Self::GetPlayOption,
        Self::SetPlayOption,
        Self::GetDebugValue,
        Self::SetDebugValue,
        Self::RequestShutdown,
        Self::GetBannedList,
        Self::AddBannedIp,
        Self::RemoveBannedIp,
        Self::AddBannedCdKey,
        Self::RemoveBannedCdKey,
        Self::AddBannedPlayerName,
        Self::RemoveBannedPlayerName,
        Self::ReloadRules,
        Self::DeletePlayerCharacter,
        Self::DeleteTurd,
        Self::GetIsInEvent,
        Self::GetCurrentEvent,
        Self::SkipCurrentEvent,
        Self::SetCurrentEventResult,
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
            Self::GetServerName => "GetServerName",
            Self::SetServerName => "SetServerName",
            Self::SetModuleName => "SetModuleName",
            Self::GetIsPlayerPasswordSet => "GetIsPlayerPasswordSet",
            Self::SetPlayerPassword => "SetPlayerPassword",
            Self::ClearPlayerPassword => "ClearPlayerPassword",
            Self::GetIsDmPasswordSet => "GetIsDmPasswordSet",
            Self::SetDmPassword => "SetDmPassword",
            Self::ClearDmPassword => "ClearDmPassword",
            Self::GetMinLevel => "GetMinLevel",
            Self::SetMinLevel => "SetMinLevel",
            Self::GetMaxLevel => "GetMaxLevel",
            Self::SetMaxLevel => "SetMaxLevel",
            Self::GetPlayOption => "GetPlayOption",
            Self::SetPlayOption => "SetPlayOption",
            Self::GetDebugValue => "GetDebugValue",
            Self::SetDebugValue => "SetDebugValue",
            Self::RequestShutdown => "RequestShutdown",
            Self::GetBannedList => "GetBannedList",
            Self::AddBannedIp => "AddBannedIp",
            Self::RemoveBannedIp => "RemoveBannedIp",
            Self::AddBannedCdKey => "AddBannedCdKey",
            Self::RemoveBannedCdKey => "RemoveBannedCdKey",
            Self::AddBannedPlayerName => "AddBannedPlayerName",
            Self::RemoveBannedPlayerName => "RemoveBannedPlayerName",
            Self::ReloadRules => "ReloadRules",
            Self::DeletePlayerCharacter => "DeletePlayerCharacter",
            Self::DeleteTurd => "DeleteTURD",
            Self::GetIsInEvent => "GetIsInEvent",
            Self::GetCurrentEvent => "GetCurrentEvent",
            Self::SkipCurrentEvent => "SkipCurrentEvent",
            Self::SetCurrentEventResult => "SetCurrentEventResult",
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
            "GetServerName" => Some(Self::GetServerName),
            "SetServerName" => Some(Self::SetServerName),
            "SetModuleName" => Some(Self::SetModuleName),
            "GetIsPlayerPasswordSet" => Some(Self::GetIsPlayerPasswordSet),
            "SetPlayerPassword" => Some(Self::SetPlayerPassword),
            "ClearPlayerPassword" => Some(Self::ClearPlayerPassword),
            "GetIsDmPasswordSet" => Some(Self::GetIsDmPasswordSet),
            "SetDmPassword" => Some(Self::SetDmPassword),
            "ClearDmPassword" => Some(Self::ClearDmPassword),
            "GetMinLevel" => Some(Self::GetMinLevel),
            "SetMinLevel" => Some(Self::SetMinLevel),
            "GetMaxLevel" => Some(Self::GetMaxLevel),
            "SetMaxLevel" => Some(Self::SetMaxLevel),
            "GetPlayOption" => Some(Self::GetPlayOption),
            "SetPlayOption" => Some(Self::SetPlayOption),
            "GetDebugValue" => Some(Self::GetDebugValue),
            "SetDebugValue" => Some(Self::SetDebugValue),
            "RequestShutdown" => Some(Self::RequestShutdown),
            "GetBannedList" => Some(Self::GetBannedList),
            "AddBannedIp" => Some(Self::AddBannedIp),
            "RemoveBannedIp" => Some(Self::RemoveBannedIp),
            "AddBannedCdKey" => Some(Self::AddBannedCdKey),
            "RemoveBannedCdKey" => Some(Self::RemoveBannedCdKey),
            "AddBannedPlayerName" => Some(Self::AddBannedPlayerName),
            "RemoveBannedPlayerName" => Some(Self::RemoveBannedPlayerName),
            "ReloadRules" => Some(Self::ReloadRules),
            "DeletePlayerCharacter" => Some(Self::DeletePlayerCharacter),
            "DeleteTURD" => Some(Self::DeleteTurd),
            "GetIsInEvent" => Some(Self::GetIsInEvent),
            "GetCurrentEvent" => Some(Self::GetCurrentEvent),
            "SkipCurrentEvent" => Some(Self::SkipCurrentEvent),
            "SetCurrentEventResult" => Some(Self::SetCurrentEventResult),
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
            Self::GetServerName
            | Self::SetServerName
            | Self::SetModuleName
            | Self::GetIsPlayerPasswordSet
            | Self::SetPlayerPassword
            | Self::ClearPlayerPassword
            | Self::GetIsDmPasswordSet
            | Self::SetDmPassword
            | Self::ClearDmPassword
            | Self::GetMinLevel
            | Self::SetMinLevel
            | Self::GetMaxLevel
            | Self::SetMaxLevel
            | Self::GetPlayOption
            | Self::SetPlayOption
            | Self::GetDebugValue
            | Self::SetDebugValue
            | Self::RequestShutdown => Some(Capability::Administration),
            Self::GetBannedList
            | Self::AddBannedIp
            | Self::RemoveBannedIp
            | Self::AddBannedCdKey
            | Self::RemoveBannedCdKey
            | Self::AddBannedPlayerName
            | Self::RemoveBannedPlayerName
            | Self::ReloadRules
            | Self::DeletePlayerCharacter
            | Self::DeleteTurd => Some(Capability::Administration),
            Self::GetIsInEvent
            | Self::GetCurrentEvent
            | Self::SkipCurrentEvent
            | Self::SetCurrentEventResult => Some(Capability::Events),
            _ => None,
        }
    }

    fn preserves_last_error(self) -> bool {
        matches!(self, Self::GetLastErrorCode | Self::GetLastErrorMessage)
    }

    const fn argument_count(self) -> usize {
        match self {
            Self::HasCapability
            | Self::Log
            | Self::SetPlayOption
            | Self::SetDebugValue
            | Self::DeleteTurd => 2,
            Self::DeletePlayerCharacter => 3,
            Self::GetCapabilityVersion
            | Self::SetServerName
            | Self::SetModuleName
            | Self::SetPlayerPassword
            | Self::SetDmPassword
            | Self::SetMinLevel
            | Self::SetMaxLevel
            | Self::GetPlayOption
            | Self::GetDebugValue
            | Self::AddBannedIp
            | Self::RemoveBannedIp
            | Self::AddBannedCdKey
            | Self::RemoveBannedCdKey
            | Self::AddBannedPlayerName
            | Self::RemoveBannedPlayerName
            | Self::SetCurrentEventResult => 1,
            _ => 0,
        }
    }
}

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
        host: &mut impl RuntimeHost,
    ) -> BridgeResult<()> {
        self.returns.clear();
        self.logs.clear();
        let parsed = BridgeFunction::from_name(function);
        if !parsed.is_some_and(BridgeFunction::preserves_last_error) {
            self.last_error = None;
        }
        let result = self.dispatch(namespace, function, parsed, context, host);
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
        host: &mut impl RuntimeHost,
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
        if self.arguments.len() > function.argument_count() {
            return Err(invalid_argument(format!(
                "{NAMESPACE}.{} received too many arguments",
                function.name()
            )));
        }
        if function == BridgeFunction::Log {
            return self.dispatch_log();
        }
        let value = match function {
            BridgeFunction::GetCapabilityVersion => {
                let capability = self.pop_capability()?;
                Some(BridgeValue::Integer(capability_version(
                    context, capability,
                )?))
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
                Some(BridgeValue::Integer(i32::from(
                    context.target.pack.capability_version(capability) >= minimum,
                )))
            }
            BridgeFunction::GetLastErrorCode => Some(BridgeValue::Integer(
                self.last_error
                    .as_ref()
                    .map_or(BridgeErrorCode::None.value(), |error| error.code().value()),
            )),
            BridgeFunction::GetLastErrorMessage => Some(BridgeValue::String(
                self.last_error
                    .as_ref()
                    .map_or_else(Vec::new, |error| error.message().as_bytes().to_vec()),
            )),
            BridgeFunction::GetApiVersion => Some(BridgeValue::Integer(
                i32::try_from(RUNTIME_API_VERSION).map_err(|_error| {
                    invalid_argument("runtime API version exceeds NWScript integer range")
                })?,
            )),
            BridgeFunction::GetRuntimeVersion => Some(string_value(env!("CARGO_PKG_VERSION"))),
            BridgeFunction::GetServerBinarySha256 => {
                Some(string_value(&context.server.sha256.to_string()))
            }
            BridgeFunction::GetServerBuild => Some(string_value(
                context
                    .target
                    .pack
                    .server
                    .build
                    .as_deref()
                    .unwrap_or_default(),
            )),
            BridgeFunction::GetServerPlatform => {
                Some(string_value(&context.server.platform.to_string()))
            }
            BridgeFunction::GetServerOperatingSystem => {
                Some(string_value(&context.server.platform.os.to_string()))
            }
            BridgeFunction::GetServerArchitecture => Some(string_value(
                &context.server.platform.architecture.to_string(),
            )),
            BridgeFunction::GetModuleName => Some(BridgeValue::String(host_string(
                host,
                HostQuery::ModuleName,
            )?)),
            BridgeFunction::GetPlayerCount => Some(BridgeValue::Integer(host_integer(
                host,
                HostQuery::PlayerCount,
            )?)),
            BridgeFunction::GetMaxPlayers => Some(BridgeValue::Integer(host_integer(
                host,
                HostQuery::MaxPlayers,
            )?)),
            BridgeFunction::GetServerPort => Some(BridgeValue::Integer(host_integer(
                host,
                HostQuery::UdpPort,
            )?)),
            BridgeFunction::GetServerName => Some(BridgeValue::String(host_string(
                host,
                HostQuery::ServerName,
            )?)),
            BridgeFunction::SetServerName => {
                let name = self.pop_string_argument("server name")?;
                self.ensure_no_arguments(function)?;
                host.execute(AdministrationCommand::SetServerName(name))?;
                None
            }
            BridgeFunction::SetModuleName => {
                let name = self.pop_string_argument("module name")?;
                self.ensure_no_arguments(function)?;
                host.execute(AdministrationCommand::SetModuleName(name))?;
                None
            }
            BridgeFunction::GetIsPlayerPasswordSet => Some(BridgeValue::Integer(i32::from(
                host_boolean(host, HostQuery::PlayerPasswordIsSet)?,
            ))),
            BridgeFunction::SetPlayerPassword => {
                let password = self.pop_string_argument("player password")?;
                self.ensure_no_arguments(function)?;
                host.execute(AdministrationCommand::SetPlayerPassword(password))?;
                None
            }
            BridgeFunction::ClearPlayerPassword => {
                self.ensure_no_arguments(function)?;
                host.execute(AdministrationCommand::SetPlayerPassword(Vec::new()))?;
                None
            }
            BridgeFunction::GetIsDmPasswordSet => Some(BridgeValue::Integer(i32::from(
                host_boolean(host, HostQuery::DmPasswordIsSet)?,
            ))),
            BridgeFunction::SetDmPassword => {
                let password = self.pop_string_argument("DM password")?;
                self.ensure_no_arguments(function)?;
                host.execute(AdministrationCommand::SetDmPassword(password))?;
                None
            }
            BridgeFunction::ClearDmPassword => {
                self.ensure_no_arguments(function)?;
                host.execute(AdministrationCommand::SetDmPassword(Vec::new()))?;
                None
            }
            BridgeFunction::GetMinLevel => Some(BridgeValue::Integer(host_integer(
                host,
                HostQuery::MinLevel,
            )?)),
            BridgeFunction::SetMinLevel => {
                let level = self.pop_integer_argument("minimum level")?;
                validate_level(level)?;
                self.ensure_no_arguments(function)?;
                host.execute(AdministrationCommand::SetMinLevel(level))?;
                None
            }
            BridgeFunction::GetMaxLevel => Some(BridgeValue::Integer(host_integer(
                host,
                HostQuery::MaxLevel,
            )?)),
            BridgeFunction::SetMaxLevel => {
                let level = self.pop_integer_argument("maximum level")?;
                validate_level(level)?;
                self.ensure_no_arguments(function)?;
                host.execute(AdministrationCommand::SetMaxLevel(level))?;
                None
            }
            BridgeFunction::GetPlayOption => {
                let option = self.pop_integer_argument("play option")?;
                let _index = play_option_index(option)?;
                Some(BridgeValue::Integer(host_integer(
                    host,
                    HostQuery::PlayOption(option),
                )?))
            }
            BridgeFunction::SetPlayOption => {
                let option = self.pop_integer_argument("play option")?;
                let value = self.pop_integer_argument("play option value")?;
                validate_play_option(option, value)?;
                self.ensure_no_arguments(function)?;
                host.execute(AdministrationCommand::SetPlayOption {
                    option,
                    value,
                })?;
                None
            }
            BridgeFunction::GetDebugValue => {
                let debug_type = self.pop_integer_argument("debug type")?;
                let _index = debug_value_index(debug_type)?;
                Some(BridgeValue::Integer(host_integer(
                    host,
                    HostQuery::DebugValue(debug_type),
                )?))
            }
            BridgeFunction::SetDebugValue => {
                let debug_type = self.pop_integer_argument("debug type")?;
                let value = self.pop_boolean_argument("debug value")?;
                let _index = debug_value_index(debug_type)?;
                self.ensure_no_arguments(function)?;
                host.execute(AdministrationCommand::SetDebugValue {
                    debug_type,
                    value,
                })?;
                None
            }
            BridgeFunction::RequestShutdown => {
                self.ensure_no_arguments(function)?;
                host.execute(AdministrationCommand::RequestShutdown)?;
                None
            }
            BridgeFunction::GetBannedList => Some(BridgeValue::String(banned_list_json(
                host_banned_lists(host)?,
            )?)),
            BridgeFunction::AddBannedIp => {
                let value = self.pop_nonempty_string_argument("banned IP address")?;
                self.ensure_no_arguments(function)?;
                host.execute(AdministrationCommand::AddBannedIp(value))?;
                None
            }
            BridgeFunction::RemoveBannedIp => {
                let value = self.pop_nonempty_string_argument("banned IP address")?;
                self.ensure_no_arguments(function)?;
                host.execute(AdministrationCommand::RemoveBannedIp(value))?;
                None
            }
            BridgeFunction::AddBannedCdKey => {
                let value = self.pop_nonempty_string_argument("banned CD key")?;
                self.ensure_no_arguments(function)?;
                host.execute(AdministrationCommand::AddBannedCdKey(value))?;
                None
            }
            BridgeFunction::RemoveBannedCdKey => {
                let value = self.pop_nonempty_string_argument("banned CD key")?;
                self.ensure_no_arguments(function)?;
                host.execute(AdministrationCommand::RemoveBannedCdKey(value))?;
                None
            }
            BridgeFunction::AddBannedPlayerName => {
                let value = self.pop_nonempty_string_argument("banned player name")?;
                self.ensure_no_arguments(function)?;
                host.execute(AdministrationCommand::AddBannedPlayerName(value))?;
                None
            }
            BridgeFunction::RemoveBannedPlayerName => {
                let value = self.pop_nonempty_string_argument("banned player name")?;
                self.ensure_no_arguments(function)?;
                host.execute(AdministrationCommand::RemoveBannedPlayerName(value))?;
                None
            }
            BridgeFunction::ReloadRules => {
                self.ensure_no_arguments(function)?;
                host.execute(AdministrationCommand::ReloadRules)?;
                None
            }
            BridgeFunction::DeletePlayerCharacter => {
                let object_id = self.pop_player_object_argument()?;
                let preserve_backup = self.pop_boolean_argument("preserve backup")? != 0;
                let kick_message = self.pop_string_argument("kick message")?;
                self.ensure_no_arguments(function)?;
                host.execute(AdministrationCommand::DeletePlayerCharacter {
                    object_id,
                    preserve_backup,
                    kick_message,
                })?;
                None
            }
            BridgeFunction::DeleteTurd => {
                let player_name = self.pop_nonempty_string_argument("player community name")?;
                let character_name = self.pop_nonempty_string_argument("character name")?;
                self.ensure_no_arguments(function)?;
                let deleted = match host.execute(AdministrationCommand::DeleteTurd {
                    player_name,
                    character_name,
                })? {
                    HostCommandResult::Boolean(value) => value,
                    HostCommandResult::None => {
                        return Err(BridgeError::new(
                            BridgeErrorCode::Engine,
                            "runtime host returned no result for DeleteTURD",
                        ));
                    }
                };
                Some(BridgeValue::Integer(i32::from(deleted)))
            }
            BridgeFunction::GetIsInEvent => Some(BridgeValue::Integer(i32::from(
                host_current_event(host)?.is_some(),
            ))),
            BridgeFunction::GetCurrentEvent => Some(BridgeValue::String(current_event_json(host)?)),
            BridgeFunction::SkipCurrentEvent => {
                self.ensure_no_arguments(function)?;
                host.control_event(EventCommand::Skip)?;
                None
            }
            BridgeFunction::SetCurrentEventResult => {
                let result = self.pop_string_argument("event result")?;
                self.ensure_no_arguments(function)?;
                host.control_event(EventCommand::SetResult(validate_event_json(result)?))?;
                None
            }
            BridgeFunction::Log => unreachable!("logging dispatched before value matching"),
        };
        if !self.arguments.is_empty() {
            return Err(invalid_argument(format!(
                "{NAMESPACE}.{} received too many arguments",
                function.name()
            )));
        }
        if let Some(value) = value {
            self.returns.push(value);
        }
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

    fn pop_integer_argument(&mut self, name: &'static str) -> BridgeResult<i32> {
        match self.pop_argument("integer")? {
            BridgeValue::Integer(value) => Ok(value),
            _ => Err(invalid_argument(format!("{name} changed type"))),
        }
    }

    fn pop_boolean_argument(&mut self, name: &'static str) -> BridgeResult<i32> {
        let value = self.pop_integer_argument(name)?;
        if matches!(value, 0 | 1) {
            Ok(value)
        } else {
            Err(invalid_argument(format!("{name} must be FALSE or TRUE")))
        }
    }

    fn pop_string_argument(&mut self, name: &'static str) -> BridgeResult<Vec<u8>> {
        match self.pop_argument("string")? {
            BridgeValue::String(value) if value.as_slice().contains(&0) => Err(invalid_argument(
                format!("{name} cannot contain a NUL byte"),
            )),
            BridgeValue::String(value) if value.len() > MAX_ADMIN_STRING_BYTES => Err(
                invalid_argument(format!("{name} exceeds {MAX_ADMIN_STRING_BYTES} bytes")),
            ),
            BridgeValue::String(value) => Ok(value),
            _ => Err(invalid_argument(format!("{name} changed type"))),
        }
    }

    fn pop_player_object_argument(&mut self) -> BridgeResult<u32> {
        match self.pop_argument("object")? {
            BridgeValue::Object(OBJECT_INVALID) => {
                Err(invalid_argument("player object cannot be OBJECT_INVALID"))
            }
            BridgeValue::Object(value) => Ok(value),
            _ => Err(invalid_argument("player object changed type")),
        }
    }

    fn pop_nonempty_string_argument(&mut self, name: &'static str) -> BridgeResult<Vec<u8>> {
        let value = self.pop_string_argument(name)?;
        if value.is_empty() {
            Err(invalid_argument(format!("{name} cannot be empty")))
        } else {
            Ok(value)
        }
    }

    fn ensure_no_arguments(&self, function: BridgeFunction) -> BridgeResult<()> {
        if self.arguments.is_empty() {
            Ok(())
        } else {
            Err(invalid_argument(format!(
                "{NAMESPACE}.{} received too many arguments",
                function.name()
            )))
        }
    }
}

fn invalid_argument(message: impl Into<String>) -> BridgeError {
    BridgeError::new(BridgeErrorCode::InvalidArgument, message)
}

fn validate_level(level: i32) -> BridgeResult<()> {
    if (1..=255).contains(&level) {
        Ok(())
    } else {
        Err(invalid_argument(format!(
            "character level must be between 1 and 255, found {level}"
        )))
    }
}

fn play_option_index(option: i32) -> BridgeResult<usize> {
    let normalized = option
        .checked_sub(10)
        .ok_or_else(|| invalid_argument(format!("unsupported play option {option}")))?;
    usize::try_from(normalized)
        .ok()
        .filter(|index| *index < 19)
        .ok_or_else(|| invalid_argument(format!("unsupported play option {option}")))
}

fn validate_play_option(option: i32, value: i32) -> BridgeResult<()> {
    let _index = play_option_index(option)?;
    if option == 10 {
        if (0..=2).contains(&value) {
            return Ok(());
        }
        return Err(invalid_argument(format!(
            "PVP setting must be between 0 and 2, found {value}"
        )));
    }
    if matches!(value, 0 | 1) {
        Ok(())
    } else {
        Err(invalid_argument(format!(
            "boolean play option {option} must be FALSE or TRUE"
        )))
    }
}

fn debug_value_index(debug_type: i32) -> BridgeResult<usize> {
    usize::try_from(debug_type)
        .ok()
        .filter(|index| *index < 4)
        .ok_or_else(|| invalid_argument(format!("unsupported debug type {debug_type}")))
}

fn host_string(host: &mut impl RuntimeHost, query: HostQuery) -> BridgeResult<Vec<u8>> {
    match host.query(query)? {
        HostValue::String(value) => Ok(value),
        _ => Err(unexpected_host_value(query, "string")),
    }
}

fn host_integer(host: &mut impl RuntimeHost, query: HostQuery) -> BridgeResult<i32> {
    match host.query(query)? {
        HostValue::Integer(value) => Ok(value),
        _ => Err(unexpected_host_value(query, "integer")),
    }
}

fn host_boolean(host: &mut impl RuntimeHost, query: HostQuery) -> BridgeResult<bool> {
    match host.query(query)? {
        HostValue::Boolean(value) => Ok(value),
        _ => Err(unexpected_host_value(query, "boolean")),
    }
}

fn host_banned_lists(host: &mut impl RuntimeHost) -> BridgeResult<BannedLists> {
    match host.query(HostQuery::BannedLists)? {
        HostValue::BannedLists(value) => Ok(value),
        _ => Err(unexpected_host_value(HostQuery::BannedLists, "ban lists")),
    }
}

fn host_current_event(host: &mut impl RuntimeHost) -> BridgeResult<Option<EventPayload>> {
    match host.query(HostQuery::CurrentEvent)? {
        HostValue::CurrentEvent(value) => Ok(value),
        _ => Err(unexpected_host_value(
            HostQuery::CurrentEvent,
            "current event",
        )),
    }
}

fn current_event_json(host: &mut impl RuntimeHost) -> BridgeResult<Vec<u8>> {
    let json = serde_json::to_vec(&host_current_event(host)?).map_err(|error| {
        BridgeError::new(
            BridgeErrorCode::Engine,
            format!("failed to serialize current event: {error}"),
        )
    })?;
    if json.len() > MAX_EVENT_JSON_BYTES {
        return Err(BridgeError::new(
            BridgeErrorCode::Engine,
            format!("current event JSON exceeds {MAX_EVENT_JSON_BYTES} bytes"),
        ));
    }
    Ok(json)
}

fn validate_event_json(value: Vec<u8>) -> BridgeResult<Vec<u8>> {
    if value.len() > MAX_EVENT_JSON_BYTES {
        return Err(invalid_argument(format!(
            "event JSON exceeds {MAX_EVENT_JSON_BYTES} bytes"
        )));
    }
    let value: serde_json::Value = serde_json::from_slice(&value)
        .map_err(|error| invalid_argument(format!("event result is not valid JSON: {error}")))?;
    serde_json::to_vec(&value).map_err(|error| {
        BridgeError::new(
            BridgeErrorCode::Engine,
            format!("failed to normalize event result JSON: {error}"),
        )
    })
}

fn unexpected_host_value(query: HostQuery, expected: &str) -> BridgeError {
    BridgeError::new(
        BridgeErrorCode::Engine,
        format!("runtime host returned the wrong value for {query:?}; expected {expected}"),
    )
}

fn banned_list_json(lists: BannedLists) -> BridgeResult<Vec<u8>> {
    let strings = |values: &[Vec<u8>]| {
        values
            .iter()
            .map(|value| String::from_utf8_lossy(value).into_owned())
            .collect::<Vec<_>>()
    };
    serde_json::to_vec(&serde_json::json!({
        "ip_addresses": strings(&lists.ip_addresses),
        "cd_keys": strings(&lists.cd_keys),
        "player_names": strings(&lists.player_names),
    }))
    .map_err(|error| {
        BridgeError::new(
            BridgeErrorCode::Engine,
            format!("failed to serialize engine ban lists: {error}"),
        )
    })
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

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, path::PathBuf};

    use super::{
        AdministrationCommand, BannedLists, BridgeError, BridgeErrorCode, BridgeResult,
        BridgeValue, EventCommand, EventControls, EventObjectId, EventPayload, HostCommandResult,
        HostQuery, HostValue, RuntimeHost, ScriptBridge, ScriptLog, ScriptLogLevel, Vector,
    };
    use crate::{
        ADMINISTRATION_CAPABILITY_VERSION, AbiLayouts, AdministrationTarget, Architecture,
        BinaryIdentity, BridgeTarget, CExoStringLayout, EVENTS_CAPABILITY_VERSION,
        EngineClassLayouts, EventTarget, FileSha256, NWSCRIPT_BRIDGE_CAPABILITY_VERSION,
        OperatingSystem, Platform, PlayerListLayout, RUNTIME_API_VERSION, RuntimeContext,
        SERVER_STATE_CAPABILITY_VERSION, SelectedTargetPack, ShutdownTarget,
        TARGET_PACK_SCHEMA_VERSION, TargetAddress, TargetPack, TargetServer, TargetSource,
        VectorLayout,
    };

    #[test]
    fn dispatches_server_identity_and_clears_failed_calls() -> Result<(), Box<dyn std::error::Error>>
    {
        let context = context();
        let mut host = FakeHost {
            module_name: b"fixture-module".to_vec(),
            player_count: 2,
            max_players: 64,
            udp_port: 5121,
            event: Some(EventPayload {
                name:     "module.load".to_string(),
                id:       3002,
                script:   "_nwnrs_onload".to_string(),
                phase:    "before".to_string(),
                depth:    1,
                target:   EventObjectId::new(0),
                controls: EventControls {
                    skippable: true,
                    result:    true,
                },
                data:     BTreeMap::new(),
            }),
            ..FakeHost::default()
        };
        let mut bridge = ScriptBridge::default();

        bridge.call("NWNRS", "GetServerBuild", &context, &mut host)?;
        assert_eq!(bridge.pop_string()?, b"fixture");
        bridge.call("NWNRS", "GetModuleName", &context, &mut host)?;
        assert_eq!(bridge.pop_string()?, b"fixture-module");
        bridge.call("NWNRS", "GetPlayerCount", &context, &mut host)?;
        assert_eq!(bridge.pop_integer()?, 2);
        bridge.call("NWNRS", "GetMaxPlayers", &context, &mut host)?;
        assert_eq!(bridge.pop_integer()?, 64);
        bridge.call("NWNRS", "GetServerPort", &context, &mut host)?;
        assert_eq!(bridge.pop_integer()?, 5121);
        bridge.call("NWNRS", "GetCurrentEvent", &context, &mut host)?;
        let event: serde_json::Value = serde_json::from_slice(&bridge.pop_string()?)?;
        assert_eq!(
            event.pointer("/name").and_then(serde_json::Value::as_str),
            Some("module.load")
        );
        assert_eq!(
            event.pointer("/id").and_then(serde_json::Value::as_i64),
            Some(3002)
        );
        assert_eq!(
            event.pointer("/script").and_then(serde_json::Value::as_str),
            Some("_nwnrs_onload")
        );
        assert_eq!(
            event.pointer("/phase").and_then(serde_json::Value::as_str),
            Some("before")
        );
        assert_eq!(
            event.pointer("/depth").and_then(serde_json::Value::as_u64),
            Some(1)
        );
        assert_eq!(
            event.pointer("/target").and_then(serde_json::Value::as_str),
            Some("00000000")
        );
        assert_eq!(
            event
                .pointer("/controls/skippable")
                .and_then(serde_json::Value::as_bool),
            Some(true)
        );

        bridge.call("NWNRS", "SkipCurrentEvent", &context, &mut host)?;
        bridge.push_argument(BridgeValue::String(b"{\"accepted\":true}".to_vec()));
        bridge.call("NWNRS", "SetCurrentEventResult", &context, &mut host)?;
        assert_eq!(
            host.event_commands,
            [
                EventCommand::Skip,
                EventCommand::SetResult(b"{\"accepted\":true}".to_vec())
            ]
        );

        bridge.push_argument(BridgeValue::Integer(1));
        assert!(
            bridge
                .call("NWNRS", "GetRuntimeVersion", &context, &mut host)
                .is_err()
        );
        bridge.call("NWNRS", "GetRuntimeVersion", &context, &mut host)?;
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
        let mut host = FakeHost::default();
        bridge.push_argument(BridgeValue::Integer(3));
        bridge.push_argument(BridgeValue::String(b"careful now".to_vec()));
        bridge.call("NWNRS", "Log", &context, &mut host)?;
        assert_eq!(
            bridge.take_logs(),
            vec![ScriptLog {
                level:   ScriptLogLevel::Warn,
                message: b"careful now".to_vec(),
            }]
        );

        bridge.push_argument(BridgeValue::Integer(9));
        bridge.push_argument(BridgeValue::String(b"invalid".to_vec()));
        assert!(bridge.call("NWNRS", "Log", &context, &mut host).is_err());
        assert!(bridge.take_logs().is_empty());
        Ok(())
    }

    #[test]
    fn validates_administration_calls_and_executes_typed_commands()
    -> Result<(), Box<dyn std::error::Error>> {
        let context = context();
        let mut host = FakeHost {
            server_name: b"fixture server".to_vec(),
            player_password_is_set: true,
            banned_lists: BannedLists {
                ip_addresses: vec![b"192.0.2.1".to_vec()],
                cd_keys:      vec![b"fixture-key".to_vec()],
                player_names: vec![b"fixture-player".to_vec()],
            },
            ..FakeHost::default()
        };
        let mut bridge = ScriptBridge::default();

        bridge.call("NWNRS", "GetServerName", &context, &mut host)?;
        assert_eq!(bridge.pop_string()?, b"fixture server");
        bridge.call("NWNRS", "GetIsPlayerPasswordSet", &context, &mut host)?;
        assert_eq!(bridge.pop_integer()?, 1);
        bridge.call("NWNRS", "GetBannedList", &context, &mut host)?;
        assert_eq!(
            String::from_utf8(bridge.pop_string()?)?,
            "{\"ip_addresses\":[\"192.0.2.1\"],\"cd_keys\":[\"fixture-key\"],\"player_names\":[\"\
             fixture-player\"]}"
        );
        assert_eq!(
            host.queries,
            vec![
                HostQuery::ServerName,
                HostQuery::PlayerPasswordIsSet,
                HostQuery::BannedLists,
            ]
        );

        bridge.push_argument(BridgeValue::String(b"new server".to_vec()));
        bridge.call("NWNRS", "SetServerName", &context, &mut host)?;

        bridge.push_argument(BridgeValue::Integer(1));
        bridge.push_argument(BridgeValue::Integer(14));
        bridge.call("NWNRS", "SetPlayOption", &context, &mut host)?;
        assert_eq!(
            host.commands,
            vec![
                AdministrationCommand::SetServerName(b"new server".to_vec()),
                AdministrationCommand::SetPlayOption {
                    option: 14,
                    value:  1,
                },
            ]
        );

        bridge.push_argument(BridgeValue::Integer(3));
        bridge.push_argument(BridgeValue::Integer(10));
        assert!(
            bridge
                .call("NWNRS", "SetPlayOption", &context, &mut host)
                .is_err()
        );
        assert_eq!(host.commands.len(), 2);

        bridge.push_argument(BridgeValue::String(b"account action".to_vec()));
        bridge.push_argument(BridgeValue::Integer(1));
        bridge.push_argument(BridgeValue::Object(0x0102_0304));
        bridge.call("NWNRS", "DeletePlayerCharacter", &context, &mut host)?;
        assert_eq!(
            host.commands.last(),
            Some(&AdministrationCommand::DeletePlayerCharacter {
                object_id:       0x0102_0304,
                preserve_backup: true,
                kick_message:    b"account action".to_vec(),
            })
        );

        bridge.push_argument(BridgeValue::String(b"Fixture Character".to_vec()));
        bridge.push_argument(BridgeValue::String(b"fixture-player".to_vec()));
        bridge.call("NWNRS", "DeleteTURD", &context, &mut host)?;
        assert_eq!(bridge.pop_integer()?, 1);
        assert_eq!(
            host.commands.last(),
            Some(&AdministrationCommand::DeleteTurd {
                player_name:    b"fixture-player".to_vec(),
                character_name: b"Fixture Character".to_vec(),
            })
        );

        bridge.push_argument(BridgeValue::String(Vec::new()));
        assert!(
            bridge
                .call("NWNRS", "AddBannedIp", &context, &mut host)
                .is_err()
        );

        bridge.push_argument(BridgeValue::String(b"bad\0name".to_vec()));
        assert!(
            bridge
                .call("NWNRS", "SetModuleName", &context, &mut host)
                .is_err()
        );

        bridge.push_argument(BridgeValue::String(Vec::new()));
        bridge.push_argument(BridgeValue::Integer(1));
        bridge.push_argument(BridgeValue::Object(0x7f00_0000));
        assert!(
            bridge
                .call("NWNRS", "DeletePlayerCharacter", &context, &mut host)
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn exposes_versions_capabilities_and_stable_errors() -> Result<(), Box<dyn std::error::Error>> {
        let mut context = context();
        let mut bridge = ScriptBridge::default();
        let mut host = FakeHost::default();

        bridge.call("NWNRS", "GetApiVersion", &context, &mut host)?;
        assert_eq!(bridge.pop_integer()?, 1);

        bridge.push_argument(BridgeValue::String(b"server_state".to_vec()));
        bridge.call("NWNRS", "GetCapabilityVersion", &context, &mut host)?;
        assert_eq!(bridge.pop_integer()?, 1);

        let error = bridge
            .call("NWNRS", "NotRegistered", &context, &mut host)
            .expect_err("unknown function must fail");
        assert_eq!(error.code(), BridgeErrorCode::UnknownFunction);
        bridge.call("NWNRS", "GetLastErrorCode", &context, &mut host)?;
        assert_eq!(
            bridge.pop_integer()?,
            BridgeErrorCode::UnknownFunction.value()
        );
        bridge.call("NWNRS", "GetLastErrorMessage", &context, &mut host)?;
        assert!(String::from_utf8(bridge.pop_string()?)?.contains("NotRegistered"));

        context.target.pack.server_state = None;
        let error = bridge
            .call("NWNRS", "GetModuleName", &context, &mut host)
            .expect_err("missing optional capability must fail");
        assert_eq!(error.code(), BridgeErrorCode::MissingCapability);
        Ok(())
    }

    #[test]
    fn public_nwscript_header_matches_the_rust_contract() {
        let header = include_str!("../../../../include/nwnrs/nwnrs.nss");
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
        for name in [
            "NWSCRIPT_BRIDGE",
            "SERVER_STATE",
            "ADMINISTRATION",
            "EVENTS",
        ] {
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

    #[test]
    fn reports_host_mutation_failures_synchronously() -> Result<(), Box<dyn std::error::Error>> {
        let context = context();
        let mut bridge = ScriptBridge::default();
        let mut host = FakeHost {
            execution_error: Some(BridgeError::new(
                BridgeErrorCode::Engine,
                "fixture mutation failed",
            )),
            ..FakeHost::default()
        };
        bridge.push_argument(BridgeValue::String(b"new server".to_vec()));
        let error = bridge
            .call("NWNRS", "SetServerName", &context, &mut host)
            .expect_err("native mutation failure must fail the same bridge call");
        assert_eq!(error.code(), BridgeErrorCode::Engine);
        assert!(host.commands.is_empty());

        host.execution_error = None;
        bridge.push_argument(BridgeValue::Integer(7));
        bridge.push_argument(BridgeValue::String(b"too many".to_vec()));
        let error = bridge
            .call("NWNRS", "SetServerName", &context, &mut host)
            .expect_err("extra arguments must fail before native execution");
        assert_eq!(error.code(), BridgeErrorCode::InvalidArgument);
        assert!(host.commands.is_empty());
        Ok(())
    }

    #[derive(Debug)]
    struct FakeHost {
        module_name:            Vec<u8>,
        player_count:           i32,
        max_players:            i32,
        udp_port:               i32,
        server_name:            Vec<u8>,
        player_password_is_set: bool,
        dm_password_is_set:     bool,
        min_level:              i32,
        max_level:              i32,
        banned_lists:           BannedLists,
        event:                  Option<EventPayload>,
        event_commands:         Vec<EventCommand>,
        queries:                Vec<HostQuery>,
        commands:               Vec<AdministrationCommand>,
        execution_error:        Option<BridgeError>,
    }

    impl Default for FakeHost {
        fn default() -> Self {
            Self {
                module_name:            Vec::new(),
                player_count:           0,
                max_players:            0,
                udp_port:               0,
                server_name:            Vec::new(),
                player_password_is_set: false,
                dm_password_is_set:     false,
                min_level:              1,
                max_level:              40,
                banned_lists:           BannedLists::default(),
                event:                  None,
                event_commands:         Vec::new(),
                queries:                Vec::new(),
                commands:               Vec::new(),
                execution_error:        None,
            }
        }
    }

    impl RuntimeHost for FakeHost {
        fn query(&mut self, query: HostQuery) -> BridgeResult<HostValue> {
            self.queries.push(query);
            let value = match query {
                HostQuery::ModuleName => HostValue::String(self.module_name.clone()),
                HostQuery::PlayerCount => HostValue::Integer(self.player_count),
                HostQuery::MaxPlayers => HostValue::Integer(self.max_players),
                HostQuery::UdpPort => HostValue::Integer(self.udp_port),
                HostQuery::ServerName => HostValue::String(self.server_name.clone()),
                HostQuery::PlayerPasswordIsSet => HostValue::Boolean(self.player_password_is_set),
                HostQuery::DmPasswordIsSet => HostValue::Boolean(self.dm_password_is_set),
                HostQuery::MinLevel => HostValue::Integer(self.min_level),
                HostQuery::MaxLevel => HostValue::Integer(self.max_level),
                HostQuery::PlayOption(option) => HostValue::Integer(option.saturating_sub(10)),
                HostQuery::DebugValue(debug_type) => {
                    HostValue::Integer(i32::from(matches!(debug_type, 1 | 3)))
                }
                HostQuery::BannedLists => HostValue::BannedLists(self.banned_lists.clone()),
                HostQuery::CurrentEvent => HostValue::CurrentEvent(self.event.clone()),
            };
            Ok(value)
        }

        fn execute(&mut self, command: AdministrationCommand) -> BridgeResult<HostCommandResult> {
            if let Some(error) = self.execution_error.clone() {
                return Err(error);
            }
            let result = if matches!(command, AdministrationCommand::DeleteTurd { .. }) {
                HostCommandResult::Boolean(true)
            } else {
                HostCommandResult::None
            };
            self.commands.push(command);
            Ok(result)
        }

        fn control_event(&mut self, command: EventCommand) -> BridgeResult<()> {
            self.event_commands.push(command);
            Ok(())
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
                    administration: Some(administration_target()),
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
        let address = || TargetAddress::Offset {
            offset: 1
        };
        EventTarget {
            version:               EVENTS_CAPABILITY_VERSION,
            virtual_machine:       address(),
            run_script:            address(),
            game_object_id_offset: 8,
            hooks:                 std::collections::BTreeMap::new(),
            functions:             std::collections::BTreeMap::new(),
        }
    }

    fn administration_target() -> AdministrationTarget {
        let address = || TargetAddress::Offset {
            offset: 1
        };
        AdministrationTarget {
            version: ADMINISTRATION_CAPABILITY_VERSION,
            get_session_name: address(),
            set_session_name: address(),
            get_player_password: address(),
            set_player_password: address(),
            get_game_master_password: address(),
            set_game_master_password: address(),
            enable_combat_debugging: address(),
            enable_saving_throw_debugging: address(),
            enable_movement_speed_debugging: address(),
            enable_hit_die_debugging: address(),
            shutdown: ShutdownTarget::ExitFlag {
                address: address()
            },
            add_banned_ip: address(),
            remove_banned_ip: address(),
            add_banned_cd_key: address(),
            remove_banned_cd_key: address(),
            add_banned_player_name: address(),
            remove_banned_player_name: address(),
            rules: address(),
            reload_rules: address(),
            get_module: address(),
            get_loc_string: address(),
            remove_linked_list_node: address(),
            main_loop: address(),
            get_client_object_by_object_id: address(),
            get_creature_by_game_object_id: address(),
            get_player_name: address(),
            get_player_info: address(),
            disconnect_player: address(),
            exo_base: address(),
            get_alias_path: address(),
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
                app_manager_server_offset: 8,
                server_info_module_offset: 8,
                server_info_joining_restrictions_offset: 136,
                server_info_play_options_offset: 252,
                server_info_persistent_world_options_offset: 404,
                persistent_world_options_server_vault_by_player_name_offset: 16,
                joining_restrictions_min_level_offset: 104,
                joining_restrictions_max_level_offset: 108,
                server_exo_app_internal_offset: 8,
                internal_banned_ip_list_offset: 65920,
                internal_banned_cd_key_list_offset: 65936,
                internal_banned_player_name_list_offset: 65952,
                module_turd_list_offset: 112,
                player_turd_community_name_offset: 752,
                player_turd_first_name_offset: 768,
                player_turd_last_name_offset: 784,
                linked_list_head_offset: 0,
                linked_list_count_offset: 16,
                linked_list_node_next_offset: 8,
                linked_list_node_object_offset: 16,
                player_id_offset: 72,
                player_file_name_offset: 181,
                player_file_name_size: 17,
                net_layer_player_info_cd_key_offset: 136,
                player_cd_key_public_offset: 0,
                exo_base_alias_list_offset: 32,
                creature_stats_offset: 2760,
                creature_stats_first_name_offset: 72,
                creature_stats_last_name_offset: 88,
                vm_recursion_level_offset: 36,
                vm_script_array_offset: 40,
                vm_script_slot_count: 8,
                vm_script_size: 152,
                vm_script_alignment: 8,
                vm_script_name_offset: 24,
                vm_script_event_id_offset: 72,
            },
        }
    }
}
