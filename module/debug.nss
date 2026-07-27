#include "nwnrs"

#[nwnrs::event::module_load::before]
void NWNRS_OnModuleLoad(json jEvent)
{
    NWNRS_Log(JsonDump(jEvent, 2), NwnrsLogLevel::Info);
}
