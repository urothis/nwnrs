//! Writes the exact-hash target pack used by the native runtime fixture.

use std::{error::Error, ffi::OsString, fs, path::PathBuf};

use nwnrs_runtime::{
    BinaryIdentity, BridgeTarget, EventTarget, OperatingSystem, RUNTIME_API_VERSION,
    ServerStateTarget, TARGET_PACK_SCHEMA_VERSION, TargetAddress, TargetPack, TargetServer,
};

fn main() -> Result<(), Box<dyn Error>> {
    let mut arguments = std::env::args_os().skip(1);
    let binary = required_argument(&mut arguments, "fixture binary")?;
    let targets = required_argument(&mut arguments, "target-pack root")?;
    if arguments.next().is_some() {
        return Err("usage: write-fixture-target-pack FIXTURE_BINARY TARGET_ROOT".into());
    }

    let identity = BinaryIdentity::read(binary)?;
    let script_stride = match identity.platform.os {
        OperatingSystem::Macos => 136,
        OperatingSystem::Linux => 152,
    };
    let pack = TargetPack {
        schema_version: TARGET_PACK_SCHEMA_VERSION,
        runtime_api:    RUNTIME_API_VERSION,
        server:         TargetServer {
            sha256:   identity.sha256.to_string(),
            platform: identity.platform,
            build:    Some("fixture".to_string()),
        },
        bridge:         BridgeTarget {
            function_management:    symbol("nwnrs_fixture_function_management"),
            virtual_machine_offset: 0,
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
        server_state:   ServerStateTarget {
            app_manager:                    symbol("nwnrs_fixture_app_manager"),
            server_exo_app_offset:          8,
            get_server_info:                symbol("nwnrs_fixture_get_server_info"),
            server_info_module_name_offset: 8,
            get_player_list:                symbol("nwnrs_fixture_get_player_list"),
            player_list_count_offset:       8,
            get_net_layer:                  symbol("nwnrs_fixture_get_net_layer"),
            get_session_max_players:        symbol("nwnrs_fixture_get_session_max_players"),
        },
        events:         EventTarget {
            recursion_level_offset: 36,
            script_array_offset: 40,
            script_slot_count: 8,
            script_stride,
            script_name_offset: 24,
            script_event_id_offset: 72,
        },
    };
    let directory = PathBuf::from(targets).join(identity.platform.directory_name());
    fs::create_dir_all(&directory)?;
    fs::write(
        directory.join(format!("{}.toml", identity.sha256)),
        toml::to_string_pretty(&pack)?,
    )?;
    Ok(())
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
