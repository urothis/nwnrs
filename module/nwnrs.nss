/// @file nwnrs.nss
/// @brief Server identity, live state, event context, and structured logging.

const string NWNRS_NAMESPACE = "NWNRS"; ///< @private

const int NWNRS_LOG_LEVEL_TRACE = 0;
const int NWNRS_LOG_LEVEL_DEBUG = 1;
const int NWNRS_LOG_LEVEL_INFO = 2;
const int NWNRS_LOG_LEVEL_WARN = 3;
const int NWNRS_LOG_LEVEL_ERROR = 4;

/// Returns TRUE when the nwnrs NWScript bridge is installed.
int NWNRS_GetIsAvailable();

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

/// Returns the server operating system, currently "macos" or "linux".
string NWNRS_GetServerOperatingSystem();

/// Returns the server architecture, currently "aarch64" or "x86_64".
string NWNRS_GetServerArchitecture();

/// Returns the name of the currently loaded module.
string NWNRS_GetModuleName();

/// Returns the number of players currently known to the server.
int NWNRS_GetPlayerCount();

/// Returns the maximum number of players configured for the session.
int NWNRS_GetMaxPlayers();

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
