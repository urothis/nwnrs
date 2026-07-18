#include "nwnrs"

void main()
{
    ExecuteScript("x2_mod_def_load", OBJECT_SELF);

    if (!NWNRS_GetIsAvailable())
    {
        WriteTimestampedLogEntry("nwnrs runtime is not available");
        return;
    }

    NWNRS_Log(
        "nwnrs runtime " + NWNRS_GetRuntimeVersion() +
        "; server " + NWNRS_GetServerBuild() +
        "; sha256 " + NWNRS_GetServerBinarySha256() +
        "; platform " + NWNRS_GetServerPlatform() +
        "; module " + NWNRS_GetModuleName() +
        "; players " + IntToString(NWNRS_GetPlayerCount()) +
        "/" + IntToString(NWNRS_GetMaxPlayers()) +
        "; event " + NWNRS_GetCurrentEvent() +
        " (" + IntToString(NWNRS_GetCurrentEventId()) + ")" +
        "; script " + NWNRS_GetCurrentEventScript() +
        "; phase " + NWNRS_GetCurrentEventPhase() +
        "; depth " + IntToString(NWNRS_GetCurrentEventDepth()),
        NWNRS_LOG_LEVEL_INFO
    );
}
