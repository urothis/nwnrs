/// @file nwnrs.nss
/// @brief Server identity, live state, administration, event context, and structured logging.

const string NWNRS_NAMESPACE = "NWNRS"; ///< @private

const int NWNRS_API_VERSION = 1;

const string NWNRS_CAPABILITY_NWSCRIPT_BRIDGE = "nwscript_bridge";
const string NWNRS_CAPABILITY_SERVER_STATE = "server_state";
const string NWNRS_CAPABILITY_ADMINISTRATION = "administration";
const string NWNRS_CAPABILITY_EVENT_CONTEXT = "event_context";

const int NWNRS_ERROR_NONE = 0;
const int NWNRS_ERROR_UNKNOWN_NAMESPACE = 1;
const int NWNRS_ERROR_UNKNOWN_FUNCTION = 2;
const int NWNRS_ERROR_INVALID_ARGUMENT = 3;
const int NWNRS_ERROR_MISSING_CAPABILITY = 4;
const int NWNRS_ERROR_ENGINE = 5;
const int NWNRS_ERROR_REENTRANT = 6;

const int NWNRS_LOG_LEVEL_TRACE = 0;
const int NWNRS_LOG_LEVEL_DEBUG = 1;
const int NWNRS_LOG_LEVEL_INFO = 2;
const int NWNRS_LOG_LEVEL_WARN = 3;
const int NWNRS_LOG_LEVEL_ERROR = 4;

// Administration play options supported by the live server.
const int NWNRS_PLAY_OPTION_PVP_SETTING = 10; // 0 = none, 1 = party, 2 = full
const int NWNRS_PLAY_OPTION_PAUSE_AND_PLAY = 11;
const int NWNRS_PLAY_OPTION_ONE_PARTY_ONLY = 12;
const int NWNRS_PLAY_OPTION_ENFORCE_LEGAL_CHARACTERS = 13;
const int NWNRS_PLAY_OPTION_ITEM_LEVEL_RESTRICTIONS = 14;
const int NWNRS_PLAY_OPTION_CDKEY_BANLIST_ALLOWLIST = 15;
const int NWNRS_PLAY_OPTION_DISALLOW_SHOUTING = 16;
const int NWNRS_PLAY_OPTION_SHOW_DM_JOIN_MESSAGE = 17;
const int NWNRS_PLAY_OPTION_BACKUP_SAVED_CHARACTERS = 18;
const int NWNRS_PLAY_OPTION_AUTO_FAIL_SAVE_ON_1 = 19;
const int NWNRS_PLAY_OPTION_VALIDATE_SPELLS = 20;
const int NWNRS_PLAY_OPTION_EXAMINE_EFFECTS = 21;
const int NWNRS_PLAY_OPTION_EXAMINE_CHALLENGE_RATING = 22;
const int NWNRS_PLAY_OPTION_USE_MAX_HITPOINTS = 23;
const int NWNRS_PLAY_OPTION_RESTORE_SPELLS_USES = 24;
const int NWNRS_PLAY_OPTION_RESET_ENCOUNTER_SPAWN_POOL = 25;
const int NWNRS_PLAY_OPTION_HIDE_HITPOINTS_GAINED = 26;
const int NWNRS_PLAY_OPTION_PLAYER_PARTY_CONTROL = 27;
const int NWNRS_PLAY_OPTION_SHOW_PLAYER_JOIN_MESSAGES = 28;

const int NWNRS_DEBUG_COMBAT = 0;
const int NWNRS_DEBUG_SAVING_THROW = 1;
const int NWNRS_DEBUG_MOVEMENT_SPEED = 2;
const int NWNRS_DEBUG_HIT_DIE = 3;

/// Returns TRUE when the nwnrs NWScript bridge is installed.
int NWNRS_GetIsAvailable();

/// Returns the integer version of the stable NWScript bridge contract.
int NWNRS_GetApiVersion();

/// Returns the supported version of a named capability, or zero when absent.
int NWNRS_GetCapabilityVersion(string sCapability);

/// Returns TRUE when a capability meets the requested minimum version.
int NWNRS_HasCapability(string sCapability, int nMinimumVersion = 1);

/// Returns the most recent NWNRS_ERROR_* value on this script thread.
int NWNRS_GetLastErrorCode();

/// Returns the diagnostic message associated with the most recent error.
string NWNRS_GetLastErrorMessage();

/// Sends a message through the runtime's structured tracing pipeline.
/// @param sMessage Message to emit.
/// @param nLevel One of the NWNRS_LOG_LEVEL_* constants.
void NWNRS_Log(string sMessage, int nLevel = NWNRS_LOG_LEVEL_INFO);

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

/// Returns the active UDP port on which the server is listening.
int NWNRS_GetServerPort();

// Administration

/// Returns the server name advertised by the network session.
string NWNRS_GetServerName();

/// Changes the server name advertised by the network session.
void NWNRS_SetServerName(string sName);

/// Changes the active module's advertised name.
void NWNRS_SetModuleName(string sName);

/// Returns TRUE when a player password is configured without exposing it.
int NWNRS_GetIsPlayerPasswordSet();

/// Sets the password required for player connections.
void NWNRS_SetPlayerPassword(string sPassword);

/// Removes the password required for player connections.
void NWNRS_ClearPlayerPassword();

/// Returns TRUE when a DM password is configured without exposing it.
int NWNRS_GetIsDMPasswordSet();

/// Sets the password required for DM connections.
void NWNRS_SetDMPassword(string sPassword);

/// Removes the password required for DM connections.
void NWNRS_ClearDMPassword();

/// Returns the minimum permitted character level.
int NWNRS_GetMinLevel();

/// Sets the minimum permitted character level, from 1 through 255.
void NWNRS_SetMinLevel(int nLevel);

/// Returns the maximum permitted character level.
int NWNRS_GetMaxLevel();

/// Sets the maximum permitted character level, from 1 through 255.
void NWNRS_SetMaxLevel(int nLevel);

/// Returns one NWNRS_PLAY_OPTION_* value.
int NWNRS_GetPlayOption(int nOption);

/// Changes one NWNRS_PLAY_OPTION_* value.
void NWNRS_SetPlayOption(int nOption, int nValue);

/// Returns one NWNRS_DEBUG_* toggle.
int NWNRS_GetDebugValue(int nDebugType);

/// Changes one NWNRS_DEBUG_* toggle to FALSE or TRUE.
void NWNRS_SetDebugValue(int nDebugType, int bEnabled);

/// Requests graceful server shutdown after the current bridge call returns.
void NWNRS_RequestShutdown();

/// Returns {"ip_addresses":[],"cd_keys":[],"player_names":[]}.
json NWNRS_GetBannedList();

/// Adds an IP address to the persistent engine ban list.
void NWNRS_AddBannedIP(string sAddress);

/// Removes an IP address from the persistent engine ban list.
void NWNRS_RemoveBannedIP(string sAddress);

/// Adds a public CD key to the persistent engine ban list.
void NWNRS_AddBannedCDKey(string sKey);

/// Removes a public CD key from the persistent engine ban list.
void NWNRS_RemoveBannedCDKey(string sKey);

/// Adds a player account name to the persistent engine ban list.
void NWNRS_AddBannedPlayerName(string sPlayerName);

/// Removes a player account name from the persistent engine ban list.
void NWNRS_RemoveBannedPlayerName(string sPlayerName);

/// Reloads the engine rules tables from the active resource manager.
void NWNRS_ReloadRules();

/// Disconnects oPC and removes its active server-vault BIC after this script call.
/// When bPreserveBackup is TRUE, preserves the BIC as the first available
/// .deletedN file. The operation also removes the matching in-memory TURD.
void NWNRS_DeletePlayerCharacter(
    object oPC,
    int bPreserveBackup = TRUE,
    string sKickMessage = ""
);

/// Removes a disconnected player's in-memory TURD by account and character name.
/// Returns TRUE when a matching TURD was found and removed.
int NWNRS_DeleteTURD(string sPlayerName, string sCharacterName);

/// Returns TRUE while an engine module, area, or object event script is running.
int NWNRS_GetIsInEvent();

/// Returns the stable semantic name of the current event, or "" outside one.
string NWNRS_GetCurrentEvent();

/// Returns the engine EVENT_SCRIPT_* identifier, or -1 outside an event.
int NWNRS_GetCurrentEventId();

/// Returns the current event script resref, or "" outside an event.
string NWNRS_GetCurrentEventScript();

/// Returns "running" while an event script is active, or "" outside one.
string NWNRS_GetCurrentEventPhase();

/// Returns the one-based VM script depth, or zero outside an event.
int NWNRS_GetCurrentEventDepth();

int NWNRS_GetIsAvailable()
{
    return NWNXGetIsAvailable();
}

int NWNRS_GetApiVersion()
{
    NWNXCall(NWNRS_NAMESPACE, "GetApiVersion");
    return NWNXPopInt();
}

int NWNRS_GetCapabilityVersion(string sCapability)
{
    NWNXPushString(sCapability);
    NWNXCall(NWNRS_NAMESPACE, "GetCapabilityVersion");
    return NWNXPopInt();
}

int NWNRS_HasCapability(string sCapability, int nMinimumVersion = 1)
{
    NWNXPushInt(nMinimumVersion);
    NWNXPushString(sCapability);
    NWNXCall(NWNRS_NAMESPACE, "HasCapability");
    return NWNXPopInt();
}

int NWNRS_GetLastErrorCode()
{
    NWNXCall(NWNRS_NAMESPACE, "GetLastErrorCode");
    return NWNXPopInt();
}

string NWNRS_GetLastErrorMessage()
{
    NWNXCall(NWNRS_NAMESPACE, "GetLastErrorMessage");
    return NWNXPopString();
}

void NWNRS_Log(string sMessage, int nLevel = NWNRS_LOG_LEVEL_INFO)
{
    NWNXPushInt(nLevel);
    NWNXPushString(sMessage);
    NWNXCall(NWNRS_NAMESPACE, "Log");
}

string NWNRS_GetRuntimeVersion()
{
    NWNXCall(NWNRS_NAMESPACE, "GetRuntimeVersion");
    return NWNXPopString();
}

string NWNRS_GetServerBinarySha256()
{
    NWNXCall(NWNRS_NAMESPACE, "GetServerBinarySha256");
    return NWNXPopString();
}

string NWNRS_GetServerBuild()
{
    NWNXCall(NWNRS_NAMESPACE, "GetServerBuild");
    return NWNXPopString();
}

string NWNRS_GetServerPlatform()
{
    NWNXCall(NWNRS_NAMESPACE, "GetServerPlatform");
    return NWNXPopString();
}

string NWNRS_GetServerOperatingSystem()
{
    NWNXCall(NWNRS_NAMESPACE, "GetServerOperatingSystem");
    return NWNXPopString();
}

string NWNRS_GetServerArchitecture()
{
    NWNXCall(NWNRS_NAMESPACE, "GetServerArchitecture");
    return NWNXPopString();
}

string NWNRS_GetModuleName()
{
    NWNXCall(NWNRS_NAMESPACE, "GetModuleName");
    return NWNXPopString();
}

int NWNRS_GetPlayerCount()
{
    NWNXCall(NWNRS_NAMESPACE, "GetPlayerCount");
    return NWNXPopInt();
}

int NWNRS_GetMaxPlayers()
{
    NWNXCall(NWNRS_NAMESPACE, "GetMaxPlayers");
    return NWNXPopInt();
}

int NWNRS_GetServerPort()
{
    NWNXCall(NWNRS_NAMESPACE, "GetServerPort");
    return NWNXPopInt();
}

string NWNRS_GetServerName()
{
    NWNXCall(NWNRS_NAMESPACE, "GetServerName");
    return NWNXPopString();
}

void NWNRS_SetServerName(string sName)
{
    NWNXPushString(sName);
    NWNXCall(NWNRS_NAMESPACE, "SetServerName");
}

void NWNRS_SetModuleName(string sName)
{
    NWNXPushString(sName);
    NWNXCall(NWNRS_NAMESPACE, "SetModuleName");
}

int NWNRS_GetIsPlayerPasswordSet()
{
    NWNXCall(NWNRS_NAMESPACE, "GetIsPlayerPasswordSet");
    return NWNXPopInt();
}

void NWNRS_SetPlayerPassword(string sPassword)
{
    NWNXPushString(sPassword);
    NWNXCall(NWNRS_NAMESPACE, "SetPlayerPassword");
}

void NWNRS_ClearPlayerPassword()
{
    NWNXCall(NWNRS_NAMESPACE, "ClearPlayerPassword");
}

int NWNRS_GetIsDMPasswordSet()
{
    NWNXCall(NWNRS_NAMESPACE, "GetIsDmPasswordSet");
    return NWNXPopInt();
}

void NWNRS_SetDMPassword(string sPassword)
{
    NWNXPushString(sPassword);
    NWNXCall(NWNRS_NAMESPACE, "SetDmPassword");
}

void NWNRS_ClearDMPassword()
{
    NWNXCall(NWNRS_NAMESPACE, "ClearDmPassword");
}

int NWNRS_GetMinLevel()
{
    NWNXCall(NWNRS_NAMESPACE, "GetMinLevel");
    return NWNXPopInt();
}

void NWNRS_SetMinLevel(int nLevel)
{
    NWNXPushInt(nLevel);
    NWNXCall(NWNRS_NAMESPACE, "SetMinLevel");
}

int NWNRS_GetMaxLevel()
{
    NWNXCall(NWNRS_NAMESPACE, "GetMaxLevel");
    return NWNXPopInt();
}

void NWNRS_SetMaxLevel(int nLevel)
{
    NWNXPushInt(nLevel);
    NWNXCall(NWNRS_NAMESPACE, "SetMaxLevel");
}

int NWNRS_GetPlayOption(int nOption)
{
    NWNXPushInt(nOption);
    NWNXCall(NWNRS_NAMESPACE, "GetPlayOption");
    return NWNXPopInt();
}

void NWNRS_SetPlayOption(int nOption, int nValue)
{
    NWNXPushInt(nValue);
    NWNXPushInt(nOption);
    NWNXCall(NWNRS_NAMESPACE, "SetPlayOption");
}

int NWNRS_GetDebugValue(int nDebugType)
{
    NWNXPushInt(nDebugType);
    NWNXCall(NWNRS_NAMESPACE, "GetDebugValue");
    return NWNXPopInt();
}

void NWNRS_SetDebugValue(int nDebugType, int bEnabled)
{
    NWNXPushInt(bEnabled);
    NWNXPushInt(nDebugType);
    NWNXCall(NWNRS_NAMESPACE, "SetDebugValue");
}

void NWNRS_RequestShutdown()
{
    NWNXCall(NWNRS_NAMESPACE, "RequestShutdown");
}

json NWNRS_GetBannedList()
{
    NWNXCall(NWNRS_NAMESPACE, "GetBannedList");
    return JsonParse(NWNXPopString());
}

void NWNRS_AddBannedIP(string sAddress)
{
    NWNXPushString(sAddress);
    NWNXCall(NWNRS_NAMESPACE, "AddBannedIp");
}

void NWNRS_RemoveBannedIP(string sAddress)
{
    NWNXPushString(sAddress);
    NWNXCall(NWNRS_NAMESPACE, "RemoveBannedIp");
}

void NWNRS_AddBannedCDKey(string sKey)
{
    NWNXPushString(sKey);
    NWNXCall(NWNRS_NAMESPACE, "AddBannedCdKey");
}

void NWNRS_RemoveBannedCDKey(string sKey)
{
    NWNXPushString(sKey);
    NWNXCall(NWNRS_NAMESPACE, "RemoveBannedCdKey");
}

void NWNRS_AddBannedPlayerName(string sPlayerName)
{
    NWNXPushString(sPlayerName);
    NWNXCall(NWNRS_NAMESPACE, "AddBannedPlayerName");
}

void NWNRS_RemoveBannedPlayerName(string sPlayerName)
{
    NWNXPushString(sPlayerName);
    NWNXCall(NWNRS_NAMESPACE, "RemoveBannedPlayerName");
}

void NWNRS_ReloadRules()
{
    NWNXCall(NWNRS_NAMESPACE, "ReloadRules");
}

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

int NWNRS_DeleteTURD(string sPlayerName, string sCharacterName)
{
    NWNXPushString(sCharacterName);
    NWNXPushString(sPlayerName);
    NWNXCall(NWNRS_NAMESPACE, "DeleteTURD");
    return NWNXPopInt();
}

int NWNRS_GetIsInEvent()
{
    NWNXCall(NWNRS_NAMESPACE, "GetIsInEvent");
    return NWNXPopInt();
}

string NWNRS_GetCurrentEvent()
{
    NWNXCall(NWNRS_NAMESPACE, "GetCurrentEvent");
    return NWNXPopString();
}

int NWNRS_GetCurrentEventId()
{
    NWNXCall(NWNRS_NAMESPACE, "GetCurrentEventId");
    return NWNXPopInt();
}

string NWNRS_GetCurrentEventScript()
{
    NWNXCall(NWNRS_NAMESPACE, "GetCurrentEventScript");
    return NWNXPopString();
}

string NWNRS_GetCurrentEventPhase()
{
    NWNXCall(NWNRS_NAMESPACE, "GetCurrentEventPhase");
    return NWNXPopString();
}

int NWNRS_GetCurrentEventDepth()
{
    NWNXCall(NWNRS_NAMESPACE, "GetCurrentEventDepth");
    return NWNXPopInt();
}
