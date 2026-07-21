use std::ffi::c_void;

use super::super::{EngineThreadToken, active_engine};

pub(super) const OBJECT_INVALID: u32 = 0x7f00_0000;
pub(super) const OBJECT_TYPE_CREATURE: u8 = 5;
pub(super) const OBJECT_TYPE_ITEM: u8 = 6;
pub(super) const OBJECT_TYPE_PLACEABLE: u8 = 9;

type GetGameObject = extern "C" fn(*mut c_void, u32) -> *mut c_void;
type GetClientObject = extern "C" fn(*mut c_void, u32) -> *mut c_void;
type GetNwsMessage = extern "C" fn(*mut c_void) -> *mut c_void;

pub(super) fn read_field<T: Copy>(object: *const c_void, layout: &str) -> Option<T> {
    if object.is_null() {
        return None;
    }
    let offset = active_engine()?.event_layout_offset(layout)?;
    // SAFETY: each caller names a compiler-derived field layout whose type is
    // checked by the Unified ABI probe, and calls synchronously in its hook.
    Some(unsafe { object.cast::<u8>().add(offset).cast::<T>().read_unaligned() })
}

pub(super) fn write_field<T: Copy>(object: *mut c_void, layout: &str, value: T) -> bool {
    if object.is_null() {
        return false;
    }
    let Some(offset) = active_engine().and_then(|engine| engine.event_layout_offset(layout)) else {
        return false;
    };
    // SAFETY: the target pack and ABI probe bind this name to the writable
    // field used by Unified for the same synchronous engine operation.
    unsafe {
        object
            .cast::<u8>()
            .add(offset)
            .cast::<T>()
            .write_unaligned(value);
    }
    true
}

pub(super) fn stats_creature(stats: *mut c_void) -> Option<*mut c_void> {
    read_field(stats, "creature_stats_base_creature")
}

pub(super) fn repository_parent(repository: *mut c_void) -> Option<u32> {
    read_field(repository, "item_repository_parent")
}

pub(super) fn object_type(object: *mut c_void) -> Option<u8> {
    read_field(object, "game_object_type")
}

pub(super) fn get_game_object(object_id: u32) -> Option<*mut c_void> {
    call_server_function(
        "server_get_game_object",
        object_id,
        |target, server, object_id| {
            // SAFETY: the target pack binds this address to
            // CServerExoApp::GetGameObject(ObjectID).
            let function = unsafe { std::mem::transmute::<usize, GetGameObject>(target) };
            function(server, object_id)
        },
    )
}

pub(super) fn get_client_object(object_id: u32) -> Option<*mut c_void> {
    call_server_function(
        "server_get_client_object",
        object_id,
        |target, server, object_id| {
            // SAFETY: the target pack binds this address to
            // CServerExoApp::GetClientObjectByObjectId(ObjectID).
            let function = unsafe { std::mem::transmute::<usize, GetClientObject>(target) };
            function(server, object_id)
        },
    )
}

pub(super) fn get_nws_message() -> Option<*mut c_void> {
    let engine = active_engine()?;
    let target = engine.event_function_target("server_get_nws_message")?;
    // SAFETY: this token is scoped to the synchronous native engine hook.
    let thread = unsafe { EngineThreadToken::new() };
    let server = engine.event_server(&thread).ok()?;
    // SAFETY: the target pack binds this address to CServerExoApp::GetNWSMessage().
    let function = unsafe { std::mem::transmute::<usize, GetNwsMessage>(target) };
    let message = function(server);
    (!message.is_null()).then_some(message)
}

fn call_server_function(
    name: &str,
    object_id: u32,
    call: impl FnOnce(usize, *mut c_void, u32) -> *mut c_void,
) -> Option<*mut c_void> {
    let engine = active_engine()?;
    let target = engine.event_function_target(name)?;
    // SAFETY: this token is scoped to the synchronous native engine hook.
    let thread = unsafe { EngineThreadToken::new() };
    let server = engine.event_server(&thread).ok()?;
    let object = call(target, server, object_id);
    (!object.is_null()).then_some(object)
}

pub(super) fn parse_boolean_result(result: Option<&[u8]>) -> Option<bool> {
    result.and_then(|result| serde_json::from_slice(result).ok())
}

pub(super) fn parse_unsigned_result(result: Option<&[u8]>) -> Option<u32> {
    result.and_then(|result| serde_json::from_slice(result).ok())
}

pub(super) fn parse_object_result(result: Option<&[u8]>) -> Option<u32> {
    let value: String = serde_json::from_slice(result?).ok()?;
    u32::from_str_radix(&value, 16).ok()
}

pub(super) fn peek_message<T: Copy>(message: *const c_void, delta: usize) -> Option<T> {
    let buffer: *const u8 = read_field(message, "message_read_buffer")?;
    let size = usize::try_from(read_field::<u32>(message, "message_read_buffer_size")?).ok()?;
    let position =
        usize::try_from(read_field::<u32>(message, "message_read_buffer_position")?).ok()?;
    let end = position
        .checked_add(delta)?
        .checked_add(std::mem::size_of::<T>())?;
    if buffer.is_null() || end > size {
        return None;
    }
    // SAFETY: the bounds check above proves the copied POD value lies within
    // the active engine message buffer.
    Some(unsafe { buffer.add(position + delta).cast::<T>().read_unaligned() })
}

pub(super) fn clear_read_message(message: *mut c_void) -> bool {
    let Some(read_size) = read_field::<u32>(message, "message_read_buffer_size") else {
        return false;
    };
    let Some(fragment_size) = read_field::<u32>(message, "message_read_fragments_size") else {
        return false;
    };
    let Some(last_bits) = read_field::<u8>(message, "message_last_byte_bits") else {
        return false;
    };
    write_field(message, "message_read_buffer_position", read_size)
        && write_field(message, "message_read_fragments_position", fragment_size)
        && write_field(message, "message_current_read_bit", last_bits)
}
