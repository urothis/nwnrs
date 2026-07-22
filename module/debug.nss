#include "nwnrs"

#[nwnrs::events(module_load)]
void NWNRS_OnModuleLoad(json jEvent)
{
    NWNRS_Log(JsonDump(jEvent, 2), NwnrsLogLevel::Info);
}
