//! Writes the exact-hash target pack used by the native runtime fixture.

use std::{error::Error, ffi::OsString, fs, path::PathBuf};

use nwnrs_runtime::{
    ADMINISTRATION_CAPABILITY_VERSION, AbiLayouts, AdministrationTarget, BinaryIdentity,
    BridgeTarget, CExoStringLayout, EVENT_CONTEXT_CAPABILITY_VERSION, EngineClassLayouts,
    EventTarget, NWSCRIPT_BRIDGE_CAPABILITY_VERSION, OperatingSystem, PlayerListLayout,
    RUNTIME_API_VERSION, SERVER_STATE_CAPABILITY_VERSION, ServerStateTarget, ShutdownTarget,
    TARGET_PACK_SCHEMA_VERSION, TargetAddress, TargetPack, TargetServer, TargetSource,
    VectorLayout,
};

fn main() -> Result<(), Box<dyn Error>> {
    let mut arguments = std::env::args_os().skip(1);
    let binary = required_argument(&mut arguments, "fixture binary")?;
    let targets = required_argument(&mut arguments, "target-pack root")?;
    if arguments.next().is_some() {
        return Err("usage: write-fixture-target-pack FIXTURE_BINARY TARGET_ROOT".into());
    }

    let identity = BinaryIdentity::read(binary)?;
    let script_size = match identity.platform.os {
        OperatingSystem::Macos => 136,
        OperatingSystem::Linux => 152,
        OperatingSystem::Windows => 160,
    };
    let pack = TargetPack {
        schema_version: TARGET_PACK_SCHEMA_VERSION,
        runtime_api:    RUNTIME_API_VERSION,
        server:         TargetServer {
            sha256:   identity.sha256.to_string(),
            platform: identity.platform,
            build:    Some("fixture".to_string()),
        },
        source:         TargetSource {
            unified_commit: "3d4c4e13c6bf01b032ffe90534fc4a19eb036c03".to_string(),
            nwn_build:      8193,
            nwn_revision:   37,
            nwn_postfix:    17,
        },
        layouts:        abi_layouts(script_size, 0),
        bridge:         BridgeTarget {
            version:                NWSCRIPT_BRIDGE_CAPABILITY_VERSION,
            function_management:    symbol("nwnrs_fixture_function_management"),
            stack_pop_integer:      symbol("nwnrs_fixture_stack_pop_integer"),
            stack_push_integer:     symbol("nwnrs_fixture_stack_push_integer"),
            stack_pop_float:        symbol("nwnrs_fixture_stack_pop_float"),
            stack_push_float:       symbol("nwnrs_fixture_stack_push_float"),
            stack_pop_object:       symbol("nwnrs_fixture_stack_pop_object"),
            stack_push_object:      symbol("nwnrs_fixture_stack_push_object"),
            stack_pop_string:       symbol("nwnrs_fixture_stack_pop_string"),
            stack_push_string:      symbol("nwnrs_fixture_stack_push_string"),
            stack_pop_vector:       symbol("nwnrs_fixture_stack_pop_vector"),
            stack_push_vector:      symbol("nwnrs_fixture_stack_push_vector"),
            free_exo_string_buffer: symbol("nwnrs_fixture_free_exo_string_buffer"),
        },
        server_state:   Some(ServerStateTarget {
            version:                 SERVER_STATE_CAPABILITY_VERSION,
            app_manager:             symbol("nwnrs_fixture_app_manager"),
            get_server_info:         symbol("nwnrs_fixture_get_server_info"),
            get_player_list:         symbol("nwnrs_fixture_get_player_list"),
            get_net_layer:           symbol("nwnrs_fixture_get_net_layer"),
            get_session_max_players: symbol("nwnrs_fixture_get_session_max_players"),
            get_udp_port:            symbol("nwnrs_fixture_get_udp_port"),
        }),
        administration: Some(AdministrationTarget {
            version: ADMINISTRATION_CAPABILITY_VERSION,
            get_session_name: symbol("nwnrs_fixture_get_session_name"),
            set_session_name: symbol("nwnrs_fixture_set_session_name"),
            get_player_password: symbol("nwnrs_fixture_get_player_password"),
            set_player_password: symbol("nwnrs_fixture_set_player_password"),
            get_game_master_password: symbol("nwnrs_fixture_get_game_master_password"),
            set_game_master_password: symbol("nwnrs_fixture_set_game_master_password"),
            enable_combat_debugging: symbol("nwnrs_fixture_enable_combat_debugging"),
            enable_saving_throw_debugging: symbol("nwnrs_fixture_enable_saving_throw_debugging"),
            enable_movement_speed_debugging: symbol(
                "nwnrs_fixture_enable_movement_speed_debugging",
            ),
            enable_hit_die_debugging: symbol("nwnrs_fixture_enable_hit_die_debugging"),
            shutdown: ShutdownTarget::ExitFlag {
                address: symbol("nwnrs_fixture_exit_program"),
            },
            add_banned_ip: symbol("nwnrs_fixture_add_banned_ip"),
            remove_banned_ip: symbol("nwnrs_fixture_remove_banned_ip"),
            add_banned_cd_key: symbol("nwnrs_fixture_add_banned_cd_key"),
            remove_banned_cd_key: symbol("nwnrs_fixture_remove_banned_cd_key"),
            add_banned_player_name: symbol("nwnrs_fixture_add_banned_player_name"),
            remove_banned_player_name: symbol("nwnrs_fixture_remove_banned_player_name"),
            rules: symbol("nwnrs_fixture_rules"),
            reload_rules: symbol("nwnrs_fixture_reload_rules"),
            get_module: symbol("nwnrs_fixture_get_module"),
            get_loc_string: symbol("nwnrs_fixture_get_loc_string"),
            remove_linked_list_node: symbol("nwnrs_fixture_remove_linked_list_node"),
            main_loop: symbol("nwnrs_fixture_main_loop"),
            get_client_object_by_object_id: symbol("nwnrs_fixture_get_client_object_by_object_id"),
            get_creature_by_game_object_id: symbol("nwnrs_fixture_get_creature_by_game_object_id"),
            get_player_name: symbol("nwnrs_fixture_get_player_name"),
            get_player_info: symbol("nwnrs_fixture_get_player_info"),
            disconnect_player: symbol("nwnrs_fixture_disconnect_player"),
            exo_base: symbol("nwnrs_fixture_exo_base"),
            get_alias_path: symbol("nwnrs_fixture_get_alias_path"),
        }),
        events:         Some(EventTarget {
            version:            EVENT_CONTEXT_CAPABILITY_VERSION,
            load_module_finish: symbol("nwnrs_fixture_load_module_finish"),
            virtual_machine:    symbol("nwnrs_fixture_virtual_machine"),
            run_script:         symbol("nwnrs_fixture_run_script"),
        }),
    };
    let directory = PathBuf::from(targets).join(identity.platform.directory_name());
    fs::create_dir_all(&directory)?;
    fs::write(
        directory.join(format!("{}.toml", identity.sha256)),
        toml::to_string_pretty(&pack)?,
    )?;
    Ok(())
}

fn abi_layouts(script_size: u64, command_implementer_vm_offset: u64) -> AbiLayouts {
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
            command_implementer_vm_offset,
            app_manager_server_offset: 8,
            server_info_module_offset: 8,
            server_info_joining_restrictions_offset: 136,
            server_info_play_options_offset: 252,
            server_info_persistent_world_options_offset: 404,
            persistent_world_options_server_vault_by_player_name_offset: 16,
            joining_restrictions_min_level_offset: 104,
            joining_restrictions_max_level_offset: 108,
            server_exo_app_internal_offset: 24,
            internal_banned_ip_list_offset: 0,
            internal_banned_cd_key_list_offset: 16,
            internal_banned_player_name_list_offset: 32,
            module_turd_list_offset: 0,
            player_turd_community_name_offset: 0,
            player_turd_first_name_offset: 16,
            player_turd_last_name_offset: 32,
            linked_list_head_offset: 0,
            linked_list_count_offset: 16,
            linked_list_node_next_offset: 8,
            linked_list_node_object_offset: 16,
            player_id_offset: 0,
            player_file_name_offset: 4,
            player_file_name_size: 17,
            net_layer_player_info_cd_key_offset: 0,
            player_cd_key_public_offset: 0,
            exo_base_alias_list_offset: 0,
            creature_stats_offset: 0,
            creature_stats_first_name_offset: 0,
            creature_stats_last_name_offset: 16,
            vm_recursion_level_offset: 36,
            vm_script_array_offset: 40,
            vm_script_slot_count: 8,
            vm_script_size: script_size,
            vm_script_alignment: 8,
            vm_script_name_offset: 24,
            vm_script_event_id_offset: 72,
        },
    }
}

fn required_argument(
    arguments: &mut impl Iterator<Item = OsString>,
    name: &str,
) -> Result<OsString, Box<dyn Error>> {
    arguments.next().ok_or_else(|| {
        format!("missing {name}; usage: write-fixture-target-pack FIXTURE_BINARY TARGET_ROOT")
            .into()
    })
}

fn symbol(name: &str) -> TargetAddress {
    TargetAddress::Symbol {
        symbol: name.to_string(),
    }
}
