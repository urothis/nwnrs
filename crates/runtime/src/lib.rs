#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

mod bridge;
mod event_catalog;
mod identity;
mod platform;

use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    error::Error,
    fmt,
    fs::{self, File},
    io::{Read, Seek as _, SeekFrom},
    path::{Path, PathBuf},
};

pub use bridge::{
    AdministrationCommand, BannedLists, BridgeError, BridgeErrorCode, BridgeFunction, BridgeResult,
    BridgeValue, EventCommand, EventControls, EventLocation, EventObjectId, EventPayload,
    EventValue, EventVector, HostCommandResult, HostQuery, HostValue, RuntimeHost, ScriptBridge,
    ScriptLog, ScriptLogLevel, Vector,
};
pub use event_catalog::{
    EVENT_CATALOG, EventDefinition, EventResultKind, FEAT_HAS_ID_WHITELIST,
    PROJECTILE_SPELL_ID_WHITELIST, PROJECTILE_TYPE_ID_WHITELIST, event_definition,
    runtime_event_definition,
};
pub use identity::{BinaryIdentity, FileSha256};
pub use platform::{Architecture, OperatingSystem, Platform};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

/// The supported target-pack schema version.
pub const TARGET_PACK_SCHEMA_VERSION: u32 = 2;
/// The runtime API implemented by this version of the crate.
pub const RUNTIME_API_VERSION: u32 = 1;
/// The supported machine-generated Unified ABI snapshot format.
pub const ABI_SNAPSHOT_SCHEMA_VERSION: u32 = 1;
/// Enables initialization when set to `1` in an injected process.
pub const ENV_ENABLED: &str = "NWNRS_ENABLED";
/// Makes initialization failure fatal when set to `1`.
pub const ENV_REQUIRED: &str = "NWNRS_REQUIRED";
/// Indicates that a supervising launcher owns final diagnostic rendering.
pub const ENV_SUPERVISED: &str = "NWNRS_SUPERVISED";
/// Names one exact target-pack file selected by the launcher.
pub const ENV_TARGET_PACK: &str = "NWNRS_TARGET_PACK";
/// Names the root directory used for hash-based target-pack lookup.
pub const ENV_TARGET_DIR: &str = "NWNRS_TARGET_DIR";
/// Enables the native Windows NWServer control panel when set to `1`.
///
/// The supervised Windows launcher is headless by default. This variable is
/// launcher-to-runtime plumbing; users normally select it with `nwnrs run
/// --gui`.
pub const ENV_WINDOWS_GUI: &str = "NWNRS_WINDOWS_GUI";

/// An error produced while identifying or configuring the runtime.
///
/// ```
/// let error: Option<nwnrs_runtime::RuntimeError> = None;
/// assert!(error.is_none());
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeError {
    message: String,
}

impl RuntimeError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for RuntimeError {}

/// A result returned by runtime identification and configuration operations.
pub type RuntimeResult<T> = Result<T, RuntimeError>;

/// Metadata binding one target pack to one exact server binary.
///
/// ```
/// use nwnrs_runtime::{Architecture, OperatingSystem, Platform, TargetServer};
/// let server = TargetServer {
///     sha256: "0".repeat(64),
///     platform: Platform {
///         os: OperatingSystem::Linux,
///         architecture: Architecture::X86_64,
///     },
///     build: None,
/// };
/// assert_eq!(server.sha256.len(), 64);
/// ```
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TargetServer {
    /// Complete lowercase SHA-256 of the server binary.
    pub sha256:   String,
    /// Operating system and architecture expected by the hook definitions.
    pub platform: Platform,
    /// Human-readable server build associated with this exact binary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build:    Option<String>,
}

/// One address resolved within the main server executable.
///
/// Symbols are useful when the executable retains a trustworthy symbol table.
/// Module-relative offsets remain available for stripped executables.
///
/// ```
/// let address = nwnrs_runtime::TargetAddress::Symbol {
///     symbol: "engine_symbol".to_string(),
/// };
/// assert!(matches!(address, nwnrs_runtime::TargetAddress::Symbol { .. }));
/// ```
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, untagged)]
pub enum TargetAddress {
    /// Resolve a symbol by its exact native name.
    Symbol {
        /// Exact symbol name recorded by the target pack.
        symbol: String,
    },
    /// Add an offset to the main executable module's load address.
    Offset {
        /// Module-relative virtual address.
        offset: u64,
    },
}

/// Provenance for the Unified declarations used to derive one target ABI.
///
/// ```
/// let source: Option<nwnrs_runtime::TargetSource> = None;
/// assert!(source.is_none());
/// ```
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TargetSource {
    /// Full Git commit of `nwnxee/unified` used as the semantic source.
    pub unified_commit: String,
    /// Numeric NWN build declared by Unified.
    pub nwn_build:      u32,
    /// Numeric NWN build revision declared by Unified.
    pub nwn_revision:   u32,
    /// Numeric NWN build postfix declared by Unified.
    pub nwn_postfix:    u32,
}

/// `CExoString` object layout derived from Unified.
///
/// ```
/// let layout: Option<nwnrs_runtime::CExoStringLayout> = None;
/// assert!(layout.is_none());
/// ```
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CExoStringLayout {
    /// Complete object size.
    pub size:                 u64,
    /// Object alignment.
    pub alignment:            u64,
    /// Offset of `m_sString`.
    pub string_offset:        u64,
    /// Offset of `m_nStringLength`.
    pub string_length_offset: u64,
    /// Offset of `m_nBufferLength`.
    pub buffer_length_offset: u64,
}

/// `CExoArrayList<CNWSPlayer*>` header layout derived from Unified.
///
/// ```
/// let layout: Option<nwnrs_runtime::PlayerListLayout> = None;
/// assert!(layout.is_none());
/// ```
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PlayerListLayout {
    /// Complete header size.
    pub size:            u64,
    /// Header alignment.
    pub alignment:       u64,
    /// Offset of the element pointer.
    pub elements_offset: u64,
    /// Offset of the live element count.
    pub count_offset:    u64,
    /// Offset of the allocated element count.
    pub capacity_offset: u64,
}

/// `Vector` layout derived from Unified.
///
/// ```
/// let layout: Option<nwnrs_runtime::VectorLayout> = None;
/// assert!(layout.is_none());
/// ```
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VectorLayout {
    /// Complete object size.
    pub size:      u64,
    /// Object alignment.
    pub alignment: u64,
    /// Offset of `x`.
    pub x_offset:  u64,
    /// Offset of `y`.
    pub y_offset:  u64,
    /// Offset of `z`.
    pub z_offset:  u64,
}

/// Engine class member offsets derived from Unified and the platform C++ ABI.
///
/// ```
/// let layouts: Option<nwnrs_runtime::EngineClassLayouts> = None;
/// assert!(layouts.is_none());
/// ```
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EngineClassLayouts {
    /// Offset of `CGameObject::m_idSelf` from its primary object pointer.
    pub game_object_id_offset: u64,
    /// Offset of `CGameObject::m_nObjectType`, when event hooks for this target
    /// use it.
    pub game_object_type_offset: Option<u64>,
    /// Offset of `CItemRepository::m_oidParent`, when repository events are
    /// supported.
    pub item_repository_parent_offset: Option<u64>,
    /// Offset of `CNWSCreatureStats::m_pBaseCreature`, when stats events are
    /// supported.
    pub creature_stats_base_creature_offset: Option<u64>,
    /// Offset of `CNWSCreatureStats::m_nExperience`, when experience events are
    /// supported.
    pub creature_stats_experience_offset: Option<u64>,
    /// Offset of `CNWSItem::m_nBaseItem`, when item-result validation is
    /// supported.
    pub item_base_item_offset: Option<u64>,
    /// Offset of `CNWSItem::m_oidPossessor`, when item-result validation is
    /// supported.
    pub item_possessor_offset: Option<u64>,
    /// Offset of `CNWMessage::m_pnReadBuffer`, when client-message events are
    /// supported.
    pub message_read_buffer_offset: Option<u64>,
    /// Offset of `CNWMessage::m_nReadBufferSize`, when client-message events
    /// are supported.
    pub message_read_buffer_size_offset: Option<u64>,
    /// Offset of `CNWMessage::m_nReadBufferPtr`, when client-message events are
    /// supported.
    pub message_read_buffer_position_offset: Option<u64>,
    /// Offset of `CNWMessage::m_nReadFragmentsBufferSize`, when message
    /// cancellation is supported.
    pub message_read_fragments_size_offset: Option<u64>,
    /// Offset of `CNWMessage::m_nReadFragmentsBufferPtr`, when message
    /// cancellation is supported.
    pub message_read_fragments_position_offset: Option<u64>,
    /// Offset of `CNWMessage::m_nCurReadBit`, when message cancellation is
    /// supported.
    pub message_current_read_bit_offset: Option<u64>,
    /// Offset of `CNWMessage::m_nLastByteBits`, when message cancellation is
    /// supported.
    pub message_last_byte_bits_offset: Option<u64>,
    /// Offset of `CNWSPlayer::m_oidNWSObject`, when inventory UI events are
    /// supported.
    pub player_object_id_offset: Option<u64>,
    /// Offset of `CNWSPlayer::m_pInventoryGUI`, when inventory UI events are
    /// supported.
    pub player_inventory_gui_offset: Option<u64>,
    /// Offset of `CNWSPlayer::m_pOtherInventoryGUI`, when inventory UI events
    /// are supported.
    pub player_other_inventory_gui_offset: Option<u64>,
    /// Offset of `CNWSPlayerInventoryGUI::m_nSelectedInventoryPanel`.
    pub inventory_gui_selected_panel_offset: Option<u64>,
    /// Offset of `CVirtualMachineCmdImplementer::m_pVM`.
    pub command_implementer_vm_offset: u64,
    /// Offset of `CAppManager::m_pServerExoApp`.
    pub app_manager_server_offset: u64,
    /// Offset of `CServerInfo::m_sModuleName`.
    pub server_info_module_offset: u64,
    /// Offset of `CServerInfo::m_JoiningRestrictions`.
    pub server_info_joining_restrictions_offset: u64,
    /// Offset of `CServerInfo::m_PlayOptions`.
    pub server_info_play_options_offset: u64,
    /// Offset of `CServerInfo::m_PersistantWorldOptions`.
    pub server_info_persistent_world_options_offset: u64,
    /// Offset of `CPersistantWorldOptions::bServerVaultByPlayerName`.
    pub persistent_world_options_server_vault_by_player_name_offset: u64,
    /// Offset of `CJoiningRestrictions::nMinLevel`.
    pub joining_restrictions_min_level_offset: u64,
    /// Offset of `CJoiningRestrictions::nMaxLevel`.
    pub joining_restrictions_max_level_offset: u64,
    /// Offset of `CServerExoApp::m_pcExoAppInternal`.
    pub server_exo_app_internal_offset: u64,
    /// Offset of `CServerExoAppInternal::m_lstBannedListIP`.
    pub internal_banned_ip_list_offset: u64,
    /// Offset of `CServerExoAppInternal::m_lstBannedListCDKey`.
    pub internal_banned_cd_key_list_offset: u64,
    /// Offset of `CServerExoAppInternal::m_lstBannedListPlayerName`.
    pub internal_banned_player_name_list_offset: u64,
    /// Offset of `CNWSModule::m_lstTURDList`.
    pub module_turd_list_offset: u64,
    /// Offset of `CNWSPlayerTURD::m_sCommunityName`.
    pub player_turd_community_name_offset: u64,
    /// Offset of `CNWSPlayerTURD::m_lsFirstName`.
    pub player_turd_first_name_offset: u64,
    /// Offset of `CNWSPlayerTURD::m_lsLastName`.
    pub player_turd_last_name_offset: u64,
    /// Offset of `CExoLinkedListInternal::pHead`.
    pub linked_list_head_offset: u64,
    /// Offset of `CExoLinkedListInternal::m_nCount`.
    pub linked_list_count_offset: u64,
    /// Offset of `CExoLinkedListNode::pNext`.
    pub linked_list_node_next_offset: u64,
    /// Offset of `CExoLinkedListNode::pObject`.
    pub linked_list_node_object_offset: u64,
    /// Offset of `CNWSPlayer::m_nPlayerID`.
    pub player_id_offset: u64,
    /// Offset of `CNWSPlayer::m_resFileName`.
    pub player_file_name_offset: u64,
    /// Complete size of `CResRef`.
    pub player_file_name_size: u64,
    /// Offset of `CNetLayerPlayerInfo::m_cCDKey`.
    pub net_layer_player_info_cd_key_offset: u64,
    /// Offset of `CNetLayerPlayerCDKeyInfo::sPublic`.
    pub player_cd_key_public_offset: u64,
    /// Offset of `CExoBase::m_pcExoAliasList`.
    pub exo_base_alias_list_offset: u64,
    /// Offset of `CNWSCreature::m_pStats`.
    pub creature_stats_offset: u64,
    /// Offset of `CNWSCreatureStats::m_lsFirstName`.
    pub creature_stats_first_name_offset: u64,
    /// Offset of `CNWSCreatureStats::m_lsLastName`.
    pub creature_stats_last_name_offset: u64,
    /// Offset of `CVirtualMachine::m_nRecursionLevel`.
    pub vm_recursion_level_offset: u64,
    /// Offset of `CVirtualMachine::m_pVirtualMachineScript`.
    pub vm_script_array_offset: u64,
    /// Number of virtual-machine script slots.
    pub vm_script_slot_count: u32,
    /// Complete size of `CVirtualMachineScript`.
    pub vm_script_size: u64,
    /// Alignment of `CVirtualMachineScript`.
    pub vm_script_alignment: u64,
    /// Offset of `CVirtualMachineScript::m_sScriptName`.
    pub vm_script_name_offset: u64,
    /// Offset of `CVirtualMachineScript::m_nScriptEventID`.
    pub vm_script_event_id_offset: u64,
}

/// Complete native layouts used by one exact target pack.
///
/// ```
/// let layouts: Option<nwnrs_runtime::AbiLayouts> = None;
/// assert!(layouts.is_none());
/// ```
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AbiLayouts {
    /// `CExoString` layout.
    pub c_exo_string: CExoStringLayout,
    /// `CExoArrayList<CNWSPlayer*>` layout.
    pub player_list:  PlayerListLayout,
    /// `Vector` layout.
    pub vector:       VectorLayout,
    /// Engine class member offsets.
    pub classes:      EngineClassLayouts,
}

/// A machine-generated ABI snapshot emitted from the pinned Unified headers.
///
/// ```
/// let snapshot: Option<nwnrs_runtime::AbiSnapshot> = None;
/// assert!(snapshot.is_none());
/// ```
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AbiSnapshot {
    /// Snapshot format version.
    pub schema_version: u32,
    /// Unified source provenance.
    pub source:         TargetSource,
    /// Platform for which the C++ compiler emitted the layout.
    pub platform:       Platform,
    /// Pointer width in bits.
    pub pointer_width:  u32,
    /// Derived native layouts.
    pub layouts:        AbiLayouts,
}

/// Stable runtime capability domains exposed through NWScript.
///
/// ```
/// assert_eq!(nwnrs_runtime::Capability::ServerState.name(), "server_state");
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Capability {
    /// Core NWScript value transport and identity functions.
    NwscriptBridge,
    /// Live server module and player state.
    ServerState,
    /// Runtime administration controls.
    Administration,
}

impl Capability {
    /// Returns the stable external capability name.
    ///
    /// ```
    /// assert_eq!(nwnrs_runtime::Capability::ServerState.name(), "server_state");
    /// ```
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::NwscriptBridge => "nwscript_bridge",
            Self::ServerState => "server_state",
            Self::Administration => "administration",
        }
    }

    /// Parses one stable external capability name.
    ///
    /// ```
    /// assert_eq!(
    ///     nwnrs_runtime::Capability::from_name("nwscript_bridge"),
    ///     Some(nwnrs_runtime::Capability::NwscriptBridge),
    /// );
    /// ```
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "nwscript_bridge" => Some(Self::NwscriptBridge),
            "server_state" => Some(Self::ServerState),
            "administration" => Some(Self::Administration),
            _ => None,
        }
    }
}

/// Exact engine entry points required by the initial NWScript bridge.
///
/// ```
/// let target: Option<nwnrs_runtime::BridgeTarget> = None;
/// assert!(target.is_none());
/// ```
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BridgeTarget {
    /// `CNWSVirtualMachineCommands::ExecuteCommandNWNXFunctionManagement`.
    pub function_management:    TargetAddress,
    /// `CVirtualMachine::StackPopInteger`.
    pub stack_pop_integer:      TargetAddress,
    /// `CVirtualMachine::StackPushInteger`.
    pub stack_push_integer:     TargetAddress,
    /// `CVirtualMachine::StackPopFloat`.
    pub stack_pop_float:        TargetAddress,
    /// `CVirtualMachine::StackPushFloat`.
    pub stack_push_float:       TargetAddress,
    /// `CVirtualMachine::StackPopObject`.
    pub stack_pop_object:       TargetAddress,
    /// `CVirtualMachine::StackPushObject`.
    pub stack_push_object:      TargetAddress,
    /// `CVirtualMachine::StackPopString`.
    pub stack_pop_string:       TargetAddress,
    /// `CVirtualMachine::StackPushString`.
    pub stack_push_string:      TargetAddress,
    /// `CVirtualMachine::StackPopVector`.
    pub stack_pop_vector:       TargetAddress,
    /// `CVirtualMachine::StackPushVector`.
    pub stack_push_vector:      TargetAddress,
    /// Array deallocator used by `CExoString::Clear`, normally `operator
    /// delete[](void*)` on the supported C++ runtimes.
    pub free_exo_string_buffer: TargetAddress,
}

/// Exact engine entry points and layouts used to read live server state.
///
/// ```
/// let target: Option<nwnrs_runtime::ServerStateTarget> = None;
/// assert!(target.is_none());
/// ```
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ServerStateTarget {
    /// Address of the global `CAppManager*` storage.
    pub app_manager:             TargetAddress,
    /// `CServerExoApp::GetServerInfo`.
    pub get_server_info:         TargetAddress,
    /// `CServerExoApp::GetPlayerList`.
    pub get_player_list:         TargetAddress,
    /// `CServerExoApp::GetNetLayer`.
    pub get_net_layer:           TargetAddress,
    /// `CNetLayer::GetSessionMaxPlayers`.
    pub get_session_max_players: TargetAddress,
    /// `CNetLayer::GetUDPPort`.
    pub get_udp_port:            TargetAddress,
}

/// Platform-specific operation used to request a graceful server shutdown.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case", tag = "strategy")]
pub enum ShutdownTarget {
    /// Write `1` to a verified engine-global exit flag.
    ExitFlag {
        /// Address of the writable engine-global exit flag.
        address: TargetAddress,
    },
    /// Post `WM_QUIT` to the current Windows engine thread.
    CurrentThreadMessageQueue,
}

/// Exact engine entry points and globals used by server administration.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AdministrationTarget {
    /// `CNetLayer::GetSessionName`.
    pub get_session_name: TargetAddress,
    /// `CNetLayer::SetSessionName`.
    pub set_session_name: TargetAddress,
    /// `CNetLayer::GetPlayerPassword`.
    pub get_player_password: TargetAddress,
    /// `CNetLayer::SetPlayerPassword`.
    pub set_player_password: TargetAddress,
    /// `CNetLayer::GetGameMasterPassword`.
    pub get_game_master_password: TargetAddress,
    /// `CNetLayer::SetGameMasterPassword`.
    pub set_game_master_password: TargetAddress,
    /// Address of `g_bEnableCombatDebugging`.
    pub enable_combat_debugging: TargetAddress,
    /// Address of `g_bEnableSavingThrowDebugging`.
    pub enable_saving_throw_debugging: TargetAddress,
    /// Address of `g_bEnableMovementSpeedDebugging`.
    pub enable_movement_speed_debugging: TargetAddress,
    /// Address of `g_bEnableHitDieDebugging`.
    pub enable_hit_die_debugging: TargetAddress,
    /// Verified platform-specific graceful-shutdown operation.
    pub shutdown: ShutdownTarget,
    /// `CServerExoApp::AddIPToBannedList`.
    pub add_banned_ip: TargetAddress,
    /// `CServerExoApp::RemoveIPFromBannedList`.
    pub remove_banned_ip: TargetAddress,
    /// `CServerExoApp::AddCDKeyToBannedList`.
    pub add_banned_cd_key: TargetAddress,
    /// `CServerExoApp::RemoveCDKeyFromBannedList`.
    pub remove_banned_cd_key: TargetAddress,
    /// `CServerExoApp::AddPlayerNameToBannedList`.
    pub add_banned_player_name: TargetAddress,
    /// `CServerExoApp::RemovePlayerNameFromBannedList`.
    pub remove_banned_player_name: TargetAddress,
    /// Address of global `g_pRules` storage.
    pub rules: TargetAddress,
    /// `CNWRules::ReloadAll`.
    pub reload_rules: TargetAddress,
    /// `CServerExoApp::GetModule`.
    pub get_module: TargetAddress,
    /// `CExoLocString::GetStringLoc`.
    pub get_loc_string: TargetAddress,
    /// `CExoLinkedListInternal::Remove`.
    pub remove_linked_list_node: TargetAddress,
    /// `CServerExoAppInternal::MainLoop`.
    pub main_loop: TargetAddress,
    /// `CServerExoApp::GetClientObjectByObjectId`.
    pub get_client_object_by_object_id: TargetAddress,
    /// `CServerExoApp::GetCreatureByGameObjectID`.
    pub get_creature_by_game_object_id: TargetAddress,
    /// `CNWSPlayer::GetPlayerName`.
    pub get_player_name: TargetAddress,
    /// `CNetLayer::GetPlayerInfo`.
    pub get_player_info: TargetAddress,
    /// `CNetLayer::DisconnectPlayer`.
    pub disconnect_player: TargetAddress,
    /// Address of global `g_pExoBase` storage.
    pub exo_base: TargetAddress,
    /// `CExoAliasList::GetAliasPath`.
    pub get_alias_path: TargetAddress,
}

/// Native event boundaries and active event-script context.
///
/// ```
/// # use nwnrs_runtime::{EventTarget, TargetAddress};
/// let address = TargetAddress::Offset { offset: 1 };
/// let target = EventTarget {
///     virtual_machine: address.clone(),
///     run_script: address,
///     hooks: std::collections::BTreeMap::new(),
///     functions: std::collections::BTreeMap::new(),
/// };
/// assert!(target.hooks.is_empty());
/// ```
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EventTarget {
    /// Address of global `g_pVirtualMachine` storage.
    pub virtual_machine: TargetAddress,
    /// `CVirtualMachine::RunScript`.
    pub run_script:      TargetAddress,
    /// Native event hook boundaries keyed by stable physical-hook identity.
    #[serde(default)]
    pub hooks:           BTreeMap<String, TargetAddress>,
    /// Native helper functions used to construct event payloads.
    #[serde(default)]
    pub functions:       BTreeMap<String, TargetAddress>,
}

/// Versioned runtime metadata for one exact server binary.
///
/// ```
/// let pack: Option<nwnrs_runtime::TargetPack> = None;
/// assert!(pack.is_none());
/// ```
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TargetPack {
    /// Version of the target-pack file schema.
    pub schema_version: u32,
    /// Exact runtime API version required by this target pack.
    pub runtime_api:    u32,
    /// Exact server identity associated with this pack.
    pub server:         TargetServer,
    /// Unified revision used to derive function and layout semantics.
    pub source:         TargetSource,
    /// Compiler-derived layout snapshot for this platform.
    pub layouts:        AbiLayouts,
    /// Minimal native ABI required by the NWScript bridge.
    pub bridge:         BridgeTarget,
    /// Exact native ABI required to read live server state.
    pub server_state:   Option<ServerStateTarget>,
    /// Exact native ABI required for administration operations.
    pub administration: Option<AdministrationTarget>,
    /// Exact native ABI required to observe existing event scripts.
    pub events:         Option<EventTarget>,
}

impl TargetPack {
    /// Reports whether the target pack contains a capability.
    ///
    /// ```no_run
    /// # let pack: nwnrs_runtime::TargetPack = unimplemented!();
    /// let available = pack.has_capability(nwnrs_runtime::Capability::ServerState);
    /// assert_eq!(available, pack.server_state.is_some());
    /// ```
    #[must_use]
    pub fn has_capability(&self, capability: Capability) -> bool {
        match capability {
            Capability::NwscriptBridge => true,
            Capability::ServerState => self.server_state.is_some(),
            Capability::Administration => self.administration.is_some(),
        }
    }

    /// Reports whether this exact target pack can provide one event identity.
    #[must_use]
    pub fn supports_event(&self, identity: &str) -> bool {
        let Some(definition) = event_definition(identity) else {
            return false;
        };
        let Some(events) = &self.events else {
            return false;
        };
        events.hooks.contains_key("module_load")
            && events.hooks.contains_key(definition.hook)
            && definition
                .helper
                .is_none_or(|helper| events.functions.contains_key(helper))
            && definition
                .requirements
                .functions
                .iter()
                .all(|helper| events.functions.contains_key(*helper))
            && (!definition.requirements.server_state || self.server_state.is_some())
            && definition
                .requirements
                .layouts
                .iter()
                .all(|layout| self.layouts.classes.has_event_layout(layout))
    }
}

impl EngineClassLayouts {
    fn has_event_layout(&self, name: &str) -> bool {
        match name {
            "game_object_type" => self.game_object_type_offset.is_some(),
            "item_repository_parent" => self.item_repository_parent_offset.is_some(),
            "creature_stats_base_creature" => self.creature_stats_base_creature_offset.is_some(),
            "creature_stats_experience" => self.creature_stats_experience_offset.is_some(),
            "item_base_item" => self.item_base_item_offset.is_some(),
            "item_possessor" => self.item_possessor_offset.is_some(),
            "message_read_buffer" => self.message_read_buffer_offset.is_some(),
            "message_read_buffer_size" => self.message_read_buffer_size_offset.is_some(),
            "message_read_buffer_position" => self.message_read_buffer_position_offset.is_some(),
            "message_read_fragments_size" => self.message_read_fragments_size_offset.is_some(),
            "message_read_fragments_position" => {
                self.message_read_fragments_position_offset.is_some()
            }
            "message_current_read_bit" => self.message_current_read_bit_offset.is_some(),
            "message_last_byte_bits" => self.message_last_byte_bits_offset.is_some(),
            "player_object_id" => self.player_object_id_offset.is_some(),
            "player_inventory_gui" => self.player_inventory_gui_offset.is_some(),
            "player_other_inventory_gui" => self.player_other_inventory_gui_offset.is_some(),
            "inventory_gui_selected_panel" => self.inventory_gui_selected_panel_offset.is_some(),
            _ => false,
        }
    }
}

/// A loaded target pack and its canonical source path.
///
/// ```
/// let selected: Option<nwnrs_runtime::SelectedTargetPack> = None;
/// assert!(selected.is_none());
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SelectedTargetPack {
    /// Canonical path to the selected pack.
    pub path: PathBuf,
    /// Parsed and validated target-pack metadata.
    pub pack: TargetPack,
}

/// Validated configuration for one injected runtime process.
///
/// ```
/// let context: Option<nwnrs_runtime::RuntimeContext> = None;
/// assert!(context.is_none());
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeContext {
    /// Identity of the current process executable.
    pub server:   BinaryIdentity,
    /// Exact target pack selected for the executable.
    pub target:   SelectedTargetPack,
    /// Whether initialization failures must terminate the process.
    pub required: bool,
}

/// Loads and validates an exact target-pack file.
///
/// # Errors
///
/// Returns an error when the file cannot be read or parsed, its schema is
/// incompatible, or its server identity does not match `binary`.
///
/// ```no_run
/// let binary = nwnrs_runtime::BinaryIdentity::read("/path/to/nwserver")?;
/// let target = nwnrs_runtime::load_target_pack("target.toml", &binary)?;
/// assert_eq!(target.pack.server.sha256, binary.sha256.to_string());
/// # Ok::<(), nwnrs_runtime::RuntimeError>(())
/// ```
pub fn load_target_pack(
    path: impl AsRef<Path>,
    binary: &BinaryIdentity,
) -> RuntimeResult<SelectedTargetPack> {
    let requested = path.as_ref();
    let path = fs::canonicalize(requested).map_err(|error| {
        RuntimeError::new(format!(
            "failed to resolve target pack {}: {error}",
            requested.display()
        ))
    })?;
    let text = fs::read_to_string(&path).map_err(|error| {
        RuntimeError::new(format!(
            "failed to read target pack {}: {error}",
            path.display()
        ))
    })?;
    let pack = toml::from_str::<TargetPack>(&text).map_err(|error| {
        RuntimeError::new(format!(
            "failed to parse target pack {}: {error}",
            path.display()
        ))
    })?;
    validate_target_pack(&pack, binary)?;
    Ok(SelectedTargetPack {
        path,
        pack,
    })
}

/// Resolves a target pack from an exact server identity.
///
/// # Errors
///
/// Returns an error when the derived pack does not exist or fails validation.
///
/// ```no_run
/// let binary = nwnrs_runtime::BinaryIdentity::read("/path/to/nwserver")?;
/// let target = nwnrs_runtime::resolve_target_pack("crates/runtime/targets", &binary)?;
/// assert_eq!(target.pack.server.platform, binary.platform);
/// # Ok::<(), nwnrs_runtime::RuntimeError>(())
/// ```
pub fn resolve_target_pack(
    target_root: impl AsRef<Path>,
    binary: &BinaryIdentity,
) -> RuntimeResult<SelectedTargetPack> {
    let path = target_root
        .as_ref()
        .join(binary.platform.directory_name())
        .join(format!("{}.toml", binary.sha256));
    load_target_pack(path, binary)
}

/// Initializes configuration for the current injected process from its
/// environment.
///
/// `NWNRS_ENABLED=1` is required. When it is absent, this returns `Ok(None)` so
/// linking the runtime crate into tests or tools has no process-wide effect.
///
/// # Errors
///
/// Returns an error when the current executable cannot be identified, no
/// target location is configured, or target-pack validation fails.
///
/// ```no_run
/// let context = nwnrs_runtime::initialize_current_process()?;
/// if let Some(context) = context {
///     assert_eq!(context.server.platform, context.target.pack.server.platform);
/// }
/// # Ok::<(), nwnrs_runtime::RuntimeError>(())
/// ```
pub fn initialize_current_process() -> RuntimeResult<Option<RuntimeContext>> {
    if env::var_os(ENV_ENABLED).as_deref() != Some(std::ffi::OsStr::new("1")) {
        return Ok(None);
    }

    let required = env::var_os(ENV_REQUIRED).as_deref() == Some(std::ffi::OsStr::new("1"));
    let executable = env::current_exe().map_err(|error| {
        RuntimeError::new(format!("failed to locate current executable: {error}"))
    })?;
    let server = BinaryIdentity::read(executable)?;
    let target = if let Some(path) = env::var_os(ENV_TARGET_PACK) {
        load_target_pack(path, &server)?
    } else if let Some(path) = env::var_os(ENV_TARGET_DIR) {
        resolve_target_pack(path, &server)?
    } else {
        return Err(RuntimeError::new(format!(
            "neither {ENV_TARGET_PACK} nor {ENV_TARGET_DIR} is configured"
        )));
    };

    Ok(Some(RuntimeContext {
        server,
        target,
        required,
    }))
}

fn validate_target_pack(pack: &TargetPack, binary: &BinaryIdentity) -> RuntimeResult<()> {
    validate_target_pack_metadata(pack)?;
    if pack.server.platform != binary.platform {
        return Err(RuntimeError::new(format!(
            "target pack platform {} does not match binary platform {}",
            pack.server.platform, binary.platform
        )));
    }
    let actual_sha256 = binary.sha256.to_string();
    if pack.server.sha256 != actual_sha256 {
        return Err(RuntimeError::new(format!(
            "target pack server SHA-256 {} does not match binary SHA-256 {actual_sha256}",
            pack.server.sha256
        )));
    }
    Ok(())
}

fn validate_target_pack_metadata(pack: &TargetPack) -> RuntimeResult<()> {
    if pack.schema_version != TARGET_PACK_SCHEMA_VERSION {
        return Err(RuntimeError::new(format!(
            "unsupported target-pack schema {}; expected {TARGET_PACK_SCHEMA_VERSION}",
            pack.schema_version
        )));
    }
    if pack.runtime_api != RUNTIME_API_VERSION {
        return Err(RuntimeError::new(format!(
            "target pack requires runtime API {}; this runtime implements {RUNTIME_API_VERSION}",
            pack.runtime_api
        )));
    }
    if !is_sha256(&pack.server.sha256) {
        return Err(RuntimeError::new(
            "target pack server.sha256 must contain 64 lowercase hexadecimal characters",
        ));
    }
    if !is_git_commit(&pack.source.unified_commit) {
        return Err(RuntimeError::new(
            "target pack source.unified_commit must contain a full lowercase Git commit",
        ));
    }
    if pack.source.nwn_build == 0 || pack.source.nwn_revision == 0 {
        return Err(RuntimeError::new(
            "target pack source build and revision must be greater than zero",
        ));
    }
    validate_layouts(&pack.layouts)?;
    for (name, address) in bridge_addresses(&pack.bridge) {
        validate_target_address("bridge", name, address)?;
    }
    if let Some(server_state) = pack.server_state.as_ref() {
        for (name, address) in server_state_addresses(server_state) {
            validate_target_address("server_state", name, address)?;
        }
    }
    if let Some(administration) = pack.administration.as_ref() {
        if pack.server_state.is_none() {
            return Err(RuntimeError::new(
                "target pack administration capability requires server_state",
            ));
        }
        for (name, address) in administration_addresses(administration) {
            validate_target_address("administration", name, address)?;
        }
        match &administration.shutdown {
            ShutdownTarget::ExitFlag {
                address,
            } => validate_target_address("administration", "shutdown.address", address)?,
            ShutdownTarget::CurrentThreadMessageQueue => {
                if pack.server.platform.os != OperatingSystem::Windows {
                    return Err(RuntimeError::new(
                        "current-thread message-queue shutdown requires Windows",
                    ));
                }
            }
        }
    }
    if let Some(events) = pack.events.as_ref() {
        if !events.hooks.contains_key("module_load") {
            return Err(RuntimeError::new(
                "target pack events require the module_load bootstrap hook",
            ));
        }
        let known_hooks: BTreeSet<_> = EVENT_CATALOG.iter().map(|event| event.hook).collect();
        let known_functions: BTreeSet<_> = EVENT_CATALOG
            .iter()
            .flat_map(|event| {
                event
                    .helper
                    .into_iter()
                    .chain(event.requirements.functions.iter().copied())
            })
            .collect();
        for (name, address) in [
            ("virtual_machine", &events.virtual_machine),
            ("run_script", &events.run_script),
        ] {
            validate_target_address("events", name, address)?;
        }
        if !pack.layouts.classes.game_object_id_offset.is_multiple_of(4) {
            return Err(RuntimeError::new(
                "target pack layouts.classes.game_object_id_offset is not four-byte aligned",
            ));
        }
        for (name, address) in &events.hooks {
            if name.is_empty() {
                return Err(RuntimeError::new(
                    "target pack events hook identity must not be empty",
                ));
            }
            if !known_hooks.contains(name.as_str()) {
                return Err(RuntimeError::new(format!(
                    "target pack events hook identity {name} is absent from the event catalog"
                )));
            }
            validate_target_address("events.hooks", name, address)?;
        }
        for (name, address) in &events.functions {
            if name.is_empty() {
                return Err(RuntimeError::new(
                    "target pack events function identity must not be empty",
                ));
            }
            if !known_functions.contains(name.as_str()) {
                return Err(RuntimeError::new(format!(
                    "target pack events function identity {name} is absent from the event catalog"
                )));
            }
            validate_target_address("events.functions", name, address)?;
        }
    }
    Ok(())
}

fn validate_layouts(layouts: &AbiLayouts) -> RuntimeResult<()> {
    let string = &layouts.c_exo_string;
    if string.size != 16
        || string.alignment != 8
        || string.string_offset != 0
        || string.string_length_offset != 8
        || string.buffer_length_offset != 12
    {
        return Err(RuntimeError::new(
            "target pack CExoString layout does not match the supported Unified ABI",
        ));
    }
    let players = &layouts.player_list;
    if players.size != 16
        || players.alignment != 8
        || players.elements_offset != 0
        || players.count_offset != 8
        || players.capacity_offset != 12
    {
        return Err(RuntimeError::new(
            "target pack player-list layout does not match the supported Unified ABI",
        ));
    }
    let vector = &layouts.vector;
    if vector.size != 12
        || vector.alignment != 4
        || vector.x_offset != 0
        || vector.y_offset != 4
        || vector.z_offset != 8
    {
        return Err(RuntimeError::new(
            "target pack Vector layout does not match the supported Unified ABI",
        ));
    }
    let classes = &layouts.classes;
    for (name, offset, alignment) in [
        (
            "command_implementer_vm_offset",
            classes.command_implementer_vm_offset,
            8,
        ),
        (
            "app_manager_server_offset",
            classes.app_manager_server_offset,
            8,
        ),
        (
            "server_info_module_offset",
            classes.server_info_module_offset,
            8,
        ),
        (
            "server_info_joining_restrictions_offset",
            classes.server_info_joining_restrictions_offset,
            4,
        ),
        (
            "server_info_play_options_offset",
            classes.server_info_play_options_offset,
            4,
        ),
        (
            "server_info_persistent_world_options_offset",
            classes.server_info_persistent_world_options_offset,
            4,
        ),
        (
            "persistent_world_options_server_vault_by_player_name_offset",
            classes.persistent_world_options_server_vault_by_player_name_offset,
            4,
        ),
        (
            "joining_restrictions_min_level_offset",
            classes.joining_restrictions_min_level_offset,
            4,
        ),
        (
            "joining_restrictions_max_level_offset",
            classes.joining_restrictions_max_level_offset,
            4,
        ),
        (
            "server_exo_app_internal_offset",
            classes.server_exo_app_internal_offset,
            8,
        ),
        (
            "internal_banned_ip_list_offset",
            classes.internal_banned_ip_list_offset,
            8,
        ),
        (
            "internal_banned_cd_key_list_offset",
            classes.internal_banned_cd_key_list_offset,
            8,
        ),
        (
            "internal_banned_player_name_list_offset",
            classes.internal_banned_player_name_list_offset,
            8,
        ),
        (
            "module_turd_list_offset",
            classes.module_turd_list_offset,
            8,
        ),
        (
            "player_turd_community_name_offset",
            classes.player_turd_community_name_offset,
            8,
        ),
        (
            "player_turd_first_name_offset",
            classes.player_turd_first_name_offset,
            8,
        ),
        (
            "player_turd_last_name_offset",
            classes.player_turd_last_name_offset,
            8,
        ),
        (
            "linked_list_head_offset",
            classes.linked_list_head_offset,
            8,
        ),
        (
            "linked_list_count_offset",
            classes.linked_list_count_offset,
            4,
        ),
        (
            "linked_list_node_next_offset",
            classes.linked_list_node_next_offset,
            8,
        ),
        (
            "linked_list_node_object_offset",
            classes.linked_list_node_object_offset,
            8,
        ),
        ("player_id_offset", classes.player_id_offset, 4),
        (
            "player_file_name_offset",
            classes.player_file_name_offset,
            1,
        ),
        ("player_file_name_size", classes.player_file_name_size, 1),
        (
            "net_layer_player_info_cd_key_offset",
            classes.net_layer_player_info_cd_key_offset,
            8,
        ),
        (
            "player_cd_key_public_offset",
            classes.player_cd_key_public_offset,
            8,
        ),
        (
            "exo_base_alias_list_offset",
            classes.exo_base_alias_list_offset,
            8,
        ),
        ("creature_stats_offset", classes.creature_stats_offset, 8),
        (
            "creature_stats_first_name_offset",
            classes.creature_stats_first_name_offset,
            8,
        ),
        (
            "creature_stats_last_name_offset",
            classes.creature_stats_last_name_offset,
            8,
        ),
        (
            "vm_recursion_level_offset",
            classes.vm_recursion_level_offset,
            4,
        ),
        ("vm_script_array_offset", classes.vm_script_array_offset, 8),
        ("vm_script_size", classes.vm_script_size, 8),
        ("vm_script_name_offset", classes.vm_script_name_offset, 8),
        (
            "vm_script_event_id_offset",
            classes.vm_script_event_id_offset,
            4,
        ),
    ] {
        if !offset.is_multiple_of(alignment) {
            return Err(RuntimeError::new(format!(
                "target pack layout {name} is not {alignment}-byte aligned"
            )));
        }
    }
    for (name, offset, alignment) in [
        (
            "game_object_type_offset",
            classes.game_object_type_offset,
            1,
        ),
        (
            "item_repository_parent_offset",
            classes.item_repository_parent_offset,
            4,
        ),
        (
            "creature_stats_base_creature_offset",
            classes.creature_stats_base_creature_offset,
            8,
        ),
        (
            "creature_stats_experience_offset",
            classes.creature_stats_experience_offset,
            4,
        ),
        ("item_base_item_offset", classes.item_base_item_offset, 4),
        ("item_possessor_offset", classes.item_possessor_offset, 4),
        (
            "message_read_buffer_offset",
            classes.message_read_buffer_offset,
            8,
        ),
        (
            "message_read_buffer_size_offset",
            classes.message_read_buffer_size_offset,
            4,
        ),
        (
            "message_read_buffer_position_offset",
            classes.message_read_buffer_position_offset,
            4,
        ),
        (
            "message_read_fragments_size_offset",
            classes.message_read_fragments_size_offset,
            4,
        ),
        (
            "message_read_fragments_position_offset",
            classes.message_read_fragments_position_offset,
            4,
        ),
        (
            "message_current_read_bit_offset",
            classes.message_current_read_bit_offset,
            1,
        ),
        (
            "message_last_byte_bits_offset",
            classes.message_last_byte_bits_offset,
            1,
        ),
        (
            "player_object_id_offset",
            classes.player_object_id_offset,
            4,
        ),
        (
            "player_inventory_gui_offset",
            classes.player_inventory_gui_offset,
            8,
        ),
        (
            "player_other_inventory_gui_offset",
            classes.player_other_inventory_gui_offset,
            8,
        ),
        (
            "inventory_gui_selected_panel_offset",
            classes.inventory_gui_selected_panel_offset,
            1,
        ),
    ] {
        if offset.is_some_and(|offset| !offset.is_multiple_of(alignment)) {
            return Err(RuntimeError::new(format!(
                "target pack layout {name} is not {alignment}-byte aligned"
            )));
        }
    }
    if classes.vm_script_slot_count == 0 {
        return Err(RuntimeError::new(
            "target pack VM script slot count must be greater than zero",
        ));
    }
    if classes.player_file_name_size != 17 {
        return Err(RuntimeError::new(
            "target pack CResRef size must match the 17-byte Unified layout",
        ));
    }
    if classes.vm_script_alignment != 8 {
        return Err(RuntimeError::new(
            "target pack CVirtualMachineScript alignment must be eight bytes",
        ));
    }
    if classes.vm_script_name_offset.saturating_add(string.size) > classes.vm_script_size
        || classes.vm_script_event_id_offset.saturating_add(4) > classes.vm_script_size
    {
        return Err(RuntimeError::new(
            "target pack VM script fields exceed the declared script size",
        ));
    }
    Ok(())
}

/// Loads a machine-generated Unified ABI snapshot.
///
/// # Errors
///
/// Returns an error when the snapshot cannot be read, parsed, or validated.
///
/// ```no_run
/// let snapshot = nwnrs_runtime::load_abi_snapshot("target/unified-abi.toml")?;
/// assert_eq!(snapshot.pointer_width, 64);
/// # Ok::<(), nwnrs_runtime::RuntimeError>(())
/// ```
pub fn load_abi_snapshot(path: impl AsRef<Path>) -> RuntimeResult<AbiSnapshot> {
    let path = path.as_ref();
    let text = fs::read_to_string(path).map_err(|error| {
        RuntimeError::new(format!(
            "failed to read ABI snapshot {}: {error}",
            path.display()
        ))
    })?;
    let snapshot = toml::from_str::<AbiSnapshot>(&text).map_err(|error| {
        RuntimeError::new(format!(
            "failed to parse ABI snapshot {}: {error}",
            path.display()
        ))
    })?;
    if snapshot.schema_version != ABI_SNAPSHOT_SCHEMA_VERSION {
        return Err(RuntimeError::new(format!(
            "unsupported ABI snapshot schema {}; expected {ABI_SNAPSHOT_SCHEMA_VERSION}",
            snapshot.schema_version
        )));
    }
    if snapshot.pointer_width != 64 {
        return Err(RuntimeError::new(format!(
            "unsupported ABI snapshot pointer width {}; expected 64",
            snapshot.pointer_width
        )));
    }
    if !is_git_commit(&snapshot.source.unified_commit) {
        return Err(RuntimeError::new(
            "ABI snapshot Unified commit must contain 40 lowercase hexadecimal characters",
        ));
    }
    validate_layouts(&snapshot.layouts)?;
    Ok(snapshot)
}

/// Verifies that a generated ABI snapshot exactly matches one target pack.
///
/// # Errors
///
/// Returns an error when provenance, platform, or any layout differs.
///
/// ```no_run
/// # let snapshot: nwnrs_runtime::AbiSnapshot = unimplemented!();
/// # let pack: nwnrs_runtime::TargetPack = unimplemented!();
/// nwnrs_runtime::validate_abi_snapshot(&snapshot, &pack)?;
/// # Ok::<(), nwnrs_runtime::RuntimeError>(())
/// ```
pub fn validate_abi_snapshot(snapshot: &AbiSnapshot, pack: &TargetPack) -> RuntimeResult<()> {
    if snapshot.source != pack.source {
        return Err(RuntimeError::new(
            "ABI snapshot Unified provenance does not match the target pack",
        ));
    }
    if snapshot.platform != pack.server.platform {
        return Err(RuntimeError::new(format!(
            "ABI snapshot platform {} does not match target platform {}",
            snapshot.platform, pack.server.platform
        )));
    }
    if snapshot.layouts != pack.layouts {
        return Err(RuntimeError::new(
            "ABI snapshot layouts do not match the target pack",
        ));
    }
    Ok(())
}

fn validate_target_address(
    section: &str,
    name: &str,
    address: &TargetAddress,
) -> RuntimeResult<()> {
    if let TargetAddress::Symbol {
        symbol,
    } = address
        && (symbol.is_empty() || symbol.as_bytes().contains(&0))
    {
        return Err(RuntimeError::new(format!(
            "target pack {section}.{name} symbol must be non-empty and contain no NUL bytes"
        )));
    }
    Ok(())
}

fn bridge_addresses(bridge: &BridgeTarget) -> [(&'static str, &TargetAddress); 12] {
    [
        ("function_management", &bridge.function_management),
        ("stack_pop_integer", &bridge.stack_pop_integer),
        ("stack_push_integer", &bridge.stack_push_integer),
        ("stack_pop_float", &bridge.stack_pop_float),
        ("stack_push_float", &bridge.stack_push_float),
        ("stack_pop_object", &bridge.stack_pop_object),
        ("stack_push_object", &bridge.stack_push_object),
        ("stack_pop_string", &bridge.stack_pop_string),
        ("stack_push_string", &bridge.stack_push_string),
        ("stack_pop_vector", &bridge.stack_pop_vector),
        ("stack_push_vector", &bridge.stack_push_vector),
        ("free_exo_string_buffer", &bridge.free_exo_string_buffer),
    ]
}

fn server_state_addresses(server_state: &ServerStateTarget) -> [(&'static str, &TargetAddress); 6] {
    [
        ("app_manager", &server_state.app_manager),
        ("get_server_info", &server_state.get_server_info),
        ("get_player_list", &server_state.get_player_list),
        ("get_net_layer", &server_state.get_net_layer),
        (
            "get_session_max_players",
            &server_state.get_session_max_players,
        ),
        ("get_udp_port", &server_state.get_udp_port),
    ]
}

fn administration_addresses(
    administration: &AdministrationTarget,
) -> [(&'static str, &TargetAddress); 29] {
    [
        ("get_session_name", &administration.get_session_name),
        ("set_session_name", &administration.set_session_name),
        ("get_player_password", &administration.get_player_password),
        ("set_player_password", &administration.set_player_password),
        (
            "get_game_master_password",
            &administration.get_game_master_password,
        ),
        (
            "set_game_master_password",
            &administration.set_game_master_password,
        ),
        (
            "enable_combat_debugging",
            &administration.enable_combat_debugging,
        ),
        (
            "enable_saving_throw_debugging",
            &administration.enable_saving_throw_debugging,
        ),
        (
            "enable_movement_speed_debugging",
            &administration.enable_movement_speed_debugging,
        ),
        (
            "enable_hit_die_debugging",
            &administration.enable_hit_die_debugging,
        ),
        ("add_banned_ip", &administration.add_banned_ip),
        ("remove_banned_ip", &administration.remove_banned_ip),
        ("add_banned_cd_key", &administration.add_banned_cd_key),
        ("remove_banned_cd_key", &administration.remove_banned_cd_key),
        (
            "add_banned_player_name",
            &administration.add_banned_player_name,
        ),
        (
            "remove_banned_player_name",
            &administration.remove_banned_player_name,
        ),
        ("rules", &administration.rules),
        ("reload_rules", &administration.reload_rules),
        ("get_module", &administration.get_module),
        ("get_loc_string", &administration.get_loc_string),
        (
            "remove_linked_list_node",
            &administration.remove_linked_list_node,
        ),
        ("main_loop", &administration.main_loop),
        (
            "get_client_object_by_object_id",
            &administration.get_client_object_by_object_id,
        ),
        (
            "get_creature_by_game_object_id",
            &administration.get_creature_by_game_object_id,
        ),
        ("get_player_name", &administration.get_player_name),
        ("get_player_info", &administration.get_player_info),
        ("disconnect_player", &administration.disconnect_player),
        ("exo_base", &administration.exo_base),
        ("get_alias_path", &administration.get_alias_path),
    ]
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn is_git_commit(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn file_sha256(path: &Path) -> RuntimeResult<FileSha256> {
    let mut file = File::open(path).map_err(|error| {
        RuntimeError::new(format!("failed to hash binary {}: {error}", path.display()))
    })?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(|error| {
            RuntimeError::new(format!("failed to hash binary {}: {error}", path.display()))
        })?;
        if count == 0 {
            break;
        }
        let chunk = buffer.get(..count).ok_or_else(|| {
            RuntimeError::new("file reader returned a byte count larger than its buffer")
        })?;
        hasher.update(chunk);
    }

    let digest = hasher.finalize();
    let mut bytes = [0_u8; 32];
    bytes.copy_from_slice(&digest);
    Ok(FileSha256(bytes))
}

fn read_platform(file: &mut File, path: &Path) -> RuntimeResult<Platform> {
    let mut prefix = [0_u8; 8];
    file.read_exact(&mut prefix).map_err(|error| {
        RuntimeError::new(format!(
            "failed to read binary header {}: {error}",
            path.display()
        ))
    })?;
    let magic = prefix
        .get(..4)
        .ok_or_else(|| RuntimeError::new("binary header is shorter than four bytes"))?;
    let header_length = if matches!(magic, b"\xca\xfe\xba\xbe" | b"\xca\xfe\xba\xbf") {
        let count = read_u32_be(&prefix, 4, "Mach-O architecture count")?;
        if count == 0 || count > 64 {
            return Err(RuntimeError::new(format!(
                "unsupported Mach-O architecture count: {count}"
            )));
        }
        let entry_size = if magic == b"\xca\xfe\xba\xbf" { 32 } else { 20 };
        8_usize
            .checked_add(
                usize::try_from(count)
                    .map_err(|_error| RuntimeError::new("Mach-O architecture count overflowed"))?
                    .checked_mul(entry_size)
                    .ok_or_else(|| RuntimeError::new("Mach-O architecture table overflowed"))?,
            )
            .ok_or_else(|| RuntimeError::new("Mach-O header length overflowed"))?
    } else if magic == b"MZ\0\0" || prefix.get(..2) == Some(b"MZ") {
        let mut dos_header = [0_u8; 64];
        dos_header
            .get_mut(..prefix.len())
            .ok_or_else(|| RuntimeError::new("invalid DOS header prefix"))?
            .copy_from_slice(&prefix);
        file.read_exact(
            dos_header
                .get_mut(prefix.len()..)
                .ok_or_else(|| RuntimeError::new("invalid DOS header remainder"))?,
        )
        .map_err(|error| {
            RuntimeError::new(format!(
                "failed to read DOS header {}: {error}",
                path.display()
            ))
        })?;
        let pe_offset = usize::try_from(read_u32_le(&dos_header, 60, "PE header offset")?)
            .map_err(|_error| RuntimeError::new("PE header offset exceeds usize"))?;
        if !(64..=1024 * 1024).contains(&pe_offset) {
            return Err(RuntimeError::new(format!(
                "unsupported PE header offset: {pe_offset:#x}"
            )));
        }
        let header_length = pe_offset
            .checked_add(26)
            .ok_or_else(|| RuntimeError::new("PE header length overflowed"))?;
        file.seek(SeekFrom::Start(0)).map_err(|error| {
            RuntimeError::new(format!(
                "failed to seek binary header {}: {error}",
                path.display()
            ))
        })?;
        let mut header = vec![0_u8; header_length];
        file.read_exact(&mut header).map_err(|error| {
            RuntimeError::new(format!(
                "failed to read PE header {}: {error}",
                path.display()
            ))
        })?;
        return parse_platform(&header);
    } else {
        64
    };
    let mut header = vec![0_u8; header_length];
    let prefix_target = header
        .get_mut(..prefix.len())
        .ok_or_else(|| RuntimeError::new("invalid binary header length"))?;
    prefix_target.copy_from_slice(&prefix);
    file.read_exact(
        header
            .get_mut(prefix.len()..)
            .ok_or_else(|| RuntimeError::new("invalid binary header remainder"))?,
    )
    .map_err(|error| {
        RuntimeError::new(format!(
            "failed to read binary header {}: {error}",
            path.display()
        ))
    })?;
    parse_platform(&header)
}

fn parse_platform(header: &[u8]) -> RuntimeResult<Platform> {
    let magic = header
        .get(..4)
        .ok_or_else(|| RuntimeError::new("binary header is shorter than four bytes"))?;
    if magic == b"\x7fELF" {
        return parse_elf_platform(header);
    }
    if magic == b"\xcf\xfa\xed\xfe" {
        return parse_macho_platform(header);
    }
    if matches!(magic, b"\xca\xfe\xba\xbe" | b"\xca\xfe\xba\xbf") {
        return parse_universal_macho_platform(header);
    }
    if magic.get(..2) == Some(b"MZ") {
        return parse_pe_platform(header);
    }
    Err(RuntimeError::new(
        "unsupported binary format; expected 64-bit ELF, little-endian Mach-O, or PE32+",
    ))
}

fn parse_pe_platform(header: &[u8]) -> RuntimeResult<Platform> {
    let pe_offset = usize::try_from(read_u32_le(header, 60, "PE header offset")?)
        .map_err(|_error| RuntimeError::new("PE header offset exceeds usize"))?;
    if header.get(pe_offset..pe_offset.saturating_add(4)) != Some(b"PE\0\0") {
        return Err(RuntimeError::new("PE header is missing its signature"));
    }
    let machine = read_u16_le(header, pe_offset.saturating_add(4), "PE machine")?;
    let optional_magic = read_u16_le(
        header,
        pe_offset.saturating_add(24),
        "PE optional-header magic",
    )?;
    if optional_magic != 0x020b {
        return Err(RuntimeError::new(
            "only PE32+ Windows binaries are supported",
        ));
    }
    let architecture = match machine {
        0x8664 => Architecture::X86_64,
        0xaa64 => Architecture::Aarch64,
        _ => {
            return Err(RuntimeError::new(format!(
                "unsupported PE machine identifier: {machine:#06x}"
            )));
        }
    };
    Ok(Platform {
        os: OperatingSystem::Windows,
        architecture,
    })
}

fn parse_universal_macho_platform(header: &[u8]) -> RuntimeResult<Platform> {
    let magic = header
        .get(..4)
        .ok_or_else(|| RuntimeError::new("Mach-O header is missing its magic"))?;
    let entry_size = if magic == b"\xca\xfe\xba\xbf" { 32 } else { 20 };
    let count = read_u32_be(header, 4, "Mach-O architecture count")?;
    let host_architecture = if cfg!(target_arch = "aarch64") {
        Architecture::Aarch64
    } else if cfg!(target_arch = "x86_64") {
        Architecture::X86_64
    } else {
        return Err(RuntimeError::new(format!(
            "unsupported host architecture: {}",
            env::consts::ARCH
        )));
    };

    for index in 0..count {
        let index = usize::try_from(index)
            .map_err(|_error| RuntimeError::new("Mach-O architecture index overflowed"))?;
        let offset = 8_usize
            .checked_add(
                index
                    .checked_mul(entry_size)
                    .ok_or_else(|| RuntimeError::new("Mach-O architecture offset overflowed"))?,
            )
            .ok_or_else(|| RuntimeError::new("Mach-O architecture offset overflowed"))?;
        let cpu_type = read_u32_be(header, offset, "Mach-O universal CPU type")?;
        let architecture = match cpu_type {
            0x0100_000c => Architecture::Aarch64,
            0x0100_0007 => Architecture::X86_64,
            _ => continue,
        };
        if architecture == host_architecture {
            return Ok(Platform {
                os: OperatingSystem::Macos,
                architecture,
            });
        }
    }
    Err(RuntimeError::new(format!(
        "universal Mach-O does not contain the host architecture {host_architecture}"
    )))
}

fn parse_elf_platform(header: &[u8]) -> RuntimeResult<Platform> {
    let class = header
        .get(4)
        .copied()
        .ok_or_else(|| RuntimeError::new("ELF header is missing its class"))?;
    let encoding = header
        .get(5)
        .copied()
        .ok_or_else(|| RuntimeError::new("ELF header is missing its byte order"))?;
    if class != 2 || encoding != 1 {
        return Err(RuntimeError::new(
            "only little-endian 64-bit ELF binaries are supported",
        ));
    }
    let machine = read_u16_le(header, 18, "ELF machine")?;
    let architecture = match machine {
        62 => Architecture::X86_64,
        183 => Architecture::Aarch64,
        _ => {
            return Err(RuntimeError::new(format!(
                "unsupported ELF machine identifier: {machine}"
            )));
        }
    };
    Ok(Platform {
        os: OperatingSystem::Linux,
        architecture,
    })
}

fn parse_macho_platform(header: &[u8]) -> RuntimeResult<Platform> {
    let cpu_type = read_u32_le(header, 4, "Mach-O CPU type")?;
    let architecture = match cpu_type {
        0x0100_000c => Architecture::Aarch64,
        0x0100_0007 => Architecture::X86_64,
        _ => {
            return Err(RuntimeError::new(format!(
                "unsupported Mach-O CPU type: {cpu_type:#010x}"
            )));
        }
    };
    Ok(Platform {
        os: OperatingSystem::Macos,
        architecture,
    })
}

fn read_u16_le(bytes: &[u8], offset: usize, field: &str) -> RuntimeResult<u16> {
    let value = bytes
        .get(offset..offset.saturating_add(2))
        .ok_or_else(|| RuntimeError::new(format!("binary header is missing {field}")))?;
    let array = <[u8; 2]>::try_from(value)
        .map_err(|_error| RuntimeError::new(format!("invalid {field} width")))?;
    Ok(u16::from_le_bytes(array))
}

fn read_u32_le(bytes: &[u8], offset: usize, field: &str) -> RuntimeResult<u32> {
    let value = bytes
        .get(offset..offset.saturating_add(4))
        .ok_or_else(|| RuntimeError::new(format!("binary header is missing {field}")))?;
    let array = <[u8; 4]>::try_from(value)
        .map_err(|_error| RuntimeError::new(format!("invalid {field} width")))?;
    Ok(u32::from_le_bytes(array))
}

fn read_u32_be(bytes: &[u8], offset: usize, field: &str) -> RuntimeResult<u32> {
    let value = bytes
        .get(offset..offset.saturating_add(4))
        .ok_or_else(|| RuntimeError::new(format!("binary header is missing {field}")))?;
    let array = <[u8; 4]>::try_from(value)
        .map_err(|_error| RuntimeError::new(format!("invalid {field} width")))?;
    Ok(u32::from_be_bytes(array))
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicUsize, Ordering},
    };

    use super::{
        AbiLayouts, Architecture, BinaryIdentity, BridgeTarget, CExoStringLayout,
        EngineClassLayouts, EventTarget, OperatingSystem, Platform, PlayerListLayout,
        RUNTIME_API_VERSION, ServerStateTarget, TARGET_PACK_SCHEMA_VERSION, TargetAddress,
        TargetPack, TargetServer, TargetSource, VectorLayout, bridge_addresses, parse_platform,
        resolve_target_pack, server_state_addresses, validate_target_pack_metadata,
    };

    static NEXT_TEST_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn parses_supported_elf_macho_and_pe_headers() -> Result<(), Box<dyn std::error::Error>> {
        let mut elf = [0_u8; 64];
        elf.get_mut(..4)
            .ok_or("ELF magic range")?
            .copy_from_slice(b"\x7fELF");
        *elf.get_mut(4).ok_or("ELF class byte")? = 2;
        *elf.get_mut(5).ok_or("ELF byte-order byte")? = 1;
        elf.get_mut(18..20)
            .ok_or("ELF machine range")?
            .copy_from_slice(&62_u16.to_le_bytes());
        assert_eq!(
            parse_platform(&elf)?,
            Platform {
                os:           OperatingSystem::Linux,
                architecture: Architecture::X86_64,
            }
        );

        let mut macho = [0_u8; 32];
        macho
            .get_mut(..4)
            .ok_or("Mach-O magic range")?
            .copy_from_slice(b"\xcf\xfa\xed\xfe");
        macho
            .get_mut(4..8)
            .ok_or("Mach-O CPU range")?
            .copy_from_slice(&0x0100_000c_u32.to_le_bytes());
        assert_eq!(
            parse_platform(&macho)?,
            Platform {
                os:           OperatingSystem::Macos,
                architecture: Architecture::Aarch64,
            }
        );

        let mut universal = [0_u8; 48];
        universal
            .get_mut(..8)
            .ok_or("Mach-O universal prefix")?
            .copy_from_slice(b"\xca\xfe\xba\xbe\0\0\0\x02");
        universal
            .get_mut(8..12)
            .ok_or("Mach-O x86 CPU range")?
            .copy_from_slice(&0x0100_0007_u32.to_be_bytes());
        universal
            .get_mut(28..32)
            .ok_or("Mach-O ARM CPU range")?
            .copy_from_slice(&0x0100_000c_u32.to_be_bytes());
        let expected_architecture = if cfg!(target_arch = "aarch64") {
            Architecture::Aarch64
        } else {
            Architecture::X86_64
        };
        assert_eq!(
            parse_platform(&universal)?,
            Platform {
                os:           OperatingSystem::Macos,
                architecture: expected_architecture,
            }
        );

        let mut pe = [0_u8; 0xa0];
        pe.get_mut(..2)
            .ok_or("DOS magic range")?
            .copy_from_slice(b"MZ");
        pe.get_mut(60..64)
            .ok_or("PE offset range")?
            .copy_from_slice(&0x80_u32.to_le_bytes());
        pe.get_mut(0x80..0x84)
            .ok_or("PE signature range")?
            .copy_from_slice(b"PE\0\0");
        pe.get_mut(0x84..0x86)
            .ok_or("PE machine range")?
            .copy_from_slice(&0x8664_u16.to_le_bytes());
        pe.get_mut(0x98..0x9a)
            .ok_or("PE optional magic range")?
            .copy_from_slice(&0x020b_u16.to_le_bytes());
        assert_eq!(
            parse_platform(&pe)?,
            Platform {
                os:           OperatingSystem::Windows,
                architecture: Architecture::X86_64,
            }
        );
        Ok(())
    }

    #[test]
    fn resolves_only_the_exact_hash_pack() -> Result<(), Box<dyn std::error::Error>> {
        let root = test_directory();
        fs::create_dir_all(&root)?;
        let binary_path = root.join("nwserver");
        let mut elf = [0_u8; 64];
        elf.get_mut(..4)
            .ok_or("ELF magic range")?
            .copy_from_slice(b"\x7fELF");
        *elf.get_mut(4).ok_or("ELF class byte")? = 2;
        *elf.get_mut(5).ok_or("ELF byte-order byte")? = 1;
        elf.get_mut(18..20)
            .ok_or("ELF machine range")?
            .copy_from_slice(&183_u16.to_le_bytes());
        fs::write(&binary_path, elf)?;
        let identity = BinaryIdentity::read(&binary_path)?;
        let mut pack = TargetPack {
            schema_version: TARGET_PACK_SCHEMA_VERSION,
            runtime_api:    RUNTIME_API_VERSION,
            server:         TargetServer {
                sha256:   identity.sha256.to_string(),
                platform: identity.platform,
                build:    Some("fixture".to_string()),
            },
            source:         fixture_source(),
            layouts:        fixture_layouts(),
            bridge:         fixture_bridge_target(),
            server_state:   Some(fixture_server_state_target()),
            administration: None,
            events:         Some(fixture_event_target()),
        };
        let pack_directory = root.join(identity.platform.directory_name());
        fs::create_dir_all(&pack_directory)?;
        let pack_path = pack_directory.join(format!("{}.toml", identity.sha256));
        fs::write(&pack_path, toml::to_string(&pack)?)?;

        let selected = resolve_target_pack(&root, &identity)?;
        assert_eq!(selected.pack, pack);
        assert_eq!(selected.path, fs::canonicalize(pack_path)?);

        pack.server_state = None;
        pack.events = None;
        fs::write(&selected.path, toml::to_string(&pack)?)?;
        let selected_without_optional_capabilities = resolve_target_pack(&root, &identity)?;
        assert!(
            selected_without_optional_capabilities
                .pack
                .server_state
                .is_none()
        );

        pack.server.sha256 = "0".repeat(64);
        fs::write(&selected.path, toml::to_string(&pack)?)?;
        assert!(resolve_target_pack(&root, &identity).is_err());
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn source_controlled_target_packs_match_their_paths() -> Result<(), Box<dyn std::error::Error>>
    {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("targets");
        let mut pack_count = 0_usize;
        for platform_entry in fs::read_dir(&root)? {
            let platform_entry = platform_entry?;
            if !platform_entry.file_type()?.is_dir() {
                continue;
            }
            let platform_name = platform_entry
                .file_name()
                .to_str()
                .ok_or("target platform directory is not UTF-8")?
                .to_string();
            for pack_entry in fs::read_dir(platform_entry.path())? {
                let pack_entry = pack_entry?;
                if pack_entry
                    .path()
                    .extension()
                    .and_then(std::ffi::OsStr::to_str)
                    != Some("toml")
                {
                    continue;
                }
                let pack = toml::from_str::<TargetPack>(&fs::read_to_string(pack_entry.path())?)?;
                validate_target_pack_metadata(&pack)?;
                let filename = pack_entry
                    .path()
                    .file_stem()
                    .and_then(std::ffi::OsStr::to_str)
                    .ok_or("target pack filename is not UTF-8")?
                    .to_string();
                assert_eq!(pack.schema_version, TARGET_PACK_SCHEMA_VERSION);
                assert_eq!(pack.runtime_api, RUNTIME_API_VERSION);
                assert_eq!(pack.server.sha256, filename);
                assert_eq!(pack.server.platform.directory_name(), platform_name);
                for (_name, address) in bridge_addresses(&pack.bridge) {
                    if let TargetAddress::Symbol {
                        symbol,
                    } = address
                    {
                        assert!(!symbol.is_empty());
                        assert!(!symbol.as_bytes().contains(&0));
                    }
                }
                if let Some(server_state) = pack.server_state.as_ref() {
                    for (_name, address) in server_state_addresses(server_state) {
                        if let TargetAddress::Symbol {
                            symbol,
                        } = address
                        {
                            assert!(!symbol.is_empty());
                            assert!(!symbol.as_bytes().contains(&0));
                        }
                    }
                }
                pack_count = pack_count.saturating_add(1);
            }
        }
        assert!(pack_count >= 3);
        Ok(())
    }

    #[test]
    fn event_support_requires_bootstrap_and_rejects_unknown_target_keys()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut events = fixture_event_target();
        events.hooks.insert(
            "object_lock".to_string(),
            TargetAddress::Offset {
                offset: 22
            },
        );
        let mut pack = TargetPack {
            schema_version: TARGET_PACK_SCHEMA_VERSION,
            runtime_api:    RUNTIME_API_VERSION,
            server:         TargetServer {
                sha256:   "0".repeat(64),
                platform: Platform {
                    os:           OperatingSystem::Linux,
                    architecture: Architecture::X86_64,
                },
                build:    None,
            },
            source:         fixture_source(),
            layouts:        fixture_layouts(),
            bridge:         fixture_bridge_target(),
            server_state:   None,
            administration: None,
            events:         Some(events),
        };
        assert!(pack.supports_event("object_lock_before"));
        pack.events
            .as_mut()
            .ok_or("fixture events")?
            .hooks
            .remove("module_load");
        assert!(!pack.supports_event("object_lock_before"));
        assert!(validate_target_pack_metadata(&pack).is_err());

        let events = pack.events.as_mut().ok_or("fixture events")?;
        events.hooks.insert(
            "module_load".to_string(),
            TargetAddress::Offset {
                offset: 19
            },
        );
        events.hooks.insert(
            "object_set_experience".to_string(),
            TargetAddress::Offset {
                offset: 24
            },
        );
        assert!(!pack.supports_event("object_set_experience_before"));
        pack.layouts.classes.creature_stats_base_creature_offset = Some(48);
        pack.layouts.classes.creature_stats_experience_offset = Some(168);
        assert!(pack.supports_event("object_set_experience_before"));

        let events = pack.events.as_mut().ok_or("fixture events")?;
        events.hooks.insert(
            "unknown".to_string(),
            TargetAddress::Offset {
                offset: 23
            },
        );
        assert!(validate_target_pack_metadata(&pack).is_err());
        Ok(())
    }

    fn fixture_bridge_target() -> BridgeTarget {
        BridgeTarget {
            function_management:    TargetAddress::Offset {
                offset: 1
            },
            stack_pop_integer:      TargetAddress::Offset {
                offset: 2
            },
            stack_push_integer:     TargetAddress::Offset {
                offset: 3
            },
            stack_pop_float:        TargetAddress::Offset {
                offset: 4
            },
            stack_push_float:       TargetAddress::Offset {
                offset: 5
            },
            stack_pop_object:       TargetAddress::Offset {
                offset: 6
            },
            stack_push_object:      TargetAddress::Offset {
                offset: 7
            },
            stack_pop_string:       TargetAddress::Offset {
                offset: 8
            },
            stack_push_string:      TargetAddress::Offset {
                offset: 9
            },
            stack_pop_vector:       TargetAddress::Offset {
                offset: 10
            },
            stack_push_vector:      TargetAddress::Offset {
                offset: 11
            },
            free_exo_string_buffer: TargetAddress::Offset {
                offset: 12
            },
        }
    }

    fn fixture_server_state_target() -> ServerStateTarget {
        ServerStateTarget {
            app_manager:             TargetAddress::Offset {
                offset: 13
            },
            get_server_info:         TargetAddress::Offset {
                offset: 14
            },
            get_player_list:         TargetAddress::Offset {
                offset: 15
            },
            get_net_layer:           TargetAddress::Offset {
                offset: 16
            },
            get_session_max_players: TargetAddress::Offset {
                offset: 17
            },
            get_udp_port:            TargetAddress::Offset {
                offset: 18
            },
        }
    }

    fn fixture_event_target() -> EventTarget {
        let hooks = std::collections::BTreeMap::from([(
            "module_load".to_string(),
            TargetAddress::Offset {
                offset: 19
            },
        )]);
        EventTarget {
            virtual_machine: TargetAddress::Offset {
                offset: 20
            },
            run_script: TargetAddress::Offset {
                offset: 21
            },
            hooks,
            functions: std::collections::BTreeMap::new(),
        }
    }

    fn fixture_source() -> TargetSource {
        TargetSource {
            unified_commit: "3d4c4e13c6bf01b032ffe90534fc4a19eb036c03".to_string(),
            nwn_build:      8193,
            nwn_revision:   37,
            nwn_postfix:    17,
        }
    }

    fn fixture_layouts() -> AbiLayouts {
        AbiLayouts {
            c_exo_string: CExoStringLayout {
                size:                 16,
                alignment:            8,
                string_offset:        0,
                string_length_offset: 8,
                buffer_length_offset: 12,
            },
            player_list:  PlayerListLayout {
                size:            16,
                alignment:       8,
                elements_offset: 0,
                count_offset:    8,
                capacity_offset: 12,
            },
            vector:       VectorLayout {
                size:      12,
                alignment: 4,
                x_offset:  0,
                y_offset:  4,
                z_offset:  8,
            },
            classes:      EngineClassLayouts {
                game_object_id_offset: 8,
                game_object_type_offset: None,
                item_repository_parent_offset: None,
                creature_stats_base_creature_offset: None,
                creature_stats_experience_offset: None,
                item_base_item_offset: None,
                item_possessor_offset: None,
                message_read_buffer_offset: None,
                message_read_buffer_size_offset: None,
                message_read_buffer_position_offset: None,
                message_read_fragments_size_offset: None,
                message_read_fragments_position_offset: None,
                message_current_read_bit_offset: None,
                message_last_byte_bits_offset: None,
                player_object_id_offset: None,
                player_inventory_gui_offset: None,
                player_other_inventory_gui_offset: None,
                inventory_gui_selected_panel_offset: None,
                command_implementer_vm_offset: 0,
                app_manager_server_offset: 8,
                server_info_module_offset: 8,
                server_info_joining_restrictions_offset: 136,
                server_info_play_options_offset: 252,
                server_info_persistent_world_options_offset: 404,
                persistent_world_options_server_vault_by_player_name_offset: 16,
                joining_restrictions_min_level_offset: 104,
                joining_restrictions_max_level_offset: 108,
                server_exo_app_internal_offset: 8,
                internal_banned_ip_list_offset: 65920,
                internal_banned_cd_key_list_offset: 65936,
                internal_banned_player_name_list_offset: 65952,
                module_turd_list_offset: 112,
                player_turd_community_name_offset: 752,
                player_turd_first_name_offset: 768,
                player_turd_last_name_offset: 784,
                linked_list_head_offset: 0,
                linked_list_count_offset: 16,
                linked_list_node_next_offset: 8,
                linked_list_node_object_offset: 16,
                player_id_offset: 72,
                player_file_name_offset: 181,
                player_file_name_size: 17,
                net_layer_player_info_cd_key_offset: 136,
                player_cd_key_public_offset: 0,
                exo_base_alias_list_offset: 32,
                creature_stats_offset: 2760,
                creature_stats_first_name_offset: 72,
                creature_stats_last_name_offset: 88,
                vm_recursion_level_offset: 36,
                vm_script_array_offset: 40,
                vm_script_slot_count: 8,
                vm_script_size: 152,
                vm_script_alignment: 8,
                vm_script_name_offset: 24,
                vm_script_event_id_offset: 72,
            },
        }
    }

    fn test_directory() -> PathBuf {
        let sequence = NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "nwnrs-runtime-test-{}-{sequence}",
            std::process::id()
        ))
    }
}
