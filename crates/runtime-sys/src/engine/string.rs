use std::{ffi::c_void, ptr, slice};

use super::abi::{CExoString, FreeExoStringBuffer};
use crate::bridge::BridgeInstallError;

const MAX_ENGINE_STRING_BYTES: usize = 16 * 1024 * 1024;

pub(crate) struct OwnedEngineString {
    raw:  CExoString,
    free: FreeExoStringBuffer,
}

impl OwnedEngineString {
    pub(crate) fn empty(free: FreeExoStringBuffer) -> Self {
        Self {
            raw: CExoString {
                string:        ptr::null_mut(),
                string_length: 0,
                buffer_length: 0,
            },
            free,
        }
    }

    pub(crate) fn as_mut_ptr(&mut self) -> *mut CExoString {
        &raw mut self.raw
    }

    pub(crate) fn copy(&self) -> Result<Vec<u8>, BridgeInstallError> {
        copy_exo_string(&self.raw)
    }
}

impl Drop for OwnedEngineString {
    fn drop(&mut self) {
        if !self.raw.string.is_null() {
            (self.free)(self.raw.string.cast::<c_void>());
            self.raw.string = ptr::null_mut();
        }
    }
}

pub(crate) fn copy_exo_string(value: &CExoString) -> Result<Vec<u8>, BridgeInstallError> {
    let length = usize::try_from(value.string_length)
        .map_err(|_error| BridgeInstallError::new("CExoString length exceeds usize"))?;
    if length == 0 {
        return Ok(Vec::new());
    }
    if value.string.is_null() {
        return Err(BridgeInstallError::new(
            "engine returned a null CExoString with a nonzero length",
        ));
    }
    if value.buffer_length < value.string_length {
        return Err(BridgeInstallError::new(
            "engine returned a CExoString length larger than its buffer",
        ));
    }
    if length > MAX_ENGINE_STRING_BYTES {
        return Err(BridgeInstallError::new(format!(
            "engine returned a CExoString larger than {MAX_ENGINE_STRING_BYTES} bytes"
        )));
    }
    // SAFETY: validated Unified layout and engine invariants guarantee at least
    // string_length readable bytes for the duration of this VM callback.
    let bytes = unsafe { slice::from_raw_parts(value.string.cast::<u8>(), length) };
    Ok(bytes.to_vec())
}
