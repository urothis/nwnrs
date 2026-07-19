#include <cstddef>
#include <cstdint>
#include <iostream>
#include <type_traits>

struct Vector;

#define PLUGIN_NAME "NWNRS_ABI_PROBE"

#include "API/CAppManager.hpp"
#include "API/CExoArrayList.hpp"
#include "API/CExoAliasList.hpp"
#include "API/CExoBase.hpp"
#include "API/CExoLinkedListInternal.hpp"
#include "API/CExoLinkedListNode.hpp"
#include "API/CExoLocString.hpp"
#include "API/CExoString.hpp"
#include "API/CNetLayer.hpp"
#include "API/CNetLayerPlayerCDKeyInfo.hpp"
#include "API/CNetLayerPlayerInfo.hpp"
#include "API/CJoiningRestrictions.hpp"
#include "API/CPersistantWorldOptions.hpp"
#include "API/CPlayOptions.hpp"
#include "API/CVirtualMachine.hpp"
#include "API/CNWSVirtualMachineCommands.hpp"
#include "API/CNWSModule.hpp"
#include "API/CNWSPlayer.hpp"
#include "API/CNWSPlayerTURD.hpp"
#include "API/CNWSCreature.hpp"
#include "API/CNWSCreatureStats.hpp"
#include "API/CServerExoApp.hpp"
#include "API/CServerExoAppInternal.hpp"
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
#elif defined(_WIN32)
    constexpr const char* operatingSystem = "windows";
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
    static_assert(std::is_same_v<decltype(&CServerExoApp::GetModule), CNWSModule* (CServerExoApp::*)()>);
    static_assert(std::is_same_v<
        decltype(&CServerExoApp::GetClientObjectByObjectId),
        CNWSPlayer* (CServerExoApp::*)(OBJECT_ID)>);
    static_assert(std::is_same_v<
        decltype(&CServerExoApp::GetCreatureByGameObjectID),
        CNWSCreature* (CServerExoApp::*)(OBJECT_ID)>);
    static_assert(std::is_same_v<decltype(&CServerExoAppInternal::MainLoop), BOOL (CServerExoAppInternal::*)()>);
    static_assert(std::is_same_v<decltype(&CNetLayer::GetSessionMaxPlayers), uint32_t (CNetLayer::*)()>);
    static_assert(std::is_same_v<decltype(&CNetLayer::GetUDPPort), uint32_t (CNetLayer::*)()>);
    static_assert(std::is_same_v<
        decltype(&CNetLayer::GetPlayerInfo),
        CNetLayerPlayerInfo* (CNetLayer::*)(uint32_t)>);
    static_assert(std::is_same_v<
        decltype(&CNetLayer::DisconnectPlayer),
        BOOL (CNetLayer::*)(uint32_t, uint32_t, BOOL, const CExoString&)>);
    static_assert(std::is_same_v<decltype(&CNetLayer::GetSessionName), CExoString (CNetLayer::*)()>);
    static_assert(std::is_same_v<decltype(&CNetLayer::SetSessionName), void (CNetLayer::*)(CExoString)>);
    static_assert(std::is_same_v<decltype(&CNetLayer::GetPlayerPassword), CExoString (CNetLayer::*)()>);
    static_assert(std::is_same_v<decltype(&CNetLayer::SetPlayerPassword), BOOL (CNetLayer::*)(CExoString)>);
    static_assert(std::is_same_v<decltype(&CNetLayer::GetGameMasterPassword), CExoString (CNetLayer::*)()>);
    static_assert(std::is_same_v<decltype(&CNetLayer::SetGameMasterPassword), BOOL (CNetLayer::*)(CExoString)>);
    static_assert(sizeof(CPlayOptions) == 29 * sizeof(int32_t));
    static_assert(std::is_same_v<decltype(&CServerExoApp::AddIPToBannedList), void (CServerExoApp::*)(CExoString)>);
    static_assert(std::is_same_v<decltype(&CServerExoApp::RemoveIPFromBannedList), void (CServerExoApp::*)(CExoString)>);
    static_assert(std::is_same_v<decltype(&CServerExoApp::AddCDKeyToBannedList), void (CServerExoApp::*)(CExoString)>);
    static_assert(std::is_same_v<decltype(&CServerExoApp::RemoveCDKeyFromBannedList), void (CServerExoApp::*)(CExoString)>);
    static_assert(std::is_same_v<decltype(&CServerExoApp::AddPlayerNameToBannedList), void (CServerExoApp::*)(CExoString)>);
    static_assert(std::is_same_v<decltype(&CServerExoApp::RemovePlayerNameFromBannedList), void (CServerExoApp::*)(CExoString)>);
    static_assert(std::is_same_v<
        decltype(&CExoLinkedListInternal::Remove),
        void* (CExoLinkedListInternal::*)(CExoLinkedListPosition)>);
    static_assert(std::is_same_v<
        decltype(&CExoLocString::GetStringLoc),
        BOOL (CExoLocString::*)(int32_t, CExoString*, uint8_t) const>);
    static_assert(std::is_same_v<decltype(&CNWSPlayer::GetPlayerName), CExoString (CNWSPlayer::*)()>);
    static_assert(std::is_same_v<
        decltype(&CExoAliasList::GetAliasPath),
        const CExoString& (CExoAliasList::*)(const CExoString&, int32_t) const>);

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
        << "server_info_joining_restrictions_offset = "
        << offsetof(CServerInfo, m_JoiningRestrictions) << "\n"
        << "server_info_play_options_offset = " << offsetof(CServerInfo, m_PlayOptions) << "\n"
        << "server_info_persistent_world_options_offset = "
        << offsetof(CServerInfo, m_PersistantWorldOptions) << "\n"
        << "persistent_world_options_server_vault_by_player_name_offset = "
        << offsetof(CPersistantWorldOptions, bServerVaultByPlayerName) << "\n"
        << "joining_restrictions_min_level_offset = "
        << offsetof(CJoiningRestrictions, nMinLevel) << "\n"
        << "joining_restrictions_max_level_offset = "
        << offsetof(CJoiningRestrictions, nMaxLevel) << "\n"
        << "server_exo_app_internal_offset = "
        << offsetof(CServerExoApp, m_pcExoAppInternal) << "\n"
        << "internal_banned_ip_list_offset = "
        << offsetof(CServerExoAppInternal, m_lstBannedListIP) << "\n"
        << "internal_banned_cd_key_list_offset = "
        << offsetof(CServerExoAppInternal, m_lstBannedListCDKey) << "\n"
        << "internal_banned_player_name_list_offset = "
        << offsetof(CServerExoAppInternal, m_lstBannedListPlayerName) << "\n"
        << "module_turd_list_offset = " << offsetof(CNWSModule, m_lstTURDList) << "\n"
        << "player_turd_community_name_offset = "
        << offsetof(CNWSPlayerTURD, m_sCommunityName) << "\n"
        << "player_turd_first_name_offset = "
        << offsetof(CNWSPlayerTURD, m_lsFirstName) << "\n"
        << "player_turd_last_name_offset = "
        << offsetof(CNWSPlayerTURD, m_lsLastName) << "\n"
        << "linked_list_head_offset = " << offsetof(CExoLinkedListInternal, pHead) << "\n"
        << "linked_list_count_offset = " << offsetof(CExoLinkedListInternal, m_nCount) << "\n"
        << "linked_list_node_next_offset = " << offsetof(CExoLinkedListNode, pNext) << "\n"
        << "linked_list_node_object_offset = " << offsetof(CExoLinkedListNode, pObject) << "\n"
        << "player_id_offset = " << offsetof(CNWSPlayer, m_nPlayerID) << "\n"
        << "player_file_name_offset = " << offsetof(CNWSPlayer, m_resFileName) << "\n"
        << "player_file_name_size = " << sizeof(CResRef) << "\n"
        << "net_layer_player_info_cd_key_offset = "
        << offsetof(CNetLayerPlayerInfo, m_cCDKey) << "\n"
        << "player_cd_key_public_offset = "
        << offsetof(CNetLayerPlayerCDKeyInfo, sPublic) << "\n"
        << "exo_base_alias_list_offset = " << offsetof(CExoBase, m_pcExoAliasList) << "\n"
        << "creature_stats_offset = " << offsetof(CNWSCreature, m_pStats) << "\n"
        << "creature_stats_first_name_offset = "
        << offsetof(CNWSCreatureStats, m_lsFirstName) << "\n"
        << "creature_stats_last_name_offset = "
        << offsetof(CNWSCreatureStats, m_lsLastName) << "\n"
        << "vm_recursion_level_offset = " << offsetof(CVirtualMachine, m_nRecursionLevel) << "\n"
        << "vm_script_array_offset = " << offsetof(CVirtualMachine, m_pVirtualMachineScript) << "\n"
        << "vm_script_slot_count = "
        << std::extent_v<decltype(CVirtualMachine::m_pVirtualMachineScript)> << "\n"
        << "vm_script_size = " << sizeof(CVirtualMachineScript) << "\n"
        << "vm_script_alignment = " << alignof(CVirtualMachineScript) << "\n"
        << "vm_script_name_offset = " << offsetof(CVirtualMachineScript, m_sScriptName) << "\n"
        << "vm_script_event_id_offset = " << offsetof(CVirtualMachineScript, m_nScriptEventID) << "\n";
}
