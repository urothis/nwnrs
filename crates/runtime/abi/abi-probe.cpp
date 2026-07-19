#include <cstddef>
#include <cstdint>
#include <iostream>
#include <type_traits>

struct Vector;

#include "API/CAppManager.hpp"
#include "API/CExoArrayList.hpp"
#include "API/CExoString.hpp"
#include "API/CNetLayer.hpp"
#include "API/CVirtualMachine.hpp"
#include "API/CNWSVirtualMachineCommands.hpp"
#include "API/CServerExoApp.hpp"
#include "API/CServerInfo.hpp"
#include "API/CVirtualMachineCmdImplementer.hpp"
#include "API/CVirtualMachineScript.hpp"
#include "API/Vector.hpp"

#ifndef NWNRS_UNIFIED_COMMIT
#error "NWNRS_UNIFIED_COMMIT must name the exact Unified commit"
#endif
#if !defined(NWNX_TARGET_NWN_BUILD) || !defined(NWNX_TARGET_NWN_BUILD_REVISION) || !defined(NWNX_TARGET_NWN_BUILD_POSTFIX)
#error "Unified NWN build definitions are required"
#endif

struct CNWSPlayer;
using PlayerList = CExoArrayList<CNWSPlayer*>;

int main()
{
#if defined(__APPLE__)
    constexpr const char* operatingSystem = "macos";
#elif defined(__linux__)
    constexpr const char* operatingSystem = "linux";
#else
#error "unsupported ABI probe operating system"
#endif

#if defined(__aarch64__) || defined(_M_ARM64)
    constexpr const char* architecture = "aarch64";
#elif defined(__x86_64__) || defined(_M_X64)
    constexpr const char* architecture = "x86_64";
#else
#error "unsupported ABI probe architecture"
#endif

    static_assert(sizeof(void*) == 8, "nwnrs supports only 64-bit server ABIs");
    static_assert(sizeof(ObjectID) == 4, "Unified ObjectID must remain uint32_t");
    static_assert(sizeof(Vector) == 12, "Unified Vector must contain three floats");
    static_assert(std::is_same_v<
        decltype(&CNWSVirtualMachineCommands::ExecuteCommandNWNXFunctionManagement),
        int32_t (CNWSVirtualMachineCommands::*)(int32_t, int32_t)>);
    static_assert(std::is_same_v<decltype(&CVirtualMachine::StackPopInteger), BOOL (CVirtualMachine::*)(int32_t*)>);
    static_assert(std::is_same_v<decltype(&CVirtualMachine::StackPushInteger), BOOL (CVirtualMachine::*)(int32_t)>);
    static_assert(std::is_same_v<decltype(&CVirtualMachine::StackPopFloat), BOOL (CVirtualMachine::*)(float*)>);
    static_assert(std::is_same_v<decltype(&CVirtualMachine::StackPushFloat), BOOL (CVirtualMachine::*)(float)>);
    static_assert(std::is_same_v<decltype(&CVirtualMachine::StackPopObject), BOOL (CVirtualMachine::*)(OBJECT_ID*)>);
    static_assert(std::is_same_v<decltype(&CVirtualMachine::StackPushObject), BOOL (CVirtualMachine::*)(OBJECT_ID)>);
    static_assert(std::is_same_v<decltype(&CVirtualMachine::StackPopString), BOOL (CVirtualMachine::*)(CExoString*)>);
    static_assert(std::is_same_v<
        decltype(static_cast<BOOL (CVirtualMachine::*)(const CExoString&)>(&CVirtualMachine::StackPushString)),
        BOOL (CVirtualMachine::*)(const CExoString&)>);
    static_assert(std::is_same_v<decltype(&CVirtualMachine::StackPopVector), BOOL (CVirtualMachine::*)(Vector*)>);
    static_assert(std::is_same_v<decltype(&CVirtualMachine::StackPushVector), BOOL (CVirtualMachine::*)(Vector)>);
    static_assert(std::is_same_v<decltype(&CServerExoApp::GetServerInfo), CServerInfo* (CServerExoApp::*)()>);
    static_assert(std::is_same_v<decltype(&CServerExoApp::GetPlayerList), const PlayerList& (CServerExoApp::*)()>);
    static_assert(std::is_same_v<decltype(&CServerExoApp::GetNetLayer), CNetLayer* (CServerExoApp::*)()>);
    static_assert(std::is_same_v<decltype(&CNetLayer::GetSessionMaxPlayers), uint32_t (CNetLayer::*)()>);
    static_assert(std::is_same_v<decltype(&CNetLayer::GetUDPPort), uint32_t (CNetLayer::*)()>);

    std::cout
        << "schema_version = 1\n"
        << "pointer_width = " << sizeof(void*) * 8 << "\n\n"
        << "[source]\n"
        << "unified_commit = \"" << NWNRS_UNIFIED_COMMIT << "\"\n"
        << "nwn_build = " << NWNX_TARGET_NWN_BUILD << "\n"
        << "nwn_revision = " << NWNX_TARGET_NWN_BUILD_REVISION << "\n"
        << "nwn_postfix = " << NWNX_TARGET_NWN_BUILD_POSTFIX << "\n\n"
        << "[platform]\n"
        << "os = \"" << operatingSystem << "\"\n"
        << "architecture = \"" << architecture << "\"\n\n"
        << "[layouts.c_exo_string]\n"
        << "size = " << sizeof(CExoString) << "\n"
        << "alignment = " << alignof(CExoString) << "\n"
        << "string_offset = " << offsetof(CExoString, m_sString) << "\n"
        << "string_length_offset = " << offsetof(CExoString, m_nStringLength) << "\n"
        << "buffer_length_offset = " << offsetof(CExoString, m_nBufferLength) << "\n\n"
        << "[layouts.player_list]\n"
        << "size = " << sizeof(PlayerList) << "\n"
        << "alignment = " << alignof(PlayerList) << "\n"
        << "elements_offset = " << offsetof(PlayerList, element) << "\n"
        << "count_offset = " << offsetof(PlayerList, num) << "\n"
        << "capacity_offset = " << offsetof(PlayerList, array_size) << "\n\n"
        << "[layouts.vector]\n"
        << "size = " << sizeof(Vector) << "\n"
        << "alignment = " << alignof(Vector) << "\n"
        << "x_offset = " << offsetof(Vector, x) << "\n"
        << "y_offset = " << offsetof(Vector, y) << "\n"
        << "z_offset = " << offsetof(Vector, z) << "\n\n"
        << "[layouts.classes]\n"
        << "command_implementer_vm_offset = "
        << offsetof(CVirtualMachineCmdImplementer, m_pVM) << "\n"
        << "app_manager_server_offset = " << offsetof(CAppManager, m_pServerExoApp) << "\n"
        << "server_info_module_offset = " << offsetof(CServerInfo, m_sModuleName) << "\n"
        << "vm_recursion_level_offset = " << offsetof(CVirtualMachine, m_nRecursionLevel) << "\n"
        << "vm_script_array_offset = " << offsetof(CVirtualMachine, m_pVirtualMachineScript) << "\n"
        << "vm_script_slot_count = "
        << std::extent_v<decltype(CVirtualMachine::m_pVirtualMachineScript)> << "\n"
        << "vm_script_size = " << sizeof(CVirtualMachineScript) << "\n"
        << "vm_script_alignment = " << alignof(CVirtualMachineScript) << "\n"
        << "vm_script_name_offset = " << offsetof(CVirtualMachineScript, m_sScriptName) << "\n"
        << "vm_script_event_id_offset = " << offsetof(CVirtualMachineScript, m_nScriptEventID) << "\n";
}
