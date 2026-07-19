use std::{ffi::c_void, mem};

use nwnrs_runtime::{EngineClassLayouts, ServerStateTarget};

use super::{
    abi::{
        CExoString, GetNetLayer, GetPlayerList, GetServerInfo, GetSessionMaxPlayers, GetUdpPort,
    },
    address::{GlobalStorage, NativeAddress, Resolver},
    string::copy_exo_string,
    thread::EngineThreadToken,
};
use crate::bridge::BridgeInstallError;

pub(crate) struct ServerEngine {
    app_manager:               NativeAddress<GlobalStorage>,
    app_manager_server_offset: usize,
    get_server_info:           GetServerInfo,
    server_info_module_offset: usize,
    get_player_list:           GetPlayerList,
    player_list_count_offset:  usize,
    get_net_layer:             GetNetLayer,
    get_session_max_players:   GetSessionMaxPlayers,
    get_udp_port:              GetUdpPort,
}

impl ServerEngine {
    pub(crate) fn resolve(
        resolver: &Resolver,
        target: &ServerStateTarget,
        layouts: &EngineClassLayouts,
        player_list_count_offset: u64,
    ) -> Result<Self, BridgeInstallError> {
        macro_rules! resolve_function {
            ($name:literal, $field:ident, $ty:ty) => {{
                let address = resolver.resolve::<$ty>("server_state", $name, &target.$field)?;
                // SAFETY: exact-hash target data binds this address to the
                // Unified declaration named by the field.
                unsafe { mem::transmute::<usize, $ty>(address.get()) }
            }};
        }
        Ok(Self {
            app_manager:               resolver.resolve::<GlobalStorage>(
                "server_state",
                "app_manager",
                &target.app_manager,
            )?,
            app_manager_server_offset: checked_offset(
                "layouts.classes.app_manager_server_offset",
                layouts.app_manager_server_offset,
            )?,
            get_server_info:           resolve_function!(
                "get_server_info",
                get_server_info,
                GetServerInfo
            ),
            server_info_module_offset: checked_offset(
                "layouts.classes.server_info_module_offset",
                layouts.server_info_module_offset,
            )?,
            get_player_list:           resolve_function!(
                "get_player_list",
                get_player_list,
                GetPlayerList
            ),
            player_list_count_offset:  checked_offset(
                "layouts.player_list.count_offset",
                player_list_count_offset,
            )?,
            get_net_layer:             resolve_function!(
                "get_net_layer",
                get_net_layer,
                GetNetLayer
            ),
            get_session_max_players:   resolve_function!(
                "get_session_max_players",
                get_session_max_players,
                GetSessionMaxPlayers
            ),
            get_udp_port:              resolve_function!("get_udp_port", get_udp_port, GetUdpPort),
        })
    }

    pub(crate) fn module_name(
        &self,
        _thread: &EngineThreadToken,
    ) -> Result<Vec<u8>, BridgeInstallError> {
        let server = self.server_exo_app()?;
        let server_info = (self.get_server_info)(server);
        if server_info.is_null() {
            return Err(BridgeInstallError::new(
                "CServerExoApp::GetServerInfo returned null",
            ));
        }
        // SAFETY: the compiler-verified layout identifies a live CExoString
        // field owned by the server for this synchronous callback.
        let name = unsafe {
            &*server_info
                .cast::<u8>()
                .add(self.server_info_module_offset)
                .cast::<CExoString>()
        };
        copy_exo_string(name)
    }

    pub(crate) fn player_count(
        &self,
        _thread: &EngineThreadToken,
    ) -> Result<i32, BridgeInstallError> {
        let list = (self.get_player_list)(self.server_exo_app()?);
        if list.is_null() {
            return Err(BridgeInstallError::new(
                "CServerExoApp::GetPlayerList returned null",
            ));
        }
        // SAFETY: the ABI snapshot bounds and aligns the i32 count field.
        let count = unsafe {
            list.cast::<u8>()
                .add(self.player_list_count_offset)
                .cast::<i32>()
                .read()
        };
        if count.is_negative() {
            return Err(BridgeInstallError::new(
                "CServerExoApp player list contains a negative count",
            ));
        }
        Ok(count)
    }

    pub(crate) fn max_players(
        &self,
        _thread: &EngineThreadToken,
    ) -> Result<i32, BridgeInstallError> {
        let network = (self.get_net_layer)(self.server_exo_app()?);
        if network.is_null() {
            return Err(BridgeInstallError::new(
                "CServerExoApp::GetNetLayer returned null",
            ));
        }
        i32::try_from((self.get_session_max_players)(network)).map_err(|_error| {
            BridgeInstallError::new("server maximum player count exceeds NWScript integer range")
        })
    }

    pub(crate) fn udp_port(&self, _thread: &EngineThreadToken) -> Result<i32, BridgeInstallError> {
        let network = (self.get_net_layer)(self.server_exo_app()?);
        if network.is_null() {
            return Err(BridgeInstallError::new(
                "CServerExoApp::GetNetLayer returned null",
            ));
        }
        i32::try_from((self.get_udp_port)(network)).map_err(|_error| {
            BridgeInstallError::new("server UDP port exceeds NWScript integer range")
        })
    }

    pub(crate) fn server_exo_app(&self) -> Result<*mut c_void, BridgeInstallError> {
        // SAFETY: the address identifies global CAppManager* storage in the
        // exact executable selected before hooks were installed.
        let manager = unsafe { (self.app_manager.get() as *const *mut c_void).read() };
        read_pointer_field(
            manager,
            self.app_manager_server_offset,
            "CAppManager::m_pServerExoApp",
        )
    }

    pub(crate) fn server_info(&self) -> Result<*mut c_void, BridgeInstallError> {
        let server_info = (self.get_server_info)(self.server_exo_app()?);
        if server_info.is_null() {
            return Err(BridgeInstallError::new(
                "CServerExoApp::GetServerInfo returned null",
            ));
        }
        Ok(server_info.cast_mut())
    }

    pub(crate) fn net_layer(&self) -> Result<*mut c_void, BridgeInstallError> {
        let network = (self.get_net_layer)(self.server_exo_app()?);
        if network.is_null() {
            return Err(BridgeInstallError::new(
                "CServerExoApp::GetNetLayer returned null",
            ));
        }
        Ok(network)
    }
}

fn read_pointer_field(
    object: *mut c_void,
    offset: usize,
    name: &str,
) -> Result<*mut c_void, BridgeInstallError> {
    if object.is_null() {
        return Err(BridgeInstallError::new(format!("{name} owner is null")));
    }
    // SAFETY: layout validation ensures pointer alignment and the live engine
    // object owns the field for the callback duration.
    let value = unsafe { object.cast::<u8>().add(offset).cast::<*mut c_void>().read() };
    if value.is_null() {
        return Err(BridgeInstallError::new(format!("{name} is null")));
    }
    Ok(value)
}

fn checked_offset(name: &str, value: u64) -> Result<usize, BridgeInstallError> {
    usize::try_from(value).map_err(|_error| {
        BridgeInstallError::new(format!("target-pack offset {name} exceeds usize"))
    })
}
