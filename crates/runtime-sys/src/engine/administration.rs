#[cfg(unix)]
use std::os::unix::ffi::OsStringExt as _;
use std::{
    collections::VecDeque,
    ffi::{OsString, c_void},
    fs, mem,
    path::{Path, PathBuf},
    sync::Mutex,
};

use nwnrs_runtime::{
    AdministrationCommand, AdministrationTarget, BannedLists, BridgeTarget, EngineClassLayouts,
    HostCommandResult, ShutdownTarget,
};

use super::{
    abi::{
        CExoString, GetClientObjectByObjectId, GetCreatureByGameObjectId, GetModule, GetPlayerInfo,
        RemoveLinkedListNode,
    },
    address::{GlobalStorage, Resolver},
    server::ServerEngine,
    string::copy_exo_string,
    thread::EngineThreadToken,
};
use crate::bridge::BridgeInstallError;

const MAX_ENGINE_STRING_BYTES: usize = 64 * 1024;
const FIRST_ACTIVE_PLAY_OPTION: i32 = 10;
const FIRST_ACTIVE_PLAY_OPTION_INDEX: usize = 10;
const ACTIVE_PLAY_OPTION_COUNT: usize = 19;
const MAX_TURD_COUNT: usize = 100_000;
const MAX_DEFERRED_ADMINISTRATION_COMMANDS: usize = 1_024;
const DELETE_CHARACTER_STRING_REFERENCE: u32 = 10_392;

#[derive(Clone, Copy)]
enum ShutdownOperation {
    ExitFlag(usize),
    CurrentThreadMessageQueue,
}

unsafe extern "C" {
    fn nwnrs_engine_get_string(
        address: *mut c_void,
        free_address: *mut c_void,
        object: *mut c_void,
        output: *mut u8,
        capacity: usize,
    ) -> usize;
    fn nwnrs_engine_set_string_bool(
        address: *mut c_void,
        object: *mut c_void,
        value: *const u8,
        length: usize,
    ) -> i32;
    fn nwnrs_engine_set_string_void(
        address: *mut c_void,
        object: *mut c_void,
        value: *const u8,
        length: usize,
    );
    fn nwnrs_engine_replace_string(destination: *mut c_void, value: *const u8, length: usize);
    fn nwnrs_engine_get_loc_string(
        address: *mut c_void,
        free_address: *mut c_void,
        object: *const c_void,
        output: *mut u8,
        capacity: usize,
    ) -> usize;
    fn nwnrs_engine_get_alias_path(
        address: *mut c_void,
        object: *const c_void,
        alias: *const u8,
        alias_length: usize,
        output: *mut u8,
        capacity: usize,
    ) -> usize;
    fn nwnrs_engine_disconnect_player(
        address: *mut c_void,
        object: *mut c_void,
        player_id: u32,
        string_reference: u32,
        cd_auth_failure: i32,
        reason: *const u8,
        reason_length: usize,
    ) -> i32;
}

struct DeferredPlayerCharacterDeletion {
    player_id:        u32,
    file:             PathBuf,
    preserve_backup:  bool,
    kick_message:     Vec<u8>,
    player_name:      Vec<u8>,
    player_directory: Vec<u8>,
    character_name:   Vec<u8>,
}

pub(crate) struct AdministrationEngine {
    get_session_name: usize,
    free_exo_string_buffer: usize,
    set_session_name: usize,
    get_player_password: usize,
    set_player_password: usize,
    get_game_master_password: usize,
    set_game_master_password: usize,
    enable_combat_debugging: usize,
    enable_saving_throw_debugging: usize,
    enable_movement_speed_debugging: usize,
    enable_hit_die_debugging: usize,
    shutdown: ShutdownOperation,
    server_info_module_offset: usize,
    server_info_joining_offset: usize,
    server_info_play_options_offset: usize,
    server_info_persistent_world_options_offset: usize,
    persistent_world_options_server_vault_by_player_name_offset: usize,
    joining_min_level_offset: usize,
    joining_max_level_offset: usize,
    server_exo_app_internal_offset: usize,
    internal_banned_ip_offset: usize,
    internal_banned_cd_key_offset: usize,
    internal_banned_player_offset: usize,
    add_banned_ip: usize,
    remove_banned_ip: usize,
    add_banned_cd_key: usize,
    remove_banned_cd_key: usize,
    add_banned_player_name: usize,
    remove_banned_player_name: usize,
    rules: usize,
    reload_rules: usize,
    get_module: usize,
    get_loc_string_address: usize,
    remove_linked_list_node: usize,
    module_turd_list_offset: usize,
    player_turd_community_name_offset: usize,
    player_turd_first_name_offset: usize,
    player_turd_last_name_offset: usize,
    linked_list_head_offset: usize,
    linked_list_count_offset: usize,
    linked_list_node_next_offset: usize,
    linked_list_node_object_offset: usize,
    player_id_offset: usize,
    player_file_name_offset: usize,
    player_file_name_size: usize,
    net_layer_player_info_cd_key_offset: usize,
    player_cd_key_public_offset: usize,
    exo_base_alias_list_offset: usize,
    creature_stats_offset: usize,
    creature_stats_first_name_offset: usize,
    creature_stats_last_name_offset: usize,
    main_loop: usize,
    get_client_object_by_object_id: usize,
    get_creature_by_game_object_id: usize,
    get_player_name: usize,
    get_player_info: usize,
    disconnect_player: usize,
    exo_base: usize,
    get_alias_path: usize,
    deferred_player_character_deletions: Mutex<VecDeque<DeferredPlayerCharacterDeletion>>,
}

impl AdministrationEngine {
    pub(crate) fn resolve(
        resolver: &Resolver,
        target: &AdministrationTarget,
        bridge: &BridgeTarget,
        layouts: &EngineClassLayouts,
    ) -> Result<Self, BridgeInstallError> {
        let resolve = |name, address| {
            resolver
                .resolve::<GlobalStorage>("administration", name, address)
                .map(|value| value.get())
        };
        Ok(Self {
            get_session_name: resolve("get_session_name", &target.get_session_name)?,
            free_exo_string_buffer: resolver
                .resolve::<GlobalStorage>(
                    "bridge",
                    "free_exo_string_buffer",
                    &bridge.free_exo_string_buffer,
                )?
                .get(),
            set_session_name: resolve("set_session_name", &target.set_session_name)?,
            get_player_password: resolve("get_player_password", &target.get_player_password)?,
            set_player_password: resolve("set_player_password", &target.set_player_password)?,
            get_game_master_password: resolve(
                "get_game_master_password",
                &target.get_game_master_password,
            )?,
            set_game_master_password: resolve(
                "set_game_master_password",
                &target.set_game_master_password,
            )?,
            enable_combat_debugging: resolve(
                "enable_combat_debugging",
                &target.enable_combat_debugging,
            )?,
            enable_saving_throw_debugging: resolve(
                "enable_saving_throw_debugging",
                &target.enable_saving_throw_debugging,
            )?,
            enable_movement_speed_debugging: resolve(
                "enable_movement_speed_debugging",
                &target.enable_movement_speed_debugging,
            )?,
            enable_hit_die_debugging: resolve(
                "enable_hit_die_debugging",
                &target.enable_hit_die_debugging,
            )?,
            shutdown: match &target.shutdown {
                ShutdownTarget::ExitFlag {
                    address,
                } => ShutdownOperation::ExitFlag(resolve("shutdown.address", address)?),
                ShutdownTarget::CurrentThreadMessageQueue => {
                    ShutdownOperation::CurrentThreadMessageQueue
                }
            },
            server_info_module_offset: checked_offset(
                "server_info_module_offset",
                layouts.server_info_module_offset,
            )?,
            server_info_joining_offset: checked_offset(
                "server_info_joining_restrictions_offset",
                layouts.server_info_joining_restrictions_offset,
            )?,
            server_info_play_options_offset: checked_offset(
                "server_info_play_options_offset",
                layouts.server_info_play_options_offset,
            )?,
            server_info_persistent_world_options_offset: checked_offset(
                "server_info_persistent_world_options_offset",
                layouts.server_info_persistent_world_options_offset,
            )?,
            persistent_world_options_server_vault_by_player_name_offset: checked_offset(
                "persistent_world_options_server_vault_by_player_name_offset",
                layouts.persistent_world_options_server_vault_by_player_name_offset,
            )?,
            joining_min_level_offset: checked_offset(
                "joining_restrictions_min_level_offset",
                layouts.joining_restrictions_min_level_offset,
            )?,
            joining_max_level_offset: checked_offset(
                "joining_restrictions_max_level_offset",
                layouts.joining_restrictions_max_level_offset,
            )?,
            server_exo_app_internal_offset: checked_offset(
                "server_exo_app_internal_offset",
                layouts.server_exo_app_internal_offset,
            )?,
            internal_banned_ip_offset: checked_offset(
                "internal_banned_ip_list_offset",
                layouts.internal_banned_ip_list_offset,
            )?,
            internal_banned_cd_key_offset: checked_offset(
                "internal_banned_cd_key_list_offset",
                layouts.internal_banned_cd_key_list_offset,
            )?,
            internal_banned_player_offset: checked_offset(
                "internal_banned_player_name_list_offset",
                layouts.internal_banned_player_name_list_offset,
            )?,
            add_banned_ip: resolve("add_banned_ip", &target.add_banned_ip)?,
            remove_banned_ip: resolve("remove_banned_ip", &target.remove_banned_ip)?,
            add_banned_cd_key: resolve("add_banned_cd_key", &target.add_banned_cd_key)?,
            remove_banned_cd_key: resolve("remove_banned_cd_key", &target.remove_banned_cd_key)?,
            add_banned_player_name: resolve(
                "add_banned_player_name",
                &target.add_banned_player_name,
            )?,
            remove_banned_player_name: resolve(
                "remove_banned_player_name",
                &target.remove_banned_player_name,
            )?,
            rules: resolve("rules", &target.rules)?,
            reload_rules: resolve("reload_rules", &target.reload_rules)?,
            get_module: resolve("get_module", &target.get_module)?,
            get_loc_string_address: resolve("get_loc_string", &target.get_loc_string)?,
            remove_linked_list_node: resolve(
                "remove_linked_list_node",
                &target.remove_linked_list_node,
            )?,
            module_turd_list_offset: checked_offset(
                "module_turd_list_offset",
                layouts.module_turd_list_offset,
            )?,
            player_turd_community_name_offset: checked_offset(
                "player_turd_community_name_offset",
                layouts.player_turd_community_name_offset,
            )?,
            player_turd_first_name_offset: checked_offset(
                "player_turd_first_name_offset",
                layouts.player_turd_first_name_offset,
            )?,
            player_turd_last_name_offset: checked_offset(
                "player_turd_last_name_offset",
                layouts.player_turd_last_name_offset,
            )?,
            linked_list_head_offset: checked_offset(
                "linked_list_head_offset",
                layouts.linked_list_head_offset,
            )?,
            linked_list_count_offset: checked_offset(
                "linked_list_count_offset",
                layouts.linked_list_count_offset,
            )?,
            linked_list_node_next_offset: checked_offset(
                "linked_list_node_next_offset",
                layouts.linked_list_node_next_offset,
            )?,
            linked_list_node_object_offset: checked_offset(
                "linked_list_node_object_offset",
                layouts.linked_list_node_object_offset,
            )?,
            player_id_offset: checked_offset("player_id_offset", layouts.player_id_offset)?,
            player_file_name_offset: checked_offset(
                "player_file_name_offset",
                layouts.player_file_name_offset,
            )?,
            player_file_name_size: checked_offset(
                "player_file_name_size",
                layouts.player_file_name_size,
            )?,
            net_layer_player_info_cd_key_offset: checked_offset(
                "net_layer_player_info_cd_key_offset",
                layouts.net_layer_player_info_cd_key_offset,
            )?,
            player_cd_key_public_offset: checked_offset(
                "player_cd_key_public_offset",
                layouts.player_cd_key_public_offset,
            )?,
            exo_base_alias_list_offset: checked_offset(
                "exo_base_alias_list_offset",
                layouts.exo_base_alias_list_offset,
            )?,
            creature_stats_offset: checked_offset(
                "creature_stats_offset",
                layouts.creature_stats_offset,
            )?,
            creature_stats_first_name_offset: checked_offset(
                "creature_stats_first_name_offset",
                layouts.creature_stats_first_name_offset,
            )?,
            creature_stats_last_name_offset: checked_offset(
                "creature_stats_last_name_offset",
                layouts.creature_stats_last_name_offset,
            )?,
            main_loop: resolve("main_loop", &target.main_loop)?,
            get_client_object_by_object_id: resolve(
                "get_client_object_by_object_id",
                &target.get_client_object_by_object_id,
            )?,
            get_creature_by_game_object_id: resolve(
                "get_creature_by_game_object_id",
                &target.get_creature_by_game_object_id,
            )?,
            get_player_name: resolve("get_player_name", &target.get_player_name)?,
            get_player_info: resolve("get_player_info", &target.get_player_info)?,
            disconnect_player: resolve("disconnect_player", &target.disconnect_player)?,
            exo_base: resolve("exo_base", &target.exo_base)?,
            get_alias_path: resolve("get_alias_path", &target.get_alias_path)?,
            deferred_player_character_deletions: Mutex::new(VecDeque::new()),
        })
    }

    pub(crate) const fn main_loop_hook_target(&self) -> usize {
        self.main_loop
    }

    pub(crate) fn process_deferred(
        &self,
        _thread: &EngineThreadToken,
        server: &ServerEngine,
    ) -> Result<(), BridgeInstallError> {
        loop {
            let command = self
                .deferred_player_character_deletions
                .lock()
                .map_err(|_error| {
                    BridgeInstallError::new("deferred administration queue was poisoned")
                })?
                .pop_front();
            let Some(command) = command else {
                return Ok(());
            };
            if let Err(error) = self.delete_player_character(server, &command) {
                tracing::error!(
                    target: "nwnrs::administration",
                    player_id = command.player_id,
                    error = %error,
                    "deferred player-character deletion failed"
                );
            }
        }
    }

    pub(crate) fn server_name(
        &self,
        _thread: &EngineThreadToken,
        server: &ServerEngine,
    ) -> Result<Vec<u8>, BridgeInstallError> {
        self.get_string(self.get_session_name, server.net_layer()?)
    }

    pub(crate) fn player_password_is_set(
        &self,
        _thread: &EngineThreadToken,
        server: &ServerEngine,
    ) -> Result<bool, BridgeInstallError> {
        Ok(self.get_string_length(self.get_player_password, server.net_layer()?)? != 0)
    }

    pub(crate) fn dm_password_is_set(
        &self,
        _thread: &EngineThreadToken,
        server: &ServerEngine,
    ) -> Result<bool, BridgeInstallError> {
        Ok(self.get_string_length(self.get_game_master_password, server.net_layer()?)? != 0)
    }

    pub(crate) fn min_level(
        &self,
        _thread: &EngineThreadToken,
        server: &ServerEngine,
    ) -> Result<i32, BridgeInstallError> {
        self.read_joining_level(
            server.server_info()?,
            self.joining_min_level_offset,
            "minimum level",
        )
    }

    pub(crate) fn max_level(
        &self,
        _thread: &EngineThreadToken,
        server: &ServerEngine,
    ) -> Result<i32, BridgeInstallError> {
        self.read_joining_level(
            server.server_info()?,
            self.joining_max_level_offset,
            "maximum level",
        )
    }

    pub(crate) fn play_option(
        &self,
        _thread: &EngineThreadToken,
        server: &ServerEngine,
        option: i32,
    ) -> Result<i32, BridgeInstallError> {
        let index = usize::try_from(option - FIRST_ACTIVE_PLAY_OPTION)
            .ok()
            .filter(|index| *index < ACTIVE_PLAY_OPTION_COUNT)
            .ok_or_else(|| BridgeInstallError::new(format!("unsupported play option {option}")))?;
        let offset = self
            .server_info_play_options_offset
            .checked_add(
                index
                    .saturating_add(FIRST_ACTIVE_PLAY_OPTION_INDEX)
                    .saturating_mul(mem::size_of::<i32>()),
            )
            .ok_or_else(|| BridgeInstallError::new("play option offset overflow"))?;
        read_i32(server.server_info()?, offset, "CPlayOptions field")
    }

    pub(crate) fn debug_value(
        &self,
        _thread: &EngineThreadToken,
        debug_type: i32,
    ) -> Result<i32, BridgeInstallError> {
        match debug_type {
            0 => read_global_i32(self.enable_combat_debugging, "combat debugging"),
            1 => read_global_i32(self.enable_saving_throw_debugging, "saving throw debugging"),
            2 => read_global_i32(
                self.enable_movement_speed_debugging,
                "movement speed debugging",
            ),
            3 => read_global_i32(self.enable_hit_die_debugging, "hit die debugging"),
            _ => Err(BridgeInstallError::new(format!(
                "unsupported debug type {debug_type}"
            ))),
        }
    }

    pub(crate) fn banned_lists(
        &self,
        _thread: &EngineThreadToken,
        server: &ServerEngine,
    ) -> Result<BannedLists, BridgeInstallError> {
        let server_internal = self.server_internal(server.server_exo_app()?)?;
        Ok(BannedLists {
            ip_addresses: self.read_string_list(
                server_internal,
                self.internal_banned_ip_offset,
                "banned IP list",
            )?,
            cd_keys:      self.read_string_list(
                server_internal,
                self.internal_banned_cd_key_offset,
                "banned CD key list",
            )?,
            player_names: self.read_string_list(
                server_internal,
                self.internal_banned_player_offset,
                "banned player-name list",
            )?,
        })
    }

    fn delete_turd(
        &self,
        server: &ServerEngine,
        player_name: &[u8],
        character_name: &[u8],
    ) -> Result<bool, BridgeInstallError> {
        // SAFETY: the exact target pack binds this address to
        // `CServerExoApp::GetModule()` with the compiler-verified signature.
        let get_module = unsafe { mem::transmute::<usize, GetModule>(self.get_module) };
        let module = get_module(server.server_exo_app()?);
        if module.is_null() {
            return Err(BridgeInstallError::new(
                "CServerExoApp::GetModule returned null",
            ));
        }
        let list = read_pointer(
            module,
            self.module_turd_list_offset,
            "CNWSModule::m_lstTURDList",
        )?;
        let count = usize::try_from(read_u32(
            list,
            self.linked_list_count_offset,
            "TURD list count",
        )?)
        .map_err(|_error| BridgeInstallError::new("TURD list count exceeds usize"))?;
        if count > MAX_TURD_COUNT {
            return Err(BridgeInstallError::new(format!(
                "TURD list count {count} exceeds the safety limit"
            )));
        }
        let mut node = read_nullable_pointer(list, self.linked_list_head_offset, "TURD list head")?;
        for _index in 0..count {
            if node.is_null() {
                return Err(BridgeInstallError::new(
                    "TURD list ended before its declared count",
                ));
            }
            let next = read_nullable_pointer(
                node,
                self.linked_list_node_next_offset,
                "TURD list next node",
            )?;
            let turd = read_pointer(
                node,
                self.linked_list_node_object_offset,
                "TURD list object",
            )?;
            if self.turd_matches(turd, player_name, character_name)? {
                // SAFETY: the exact target pack binds this address to
                // `CExoLinkedListInternal::Remove` and node belongs to list.
                let remove = unsafe {
                    mem::transmute::<usize, RemoveLinkedListNode>(self.remove_linked_list_node)
                };
                let removed = remove(list, node);
                if removed.is_null() {
                    return Err(BridgeInstallError::new(
                        "CExoLinkedListInternal::Remove returned null",
                    ));
                }
                return Ok(true);
            }
            node = next;
        }
        if !node.is_null() {
            return Err(BridgeInstallError::new(
                "TURD list contains more nodes than its declared count",
            ));
        }
        Ok(false)
    }

    fn prepare_player_character_deletion(
        &self,
        server: &ServerEngine,
        object_id: u32,
        preserve_backup: bool,
        kick_message: &[u8],
    ) -> Result<DeferredPlayerCharacterDeletion, BridgeInstallError> {
        // SAFETY: the target pack binds the verified player lookup signature.
        let get_player = unsafe {
            mem::transmute::<usize, GetClientObjectByObjectId>(self.get_client_object_by_object_id)
        };
        let player = get_player(server.server_exo_app()?, object_id);
        if player.is_null() {
            return Err(BridgeInstallError::new(format!(
                "object {object_id:#010x} is not controlled by a connected player"
            )));
        }
        let player_id = read_u32(player, self.player_id_offset, "CNWSPlayer::m_nPlayerID")?;
        let file_name = self.player_file_name(player)?;
        validate_path_component(&file_name, "player character resref")?;
        let player_name = self.get_string(self.get_player_name, player)?;
        validate_path_component(&player_name, "player community name")?;

        let server_info = server.server_info()?;
        let vault_option_offset = self
            .server_info_persistent_world_options_offset
            .checked_add(self.persistent_world_options_server_vault_by_player_name_offset)
            .ok_or_else(|| {
                BridgeInstallError::new("server-vault player-name option offset overflow")
            })?;
        let player_directory = if read_i32(
            server_info,
            vault_option_offset,
            "CPersistantWorldOptions::bServerVaultByPlayerName",
        )? != 0
        {
            player_name.clone()
        } else {
            self.player_public_cd_key(server.net_layer()?, player_id)?
        };
        validate_path_component(&player_directory, "server-vault player directory")?;

        let mut file = self.server_vault_path()?;
        file.push(engine_os_string(
            player_directory.clone(),
            "server-vault player directory",
        )?);
        let mut bic_name = file_name;
        bic_name.extend_from_slice(b".bic");
        file.push(engine_os_string(bic_name, "player character filename")?);
        ensure_regular_file(&file)?;

        let character_name = self.player_character_name(server, object_id)?;
        Ok(DeferredPlayerCharacterDeletion {
            player_id,
            file,
            preserve_backup,
            kick_message: kick_message.to_vec(),
            player_name,
            player_directory,
            character_name,
        })
    }

    fn queue_player_character_deletion(
        &self,
        command: DeferredPlayerCharacterDeletion,
    ) -> Result<(), BridgeInstallError> {
        let mut queue = self
            .deferred_player_character_deletions
            .lock()
            .map_err(|_error| {
                BridgeInstallError::new("deferred administration queue was poisoned")
            })?;
        if queue.len() >= MAX_DEFERRED_ADMINISTRATION_COMMANDS {
            return Err(BridgeInstallError::new(
                "deferred administration queue reached its safety limit",
            ));
        }
        queue.push_back(command);
        Ok(())
    }

    fn delete_player_character(
        &self,
        server: &ServerEngine,
        command: &DeferredPlayerCharacterDeletion,
    ) -> Result<(), BridgeInstallError> {
        ensure_regular_file(&command.file)?;
        let network = server.net_layer()?;
        // SAFETY: the target pack binds `CNetLayer::DisconnectPlayer`, while
        // the C++ thunk owns the temporary reason string for the complete call.
        let disconnected = unsafe {
            nwnrs_engine_disconnect_player(
                self.disconnect_player as *mut c_void,
                network,
                command.player_id,
                DELETE_CHARACTER_STRING_REFERENCE,
                1,
                command.kick_message.as_ptr(),
                command.kick_message.len(),
            )
        };
        if disconnected == 0 {
            return Err(BridgeInstallError::new(format!(
                "engine refused to disconnect player {}",
                command.player_id
            )));
        }

        let backup = if command.preserve_backup {
            Some(backup_and_remove(&command.file).map_err(|error| {
                BridgeInstallError::new(format!(
                    "failed to back up and remove {}: {error}",
                    command.file.display()
                ))
            })?)
        } else {
            fs::remove_file(&command.file).map_err(|error| {
                BridgeInstallError::new(format!(
                    "failed to remove {}: {error}",
                    command.file.display()
                ))
            })?;
            None
        };

        let mut turd_removed =
            self.delete_turd(server, &command.player_name, &command.character_name)?;
        if !turd_removed && command.player_directory != command.player_name {
            turd_removed =
                self.delete_turd(server, &command.player_directory, &command.character_name)?;
        }
        tracing::info!(
            target: "nwnrs::administration",
            player_id = command.player_id,
            character = %String::from_utf8_lossy(&command.character_name),
            file = %command.file.display(),
            backup = backup.as_ref().map(|path| path.display().to_string()),
            turd_removed,
            "deleted player character"
        );
        Ok(())
    }

    fn player_file_name(&self, player: *mut c_void) -> Result<Vec<u8>, BridgeInstallError> {
        if self.player_file_name_size == 0 || self.player_file_name_size > 1_024 {
            return Err(BridgeInstallError::new(
                "target-pack CResRef size is outside the supported range",
            ));
        }
        // SAFETY: the ABI probe locates the fixed-size inline CResRef buffer.
        let bytes = unsafe {
            std::slice::from_raw_parts(
                player.cast::<u8>().add(self.player_file_name_offset),
                self.player_file_name_size,
            )
        };
        let length = bytes.iter().position(|byte| *byte == 0).ok_or_else(|| {
            BridgeInstallError::new("player character CResRef is not NUL terminated")
        })?;
        if length == 0 {
            return Err(BridgeInstallError::new(
                "player character has an empty server-vault resref",
            ));
        }
        Ok(bytes.get(..length).unwrap_or_default().to_vec())
    }

    fn player_public_cd_key(
        &self,
        network: *mut c_void,
        player_id: u32,
    ) -> Result<Vec<u8>, BridgeInstallError> {
        // SAFETY: the target pack binds `CNetLayer::GetPlayerInfo` exactly.
        let get_player_info =
            unsafe { mem::transmute::<usize, GetPlayerInfo>(self.get_player_info) };
        let player_info = get_player_info(network, player_id);
        if player_info.is_null() {
            return Err(BridgeInstallError::new(format!(
                "CNetLayer::GetPlayerInfo returned null for player {player_id}"
            )));
        }
        let offset = self
            .net_layer_player_info_cd_key_offset
            .checked_add(self.player_cd_key_public_offset)
            .ok_or_else(|| BridgeInstallError::new("public CD-key offset overflow"))?;
        // SAFETY: the ABI probe locates the owned public-key CExoString.
        copy_exo_string(unsafe { &*player_info.cast::<u8>().add(offset).cast::<CExoString>() })
    }

    fn server_vault_path(&self) -> Result<PathBuf, BridgeInstallError> {
        // SAFETY: the target address names global `g_pExoBase` storage.
        let exo_base = unsafe { (self.exo_base as *const *mut c_void).read() };
        let alias_list = read_pointer(
            exo_base,
            self.exo_base_alias_list_offset,
            "CExoBase::m_pcExoAliasList",
        )?;
        let alias = b"SERVERVAULT";
        let mut output = vec![0; MAX_ENGINE_STRING_BYTES];
        // SAFETY: the target binds the compiler-verified const reference ABI.
        let length = unsafe {
            nwnrs_engine_get_alias_path(
                self.get_alias_path as *mut c_void,
                alias_list,
                alias.as_ptr(),
                alias.len(),
                output.as_mut_ptr(),
                output.len(),
            )
        };
        if length == 0 || length > output.len() {
            return Err(BridgeInstallError::new(
                "SERVERVAULT alias is empty or exceeds the engine string limit",
            ));
        }
        output.truncate(length);
        Ok(PathBuf::from(engine_os_string(
            output,
            "SERVERVAULT alias",
        )?))
    }

    fn player_character_name(
        &self,
        server: &ServerEngine,
        object_id: u32,
    ) -> Result<Vec<u8>, BridgeInstallError> {
        // SAFETY: the target pack binds the verified creature lookup signature.
        let get_creature = unsafe {
            mem::transmute::<usize, GetCreatureByGameObjectId>(self.get_creature_by_game_object_id)
        };
        let creature = get_creature(server.server_exo_app()?, object_id);
        if creature.is_null() {
            return Err(BridgeInstallError::new(format!(
                "object {object_id:#010x} is not a live creature"
            )));
        }
        let stats = read_pointer(
            creature,
            self.creature_stats_offset,
            "CNWSCreature::m_pStats",
        )?;
        let first_name = self.get_loc_string(unsafe {
            stats
                .cast::<u8>()
                .add(self.creature_stats_first_name_offset)
                .cast()
        })?;
        let last_name = self.get_loc_string(unsafe {
            stats
                .cast::<u8>()
                .add(self.creature_stats_last_name_offset)
                .cast()
        })?;
        let mut full_name = first_name;
        if !last_name.is_empty() {
            if !full_name.is_empty() {
                full_name.push(b' ');
            }
            full_name.extend_from_slice(&last_name);
        }
        if full_name.is_empty() {
            return Err(BridgeInstallError::new(
                "player character has an empty localized name",
            ));
        }
        Ok(full_name)
    }

    fn turd_matches(
        &self,
        turd: *mut c_void,
        player_name: &[u8],
        character_name: &[u8],
    ) -> Result<bool, BridgeInstallError> {
        // SAFETY: the ABI probe locates the embedded live CExoString.
        let community_name = copy_exo_string(unsafe {
            &*turd
                .cast::<u8>()
                .add(self.player_turd_community_name_offset)
                .cast::<CExoString>()
        })?;
        if community_name != player_name {
            return Ok(false);
        }
        let first_name = self.get_loc_string(unsafe {
            turd.cast::<u8>()
                .add(self.player_turd_first_name_offset)
                .cast()
        })?;
        let last_name = self.get_loc_string(unsafe {
            turd.cast::<u8>()
                .add(self.player_turd_last_name_offset)
                .cast()
        })?;
        let mut full_name = first_name;
        if !last_name.is_empty() {
            if !full_name.is_empty() {
                full_name.push(b' ');
            }
            full_name.extend_from_slice(&last_name);
        }
        Ok(full_name == character_name)
    }

    fn get_loc_string(&self, object: *const c_void) -> Result<Vec<u8>, BridgeInstallError> {
        let mut output = vec![0; MAX_ENGINE_STRING_BYTES];
        // SAFETY: the exact target binds the verified GetStringLoc method and
        // object points to an ABI-probed embedded CExoLocString.
        let length = unsafe {
            nwnrs_engine_get_loc_string(
                self.get_loc_string_address as *mut c_void,
                self.free_exo_string_buffer as *mut c_void,
                object,
                output.as_mut_ptr(),
                output.len(),
            )
        };
        if length > output.len() {
            return Err(BridgeInstallError::new(format!(
                "localized engine string exceeds {MAX_ENGINE_STRING_BYTES} bytes"
            )));
        }
        output.truncate(length);
        Ok(output)
    }

    pub(crate) fn execute(
        &self,
        _thread: &EngineThreadToken,
        server: &ServerEngine,
        command: &AdministrationCommand,
    ) -> Result<HostCommandResult, BridgeInstallError> {
        match command {
            AdministrationCommand::SetModuleName(value) => {
                let server_info = server.server_info()?;
                // SAFETY: the ABI snapshot locates the live owned CExoString,
                // and the C++ thunk performs its normal allocation semantics.
                unsafe {
                    nwnrs_engine_replace_string(
                        server_info
                            .cast::<u8>()
                            .add(self.server_info_module_offset)
                            .cast(),
                        value.as_ptr(),
                        value.len(),
                    );
                }
            }
            AdministrationCommand::SetServerName(value) => {
                let network = server.net_layer()?;
                // SAFETY: the target address and C++ thunk share the verified
                // `CNetLayer::SetSessionName(CExoString)` ABI.
                unsafe {
                    nwnrs_engine_set_string_void(
                        self.set_session_name as *mut c_void,
                        network,
                        value.as_ptr(),
                        value.len(),
                    );
                }
            }
            AdministrationCommand::SetPlayerPassword(value) => {
                self.set_password(
                    self.set_player_password,
                    server.net_layer()?,
                    value,
                    "player password",
                )?;
            }
            AdministrationCommand::SetDmPassword(value) => {
                self.set_password(
                    self.set_game_master_password,
                    server.net_layer()?,
                    value,
                    "DM password",
                )?;
            }
            AdministrationCommand::SetMinLevel(level) => self.write_joining_level(
                server.server_info()?,
                self.joining_min_level_offset,
                *level,
                "minimum level",
            )?,
            AdministrationCommand::SetMaxLevel(level) => self.write_joining_level(
                server.server_info()?,
                self.joining_max_level_offset,
                *level,
                "maximum level",
            )?,
            AdministrationCommand::SetPlayOption {
                option,
                value,
            } => {
                let index =
                    usize::try_from(option - FIRST_ACTIVE_PLAY_OPTION).map_err(|_error| {
                        BridgeInstallError::new("validated play option cannot be represented")
                    })?;
                let offset = self
                    .server_info_play_options_offset
                    .checked_add(
                        index
                            .saturating_add(10)
                            .saturating_mul(mem::size_of::<i32>()),
                    )
                    .ok_or_else(|| BridgeInstallError::new("play option offset overflow"))?;
                write_i32(server.server_info()?, offset, *value, "CPlayOptions field")?;
            }
            AdministrationCommand::SetDebugValue {
                debug_type,
                value,
            } => {
                let address = match debug_type {
                    0 => self.enable_combat_debugging,
                    1 => self.enable_saving_throw_debugging,
                    2 => self.enable_movement_speed_debugging,
                    3 => self.enable_hit_die_debugging,
                    _ => {
                        return Err(BridgeInstallError::new(
                            "validated debug type is outside the engine table",
                        ));
                    }
                };
                write_global_i32(address, *value, "debug toggle")?;
            }
            AdministrationCommand::RequestShutdown => match self.shutdown {
                ShutdownOperation::ExitFlag(address) => {
                    write_global_i32(address, 1, "server exit flag")?;
                }
                ShutdownOperation::CurrentThreadMessageQueue => {
                    request_current_thread_shutdown()?;
                }
            },
            AdministrationCommand::AddBannedIp(value) => {
                self.call_server_string(self.add_banned_ip, server.server_exo_app()?, value);
            }
            AdministrationCommand::RemoveBannedIp(value) => {
                self.call_server_string(self.remove_banned_ip, server.server_exo_app()?, value);
            }
            AdministrationCommand::AddBannedCdKey(value) => {
                self.call_server_string(self.add_banned_cd_key, server.server_exo_app()?, value);
            }
            AdministrationCommand::RemoveBannedCdKey(value) => {
                self.call_server_string(self.remove_banned_cd_key, server.server_exo_app()?, value);
            }
            AdministrationCommand::AddBannedPlayerName(value) => {
                self.call_server_string(
                    self.add_banned_player_name,
                    server.server_exo_app()?,
                    value,
                );
            }
            AdministrationCommand::RemoveBannedPlayerName(value) => {
                self.call_server_string(
                    self.remove_banned_player_name,
                    server.server_exo_app()?,
                    value,
                );
            }
            AdministrationCommand::ReloadRules => {
                // SAFETY: the target binds the global storage and method to
                // the pinned `CNWRules` ABI.
                let rules = unsafe { (self.rules as *const *mut c_void).read() };
                if rules.is_null() {
                    return Err(BridgeInstallError::new("global CNWRules pointer is null"));
                }
                // SAFETY: `reload_rules` is `CNWRules::ReloadAll()` and this
                // callback executes synchronously on the server thread.
                let reload = unsafe {
                    mem::transmute::<usize, extern "C" fn(*mut c_void)>(self.reload_rules)
                };
                reload(rules);
            }
            AdministrationCommand::DeletePlayerCharacter {
                object_id,
                preserve_backup,
                kick_message,
            } => {
                let command = self.prepare_player_character_deletion(
                    server,
                    *object_id,
                    *preserve_backup,
                    kick_message,
                )?;
                self.queue_player_character_deletion(command)?;
            }
            AdministrationCommand::DeleteTurd {
                player_name,
                character_name,
            } => {
                return self
                    .delete_turd(server, player_name, character_name)
                    .map(HostCommandResult::Boolean);
            }
        }
        Ok(HostCommandResult::None)
    }

    fn get_string(
        &self,
        address: usize,
        object: *mut c_void,
    ) -> Result<Vec<u8>, BridgeInstallError> {
        let mut output = vec![0; MAX_ENGINE_STRING_BYTES];
        // SAFETY: the exact target address is a compiler-verified CExoString
        // getter and the output buffer is writable for its declared capacity.
        let length = unsafe {
            nwnrs_engine_get_string(
                address as *mut c_void,
                self.free_exo_string_buffer as *mut c_void,
                object,
                output.as_mut_ptr(),
                output.len(),
            )
        };
        if length > output.len() {
            return Err(BridgeInstallError::new(format!(
                "engine string exceeds {MAX_ENGINE_STRING_BYTES} bytes"
            )));
        }
        output.truncate(length);
        Ok(output)
    }

    fn get_string_length(
        &self,
        address: usize,
        object: *mut c_void,
    ) -> Result<usize, BridgeInstallError> {
        // SAFETY: the exact target address is a compiler-verified CExoString
        // getter. A null output requests only the returned string length.
        let length = unsafe {
            nwnrs_engine_get_string(
                address as *mut c_void,
                self.free_exo_string_buffer as *mut c_void,
                object,
                std::ptr::null_mut(),
                0,
            )
        };
        if length > MAX_ENGINE_STRING_BYTES {
            Err(BridgeInstallError::new(format!(
                "engine string exceeds {MAX_ENGINE_STRING_BYTES} bytes"
            )))
        } else {
            Ok(length)
        }
    }

    fn set_password(
        &self,
        address: usize,
        network: *mut c_void,
        value: &[u8],
        name: &str,
    ) -> Result<(), BridgeInstallError> {
        // SAFETY: the exact target address is the verified CNetLayer setter;
        // the C++ thunk constructs and destroys its by-value CExoString.
        let result = unsafe {
            nwnrs_engine_set_string_bool(
                address as *mut c_void,
                network,
                value.as_ptr(),
                value.len(),
            )
        };
        if result == 0 {
            Err(BridgeInstallError::new(format!(
                "engine rejected the {name} update"
            )))
        } else {
            Ok(())
        }
    }

    fn call_server_string(&self, address: usize, server: *mut c_void, value: &[u8]) {
        // SAFETY: every address passed here is a verified void member function
        // accepting one by-value CExoString. The C++ thunk owns the temporary.
        unsafe {
            nwnrs_engine_set_string_void(
                address as *mut c_void,
                server,
                value.as_ptr(),
                value.len(),
            );
        }
    }

    fn server_internal(&self, server: *mut c_void) -> Result<*mut c_void, BridgeInstallError> {
        if server.is_null() {
            return Err(BridgeInstallError::new("CServerExoApp is null"));
        }
        // SAFETY: the ABI probe identifies the internal-pointer field.
        let internal = unsafe {
            server
                .cast::<u8>()
                .add(self.server_exo_app_internal_offset)
                .cast::<*mut c_void>()
                .read()
        };
        if internal.is_null() {
            Err(BridgeInstallError::new(
                "CServerExoApp::m_pcExoAppInternal is null",
            ))
        } else {
            Ok(internal)
        }
    }

    fn read_string_list(
        &self,
        internal: *mut c_void,
        offset: usize,
        name: &str,
    ) -> Result<Vec<Vec<u8>>, BridgeInstallError> {
        // SAFETY: the ABI probe binds offset to a CExoArrayList<CExoString>.
        let list = unsafe { internal.cast::<u8>().add(offset) };
        // SAFETY: CExoArrayList begins with its element pointer and i32 count.
        let elements = unsafe { list.cast::<*const CExoString>().read() };
        // SAFETY: the verified common list ABI places count at byte offset 8.
        let count = unsafe { list.add(8).cast::<i32>().read() };
        let count = usize::try_from(count).map_err(|_error| {
            BridgeInstallError::new(format!("{name} contains a negative count"))
        })?;
        if count > 100_000 {
            return Err(BridgeInstallError::new(format!(
                "{name} count {count} exceeds the safety limit"
            )));
        }
        if count != 0 && elements.is_null() {
            return Err(BridgeInstallError::new(format!(
                "{name} has elements but a null buffer"
            )));
        }
        let mut values = Vec::with_capacity(count);
        for index in 0..count {
            // SAFETY: count bounds the live contiguous element buffer during
            // this synchronous engine callback.
            values.push(copy_exo_string(unsafe { &*elements.add(index) })?);
        }
        Ok(values)
    }

    fn read_joining_level(
        &self,
        server_info: *mut c_void,
        field_offset: usize,
        name: &str,
    ) -> Result<i32, BridgeInstallError> {
        let offset = self
            .server_info_joining_offset
            .checked_add(field_offset)
            .ok_or_else(|| BridgeInstallError::new(format!("{name} offset overflow")))?;
        read_i32(server_info, offset, name)
    }

    fn write_joining_level(
        &self,
        server_info: *mut c_void,
        field_offset: usize,
        value: i32,
        name: &str,
    ) -> Result<(), BridgeInstallError> {
        let offset = self
            .server_info_joining_offset
            .checked_add(field_offset)
            .ok_or_else(|| BridgeInstallError::new(format!("{name} offset overflow")))?;
        write_i32(server_info, offset, value, name)
    }
}

fn validate_path_component(value: &[u8], name: &str) -> Result<(), BridgeInstallError> {
    if value.is_empty() {
        return Err(BridgeInstallError::new(format!("{name} cannot be empty")));
    }
    if matches!(value, b"." | b"..") || value.iter().any(|byte| matches!(*byte, 0 | b'/' | b'\\')) {
        return Err(BridgeInstallError::new(format!(
            "{name} is not a safe path component"
        )));
    }
    Ok(())
}

fn ensure_regular_file(file: &Path) -> Result<(), BridgeInstallError> {
    let metadata = fs::symlink_metadata(file).map_err(|error| {
        BridgeInstallError::new(format!(
            "server-vault character {} is unavailable: {error}",
            file.display()
        ))
    })?;
    if !metadata.file_type().is_file() {
        return Err(BridgeInstallError::new(format!(
            "server-vault character {} is not a regular file",
            file.display()
        )));
    }
    Ok(())
}

fn backup_and_remove(file: &Path) -> std::io::Result<PathBuf> {
    for index in 0..10_000_u32 {
        let mut backup = file.as_os_str().to_os_string();
        backup.push(format!(".deleted{index}"));
        let backup = PathBuf::from(backup);
        match fs::hard_link(file, &backup) {
            Ok(()) => {
                fs::remove_file(file)?;
                return Ok(backup);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "no unused .deleted backup name remains",
    ))
}

#[cfg(unix)]
fn engine_os_string(bytes: Vec<u8>, _name: &str) -> Result<OsString, BridgeInstallError> {
    Ok(OsString::from_vec(bytes))
}

#[cfg(windows)]
fn engine_os_string(bytes: Vec<u8>, name: &str) -> Result<OsString, BridgeInstallError> {
    String::from_utf8(bytes)
        .map(OsString::from)
        .map_err(|_error| BridgeInstallError::new(format!("{name} is not valid UTF-8")))
}

fn checked_offset(name: &str, value: u64) -> Result<usize, BridgeInstallError> {
    usize::try_from(value).map_err(|_error| {
        BridgeInstallError::new(format!("target-pack offset {name} exceeds usize"))
    })
}

fn read_i32(object: *mut c_void, offset: usize, name: &str) -> Result<i32, BridgeInstallError> {
    if object.is_null() {
        return Err(BridgeInstallError::new(format!("{name} owner is null")));
    }
    // SAFETY: every caller supplies an ABI-probed, i32-aligned field offset.
    Ok(unsafe { object.cast::<u8>().add(offset).cast::<i32>().read() })
}

fn read_u32(object: *mut c_void, offset: usize, name: &str) -> Result<u32, BridgeInstallError> {
    if object.is_null() {
        return Err(BridgeInstallError::new(format!("{name} owner is null")));
    }
    // SAFETY: the ABI probe supplies a u32-aligned field offset.
    Ok(unsafe { object.cast::<u8>().add(offset).cast::<u32>().read() })
}

fn read_nullable_pointer(
    object: *mut c_void,
    offset: usize,
    name: &str,
) -> Result<*mut c_void, BridgeInstallError> {
    if object.is_null() {
        return Err(BridgeInstallError::new(format!("{name} owner is null")));
    }
    // SAFETY: the ABI probe supplies a pointer-aligned field offset.
    Ok(unsafe { object.cast::<u8>().add(offset).cast::<*mut c_void>().read() })
}

fn read_pointer(
    object: *mut c_void,
    offset: usize,
    name: &str,
) -> Result<*mut c_void, BridgeInstallError> {
    let value = read_nullable_pointer(object, offset, name)?;
    if value.is_null() {
        Err(BridgeInstallError::new(format!("{name} is null")))
    } else {
        Ok(value)
    }
}

fn write_i32(
    object: *mut c_void,
    offset: usize,
    value: i32,
    name: &str,
) -> Result<(), BridgeInstallError> {
    if object.is_null() {
        return Err(BridgeInstallError::new(format!("{name} owner is null")));
    }
    // SAFETY: every caller supplies an ABI-probed, i32-aligned mutable field.
    unsafe {
        object.cast::<u8>().add(offset).cast::<i32>().write(value);
    }
    Ok(())
}

fn read_global_i32(address: usize, name: &str) -> Result<i32, BridgeInstallError> {
    if address == 0 {
        return Err(BridgeInstallError::new(format!("{name} address is null")));
    }
    // SAFETY: the exact target pack binds this address to live i32 storage.
    Ok(unsafe { (address as *const i32).read() })
}

fn write_global_i32(address: usize, value: i32, name: &str) -> Result<(), BridgeInstallError> {
    if address == 0 {
        return Err(BridgeInstallError::new(format!("{name} address is null")));
    }
    // SAFETY: the exact target pack binds this address to writable i32 storage.
    unsafe {
        (address as *mut i32).write(value);
    }
    Ok(())
}

#[cfg(windows)]
fn request_current_thread_shutdown() -> Result<(), BridgeInstallError> {
    // SAFETY: administration commands execute synchronously on the engine
    // thread. PostQuitMessage posts WM_QUIT to that current thread's queue and
    // does not retain any Rust-owned data.
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::PostQuitMessage(0);
    }
    Ok(())
}

#[cfg(not(windows))]
fn request_current_thread_shutdown() -> Result<(), BridgeInstallError> {
    Err(BridgeInstallError::new(
        "current-thread message-queue shutdown is available only on Windows",
    ))
}
