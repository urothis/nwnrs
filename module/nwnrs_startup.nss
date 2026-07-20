#include "nwnrs"

#[nwnrs::events(module_load)]
void NWNRS_OnModuleLoad(json jEvent)
{
    json jRuntime = JsonObject();
    jRuntime = JsonObjectSet(jRuntime, "version", JsonString(NWNRS_GetRuntimeVersion()));
    jRuntime = JsonObjectSet(jRuntime, "api_version", JsonInt(NWNRS_GetApiVersion()));

    json jServer = JsonObject();
    jServer = JsonObjectSet(jServer, "build", JsonString(NWNRS_GetServerBuild()));
    jServer = JsonObjectSet(jServer, "binary_sha256", JsonString(NWNRS_GetServerBinarySha256()));
    jServer = JsonObjectSet(jServer, "platform", JsonString(NWNRS_GetServerPlatform()));
    jServer = JsonObjectSet(jServer, "operating_system", JsonString(NWNRS_GetServerOperatingSystem()));
    jServer = JsonObjectSet(jServer, "architecture", JsonString(NWNRS_GetServerArchitecture()));
    jServer = JsonObjectSet(jServer, "name", JsonString(NWNRS_GetServerName()));
    jServer = JsonObjectSet(jServer, "port", JsonInt(NWNRS_GetServerPort()));

    json jAccess = JsonObject();
    jAccess = JsonObjectSet(jAccess, "player_password", JsonBool(NWNRS_GetIsPlayerPasswordSet()));
    jAccess = JsonObjectSet(jAccess, "dm_password", JsonBool(NWNRS_GetIsDMPasswordSet()));
    jAccess = JsonObjectSet(jAccess, "minimum_level", JsonInt(NWNRS_GetMinLevel()));
    jAccess = JsonObjectSet(jAccess, "maximum_level", JsonInt(NWNRS_GetMaxLevel()));
    jServer = JsonObjectSet(jServer, "access", jAccess);

    json jPlayers = JsonObject();
    jPlayers = JsonObjectSet(jPlayers, "current", JsonInt(NWNRS_GetPlayerCount()));
    jPlayers = JsonObjectSet(jPlayers, "maximum", JsonInt(NWNRS_GetMaxPlayers()));

    json jModule = JsonObject();
    jModule = JsonObjectSet(jModule, "name", JsonString(NWNRS_GetModuleName()));
    jModule = JsonObjectSet(jModule, "players", jPlayers);

    json jStartup = JsonObject();
    jStartup = JsonObjectSet(jStartup, "runtime", jRuntime);
    jStartup = JsonObjectSet(jStartup, "server", jServer);
    jStartup = JsonObjectSet(jStartup, "module", jModule);
    jStartup = JsonObjectSet(jStartup, "event", jEvent);

    NWNRS_Log(JsonDump(jStartup, 2), NWNRS_LOG_LEVEL_INFO);
    NWNRS_Log("nwnrs native module-load dispatcher initialized", NWNRS_LOG_LEVEL_INFO);
}
