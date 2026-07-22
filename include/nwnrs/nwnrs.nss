/// @file nwnrs.nss
/// @brief Server identity, live state, administration, JSON events, and structured logging.

#include "nwnrs_macros"

/// Internal NWNX namespace used by every nwnrs bridge call.
/// @private
const string NWNRS_NAMESPACE = "NWNRS";

/// Numeric bridge API identifier returned by NWNRS_GetApiVersion().
const int NWNRS_API_VERSION = 1;

/// Capability name for the base NWScript-to-runtime bridge.
const string NWNRS_CAPABILITY_NWSCRIPT_BRIDGE = "nwscript_bridge";
/// Capability name for live server identity and state queries.
const string NWNRS_CAPABILITY_SERVER_STATE = "server_state";
/// Capability name for live server administration operations.
const string NWNRS_CAPABILITY_ADMINISTRATION = "administration";

/// Error reported by the NWScript bridge on the current script thread.
enum NwnrsError {
    /// No bridge error occurred.
    #[default] #[alias(NWNRS_ERROR_NONE)] None = 0,
    /// The requested NWNX namespace is not registered.
    #[alias(NWNRS_ERROR_UNKNOWN_NAMESPACE)] UnknownNamespace = 1,
    /// The requested bridge function is not registered.
    #[alias(NWNRS_ERROR_UNKNOWN_FUNCTION)] UnknownFunction = 2,
    /// A bridge argument failed validation.
    #[alias(NWNRS_ERROR_INVALID_ARGUMENT)] InvalidArgument = 3,
    /// The selected target pack does not provide the required capability.
    #[alias(NWNRS_ERROR_MISSING_CAPABILITY)] MissingCapability = 4,
    /// The engine rejected or failed the requested operation.
    #[alias(NWNRS_ERROR_ENGINE)] Engine = 5,
    /// A bridge call attempted unsupported reentrant execution.
    #[alias(NWNRS_ERROR_REENTRANT)] Reentrant = 6,
}

/// Severity used by the runtime's structured tracing pipeline.
enum NwnrsLogLevel {
    /// Most verbose structured logging level.
    #[alias(NWNRS_LOG_LEVEL_TRACE)] Trace = 0,
    /// Debug-oriented structured logging level.
    #[alias(NWNRS_LOG_LEVEL_DEBUG)] Debug,
    /// Normal informational structured logging level.
    #[default] #[alias(NWNRS_LOG_LEVEL_INFO)] Info,
    /// Warning structured logging level.
    #[alias(NWNRS_LOG_LEVEL_WARN)] Warn,
    /// Error structured logging level.
    #[alias(NWNRS_LOG_LEVEL_ERROR)] Error,
}

/// Server play-option identifier accepted by the administration bridge.
enum NwnrsPlayOption {
    /// PVP mode: 0 disables PVP, 1 permits party PVP, and 2 permits full PVP.
    #[alias(NWNRS_PLAY_OPTION_PVP_SETTING)] PvpSetting = 10,
    /// Whether players may pause and resume the server.
    #[alias(NWNRS_PLAY_OPTION_PAUSE_AND_PLAY)] PauseAndPlay,
    /// Whether all players must remain in one party.
    #[alias(NWNRS_PLAY_OPTION_ONE_PARTY_ONLY)] OnePartyOnly,
    /// Whether joining characters must satisfy legal-character validation.
    #[alias(NWNRS_PLAY_OPTION_ENFORCE_LEGAL_CHARACTERS)] EnforceLegalCharacters,
    /// Whether item-level restrictions are enforced.
    #[alias(NWNRS_PLAY_OPTION_ITEM_LEVEL_RESTRICTIONS)] ItemLevelRestrictions,
    /// Whether the CD-key ban list operates as an allowlist.
    #[alias(NWNRS_PLAY_OPTION_CDKEY_BANLIST_ALLOWLIST)] CdkeyBanlistAllowlist,
    /// Whether player shouting is disabled.
    #[alias(NWNRS_PLAY_OPTION_DISALLOW_SHOUTING)] DisallowShouting,
    /// Whether the server announces DM joins.
    #[alias(NWNRS_PLAY_OPTION_SHOW_DM_JOIN_MESSAGE)] ShowDmJoinMessage,
    /// Whether server-vault characters are backed up when saved.
    #[alias(NWNRS_PLAY_OPTION_BACKUP_SAVED_CHARACTERS)] BackupSavedCharacters,
    /// Whether a natural saving-throw roll of 1 automatically fails.
    #[alias(NWNRS_PLAY_OPTION_AUTO_FAIL_SAVE_ON_1)] AutoFailSaveOn1,
    /// Whether spell use is validated against the active rules.
    #[alias(NWNRS_PLAY_OPTION_VALIDATE_SPELLS)] ValidateSpells,
    /// Whether effect details are visible during examination.
    #[alias(NWNRS_PLAY_OPTION_EXAMINE_EFFECTS)] ExamineEffects,
    /// Whether challenge ratings are visible during examination.
    #[alias(NWNRS_PLAY_OPTION_EXAMINE_CHALLENGE_RATING)] ExamineChallengeRating,
    /// Whether creatures receive maximum hit points for each hit die.
    #[alias(NWNRS_PLAY_OPTION_USE_MAX_HITPOINTS)] UseMaxHitpoints,
    /// Whether resting restores expended spell uses.
    #[alias(NWNRS_PLAY_OPTION_RESTORE_SPELLS_USES)] RestoreSpellsUses,
    /// Whether encounter spawn pools reset after use.
    #[alias(NWNRS_PLAY_OPTION_RESET_ENCOUNTER_SPAWN_POOL)] ResetEncounterSpawnPool,
    /// Whether gained hit points are hidden from player feedback.
    #[alias(NWNRS_PLAY_OPTION_HIDE_HITPOINTS_GAINED)] HideHitpointsGained,
    /// Whether players may control other party members.
    #[alias(NWNRS_PLAY_OPTION_PLAYER_PARTY_CONTROL)] PlayerPartyControl,
    /// Whether the server announces player joins.
    #[alias(NWNRS_PLAY_OPTION_SHOW_PLAYER_JOIN_MESSAGES)] ShowPlayerJoinMessages,
}

/// Debug-output category accepted by the administration bridge.
enum NwnrsDebugType {
    /// Combat debug-output toggle identifier.
    #[default] #[alias(NWNRS_DEBUG_COMBAT)] Combat = 0,
    /// Saving-throw debug-output toggle identifier.
    #[alias(NWNRS_DEBUG_SAVING_THROW)] SavingThrow,
    /// Movement-speed debug-output toggle identifier.
    #[alias(NWNRS_DEBUG_MOVEMENT_SPEED)] MovementSpeed,
    /// Hit-die debug-output toggle identifier.
    #[alias(NWNRS_DEBUG_HIT_DIE)] HitDie,
}

/// Whitelist controlling safe-projectile projectile-type identifiers.
const string NWNRS_EVENT_ID_WHITELIST_PROJECTILE_TYPE =
    "object.broadcast_safe_projectile.projectile_type";
/// Whitelist controlling safe-projectile spell identifiers.
const string NWNRS_EVENT_ID_WHITELIST_PROJECTILE_SPELL_ID =
    "object.broadcast_safe_projectile.spell_id";

/// Returns TRUE when the nwnrs NWScript bridge is installed.
int NWNRS_GetIsAvailable();

/// Returns the integer version of the stable NWScript bridge contract.
int NWNRS_GetApiVersion();

/// Returns TRUE when a named capability is present in the selected target pack.
/// @param sCapability One NWNRS_CAPABILITY_* name.
int NWNRS_HasCapability(string sCapability);

/// Returns the most recent bridge error on this script thread.
/// Unknown bridge error codes safely map to NwnrsError::Engine.
NwnrsError NWNRS_GetLastErrorCode();

/// Returns the diagnostic message associated with the most recent error.
string NWNRS_GetLastErrorMessage();

/// Sends a message through the runtime's structured tracing pipeline.
/// @param sMessage Message to emit.
/// @param nLevel Structured tracing severity.
void NWNRS_Log(string sMessage, NwnrsLogLevel nLevel = NWNRS_LOG_LEVEL_INFO);

/// Returns the semantic version of the injected nwnrs runtime.
string NWNRS_GetRuntimeVersion();

/// Returns the lowercase SHA-256 of the complete server executable.
string NWNRS_GetServerBinarySha256();

/// Returns the human-readable build recorded by the exact target pack.
string NWNRS_GetServerBuild();

/// Returns the server platform as "operating-system-architecture".
string NWNRS_GetServerPlatform();

/// Returns the server operating system: "macos", "linux", or "windows".
string NWNRS_GetServerOperatingSystem();

/// Returns the server architecture, currently "aarch64" or "x86_64".
string NWNRS_GetServerArchitecture();

/// Returns the name of the currently loaded module.
string NWNRS_GetModuleName();

/// Returns the number of players currently known to the server.
int NWNRS_GetPlayerCount();

/// Returns the maximum number of players configured for the session.
int NWNRS_GetMaxPlayers();

/// Returns the active UDP port, or zero before network startup completes.
int NWNRS_GetServerPort();

// Administration

/// Returns the server name advertised by the network session.
string NWNRS_GetServerName();

/// Changes the server name advertised by the network session.
/// @param sName New advertised server name.
void NWNRS_SetServerName(string sName);

/// Changes the active module's advertised name.
/// @param sName New advertised module name.
void NWNRS_SetModuleName(string sName);

/// Returns TRUE when a player password is configured without exposing it.
int NWNRS_GetIsPlayerPasswordSet();

/// Sets the password required for player connections.
/// @param sPassword New player password.
void NWNRS_SetPlayerPassword(string sPassword);

/// Removes the password required for player connections.
void NWNRS_ClearPlayerPassword();

/// Returns TRUE when a DM password is configured without exposing it.
int NWNRS_GetIsDMPasswordSet();

/// Sets the password required for DM connections.
/// @param sPassword New DM password.
void NWNRS_SetDMPassword(string sPassword);

/// Removes the password required for DM connections.
void NWNRS_ClearDMPassword();

/// Returns the minimum permitted character level.
int NWNRS_GetMinLevel();

/// Sets the minimum permitted character level, from 1 through 255.
/// @param nLevel New minimum character level.
void NWNRS_SetMinLevel(int nLevel);

/// Returns the maximum permitted character level.
int NWNRS_GetMaxLevel();

/// Sets the maximum permitted character level, from 1 through 255.
/// @param nLevel New maximum character level.
void NWNRS_SetMaxLevel(int nLevel);

/// Returns one server play-option value.
/// @param nOption Play-option identifier to query.
int NWNRS_GetPlayOption(NwnrsPlayOption nOption);

/// Changes one NWNRS_PLAY_OPTION_* value.
/// @param nOption Play-option identifier to change.
/// @param nValue New option value.
void NWNRS_SetPlayOption(NwnrsPlayOption nOption, int nValue);

/// Returns one debug-output toggle.
/// @param nDebugType Debug toggle identifier to query.
int NWNRS_GetDebugValue(NwnrsDebugType nDebugType);

/// Changes one NWNRS_DEBUG_* toggle to FALSE or TRUE.
/// @param nDebugType Debug toggle identifier to change.
/// @param bEnabled TRUE to enable the output; FALSE to disable it.
void NWNRS_SetDebugValue(NwnrsDebugType nDebugType, int bEnabled);

/// Requests graceful server shutdown after the current bridge call returns.
void NWNRS_RequestShutdown();

/// Returns {"ip_addresses":[],"cd_keys":[],"player_names":[]}.
json NWNRS_GetBannedList();

/// Adds an IP address to the persistent engine ban list.
/// @param sAddress IP address to ban.
void NWNRS_AddBannedIP(string sAddress);

/// Removes an IP address from the persistent engine ban list.
/// @param sAddress IP address to unban.
void NWNRS_RemoveBannedIP(string sAddress);

/// Adds a public CD key to the persistent engine ban list.
/// @param sKey Public CD key to ban.
void NWNRS_AddBannedCDKey(string sKey);

/// Removes a public CD key from the persistent engine ban list.
/// @param sKey Public CD key to unban.
void NWNRS_RemoveBannedCDKey(string sKey);

/// Adds a player account name to the persistent engine ban list.
/// @param sPlayerName Player account name to ban.
void NWNRS_AddBannedPlayerName(string sPlayerName);

/// Removes a player account name from the persistent engine ban list.
/// @param sPlayerName Player account name to unban.
void NWNRS_RemoveBannedPlayerName(string sPlayerName);

/// Reloads the engine rules tables from the active resource manager.
void NWNRS_ReloadRules();

/// Disconnects oPC and removes its active server-vault BIC after this script call.
/// When bPreserveBackup is TRUE, preserves the BIC as the first available
/// .deletedN file. The operation also removes the matching in-memory TURD.
/// @param oPC Player character to disconnect and delete.
/// @param bPreserveBackup TRUE to retain a numbered deleted-character backup.
/// @param sKickMessage Message shown when the player is disconnected.
void NWNRS_DeletePlayerCharacter(
    object oPC,
    int bPreserveBackup = TRUE,
    string sKickMessage = ""
);

/// Removes a disconnected player's in-memory TURD by account and character name.
/// Returns TRUE when a matching TURD was found and removed.
/// @param sPlayerName Player account name owning the TURD.
/// @param sCharacterName Character name stored by the TURD.
int NWNRS_DeleteTURD(string sPlayerName, string sCharacterName);

/// Returns TRUE while an nwnrs event dispatcher is running.
int NWNRS_GetIsInEvent();

/// Returns the immutable current event object, or JsonNull outside dispatch.
/// Every event contains name, id, script, phase, depth, target, controls, and
/// event-specific data. Object identifiers are eight-digit hexadecimal strings.
json NWNRS_GetCurrentEvent();

/// Prevents the current event's original engine operation when it is skippable.
void NWNRS_SkipCurrentEvent();

/// Sets the current event result when its schema supports a JSON result.
/// @param jResult Replacement result matching the current event schema.
void NWNRS_SetCurrentEventResult(json jResult);

/// Returns TRUE when this exact server target supports an event annotation.
/// @param sEventIdentity Event annotation identity to query.
int NWNRS_GetEventSupported(string sEventIdentity);

/// Internal: generated dispatchers register subscriptions during module.load.
/// Unsupported target-pack events emit a warning and remain unsubscribed.
/// @param sEventIdentity Event annotation identity to subscribe.
/// @private
void NWNRS_SubscribeEvent(string sEventIdentity);

/// Enables or disables a named integer whitelist. Enabling starts with no IDs.
/// @param sWhitelist One NWNRS_EVENT_ID_WHITELIST_* name.
/// @param bEnabled TRUE to enable filtering; FALSE to disable it.
void NWNRS_ToggleEventIdWhitelist(string sWhitelist, int bEnabled);

/// Adds an integer to an enabled event whitelist.
/// @param sWhitelist One NWNRS_EVENT_ID_WHITELIST_* name.
/// @param nId Integer identifier to allow.
void NWNRS_AddEventIdToWhitelist(string sWhitelist, int nId);

/// Removes an integer from an enabled event whitelist.
/// @param sWhitelist One NWNRS_EVENT_ID_WHITELIST_* name.
/// @param nId Integer identifier to remove.
void NWNRS_RemoveEventIdFromWhitelist(string sWhitelist, int nId);

/// Returns TRUE when the nwnrs NWScript bridge is installed.
int NWNRS_GetIsAvailable()
{
    return NWNXGetIsAvailable();
}

/// Returns the integer version of the stable NWScript bridge contract.
int NWNRS_GetApiVersion()
{
    NWNXCall(NWNRS_NAMESPACE, "GetApiVersion");
    return NWNXPopInt();
}

/// Returns TRUE when a named capability is present in the selected target pack.
/// @param sCapability One NWNRS_CAPABILITY_* name.
int NWNRS_HasCapability(string sCapability)
{
    NWNXPushString(sCapability);
    NWNXCall(NWNRS_NAMESPACE, "HasCapability");
    return NWNXPopInt();
}

/// Returns the most recent bridge error on this script thread.
/// Unknown bridge error codes safely map to NwnrsError::Engine.
NwnrsError NWNRS_GetLastErrorCode()
{
    NWNXCall(NWNRS_NAMESPACE, "GetLastErrorCode");
    return NwnrsError(NWNXPopInt(), NwnrsError::Engine);
}

/// Returns the diagnostic message associated with the most recent error.
string NWNRS_GetLastErrorMessage()
{
    NWNXCall(NWNRS_NAMESPACE, "GetLastErrorMessage");
    return NWNXPopString();
}

/// Sends a message through the runtime's structured tracing pipeline.
/// @param sMessage Message to emit.
/// @param nLevel Structured tracing severity.
void NWNRS_Log(string sMessage, NwnrsLogLevel nLevel = NWNRS_LOG_LEVEL_INFO)
{
    NWNXPushInt(int(nLevel));
    NWNXPushString(sMessage);
    NWNXCall(NWNRS_NAMESPACE, "Log");
}

/// Returns the semantic version of the injected nwnrs runtime.
string NWNRS_GetRuntimeVersion()
{
    NWNXCall(NWNRS_NAMESPACE, "GetRuntimeVersion");
    return NWNXPopString();
}

/// Returns the lowercase SHA-256 of the complete server executable.
string NWNRS_GetServerBinarySha256()
{
    NWNXCall(NWNRS_NAMESPACE, "GetServerBinarySha256");
    return NWNXPopString();
}

/// Returns the human-readable build recorded by the exact target pack.
string NWNRS_GetServerBuild()
{
    NWNXCall(NWNRS_NAMESPACE, "GetServerBuild");
    return NWNXPopString();
}

/// Returns the server platform as "operating-system-architecture".
string NWNRS_GetServerPlatform()
{
    NWNXCall(NWNRS_NAMESPACE, "GetServerPlatform");
    return NWNXPopString();
}

/// Returns the server operating system: "macos", "linux", or "windows".
string NWNRS_GetServerOperatingSystem()
{
    NWNXCall(NWNRS_NAMESPACE, "GetServerOperatingSystem");
    return NWNXPopString();
}

/// Returns the server architecture, currently "aarch64" or "x86_64".
string NWNRS_GetServerArchitecture()
{
    NWNXCall(NWNRS_NAMESPACE, "GetServerArchitecture");
    return NWNXPopString();
}

/// Returns the name of the currently loaded module.
string NWNRS_GetModuleName()
{
    NWNXCall(NWNRS_NAMESPACE, "GetModuleName");
    return NWNXPopString();
}

/// Returns the number of players currently known to the server.
int NWNRS_GetPlayerCount()
{
    NWNXCall(NWNRS_NAMESPACE, "GetPlayerCount");
    return NWNXPopInt();
}

/// Returns the maximum number of players configured for the session.
int NWNRS_GetMaxPlayers()
{
    NWNXCall(NWNRS_NAMESPACE, "GetMaxPlayers");
    return NWNXPopInt();
}

/// Returns the active UDP port, or zero before network startup completes.
int NWNRS_GetServerPort()
{
    NWNXCall(NWNRS_NAMESPACE, "GetServerPort");
    return NWNXPopInt();
}

/// Returns the server name advertised by the network session.
string NWNRS_GetServerName()
{
    NWNXCall(NWNRS_NAMESPACE, "GetServerName");
    return NWNXPopString();
}

/// Changes the server name advertised by the network session.
/// @param sName New advertised server name.
void NWNRS_SetServerName(string sName)
{
    NWNXPushString(sName);
    NWNXCall(NWNRS_NAMESPACE, "SetServerName");
}

/// Changes the active module's advertised name.
/// @param sName New advertised module name.
void NWNRS_SetModuleName(string sName)
{
    NWNXPushString(sName);
    NWNXCall(NWNRS_NAMESPACE, "SetModuleName");
}

/// Returns TRUE when a player password is configured without exposing it.
int NWNRS_GetIsPlayerPasswordSet()
{
    NWNXCall(NWNRS_NAMESPACE, "GetIsPlayerPasswordSet");
    return NWNXPopInt();
}

/// Sets the password required for player connections.
/// @param sPassword New player password.
void NWNRS_SetPlayerPassword(string sPassword)
{
    NWNXPushString(sPassword);
    NWNXCall(NWNRS_NAMESPACE, "SetPlayerPassword");
}

/// Removes the password required for player connections.
void NWNRS_ClearPlayerPassword()
{
    NWNXCall(NWNRS_NAMESPACE, "ClearPlayerPassword");
}

/// Returns TRUE when a DM password is configured without exposing it.
int NWNRS_GetIsDMPasswordSet()
{
    NWNXCall(NWNRS_NAMESPACE, "GetIsDmPasswordSet");
    return NWNXPopInt();
}

/// Sets the password required for DM connections.
/// @param sPassword New DM password.
void NWNRS_SetDMPassword(string sPassword)
{
    NWNXPushString(sPassword);
    NWNXCall(NWNRS_NAMESPACE, "SetDmPassword");
}

/// Removes the password required for DM connections.
void NWNRS_ClearDMPassword()
{
    NWNXCall(NWNRS_NAMESPACE, "ClearDmPassword");
}

/// Returns the minimum permitted character level.
int NWNRS_GetMinLevel()
{
    NWNXCall(NWNRS_NAMESPACE, "GetMinLevel");
    return NWNXPopInt();
}

/// Sets the minimum permitted character level, from 1 through 255.
/// @param nLevel New minimum character level.
void NWNRS_SetMinLevel(int nLevel)
{
    NWNXPushInt(nLevel);
    NWNXCall(NWNRS_NAMESPACE, "SetMinLevel");
}

/// Returns the maximum permitted character level.
int NWNRS_GetMaxLevel()
{
    NWNXCall(NWNRS_NAMESPACE, "GetMaxLevel");
    return NWNXPopInt();
}

/// Sets the maximum permitted character level, from 1 through 255.
/// @param nLevel New maximum character level.
void NWNRS_SetMaxLevel(int nLevel)
{
    NWNXPushInt(nLevel);
    NWNXCall(NWNRS_NAMESPACE, "SetMaxLevel");
}

/// Returns one server play-option value.
/// @param nOption Play-option identifier to query.
int NWNRS_GetPlayOption(NwnrsPlayOption nOption)
{
    NWNXPushInt(int(nOption));
    NWNXCall(NWNRS_NAMESPACE, "GetPlayOption");
    return NWNXPopInt();
}

/// Changes one NWNRS_PLAY_OPTION_* value.
/// @param nOption Play-option identifier to change.
/// @param nValue New option value.
void NWNRS_SetPlayOption(NwnrsPlayOption nOption, int nValue)
{
    NWNXPushInt(nValue);
    NWNXPushInt(int(nOption));
    NWNXCall(NWNRS_NAMESPACE, "SetPlayOption");
}

/// Returns one debug-output toggle.
/// @param nDebugType Debug toggle identifier to query.
int NWNRS_GetDebugValue(NwnrsDebugType nDebugType)
{
    NWNXPushInt(int(nDebugType));
    NWNXCall(NWNRS_NAMESPACE, "GetDebugValue");
    return NWNXPopInt();
}

/// Changes one NWNRS_DEBUG_* toggle to FALSE or TRUE.
/// @param nDebugType Debug toggle identifier to change.
/// @param bEnabled TRUE to enable the output; FALSE to disable it.
void NWNRS_SetDebugValue(NwnrsDebugType nDebugType, int bEnabled)
{
    NWNXPushInt(bEnabled);
    NWNXPushInt(int(nDebugType));
    NWNXCall(NWNRS_NAMESPACE, "SetDebugValue");
}

/// Requests graceful server shutdown after the current bridge call returns.
void NWNRS_RequestShutdown()
{
    NWNXCall(NWNRS_NAMESPACE, "RequestShutdown");
}

/// Returns {"ip_addresses":[],"cd_keys":[],"player_names":[]}.
json NWNRS_GetBannedList()
{
    NWNXCall(NWNRS_NAMESPACE, "GetBannedList");
    return JsonParse(NWNXPopString());
}

/// Adds an IP address to the persistent engine ban list.
/// @param sAddress IP address to ban.
void NWNRS_AddBannedIP(string sAddress)
{
    NWNXPushString(sAddress);
    NWNXCall(NWNRS_NAMESPACE, "AddBannedIp");
}

/// Removes an IP address from the persistent engine ban list.
/// @param sAddress IP address to unban.
void NWNRS_RemoveBannedIP(string sAddress)
{
    NWNXPushString(sAddress);
    NWNXCall(NWNRS_NAMESPACE, "RemoveBannedIp");
}

/// Adds a public CD key to the persistent engine ban list.
/// @param sKey Public CD key to ban.
void NWNRS_AddBannedCDKey(string sKey)
{
    NWNXPushString(sKey);
    NWNXCall(NWNRS_NAMESPACE, "AddBannedCdKey");
}

/// Removes a public CD key from the persistent engine ban list.
/// @param sKey Public CD key to unban.
void NWNRS_RemoveBannedCDKey(string sKey)
{
    NWNXPushString(sKey);
    NWNXCall(NWNRS_NAMESPACE, "RemoveBannedCdKey");
}

/// Adds a player account name to the persistent engine ban list.
/// @param sPlayerName Player account name to ban.
void NWNRS_AddBannedPlayerName(string sPlayerName)
{
    NWNXPushString(sPlayerName);
    NWNXCall(NWNRS_NAMESPACE, "AddBannedPlayerName");
}

/// Removes a player account name from the persistent engine ban list.
/// @param sPlayerName Player account name to unban.
void NWNRS_RemoveBannedPlayerName(string sPlayerName)
{
    NWNXPushString(sPlayerName);
    NWNXCall(NWNRS_NAMESPACE, "RemoveBannedPlayerName");
}

/// Reloads the engine rules tables from the active resource manager.
void NWNRS_ReloadRules()
{
    NWNXCall(NWNRS_NAMESPACE, "ReloadRules");
}

/// Disconnects oPC and removes its active server-vault BIC after this script call.
/// When bPreserveBackup is TRUE, preserves the BIC as the first available
/// .deletedN file. The operation also removes the matching in-memory TURD.
/// @param oPC Player character to disconnect and delete.
/// @param bPreserveBackup TRUE to retain a numbered deleted-character backup.
/// @param sKickMessage Message shown when the player is disconnected.
void NWNRS_DeletePlayerCharacter(
    object oPC,
    int bPreserveBackup = TRUE,
    string sKickMessage = ""
)
{
    NWNXPushString(sKickMessage);
    NWNXPushInt(bPreserveBackup);
    NWNXPushObject(oPC);
    NWNXCall(NWNRS_NAMESPACE, "DeletePlayerCharacter");
}

/// Removes a disconnected player's in-memory TURD by account and character name.
/// Returns TRUE when a matching TURD was found and removed.
/// @param sPlayerName Player account name owning the TURD.
/// @param sCharacterName Character name stored by the TURD.
int NWNRS_DeleteTURD(string sPlayerName, string sCharacterName)
{
    NWNXPushString(sCharacterName);
    NWNXPushString(sPlayerName);
    NWNXCall(NWNRS_NAMESPACE, "DeleteTURD");
    return NWNXPopInt();
}

/// Returns TRUE while an nwnrs event dispatcher is running.
int NWNRS_GetIsInEvent()
{
    NWNXCall(NWNRS_NAMESPACE, "GetIsInEvent");
    return NWNXPopInt();
}

/// Returns the immutable current event object, or JsonNull outside dispatch.
/// Every event contains name, id, script, phase, depth, target, controls, and
/// event-specific data. Object identifiers are eight-digit hexadecimal strings.
json NWNRS_GetCurrentEvent()
{
    NWNXCall(NWNRS_NAMESPACE, "GetCurrentEvent");
    return JsonParse(NWNXPopString());
}

/// Prevents the current event's original engine operation when it is skippable.
void NWNRS_SkipCurrentEvent()
{
    NWNXCall(NWNRS_NAMESPACE, "SkipCurrentEvent");
}

/// Sets the current event result when its schema supports a JSON result.
/// @param jResult Replacement result matching the current event schema.
void NWNRS_SetCurrentEventResult(json jResult)
{
    NWNXPushString(JsonDump(jResult));
    NWNXCall(NWNRS_NAMESPACE, "SetCurrentEventResult");
}

/// Returns TRUE when this exact server target supports an event annotation.
/// @param sEventIdentity Event annotation identity to query.
int NWNRS_GetEventSupported(string sEventIdentity)
{
    NWNXPushString(sEventIdentity);
    NWNXCall(NWNRS_NAMESPACE, "GetEventSupported");
    return NWNXPopInt();
}

/// Internal: generated dispatchers register subscriptions during module.load.
/// Unsupported target-pack events emit a warning and remain unsubscribed.
/// @param sEventIdentity Event annotation identity to subscribe.
/// @private
void NWNRS_SubscribeEvent(string sEventIdentity)
{
    NWNXPushString(sEventIdentity);
    NWNXCall(NWNRS_NAMESPACE, "SubscribeEvent");
}

/// Enables or disables a named integer whitelist. Enabling starts with no IDs.
/// @param sWhitelist One NWNRS_EVENT_ID_WHITELIST_* name.
/// @param bEnabled TRUE to enable filtering; FALSE to disable it.
void NWNRS_ToggleEventIdWhitelist(string sWhitelist, int bEnabled)
{
    NWNXPushInt(bEnabled);
    NWNXPushString(sWhitelist);
    NWNXCall(NWNRS_NAMESPACE, "ToggleEventIdWhitelist");
}

/// Adds an integer to an enabled event whitelist.
/// @param sWhitelist One NWNRS_EVENT_ID_WHITELIST_* name.
/// @param nId Integer identifier to allow.
void NWNRS_AddEventIdToWhitelist(string sWhitelist, int nId)
{
    NWNXPushInt(nId);
    NWNXPushString(sWhitelist);
    NWNXCall(NWNRS_NAMESPACE, "AddEventIdToWhitelist");
}

/// Removes an integer from an enabled event whitelist.
/// @param sWhitelist One NWNRS_EVENT_ID_WHITELIST_* name.
/// @param nId Integer identifier to remove.
void NWNRS_RemoveEventIdFromWhitelist(string sWhitelist, int nId)
{
    NWNXPushInt(nId);
    NWNXPushString(sWhitelist);
    NWNXCall(NWNRS_NAMESPACE, "RemoveEventIdFromWhitelist");
}
