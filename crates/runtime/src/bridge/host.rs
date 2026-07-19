//! Safe operations exposed by the native NWServer adapter.

use super::BridgeResult;

/// One administration mutation validated by the NWScript dispatcher.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AdministrationCommand {
    /// Replaces the module's advertised name.
    SetModuleName(Vec<u8>),
    /// Replaces the network session name.
    SetServerName(Vec<u8>),
    /// Replaces the player password; an empty value clears it.
    SetPlayerPassword(Vec<u8>),
    /// Replaces the DM password; an empty value clears it.
    SetDmPassword(Vec<u8>),
    /// Replaces the minimum permitted character level.
    SetMinLevel(i32),
    /// Replaces the maximum permitted character level.
    SetMaxLevel(i32),
    /// Replaces one active play option.
    SetPlayOption {
        /// Numeric `NWNRS_PLAY_OPTION_*` identifier.
        option: i32,
        /// Validated option value.
        value:  i32,
    },
    /// Replaces one engine debug toggle.
    SetDebugValue {
        /// Numeric `NWNRS_DEBUG_*` identifier.
        debug_type: i32,
        /// Boolean integer value.
        value:      i32,
    },
    /// Requests a graceful server shutdown.
    RequestShutdown,
    /// Adds an IP address to the engine ban list.
    AddBannedIp(Vec<u8>),
    /// Removes an IP address from the engine ban list.
    RemoveBannedIp(Vec<u8>),
    /// Adds a public CD key to the engine ban list.
    AddBannedCdKey(Vec<u8>),
    /// Removes a public CD key from the engine ban list.
    RemoveBannedCdKey(Vec<u8>),
    /// Adds a player account name to the engine ban list.
    AddBannedPlayerName(Vec<u8>),
    /// Removes a player account name from the engine ban list.
    RemoveBannedPlayerName(Vec<u8>),
    /// Reloads the engine rules tables.
    ReloadRules,
    /// Disconnects a player and removes the active server-vault character.
    DeletePlayerCharacter {
        /// Player-controlled creature object identifier.
        object_id:       u32,
        /// Whether the BIC must be preserved under a unique backup name.
        preserve_backup: bool,
        /// Optional disconnect message.
        kick_message:    Vec<u8>,
    },
    /// Deletes one stored Temporary User Resource Data record.
    DeleteTurd {
        /// Player community name.
        player_name:    Vec<u8>,
        /// Full character name.
        character_name: Vec<u8>,
    },
}

/// One live value requested from the native NWServer adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostQuery {
    /// Current module name.
    ModuleName,
    /// Current player count.
    PlayerCount,
    /// Configured maximum player count.
    MaxPlayers,
    /// Active server UDP port.
    UdpPort,
    /// Current advertised server name.
    ServerName,
    /// Whether a player password is configured.
    PlayerPasswordIsSet,
    /// Whether a DM password is configured.
    DmPasswordIsSet,
    /// Minimum permitted character level.
    MinLevel,
    /// Maximum permitted character level.
    MaxLevel,
    /// One play option by numeric identifier.
    PlayOption(i32),
    /// One debug toggle by numeric identifier.
    DebugValue(i32),
    /// All three engine ban lists.
    BannedLists,
    /// Current VM event-script context.
    EventContext,
}

/// The three administration ban lists returned as one coherent value.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BannedLists {
    /// Banned IP address strings.
    pub ip_addresses: Vec<Vec<u8>>,
    /// Banned public CD key strings.
    pub cd_keys:      Vec<Vec<u8>>,
    /// Banned player-account name strings.
    pub player_names: Vec<Vec<u8>>,
}

/// A typed value returned by [`RuntimeHost::query`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HostValue {
    /// An NWScript integer value.
    Integer(i32),
    /// A boolean value.
    Boolean(bool),
    /// Engine string bytes.
    String(Vec<u8>),
    /// All administration ban lists.
    BannedLists(BannedLists),
    /// Current event context.
    EventContext(EventContext),
}

/// Result produced by one synchronous administration command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HostCommandResult {
    /// Command has no return value.
    None,
    /// Command returns a boolean value.
    Boolean(bool),
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

/// Safe interface through which the dispatcher accesses live NWServer state.
///
/// The injected native crate implements this interface. Unit tests can use a
/// deterministic in-memory host, while the dispatcher remains entirely safe.
pub trait RuntimeHost {
    /// Reads exactly one live engine value.
    ///
    /// # Errors
    ///
    /// Returns an engine bridge error when the value cannot be read.
    fn query(&mut self, query: HostQuery) -> BridgeResult<HostValue>;

    /// Executes one already validated administration mutation synchronously.
    ///
    /// # Errors
    ///
    /// Returns an engine bridge error when the mutation fails.
    fn execute(&mut self, command: AdministrationCommand) -> BridgeResult<HostCommandResult>;
}
