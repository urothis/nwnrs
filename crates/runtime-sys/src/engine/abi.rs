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
pub(crate) type AddAssociate = extern "C" fn(*mut c_void, u32, u16);
pub(crate) type RemoveAssociate = extern "C" fn(*mut c_void, u32);
pub(crate) type FamiliarAction = extern "C" fn(*mut c_void);
pub(crate) type GetAssociateId = extern "C" fn(*mut c_void, u16, i32) -> u32;
pub(crate) type ObjectAction = extern "C" fn(*mut c_void, u32) -> i32;
pub(crate) type UnlockObjectAction = extern "C" fn(*mut c_void, u32, u32, i32) -> i32;
pub(crate) type OpenInventory = extern "C" fn(*mut c_void, u32);
pub(crate) type CloseInventory = extern "C" fn(*mut c_void, u32, i32);
pub(crate) type ModifyGold = extern "C" fn(*mut c_void, i32, i32);
pub(crate) type RepositoryAddItem =
    extern "C" fn(*mut c_void, *mut *mut c_void, u8, u8, i32, i32) -> i32;
pub(crate) type RepositoryRemoveItem = extern "C" fn(*mut c_void, *mut c_void) -> i32;
pub(crate) type InventoryStatus = extern "C" fn(*mut c_void, *mut c_void, i32, u32) -> i32;
pub(crate) type InventoryGuiSetOpen = extern "C" fn(*mut c_void, i32, i32);
pub(crate) type InventorySelectPanel = extern "C" fn(*mut c_void, u32, u8) -> i32;
pub(crate) type SetExperience = extern "C" fn(*mut c_void, u32, i32);
pub(crate) type UseFeat =
    extern "C" fn(*mut c_void, u16, u16, u32, u32, *const EngineVector) -> i32;
pub(crate) type DecrementFeatRemainingUses = extern "C" fn(*mut c_void, u16);
pub(crate) type HasFeat = extern "C" fn(*mut c_void, u16) -> i32;
pub(crate) type GetFeatRemainingUses = extern "C" fn(*mut c_void, u16) -> u8;
pub(crate) type UseSkill =
    extern "C" fn(*mut c_void, u8, u8, u32, EngineVector, u32, u32, i32) -> i32;
pub(crate) type UseItem =
    extern "C" fn(*mut c_void, u32, u8, u8, u32, EngineVector, u32, i32) -> i32;
pub(crate) type ValidateUseItem = extern "C" fn(*mut c_void, *mut c_void, i32) -> i32;
pub(crate) type FindItemWithBaseItemId = extern "C" fn(*mut c_void, u32, i32) -> u32;
pub(crate) type ValidateEquipItem =
    extern "C" fn(*mut c_void, *mut c_void, *mut u32, i32, i32, i32, *mut c_void) -> u8;
pub(crate) type RunEquip = extern "C" fn(*mut c_void, u32, u32, u32) -> i32;
pub(crate) type RunUnequip = extern "C" fn(*mut c_void, u32, u32, u8, u8, i32, u32) -> i32;
pub(crate) type SplitItem = extern "C" fn(*mut c_void, *mut c_void, i32);
pub(crate) type MergeItem = extern "C" fn(*mut c_void, *mut c_void, *mut c_void);
pub(crate) type AcquireItem =
    extern "C" fn(*mut c_void, *mut *mut c_void, u32, u32, u8, u8, i32, i32) -> i32;
pub(crate) type InventoryEquipCancel = extern "C" fn(*mut c_void, u32, u32, u32, i32) -> i32;
pub(crate) type InventoryUnequipCancel = extern "C" fn(*mut c_void, u32, u32, i32) -> i32;
pub(crate) type ItemObjectAction = extern "C" fn(*mut c_void, u32) -> i32;
pub(crate) type ItemInventoryOpen = extern "C" fn(*mut c_void, u32);
pub(crate) type ItemInventoryClose = extern "C" fn(*mut c_void, u32, i32);
pub(crate) type PayToIdentifyItem = extern "C" fn(*mut c_void, u32, u32);
pub(crate) type ItemEventHandler = extern "C" fn(*mut c_void, u32, u32, *mut c_void, u32, u32);
pub(crate) type BroadcastSafeProjectile =
    extern "C" fn(*mut c_void, u32, u32, EngineVector, EngineVector, u32, u8, u32, u8, u8);
pub(crate) type GetPlayerGameObject = extern "C" fn(*mut c_void) -> *mut c_void;
pub(crate) type PlayerMessage = extern "C" fn(*mut c_void, *mut c_void, u8) -> i32;
pub(crate) type SendTimingEvent = extern "C" fn(*mut c_void, *mut c_void, i32, u8, u32) -> i32;
pub(crate) type CancelTimingEvent = extern "C" fn(*mut c_void, *mut c_void) -> i32;
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
