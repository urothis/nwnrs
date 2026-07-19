#include "nwnrs"

void main()
{
    ExecuteScript("x2_mod_def_load", OBJECT_SELF);

    if (!NWNRS_GetIsAvailable())
    {
        WriteTimestampedLogEntry("nwnrs runtime is not available");
        return;
    }
    if (NWNRS_GetApiVersion() != NWNRS_API_VERSION)
    {
        NWNRS_Log("nwnrs NWScript API version mismatch", NWNRS_LOG_LEVEL_ERROR);
        return;
    }
    if (!NWNRS_HasCapability(NWNRS_CAPABILITY_SERVER_STATE) ||
        !NWNRS_HasCapability(NWNRS_CAPABILITY_EVENT_CONTEXT))
    {
        NWNRS_Log("nwnrs target pack lacks module initialization capabilities", NWNRS_LOG_LEVEL_ERROR);
        return;
    }

    json jRuntime = JsonObject();
    jRuntime = JsonObjectSet(jRuntime, "version", JsonString(NWNRS_GetRuntimeVersion()));
    jRuntime = JsonObjectSet(jRuntime, "api_version", JsonInt(NWNRS_GetApiVersion()));

    json jServer = JsonObject();
    jServer = JsonObjectSet(jServer, "build", JsonString(NWNRS_GetServerBuild()));
    jServer = JsonObjectSet(jServer, "binary_sha256", JsonString(NWNRS_GetServerBinarySha256()));
    jServer = JsonObjectSet(jServer, "platform", JsonString(NWNRS_GetServerPlatform()));
    jServer = JsonObjectSet(jServer, "operating_system", JsonString(NWNRS_GetServerOperatingSystem()));
    jServer = JsonObjectSet(jServer, "architecture", JsonString(NWNRS_GetServerArchitecture()));
    jServer = JsonObjectSet(jServer, "port", JsonInt(NWNRS_GetServerPort()));

    json jPlayers = JsonObject();
    jPlayers = JsonObjectSet(jPlayers, "current", JsonInt(NWNRS_GetPlayerCount()));
    jPlayers = JsonObjectSet(jPlayers, "maximum", JsonInt(NWNRS_GetMaxPlayers()));

    json jModule = JsonObject();
    jModule = JsonObjectSet(jModule, "name", JsonString(NWNRS_GetModuleName()));
    jModule = JsonObjectSet(jModule, "players", jPlayers);

    json jEvent = JsonObject();
    jEvent = JsonObjectSet(jEvent, "name", JsonString(NWNRS_GetCurrentEvent()));
    jEvent = JsonObjectSet(jEvent, "id", JsonInt(NWNRS_GetCurrentEventId()));
    jEvent = JsonObjectSet(jEvent, "script", JsonString(NWNRS_GetCurrentEventScript()));
    jEvent = JsonObjectSet(jEvent, "phase", JsonString(NWNRS_GetCurrentEventPhase()));
    jEvent = JsonObjectSet(jEvent, "depth", JsonInt(NWNRS_GetCurrentEventDepth()));

    json jStartup = JsonObject();
    jStartup = JsonObjectSet(jStartup, "runtime", jRuntime);
    jStartup = JsonObjectSet(jStartup, "server", jServer);
    jStartup = JsonObjectSet(jStartup, "module", jModule);
    jStartup = JsonObjectSet(jStartup, "event", jEvent);

    NWNRS_Log(JsonDump(jStartup, 2), NWNRS_LOG_LEVEL_INFO);
}
