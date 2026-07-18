use std::{
    cell::RefCell,
    error::Error,
    ffi::{CString, c_char, c_void},
    fmt,
    panic::{self, AssertUnwindSafe},
    ptr, slice,
    sync::{
        OnceLock,
        atomic::{AtomicPtr, Ordering},
    },
};

use nwnrs_runtime::{
    BridgeValue, EventContext, RuntimeContext, ScriptBridge, ScriptLog, ScriptLogLevel,
    ServerState, TargetAddress, Vector, event_name,
};

use super::{RUNTIME_CONTEXT, write_diagnostic};

const VM_SUCCESS: i32 = 0;
const VM_STACK_OVERFLOW: i32 = -638;
const VM_STACK_UNDERFLOW: i32 = -639;
const VM_FAKE_ABORT_SCRIPT: i32 = -645;

const NWNX_GET_IS_AVAILABLE: i32 = 1151;
const NWNX_CALL: i32 = 1152;
const NWNX_PUSH_INTEGER: i32 = 1153;
const NWNX_PUSH_FLOAT: i32 = 1154;
const NWNX_PUSH_OBJECT: i32 = 1155;
const NWNX_PUSH_STRING: i32 = 1156;
const NWNX_PUSH_VECTOR: i32 = 1157;
const NWNX_POP_INTEGER: i32 = 1167;
const NWNX_POP_FLOAT: i32 = 1168;
const NWNX_POP_OBJECT: i32 = 1169;
const NWNX_POP_STRING: i32 = 1170;
const NWNX_POP_VECTOR: i32 = 1171;

const MAX_ENGINE_STRING_BYTES: usize = 16 * 1024 * 1024;
const OBJECT_INVALID: u32 = 0x7f00_0000;

static ENGINE_BRIDGE: OnceLock<EngineBridge> = OnceLock::new();
static FUNCTION_MANAGEMENT_ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

thread_local! {
    static SCRIPT_BRIDGE: RefCell<ScriptBridge> = RefCell::new(ScriptBridge::default());
}

type FunctionManagement = extern "C" fn(*mut c_void, i32, i32) -> i32;
type StackPopInteger = extern "C" fn(*mut c_void, *mut i32) -> i32;
type StackPushInteger = extern "C" fn(*mut c_void, i32) -> i32;
type StackPopFloat = extern "C" fn(*mut c_void, *mut f32) -> i32;
type StackPushFloat = extern "C" fn(*mut c_void, f32) -> i32;
type StackPopObject = extern "C" fn(*mut c_void, *mut u32) -> i32;
type StackPushObject = extern "C" fn(*mut c_void, u32) -> i32;
type StackPopString = extern "C" fn(*mut c_void, *mut CExoString) -> i32;
type StackPushString = extern "C" fn(*mut c_void, *const CExoString) -> i32;
type StackPopVector = extern "C" fn(*mut c_void, *mut EngineVector) -> i32;
type StackPushVector = extern "C" fn(*mut c_void, EngineVector) -> i32;
type FreeExoStringBuffer = extern "C" fn(*mut c_void);
type GetServerInfo = extern "C" fn(*mut c_void) -> *const c_void;
type GetPlayerList = extern "C" fn(*mut c_void) -> *const c_void;
type GetNetLayer = extern "C" fn(*mut c_void) -> *mut c_void;
type GetSessionMaxPlayers = extern "C" fn(*mut c_void) -> u32;

#[repr(C)]
struct CExoString {
    string:        *mut c_char,
    string_length: u32,
    buffer_length: u32,
}

#[derive(Clone, Copy)]
#[repr(C)]
struct EngineVector {
    x: f32,
    y: f32,
    z: f32,
}

struct EngineBridge {
    virtual_machine_offset:         usize,
    function_management:            usize,
    stack_pop_integer:              usize,
    stack_push_integer:             usize,
    stack_pop_float:                usize,
    stack_push_float:               usize,
    stack_pop_object:               usize,
    stack_push_object:              usize,
    stack_pop_string:               usize,
    stack_push_string:              usize,
    stack_pop_vector:               usize,
    stack_push_vector:              usize,
    free_exo_string_buffer:         usize,
    app_manager:                    usize,
    server_exo_app_offset:          usize,
    get_server_info:                usize,
    server_info_module_name_offset: usize,
    get_player_list:                usize,
    player_list_count_offset:       usize,
    get_net_layer:                  usize,
    get_session_max_players:        usize,
    recursion_level_offset:         usize,
    script_array_offset:            usize,
    script_slot_count:              usize,
    script_stride:                  usize,
    script_name_offset:             usize,
    script_event_id_offset:         usize,
}

/// An error produced while resolving or installing the NWScript bridge.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BridgeInstallError {
    message: String,
}

impl BridgeInstallError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for BridgeInstallError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for BridgeInstallError {}

/// Resolves the exact target-pack ABI and replaces the engine's NWNX command
/// handler with the nwnrs bridge.
///
/// # Errors
///
/// Returns an error when an address cannot be resolved, Gum cannot install the
/// replacement, or the bridge was already initialized.
pub fn install_nwscript_bridge(context: &RuntimeContext) -> Result<(), BridgeInstallError> {
    // SAFETY: Gum was initialized by the injected-runtime initializer before
    // this function is called.
    let module = unsafe { frida_gum_sys::gum_process_get_main_module() };
    if module.is_null() {
        return Err(BridgeInstallError::new(
            "Frida Gum returned no main executable module",
        ));
    }

    let resolved = EngineBridge::resolve(module, context);
    // SAFETY: gum_process_get_main_module returns a retained GObject reference.
    unsafe {
        frida_gum_sys::g_object_unref(module.cast());
    }
    if ENGINE_BRIDGE.set(resolved?).is_err() {
        return Err(BridgeInstallError::new(
            "NWScript bridge was initialized more than once",
        ));
    }
    let engine = ENGINE_BRIDGE
        .get()
        .ok_or_else(|| BridgeInstallError::new("NWScript bridge state was not published"))?;

    // SAFETY: Gum was initialized and returns a retained interceptor or null.
    let interceptor = unsafe { frida_gum_sys::gum_interceptor_obtain() };
    if interceptor.is_null() {
        return Err(BridgeInstallError::new("Frida Gum returned no interceptor"));
    }
    let hooks = [HookSpec {
        name:        "NWScript bridge",
        target:      engine.function_management,
        replacement: function_management_replacement as FunctionManagement as *const () as usize,
        original:    &FUNCTION_MANAGEMENT_ORIGINAL,
    }];
    let installed = install_hooks(interceptor, &hooks);
    release_interceptor(interceptor);
    installed
}

struct HookSpec {
    name:        &'static str,
    target:      usize,
    replacement: usize,
    original:    &'static AtomicPtr<c_void>,
}

fn install_hooks(
    interceptor: *mut frida_gum_sys::GumInterceptor,
    hooks: &[HookSpec],
) -> Result<(), BridgeInstallError> {
    let mut installed = Vec::with_capacity(hooks.len());
    let mut failure = None;
    // SAFETY: every target was resolved from the exact-hash executable and
    // every replacement has the corresponding Unix C++ member-function ABI.
    unsafe {
        frida_gum_sys::gum_interceptor_begin_transaction(interceptor);
        for hook in hooks {
            let mut original = ptr::null_mut();
            let status = frida_gum_sys::gum_interceptor_replace(
                interceptor,
                hook.target as *mut c_void,
                hook.replacement as *mut c_void,
                ptr::null_mut(),
                &raw mut original,
            );
            if status == 0 {
                installed.push(hook);
            }
            if status != 0 || original.is_null() {
                failure = Some(format!(
                    "Frida Gum could not install {}: status {status}, trampoline {}",
                    hook.name,
                    if original.is_null() {
                        "missing"
                    } else {
                        "present"
                    }
                ));
                break;
            }
            hook.original.store(original, Ordering::Release);
        }
        frida_gum_sys::gum_interceptor_end_transaction(interceptor);
        let _flushed = frida_gum_sys::gum_interceptor_flush(interceptor);
    }

    let Some(failure) = failure else {
        return Ok(());
    };
    // SAFETY: installed contains only replacements successfully added above.
    unsafe {
        frida_gum_sys::gum_interceptor_begin_transaction(interceptor);
        for hook in &installed {
            frida_gum_sys::gum_interceptor_revert(interceptor, hook.target as *mut c_void);
            hook.original.store(ptr::null_mut(), Ordering::Release);
        }
        frida_gum_sys::gum_interceptor_end_transaction(interceptor);
        let _flushed = frida_gum_sys::gum_interceptor_flush(interceptor);
    }
    Err(BridgeInstallError::new(failure))
}

fn release_interceptor(interceptor: *mut frida_gum_sys::GumInterceptor) {
    // SAFETY: gum_interceptor_obtain returned one retained GObject reference.
    unsafe {
        frida_gum_sys::g_object_unref(interceptor.cast());
    }
}

impl EngineBridge {
    fn resolve(
        module: *mut frida_gum_sys::GumModule,
        context: &RuntimeContext,
    ) -> Result<Self, BridgeInstallError> {
        let target = &context.target.pack.bridge;
        let server = &context.target.pack.server_state;
        let events = &context.target.pack.events;
        let virtual_machine_offset =
            usize::try_from(target.virtual_machine_offset).map_err(|_error| {
                BridgeInstallError::new("virtual-machine pointer offset exceeds usize")
            })?;
        Ok(Self {
            virtual_machine_offset,
            function_management: resolve_address(
                module,
                "function_management",
                &target.function_management,
            )?,
            stack_pop_integer: resolve_address(
                module,
                "stack_pop_integer",
                &target.stack_pop_integer,
            )?,
            stack_push_integer: resolve_address(
                module,
                "stack_push_integer",
                &target.stack_push_integer,
            )?,
            stack_pop_float: resolve_address(module, "stack_pop_float", &target.stack_pop_float)?,
            stack_push_float: resolve_address(
                module,
                "stack_push_float",
                &target.stack_push_float,
            )?,
            stack_pop_object: resolve_address(
                module,
                "stack_pop_object",
                &target.stack_pop_object,
            )?,
            stack_push_object: resolve_address(
                module,
                "stack_push_object",
                &target.stack_push_object,
            )?,
            stack_pop_string: resolve_address(
                module,
                "stack_pop_string",
                &target.stack_pop_string,
            )?,
            stack_push_string: resolve_address(
                module,
                "stack_push_string",
                &target.stack_push_string,
            )?,
            stack_pop_vector: resolve_address(
                module,
                "stack_pop_vector",
                &target.stack_pop_vector,
            )?,
            stack_push_vector: resolve_address(
                module,
                "stack_push_vector",
                &target.stack_push_vector,
            )?,
            free_exo_string_buffer: resolve_address(
                module,
                "free_exo_string_buffer",
                &target.free_exo_string_buffer,
            )?,
            app_manager: resolve_address(module, "app_manager", &server.app_manager)?,
            server_exo_app_offset: checked_offset(
                "server_exo_app_offset",
                server.server_exo_app_offset,
            )?,
            get_server_info: resolve_address(module, "get_server_info", &server.get_server_info)?,
            server_info_module_name_offset: checked_offset(
                "server_info_module_name_offset",
                server.server_info_module_name_offset,
            )?,
            get_player_list: resolve_address(module, "get_player_list", &server.get_player_list)?,
            player_list_count_offset: checked_offset(
                "player_list_count_offset",
                server.player_list_count_offset,
            )?,
            get_net_layer: resolve_address(module, "get_net_layer", &server.get_net_layer)?,
            get_session_max_players: resolve_address(
                module,
                "get_session_max_players",
                &server.get_session_max_players,
            )?,
            recursion_level_offset: checked_offset(
                "recursion_level_offset",
                events.recursion_level_offset,
            )?,
            script_array_offset: checked_offset("script_array_offset", events.script_array_offset)?,
            script_slot_count: usize::try_from(events.script_slot_count).map_err(|_error| {
                BridgeInstallError::new("events.script_slot_count exceeds usize")
            })?,
            script_stride: checked_offset("script_stride", events.script_stride)?,
            script_name_offset: checked_offset("script_name_offset", events.script_name_offset)?,
            script_event_id_offset: checked_offset(
                "script_event_id_offset",
                events.script_event_id_offset,
            )?,
        })
    }

    fn virtual_machine(&self, commands: *mut c_void) -> Result<*mut c_void, BridgeInstallError> {
        if commands.is_null() {
            return Err(BridgeInstallError::new(
                "engine passed a null command implementer",
            ));
        }
        // SAFETY: the offset is exact-hash target data and was validated for
        // pointer alignment; commands is the live object supplied by the engine.
        let virtual_machine = unsafe {
            commands
                .cast::<u8>()
                .add(self.virtual_machine_offset)
                .cast::<*mut c_void>()
                .read()
        };
        if virtual_machine.is_null() {
            return Err(BridgeInstallError::new(
                "command implementer contains a null virtual-machine pointer",
            ));
        }
        Ok(virtual_machine)
    }

    fn pop_integer(&self, vm: *mut c_void) -> Result<i32, i32> {
        let mut value = 0;
        // SAFETY: the target pack binds this exact address to StackPopInteger.
        let function =
            unsafe { std::mem::transmute::<usize, StackPopInteger>(self.stack_pop_integer) };
        bool_result(function(vm, &raw mut value), VM_STACK_UNDERFLOW).map(|()| value)
    }

    fn push_integer(&self, vm: *mut c_void, value: i32) -> Result<(), i32> {
        // SAFETY: the target pack binds this exact address to StackPushInteger.
        let function =
            unsafe { std::mem::transmute::<usize, StackPushInteger>(self.stack_push_integer) };
        bool_result(function(vm, value), VM_STACK_OVERFLOW)
    }

    fn pop_float(&self, vm: *mut c_void) -> Result<f32, i32> {
        let mut value = 0.0;
        // SAFETY: the target pack binds this exact address to StackPopFloat.
        let function = unsafe { std::mem::transmute::<usize, StackPopFloat>(self.stack_pop_float) };
        bool_result(function(vm, &raw mut value), VM_STACK_UNDERFLOW).map(|()| value)
    }

    fn push_float(&self, vm: *mut c_void, value: f32) -> Result<(), i32> {
        // SAFETY: the target pack binds this exact address to StackPushFloat.
        let function =
            unsafe { std::mem::transmute::<usize, StackPushFloat>(self.stack_push_float) };
        bool_result(function(vm, value), VM_STACK_OVERFLOW)
    }

    fn pop_object(&self, vm: *mut c_void) -> Result<u32, i32> {
        let mut value = 0;
        // SAFETY: the target pack binds this exact address to StackPopObject.
        let function =
            unsafe { std::mem::transmute::<usize, StackPopObject>(self.stack_pop_object) };
        bool_result(function(vm, &raw mut value), VM_STACK_UNDERFLOW).map(|()| value)
    }

    fn push_object(&self, vm: *mut c_void, value: u32) -> Result<(), i32> {
        // SAFETY: the target pack binds this exact address to StackPushObject.
        let function =
            unsafe { std::mem::transmute::<usize, StackPushObject>(self.stack_push_object) };
        bool_result(function(vm, value), VM_STACK_OVERFLOW)
    }

    fn pop_string(&self, vm: *mut c_void) -> Result<Vec<u8>, i32> {
        let mut value = CExoString {
            string:        ptr::null_mut(),
            string_length: 0,
            buffer_length: 0,
        };
        // SAFETY: the target pack binds this exact address to StackPopString.
        let pop = unsafe { std::mem::transmute::<usize, StackPopString>(self.stack_pop_string) };
        if pop(vm, &raw mut value) == 0 {
            return Err(VM_STACK_UNDERFLOW);
        }
        let copied = copy_exo_string(&value);
        if !value.string.is_null() {
            // SAFETY: StackPopString initialized string as an owned new[]
            // allocation, and the target pack binds this address to its exact
            // array deallocator.
            let free = unsafe {
                std::mem::transmute::<usize, FreeExoStringBuffer>(self.free_exo_string_buffer)
            };
            free(value.string.cast());
        }
        copied.map_err(|error| {
            write_diagnostic(&error.to_string());
            VM_FAKE_ABORT_SCRIPT
        })
    }

    fn push_string(&self, vm: *mut c_void, value: &[u8]) -> Result<(), i32> {
        let string_length = u32::try_from(value.len()).map_err(|_error| VM_FAKE_ABORT_SCRIPT)?;
        let buffer_length = string_length.checked_add(1).ok_or(VM_FAKE_ABORT_SCRIPT)?;
        let mut bytes = Vec::with_capacity(value.len().saturating_add(1));
        bytes.extend_from_slice(value);
        bytes.push(0);
        let engine_string = CExoString {
            string: bytes.as_mut_ptr().cast(),
            string_length,
            buffer_length,
        };
        // SAFETY: the target pack binds this exact address to StackPushString;
        // it copies from the borrowed CExoString before this function returns.
        let function =
            unsafe { std::mem::transmute::<usize, StackPushString>(self.stack_push_string) };
        bool_result(function(vm, &raw const engine_string), VM_STACK_OVERFLOW)
    }

    fn pop_vector(&self, vm: *mut c_void) -> Result<Vector, i32> {
        let mut value = EngineVector {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        // SAFETY: the target pack binds this exact address to StackPopVector.
        let function =
            unsafe { std::mem::transmute::<usize, StackPopVector>(self.stack_pop_vector) };
        bool_result(function(vm, &raw mut value), VM_STACK_UNDERFLOW).map(|()| Vector {
            x: value.x,
            y: value.y,
            z: value.z,
        })
    }

    fn push_vector(&self, vm: *mut c_void, value: Vector) -> Result<(), i32> {
        // SAFETY: the target pack binds this exact address to StackPushVector.
        let function =
            unsafe { std::mem::transmute::<usize, StackPushVector>(self.stack_push_vector) };
        bool_result(
            function(
                vm,
                EngineVector {
                    x: value.x,
                    y: value.y,
                    z: value.z,
                },
            ),
            VM_STACK_OVERFLOW,
        )
    }

    fn server_exo_app(&self) -> Result<*mut c_void, BridgeInstallError> {
        // SAFETY: app_manager is the exact address of global CAppManager*
        // storage selected by the server binary's hash-specific target pack.
        let app_manager = unsafe { (self.app_manager as *const *mut c_void).read() };
        read_pointer_field(
            app_manager,
            self.server_exo_app_offset,
            "CAppManager::m_pServerExoApp",
        )
    }

    fn module_name(&self) -> Result<Vec<u8>, BridgeInstallError> {
        let server = self.server_exo_app()?;
        // SAFETY: the address is bound by the exact target pack to a C++ member
        // function with one `this` pointer and a pointer return on supported
        // Unix ABIs.
        let server_info =
            unsafe { std::mem::transmute::<usize, GetServerInfo>(self.get_server_info)(server) };
        if server_info.is_null() {
            return Err(BridgeInstallError::new(
                "CServerExoApp::GetServerInfo returned null",
            ));
        }
        // SAFETY: the field offset and CExoString layout belong to the selected
        // exact-hash target pack; the engine owns the field while it is copied.
        unsafe {
            copy_exo_string(
                &*server_info
                    .cast::<u8>()
                    .add(self.server_info_module_name_offset)
                    .cast::<CExoString>(),
            )
        }
    }

    fn player_count(&self) -> Result<i32, BridgeInstallError> {
        let server = self.server_exo_app()?;
        // SAFETY: the address is bound by the exact target pack to a C++ member
        // function with one `this` pointer and a pointer return.
        let player_list =
            unsafe { std::mem::transmute::<usize, GetPlayerList>(self.get_player_list)(server) };
        if player_list.is_null() {
            return Err(BridgeInstallError::new(
                "CServerExoApp::GetPlayerList returned null",
            ));
        }
        // SAFETY: the selected target pack records the exact i32 num field.
        let player_count = unsafe {
            player_list
                .cast::<u8>()
                .add(self.player_list_count_offset)
                .cast::<i32>()
                .read()
        };
        if player_count.is_negative() {
            return Err(BridgeInstallError::new(
                "CServerExoApp player list contains a negative count",
            ));
        }
        Ok(player_count)
    }

    fn max_players(&self) -> Result<i32, BridgeInstallError> {
        let server = self.server_exo_app()?;
        // SAFETY: the address is bound by the exact target pack to a C++ member
        // function with one `this` pointer and a pointer return.
        let net_layer =
            unsafe { std::mem::transmute::<usize, GetNetLayer>(self.get_net_layer)(server) };
        if net_layer.is_null() {
            return Err(BridgeInstallError::new(
                "CServerExoApp::GetNetLayer returned null",
            ));
        }
        // SAFETY: the exact target pack binds the address to the trivial u32
        // return ABI of CNetLayer::GetSessionMaxPlayers.
        let max_players = unsafe {
            std::mem::transmute::<usize, GetSessionMaxPlayers>(self.get_session_max_players)(
                net_layer,
            )
        };
        i32::try_from(max_players).map_err(|_error| {
            BridgeInstallError::new("server maximum player count exceeds NWScript integer range")
        })
    }

    fn event_context(&self, vm: *mut c_void) -> Result<EventContext, BridgeInstallError> {
        if vm.is_null() {
            return Err(BridgeInstallError::new(
                "event context received a null virtual machine",
            ));
        }
        // SAFETY: vm is the command implementer's live CVirtualMachine and the
        // exact target pack records its recursion-level field offset.
        let recursion_level = unsafe {
            vm.cast::<u8>()
                .add(self.recursion_level_offset)
                .cast::<i32>()
                .read()
        };
        let Ok(slot_index) = usize::try_from(recursion_level) else {
            return Ok(EventContext::default());
        };
        if slot_index >= self.script_slot_count {
            return Err(BridgeInstallError::new(format!(
                "virtual-machine recursion level {recursion_level} exceeds the {} script slots",
                self.script_slot_count
            )));
        }
        let slot_offset = slot_index
            .checked_mul(self.script_stride)
            .and_then(|offset| self.script_array_offset.checked_add(offset))
            .ok_or_else(|| BridgeInstallError::new("virtual-machine script slot overflowed"))?;
        // SAFETY: the selected slot is bounded above and every field offset is
        // validated against the target pack's CVirtualMachineScript stride.
        let slot = unsafe { vm.cast::<u8>().add(slot_offset) };
        let id = unsafe { slot.add(self.script_event_id_offset).cast::<i32>().read() };
        if id <= 0 {
            return Ok(EventContext::default());
        }
        let script = unsafe { &*slot.add(self.script_name_offset).cast::<CExoString>() };
        let depth = recursion_level
            .checked_add(1)
            .ok_or_else(|| BridgeInstallError::new("event recursion depth overflowed"))?;
        Ok(EventContext {
            name: event_name(id).to_string(),
            id,
            script_name: copy_exo_string(script)?,
            phase: "running".to_string(),
            depth,
        })
    }
}

fn checked_offset(name: &str, offset: u64) -> Result<usize, BridgeInstallError> {
    usize::try_from(offset).map_err(|_error| {
        BridgeInstallError::new(format!("target-pack offset {name} exceeds usize"))
    })
}

fn read_pointer_field(
    object: *mut c_void,
    offset: usize,
    name: &str,
) -> Result<*mut c_void, BridgeInstallError> {
    if object.is_null() {
        return Err(BridgeInstallError::new(format!("{name} owner is null")));
    }
    // SAFETY: the object comes from the engine and offset is hash-specific,
    // pointer-aligned target data validated before runtime initialization.
    let value = unsafe { object.cast::<u8>().add(offset).cast::<*mut c_void>().read() };
    if value.is_null() {
        return Err(BridgeInstallError::new(format!("{name} is null")));
    }
    Ok(value)
}

fn resolve_address(
    module: *mut frida_gum_sys::GumModule,
    name: &str,
    address: &TargetAddress,
) -> Result<usize, BridgeInstallError> {
    let resolved = match address {
        TargetAddress::Symbol {
            symbol,
        } => {
            let symbol = CString::new(symbol.as_str()).map_err(|_error| {
                BridgeInstallError::new(format!("bridge.{name} contains a NUL byte"))
            })?;
            // SAFETY: module is retained for this resolution pass and symbol is
            // a valid NUL-terminated name. The global fallback permits an exact
            // target pack to name imported C++ allocation functions.
            unsafe {
                let local = frida_gum_sys::gum_module_find_symbol_by_name(module, symbol.as_ptr());
                if local == 0 {
                    frida_gum_sys::gum_module_find_global_export_by_name(symbol.as_ptr())
                } else {
                    local
                }
            }
        }
        TargetAddress::Offset {
            offset,
        } => {
            // SAFETY: module is retained for this resolution pass.
            let range = unsafe { frida_gum_sys::gum_module_get_range(module) };
            if range.is_null() {
                return Err(BridgeInstallError::new(
                    "Frida Gum returned no range for the main executable",
                ));
            }
            // SAFETY: range is borrowed from the retained module.
            let range = unsafe { &*range };
            if *offset >= range.size {
                return Err(BridgeInstallError::new(format!(
                    "bridge.{name} offset {offset:#x} is outside the main executable"
                )));
            }
            range.base_address.checked_add(*offset).ok_or_else(|| {
                BridgeInstallError::new(format!("bridge.{name} address overflowed"))
            })?
        }
    };
    if resolved == 0 {
        return Err(BridgeInstallError::new(format!(
            "could not resolve bridge.{name} in the main executable"
        )));
    }
    usize::try_from(resolved)
        .map_err(|_error| BridgeInstallError::new(format!("bridge.{name} address exceeds usize")))
}

fn copy_exo_string(value: &CExoString) -> Result<Vec<u8>, BridgeInstallError> {
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
    if length > MAX_ENGINE_STRING_BYTES {
        return Err(BridgeInstallError::new(format!(
            "engine returned a CExoString larger than {MAX_ENGINE_STRING_BYTES} bytes"
        )));
    }
    // SAFETY: the engine owns a live CExoString containing at least its stated
    // length for the duration of the native bridge operation.
    let bytes = unsafe { slice::from_raw_parts(value.string.cast::<u8>(), length) };
    Ok(bytes.to_vec())
}

fn bool_result(value: i32, error: i32) -> Result<(), i32> {
    if value == 0 { Err(error) } else { Ok(()) }
}

extern "C" fn function_management_replacement(
    commands: *mut c_void,
    command: i32,
    parameters: i32,
) -> i32 {
    panic::catch_unwind(AssertUnwindSafe(|| {
        handle_function_management(commands, command, parameters)
    }))
    .unwrap_or(VM_FAKE_ABORT_SCRIPT)
}

fn handle_function_management(commands: *mut c_void, command: i32, parameters: i32) -> i32 {
    let Some(engine) = ENGINE_BRIDGE.get() else {
        return VM_FAKE_ABORT_SCRIPT;
    };
    let vm = match engine.virtual_machine(commands) {
        Ok(vm) => vm,
        Err(error) => {
            write_diagnostic(&error.to_string());
            return VM_FAKE_ABORT_SCRIPT;
        }
    };

    match command {
        NWNX_GET_IS_AVAILABLE => result_code(engine.push_integer(vm, 1)),
        NWNX_CALL => handle_call(engine, vm),
        NWNX_PUSH_INTEGER => push_argument(engine.pop_integer(vm).map(BridgeValue::Integer)),
        NWNX_PUSH_FLOAT => push_argument(engine.pop_float(vm).map(BridgeValue::Float)),
        NWNX_PUSH_OBJECT => push_argument(engine.pop_object(vm).map(BridgeValue::Object)),
        NWNX_PUSH_STRING => push_argument(engine.pop_string(vm).map(BridgeValue::String)),
        NWNX_PUSH_VECTOR => push_argument(engine.pop_vector(vm).map(BridgeValue::Vector)),
        NWNX_POP_INTEGER => {
            let value = pop_return(|bridge| bridge.pop_integer()).unwrap_or_default();
            result_code(engine.push_integer(vm, value))
        }
        NWNX_POP_FLOAT => {
            let value = pop_return(|bridge| bridge.pop_float()).unwrap_or_default();
            result_code(engine.push_float(vm, value))
        }
        NWNX_POP_OBJECT => {
            let value = pop_return(|bridge| bridge.pop_object()).unwrap_or(OBJECT_INVALID);
            result_code(engine.push_object(vm, value))
        }
        NWNX_POP_STRING => {
            let value = pop_return(|bridge| bridge.pop_string()).unwrap_or_default();
            result_code(engine.push_string(vm, &value))
        }
        NWNX_POP_VECTOR => {
            let value = pop_return(|bridge| bridge.pop_vector()).unwrap_or(Vector {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            });
            result_code(engine.push_vector(vm, value))
        }
        _ => call_original(commands, command, parameters),
    }
}

fn handle_call(engine: &EngineBridge, vm: *mut c_void) -> i32 {
    let namespace = match engine.pop_string(vm) {
        Ok(value) => value,
        Err(error) => return error,
    };
    let function = match engine.pop_string(vm) {
        Ok(value) => value,
        Err(error) => return error,
    };
    let namespace = match std::str::from_utf8(&namespace) {
        Ok(value) => value,
        Err(_error) => {
            write_diagnostic("NWScript bridge namespace is not UTF-8");
            return VM_SUCCESS;
        }
    };
    let function = match std::str::from_utf8(&function) {
        Ok(value) => value,
        Err(_error) => {
            write_diagnostic("NWScript bridge function is not UTF-8");
            return VM_SUCCESS;
        }
    };
    let Some(context) = RUNTIME_CONTEXT.get() else {
        return VM_FAKE_ABORT_SCRIPT;
    };
    let server = match server_state_for_call(engine, function) {
        Ok(server) => server,
        Err(error) => {
            write_diagnostic(&error.to_string());
            return VM_SUCCESS;
        }
    };
    let event = match engine.event_context(vm) {
        Ok(event) => event,
        Err(error) => {
            write_diagnostic(&error.to_string());
            EventContext::default()
        }
    };
    let result = SCRIPT_BRIDGE.try_with(|bridge| {
        let mut bridge = bridge
            .try_borrow_mut()
            .map_err(|_error| "NWScript bridge call was reentrant".to_string())?;
        bridge
            .call(namespace, function, context, &server, &event)
            .map_err(|error| error.to_string())?;
        Ok::<_, String>(bridge.take_logs())
    });
    match result {
        Ok(Ok(logs)) => {
            for log in logs {
                emit_script_log(&log);
            }
            VM_SUCCESS
        }
        Ok(Err(error)) => {
            write_diagnostic(&error);
            VM_SUCCESS
        }
        Err(_error) => VM_FAKE_ABORT_SCRIPT,
    }
}

fn emit_script_log(log: &ScriptLog) {
    let message = String::from_utf8_lossy(&log.message);
    match log.level {
        ScriptLogLevel::Trace => tracing::trace!(target: "nwnrs::script", "{message}"),
        ScriptLogLevel::Debug => tracing::debug!(target: "nwnrs::script", "{message}"),
        ScriptLogLevel::Info => tracing::info!(target: "nwnrs::script", "{message}"),
        ScriptLogLevel::Warn => tracing::warn!(target: "nwnrs::script", "{message}"),
        ScriptLogLevel::Error => tracing::error!(target: "nwnrs::script", "{message}"),
    }
}

fn server_state_for_call(
    engine: &EngineBridge,
    function: &str,
) -> Result<ServerState, BridgeInstallError> {
    let mut server = ServerState::default();
    match function {
        "GetModuleName" => server.module_name = engine.module_name()?,
        "GetPlayerCount" => server.player_count = engine.player_count()?,
        "GetMaxPlayers" => server.max_players = engine.max_players()?,
        _ => {}
    }
    Ok(server)
}

fn push_argument(value: Result<BridgeValue, i32>) -> i32 {
    let value = match value {
        Ok(value) => value,
        Err(error) => return error,
    };
    match SCRIPT_BRIDGE.try_with(|bridge| {
        bridge
            .try_borrow_mut()
            .map(|mut bridge| bridge.push_argument(value))
    }) {
        Ok(Ok(())) => VM_SUCCESS,
        Ok(Err(_error)) => VM_FAKE_ABORT_SCRIPT,
        Err(_error) => VM_FAKE_ABORT_SCRIPT,
    }
}

fn pop_return<T>(
    pop: impl FnOnce(&mut ScriptBridge) -> nwnrs_runtime::BridgeResult<T>,
) -> Option<T> {
    let result = SCRIPT_BRIDGE
        .try_with(|bridge| {
            let mut bridge = bridge
                .try_borrow_mut()
                .map_err(|_error| "NWScript bridge return pop was reentrant".to_string())?;
            pop(&mut bridge).map_err(|error| error.to_string())
        })
        .ok()?;
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            write_diagnostic(&error);
            None
        }
    }
}

fn result_code(result: Result<(), i32>) -> i32 {
    result.map_or_else(|error| error, |()| VM_SUCCESS)
}

fn call_original(commands: *mut c_void, command: i32, parameters: i32) -> i32 {
    let original = FUNCTION_MANAGEMENT_ORIGINAL.load(Ordering::Acquire);
    if original.is_null() {
        return VM_FAKE_ABORT_SCRIPT;
    }
    // SAFETY: Gum populated this pointer with the exact original trampoline for
    // FunctionManagement before publishing it to the callback.
    let original = unsafe { std::mem::transmute::<*mut c_void, FunctionManagement>(original) };
    original(commands, command, parameters)
}
