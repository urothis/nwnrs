//! Safe operations exposed by the native NWServer adapter.

use std::collections::BTreeMap;

use serde::Serialize;

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
    /// Current scoped nwnrs event, if any.
    CurrentEvent,
}

/// Mutations supported by the currently active event frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EventCommand {
    /// Prevents a skippable event's original engine operation.
    Skip,
    /// Replaces the event's JSON result.
    SetResult(Vec<u8>),
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
#[derive(Clone, Debug, PartialEq)]
pub enum HostValue {
    /// An NWScript integer value.
    Integer(i32),
    /// A boolean value.
    Boolean(bool),
    /// Engine string bytes.
    String(Vec<u8>),
    /// All administration ban lists.
    BannedLists(BannedLists),
    /// Current scoped event, or `None` outside nwnrs event dispatch.
    CurrentEvent(Option<EventPayload>),
}

/// Result produced by one synchronous administration command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HostCommandResult {
    /// Command has no return value.
    None,
    /// Command returns a boolean value.
    Boolean(bool),
}

/// Controls advertised to NWScript for one event kind.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct EventControls {
    /// Whether [`EventCommand::Skip`] is accepted.
    pub skippable: bool,
    /// Whether [`EventCommand::SetResult`] is accepted.
    pub result:    bool,
}

/// An NWScript object identifier serialized as exactly eight hexadecimal
/// digits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EventObjectId(u32);

impl EventObjectId {
    /// Creates an event object identifier from its engine representation.
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    /// Returns the raw engine object identifier.
    #[must_use]
    pub const fn raw(self) -> u32 {
        self.0
    }
}

impl Serialize for EventObjectId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&format!("{:08x}", self.0))
    }
}

/// A three-dimensional event value.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct EventVector {
    /// X coordinate.
    pub x: f32,
    /// Y coordinate.
    pub y: f32,
    /// Z coordinate.
    pub z: f32,
}

/// A location copied from live engine state for one event dispatch.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct EventLocation {
    /// Containing area object identifier.
    pub area:     EventObjectId,
    /// World-space position.
    pub position: EventVector,
    /// Facing in degrees.
    pub facing:   f32,
}

/// One owned, typed value in an event's data object.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum EventValue {
    /// Explicit JSON null.
    Null,
    /// Boolean event field.
    Boolean(bool),
    /// Signed integer event field.
    Integer(i32),
    /// Unsigned integer event field.
    Unsigned(u32),
    /// Floating-point event field.
    Float(f32),
    /// UTF-8 string event field.
    String(String),
    /// Neverwinter Nights object identifier.
    Object(EventObjectId),
    /// Three-dimensional vector event field.
    Vector(EventVector),
    /// Area, position, and facing event field.
    Location(EventLocation),
}

/// One immutable JSON-shaped event snapshot passed to every NWScript handler.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EventPayload {
    /// Stable semantic event identity.
    pub name:     String,
    /// Engine `EVENT_SCRIPT_*` identifier, or `-1` when not applicable.
    pub id:       i32,
    /// Generated dispatcher script resref.
    pub script:   String,
    /// Hook phase such as `before` or `after`.
    pub phase:    String,
    /// One-based nwnrs event nesting depth.
    pub depth:    u32,
    /// Object used as `OBJECT_SELF` for the dispatcher.
    pub target:   EventObjectId,
    /// Mutations supported by this event kind.
    pub controls: EventControls,
    /// Event-specific owned values.
    pub data:     BTreeMap<String, EventValue>,
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

    /// Mutates the current scoped event frame.
    ///
    /// # Errors
    ///
    /// Returns an engine bridge error outside event dispatch or when the
    /// current event does not support the requested control.
    fn control_event(&mut self, command: EventCommand) -> BridgeResult<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_values_have_stable_json_representations() -> Result<(), serde_json::Error> {
        let values = BTreeMap::from([
            (
                "location".to_string(),
                EventValue::Location(EventLocation {
                    area:     EventObjectId::new(0x0102_0304),
                    position: EventVector {
                        x: 1.25,
                        y: -2.5,
                        z: 3.75,
                    },
                    facing:   90.0,
                }),
            ),
            (
                "object".to_string(),
                EventValue::Object(EventObjectId::new(0x7f00_0000)),
            ),
            (
                "vector".to_string(),
                EventValue::Vector(EventVector {
                    x: 4.0,
                    y: 5.0,
                    z: 6.0,
                }),
            ),
        ]);
        let json = serde_json::to_value(values)?;
        assert_eq!(
            json.pointer("/object").and_then(serde_json::Value::as_str),
            Some("7f000000")
        );
        assert_eq!(
            json.pointer("/vector/y")
                .and_then(serde_json::Value::as_f64),
            Some(5.0)
        );
        assert_eq!(
            json.pointer("/location/area")
                .and_then(serde_json::Value::as_str),
            Some("01020304")
        );
        assert_eq!(
            json.pointer("/location/position/z")
                .and_then(serde_json::Value::as_f64),
            Some(3.75)
        );
        assert_eq!(
            json.pointer("/location/facing")
                .and_then(serde_json::Value::as_f64),
            Some(90.0)
        );
        Ok(())
    }
}
