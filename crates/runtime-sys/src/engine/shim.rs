use std::ffi::c_void;

use crate::bridge::BridgeInstallError;

const ENGINE_SHIM_ABI_VERSION: u32 = 1;
const ENGINE_SHIM_STRING3_I32_V1: u32 = 1;

#[derive(Clone, Copy)]
#[repr(C)]
#[allow(dead_code)]
pub(crate) struct NativeByteSpan {
    pub(crate) data:   *const u8,
    pub(crate) length: usize,
}

#[derive(Clone, Copy)]
#[repr(C)]
#[allow(dead_code)]
pub(crate) struct NativeMutableByteSpan {
    pub(crate) data:   *mut u8,
    pub(crate) length: usize,
}

#[derive(Clone, Copy)]
#[repr(C)]
#[allow(dead_code)]
pub(crate) struct NativeVector {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) z: f32,
}

unsafe extern "C" {
    fn nwnrs_engine_shim_abi_version() -> u32;
    fn nwnrs_engine_shim_supports_signature(signature: u32) -> i32;
    fn nwnrs_engine_shim_self_test_string3_i32() -> i32;

    pub(crate) fn nwnrs_engine_get_string(
        address: *mut c_void,
        free_address: *mut c_void,
        object: *mut c_void,
        output: *mut u8,
        capacity: usize,
    ) -> usize;
    pub(crate) fn nwnrs_engine_set_string_bool(
        address: *mut c_void,
        object: *mut c_void,
        value: *const u8,
        length: usize,
    ) -> i32;
    pub(crate) fn nwnrs_engine_set_string_void(
        address: *mut c_void,
        object: *mut c_void,
        value: *const u8,
        length: usize,
    );
    pub(crate) fn nwnrs_engine_replace_string(
        destination: *mut c_void,
        value: *const u8,
        length: usize,
    );
    pub(crate) fn nwnrs_engine_get_loc_string(
        address: *mut c_void,
        free_address: *mut c_void,
        object: *const c_void,
        output: *mut u8,
        capacity: usize,
    ) -> usize;
    pub(crate) fn nwnrs_engine_get_alias_path(
        address: *mut c_void,
        object: *const c_void,
        alias: *const u8,
        alias_length: usize,
        output: *mut u8,
        capacity: usize,
    ) -> usize;
    pub(crate) fn nwnrs_engine_disconnect_player(
        address: *mut c_void,
        object: *mut c_void,
        player_id: u32,
        string_reference: u32,
        cd_auth_failure: i32,
        reason: *const u8,
        reason_length: usize,
    ) -> i32;

    #[allow(dead_code)]
    pub(crate) fn nwnrs_engine_call_string3_i32(
        address: *mut c_void,
        object: *mut c_void,
        first: NativeByteSpan,
        second: NativeByteSpan,
        third: NativeByteSpan,
        tail: i32,
    ) -> i32;
}

pub(crate) fn verify_native_shim() -> Result<(), BridgeInstallError> {
    // SAFETY: both functions are compiled into this crate from
    // native/abi_shim.cpp and have scalar C signatures.
    let version = unsafe { nwnrs_engine_shim_abi_version() };
    if version != ENGINE_SHIM_ABI_VERSION {
        return Err(BridgeInstallError::new(format!(
            "native engine shim ABI {version} does not match Rust ABI {ENGINE_SHIM_ABI_VERSION}"
        )));
    }
    // SAFETY: the signature identifier and return value are fixed-width
    // scalars in the versioned C boundary.
    if unsafe { nwnrs_engine_shim_supports_signature(ENGINE_SHIM_STRING3_I32_V1) } == 0 {
        return Err(BridgeInstallError::new(
            "native engine shim lacks String3I32.v1 support",
        ));
    }
    // SAFETY: the self-test owns every C++ value and returns one scalar.
    let self_test = unsafe { nwnrs_engine_shim_self_test_string3_i32() };
    if self_test != 330 {
        return Err(BridgeInstallError::new(format!(
            "native String3I32.v1 shim self-test returned {self_test}, expected 330"
        )));
    }
    Ok(())
}

const _: () = {
    assert!(std::mem::size_of::<NativeByteSpan>() == 2 * std::mem::size_of::<usize>());
    assert!(std::mem::size_of::<NativeMutableByteSpan>() == 2 * std::mem::size_of::<usize>());
    assert!(std::mem::size_of::<NativeVector>() == 12);
    assert!(std::mem::align_of::<NativeVector>() == 4);
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiled_shim_and_rust_boundary_have_the_same_version() {
        verify_native_shim().expect("compiled shim ABI must match Rust");
    }
}
