use std::ffi::{c_char, c_void};

pub(crate) const OBJECT_INVALID: u32 = 0x7f00_0000;

pub(crate) type FunctionManagement = extern "C" fn(*mut c_void, i32, i32) -> i32;
pub(crate) type StackPopInteger = extern "C" fn(*mut c_void, *mut i32) -> i32;
pub(crate) type StackPushInteger = extern "C" fn(*mut c_void, i32) -> i32;
pub(crate) type StackPopFloat = extern "C" fn(*mut c_void, *mut f32) -> i32;
pub(crate) type StackPushFloat = extern "C" fn(*mut c_void, f32) -> i32;
pub(crate) type StackPopObject = extern "C" fn(*mut c_void, *mut u32) -> i32;
pub(crate) type StackPushObject = extern "C" fn(*mut c_void, u32) -> i32;
pub(crate) type StackPopString = extern "C" fn(*mut c_void, *mut CExoString) -> i32;
pub(crate) type StackPushString = extern "C" fn(*mut c_void, *const CExoString) -> i32;
pub(crate) type StackPopVector = extern "C" fn(*mut c_void, *mut EngineVector) -> i32;
pub(crate) type StackPushVector = extern "C" fn(*mut c_void, EngineVector) -> i32;
pub(crate) type FreeExoStringBuffer = extern "C" fn(*mut c_void);
pub(crate) type GetServerInfo = extern "C" fn(*mut c_void) -> *const c_void;
pub(crate) type GetPlayerList = extern "C" fn(*mut c_void) -> *const c_void;
pub(crate) type GetNetLayer = extern "C" fn(*mut c_void) -> *mut c_void;
pub(crate) type GetSessionMaxPlayers = extern "C" fn(*mut c_void) -> u32;
pub(crate) type GetUdpPort = extern "C" fn(*mut c_void) -> u32;
pub(crate) type GetModule = extern "C" fn(*mut c_void) -> *mut c_void;
pub(crate) type RemoveLinkedListNode = extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void;
pub(crate) type MainLoop = extern "C" fn(*mut c_void) -> i32;
pub(crate) type LoadModuleFinish = extern "C" fn(*mut c_void) -> u32;
pub(crate) type RunScript = extern "C" fn(*mut c_void, *mut CExoString, u32, i32, i32) -> i32;
pub(crate) type GetClientObjectByObjectId = extern "C" fn(*mut c_void, u32) -> *mut c_void;
pub(crate) type GetCreatureByGameObjectId = extern "C" fn(*mut c_void, u32) -> *mut c_void;
pub(crate) type GetPlayerInfo = extern "C" fn(*mut c_void, u32) -> *mut c_void;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ObjectId(u32);

impl ObjectId {
    pub(crate) const fn from_raw(value: u32) -> Self {
        Self(value)
    }

    pub(crate) const fn raw(self) -> u32 {
        self.0
    }

    pub(crate) const fn invalid() -> Self {
        Self(OBJECT_INVALID)
    }
}

#[repr(C)]
pub(crate) struct CExoString {
    pub(crate) string:        *mut c_char,
    pub(crate) string_length: u32,
    pub(crate) buffer_length: u32,
}

#[derive(Clone, Copy)]
#[repr(C)]
pub(crate) struct EngineVector {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) z: f32,
}

const _: () = {
    assert!(std::mem::size_of::<CExoString>() == 16);
    assert!(std::mem::align_of::<CExoString>() == 8);
    assert!(std::mem::size_of::<EngineVector>() == 12);
    assert!(std::mem::align_of::<EngineVector>() == 4);
};
