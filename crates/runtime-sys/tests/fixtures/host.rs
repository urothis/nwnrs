//! Native fixture process used to verify injected NWScript bridge calls.

use std::{
    ffi::c_void,
    fs,
    slice,
    sync::atomic::{AtomicU32, Ordering},
};

#[cfg(windows)]
#[path = "windows_theme.rs"]
mod windows_theme;

unsafe extern "C" {
    fn malloc(size: usize) -> *mut c_void;
    fn free(pointer: *mut c_void);
    fn nwnrs_fixture_admin_init(
        net_layer: *mut c_void,
        server_vault: *const u8,
        server_vault_length: usize,
    );
    fn nwnrs_fixture_reset_turd();
    fn nwnrs_fixture_admin_keep_symbols() -> *mut c_void;
}

const NWNX_GET_IS_AVAILABLE: i32 = 1151;
const NWNX_CALL: i32 = 1152;
const NWNX_PUSH_INTEGER: i32 = 1153;
const NWNX_PUSH_OBJECT: i32 = 1155;
const NWNX_PUSH_STRING: i32 = 1156;
const NWNX_POP_INTEGER: i32 = 1167;
const NWNX_POP_STRING: i32 = 1170;
const VM_SCRIPT_IMPLEMENTATION_BYTES: usize = if cfg!(target_os = "linux") {
    76
} else if cfg!(target_os = "windows") {
    84
} else {
    60
};

#[derive(Debug)]
enum Value {
    Integer(i32),
    Float(f32),
    Object(u32),
    String(String),
    Vector(Vector),
}

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct Vector {
    x: f32,
    y: f32,
    z: f32,
}

#[repr(C)]
struct VirtualMachine {
    resource_types:       [u16; 3],
    resource_padding:     [u8; 2],
    jit_compiler:         *mut c_void,
    return_value_type:    i32,
    return_value_padding: [u8; 4],
    return_value:         *mut c_void,
    instructions:         u32,
    recursion_level:      i32,
    scripts:              [VirtualMachineScript; 8],
    stack:                Vec<Value>,
}

impl Default for VirtualMachine {
    fn default() -> Self {
        Self {
            resource_types:       [0; 3],
            resource_padding:     [0; 2],
            jit_compiler:         std::ptr::null_mut(),
            return_value_type:    0,
            return_value_padding: [0; 4],
            return_value:         std::ptr::null_mut(),
            instructions:         0,
            recursion_level:      -1,
            scripts:              std::array::from_fn(|_| VirtualMachineScript::default()),
            stack:                Vec::new(),
        }
    }
}

#[repr(C)]
struct Commands {
    virtual_machine: *mut c_void,
}

#[repr(C)]
struct AppManager {
    client_exo_app: *mut c_void,
    server_exo_app: *mut c_void,
}

#[repr(C)]
struct ServerExoApp {
    server_info: *mut ServerInfo,
    player_list: *mut PlayerList,
    net_layer:   *mut NetLayer,
    internal:    *mut ServerInternal,
}

#[repr(C)]
struct StringList {
    elements: *mut CExoString,
    count:    i32,
    capacity: i32,
}

#[repr(C)]
struct ServerInternal {
    banned_ip_addresses: StringList,
    banned_cd_keys:      StringList,
    banned_player_names: StringList,
}

#[repr(C)]
struct ServerInfo {
    data_changed_flags: i32,
    server_mode:       i16,
    padding:           [u8; 2],
    module_name:       CExoString,
    before_joining:    [u8; 112],
    joining:           [i32; 29],
    play_options:      [i32; 29],
    before_persistent: [u8; 36],
    persistent_world_options: [i32; 5],
}

#[repr(C)]
struct PlayerList {
    elements: *mut c_void,
    count:    i32,
    capacity: i32,
}

#[repr(C)]
struct NetLayer {
    max_players: u32,
    udp_port:    u32,
    session_name: CExoString,
    player_password: CExoString,
    dm_password: CExoString,
}

#[derive(Debug, Eq, PartialEq)]
struct ObservedEvent {
    name:        String,
    id:          i32,
    script_name: String,
    phase:       String,
    depth:       i32,
    is_in_event: i32,
}

#[unsafe(no_mangle)]
pub static mut nwnrs_fixture_app_manager: *mut c_void = std::ptr::null_mut();

#[unsafe(no_mangle)]
pub static mut nwnrs_fixture_virtual_machine: *mut c_void = std::ptr::null_mut();

static MODULE_ONLOAD_CALLS: AtomicU32 = AtomicU32::new(0);

unsafe extern "C" {
    static mut nwnrs_fixture_enable_combat_debugging: i32;
    static mut nwnrs_fixture_enable_saving_throw_debugging: i32;
    static mut nwnrs_fixture_enable_movement_speed_debugging: i32;
    static mut nwnrs_fixture_enable_hit_die_debugging: i32;
    static mut nwnrs_fixture_exit_program: i32;
    static mut nwnrs_fixture_rules: *mut c_void;
    static mut nwnrs_fixture_disconnect_count: i32;
    static mut nwnrs_fixture_disconnect_reason_length: u32;
}

#[repr(C)]
pub struct CExoString {
    string:        *mut u8,
    string_length: u32,
    buffer_length: u32,
}

#[repr(C)]
struct VirtualMachineScript {
    stack:                 *mut c_void,
    stack_size:            i32,
    instruction_pointer:   i32,
    secondary_pointer:     i32,
    script_name_padding:   [u8; 4],
    script_name:           CExoString,
    code:                  [usize; 2],
    debug_data:            [usize; 2],
    event_id:              i32,
    implementation_detail: [u8; VM_SCRIPT_IMPLEMENTATION_BYTES],
}

impl Default for VirtualMachineScript {
    fn default() -> Self {
        Self {
            stack:                 std::ptr::null_mut(),
            stack_size:            0,
            instruction_pointer:   0,
            secondary_pointer:     0,
            script_name_padding:   [0; 4],
            script_name:           empty_exo_string(),
            code:                  [0; 2],
            debug_data:            [0; 2],
            event_id:              0,
            implementation_detail: [0; VM_SCRIPT_IMPLEMENTATION_BYTES],
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn nwnrs_fixture_function_management(
    _commands: *mut c_void,
    _command: i32,
    _parameters: i32,
) -> i32 {
    -642
}

#[unsafe(no_mangle)]
pub extern "C" fn nwnrs_fixture_stack_pop_integer(
    vm: *mut c_void,
    output: *mut i32,
) -> i32 {
    let Some(Value::Integer(value)) = pop_value(vm) else {
        return 0;
    };
    // SAFETY: the bridge supplies writable storage for the requested VM type.
    unsafe {
        output.write(value);
    }
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn nwnrs_fixture_stack_push_integer(vm: *mut c_void, value: i32) -> i32 {
    push_value(vm, Value::Integer(value))
}

#[unsafe(no_mangle)]
pub extern "C" fn nwnrs_fixture_stack_pop_float(
    vm: *mut c_void,
    output: *mut f32,
) -> i32 {
    let Some(Value::Float(value)) = pop_value(vm) else {
        return 0;
    };
    // SAFETY: the bridge supplies writable storage for the requested VM type.
    unsafe {
        output.write(value);
    }
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn nwnrs_fixture_stack_push_float(vm: *mut c_void, value: f32) -> i32 {
    push_value(vm, Value::Float(value))
}

#[unsafe(no_mangle)]
pub extern "C" fn nwnrs_fixture_stack_pop_object(
    vm: *mut c_void,
    output: *mut u32,
) -> i32 {
    let Some(Value::Object(value)) = pop_value(vm) else {
        return 0;
    };
    // SAFETY: the bridge supplies writable storage for the requested VM type.
    unsafe {
        output.write(value);
    }
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn nwnrs_fixture_stack_push_object(vm: *mut c_void, value: u32) -> i32 {
    push_value(vm, Value::Object(value))
}

#[unsafe(no_mangle)]
pub extern "C" fn nwnrs_fixture_stack_pop_string(
    vm: *mut c_void,
    output: *mut CExoString,
) -> i32 {
    let Some(Value::String(value)) = pop_value(vm) else {
        return 0;
    };
    let Ok(string_length) = u32::try_from(value.len()) else {
        return 0;
    };
    let mut bytes = value.into_bytes();
    bytes.push(0);
    let Ok(buffer_length) = u32::try_from(bytes.len()) else {
        return 0;
    };
    // SAFETY: buffer_length is nonzero and the result is checked before use.
    let string = unsafe { malloc(bytes.len()) }.cast::<u8>();
    if string.is_null() {
        return 0;
    }
    // SAFETY: malloc returned at least bytes.len() writable bytes and neither
    // source nor destination overlaps.
    unsafe {
        string.copy_from_nonoverlapping(bytes.as_ptr(), bytes.len());
    }
    // SAFETY: the bridge supplies writable CExoString storage and assumes
    // ownership of the allocation until it calls the fixture destructor.
    unsafe {
        output.write(CExoString {
            string,
            string_length,
            buffer_length,
        });
    }
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn nwnrs_fixture_stack_push_string(
    vm: *mut c_void,
    value: *const CExoString,
) -> i32 {
    // SAFETY: the bridge passes a live CExoString for the duration of this call.
    let value = unsafe { &*value };
    let Ok(length) = usize::try_from(value.string_length) else {
        return 0;
    };
    if value.string.is_null() && length != 0 {
        return 0;
    }
    let bytes = if length == 0 {
        &[]
    } else {
        // SAFETY: CExoString points at at least string_length readable bytes.
        unsafe { slice::from_raw_parts(value.string, length) }
    };
    let Ok(string) = String::from_utf8(bytes.to_vec()) else {
        return 0;
    };
    push_value(vm, Value::String(string))
}

#[unsafe(no_mangle)]
pub extern "C" fn nwnrs_fixture_stack_pop_vector(
    vm: *mut c_void,
    output: *mut Vector,
) -> i32 {
    let Some(Value::Vector(value)) = pop_value(vm) else {
        return 0;
    };
    // SAFETY: the bridge supplies writable storage for the requested VM type.
    unsafe {
        output.write(value);
    }
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn nwnrs_fixture_stack_push_vector(vm: *mut c_void, value: Vector) -> i32 {
    push_value(vm, Value::Vector(value))
}

#[unsafe(no_mangle)]
pub extern "C" fn nwnrs_fixture_free_exo_string_buffer(value: *mut c_void) {
    // SAFETY: this receives the unique malloc allocation created by the
    // fixture StackPopString implementation.
    unsafe {
        free(value);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn nwnrs_fixture_get_server_info(server: *mut c_void) -> *mut c_void {
    if server.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: the fixture passes its live ServerExoApp as the method receiver.
    unsafe { (*server.cast::<ServerExoApp>()).server_info.cast() }
}

#[unsafe(no_mangle)]
pub extern "C" fn nwnrs_fixture_get_player_list(server: *mut c_void) -> *mut c_void {
    if server.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: the fixture passes its live ServerExoApp as the method receiver.
    unsafe { (*server.cast::<ServerExoApp>()).player_list.cast() }
}

#[unsafe(no_mangle)]
pub extern "C" fn nwnrs_fixture_get_net_layer(server: *mut c_void) -> *mut c_void {
    if server.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: the fixture passes its live ServerExoApp as the method receiver.
    unsafe { (*server.cast::<ServerExoApp>()).net_layer.cast() }
}

#[unsafe(no_mangle)]
pub extern "C" fn nwnrs_fixture_get_session_max_players(net_layer: *mut c_void) -> u32 {
    if net_layer.is_null() {
        return 0;
    }
    // SAFETY: the fixture passes its live NetLayer as the method receiver.
    unsafe { (*net_layer.cast::<NetLayer>()).max_players }
}

#[unsafe(no_mangle)]
pub extern "C" fn nwnrs_fixture_get_udp_port(net_layer: *mut c_void) -> u32 {
    if net_layer.is_null() {
        return 0;
    }
    // SAFETY: the fixture passes its live NetLayer as the method receiver.
    unsafe { (*net_layer.cast::<NetLayer>()).udp_port }
}

#[unsafe(no_mangle)]
pub extern "C" fn nwnrs_fixture_main_loop(_server_internal: *mut c_void) -> i32 {
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn nwnrs_fixture_load_module_finish(_module: *mut c_void) -> u32 {
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn nwnrs_fixture_run_script(
    vm: *mut c_void,
    script: *mut CExoString,
    owner: u32,
    owner_is_valid: i32,
    event_id: i32,
) -> i32 {
    if vm.is_null() || script.is_null() || owner != 0 || owner_is_valid != 1 || event_id != 0 {
        return 0;
    }
    // SAFETY: the fixture bridge supplies one live CExoString for the
    // synchronous call.
    let script = unsafe { &*script };
    if script.string.is_null() {
        return 0;
    }
    let Ok(length) = usize::try_from(script.string_length) else {
        return 0;
    };
    // SAFETY: the fixture string buffer contains string_length bytes.
    let name = unsafe { slice::from_raw_parts(script.string, length) };
    if name != b"_nwnrs_onload" {
        return 0;
    }
    MODULE_ONLOAD_CALLS.fetch_add(1, Ordering::Relaxed);
    1
}

fn pop_value(vm: *mut c_void) -> Option<Value> {
    if vm.is_null() {
        return None;
    }
    // SAFETY: the fixture passes a live VirtualMachine pointer to every ABI
    // function and executes all calls on its main thread.
    unsafe { &mut *vm.cast::<VirtualMachine>() }.stack.pop()
}

fn push_value(vm: *mut c_void, value: Value) -> i32 {
    if vm.is_null() {
        return 0;
    }
    // SAFETY: the fixture passes a live VirtualMachine pointer to every ABI
    // function and executes all calls on its main thread.
    unsafe { &mut *vm.cast::<VirtualMachine>() }
        .stack
        .push(value);
    1
}

fn call(commands: &mut Commands, command: i32) -> i32 {
    nwnrs_fixture_function_management(commands as *mut Commands as *mut c_void, command, 0)
}

fn call_string(commands: &mut Commands, vm: &mut VirtualMachine, function: &str) -> String {
    vm.stack.push(Value::String(function.to_string()));
    vm.stack.push(Value::String("NWNRS".to_string()));
    assert_eq!(call(commands, NWNX_CALL), 0);
    assert_eq!(call(commands, NWNX_POP_STRING), 0);
    match vm.stack.pop() {
        Some(Value::String(value)) => value,
        value => panic!("expected string bridge result, found {value:?}"),
    }
}

fn call_integer(commands: &mut Commands, vm: &mut VirtualMachine, function: &str) -> i32 {
    vm.stack.push(Value::String(function.to_string()));
    vm.stack.push(Value::String("NWNRS".to_string()));
    assert_eq!(call(commands, NWNX_CALL), 0);
    assert_eq!(call(commands, NWNX_POP_INTEGER), 0);
    match vm.stack.pop() {
        Some(Value::Integer(value)) => value,
        value => panic!("expected integer bridge result, found {value:?}"),
    }
}

fn call_log(commands: &mut Commands, vm: &mut VirtualMachine, level: i32, message: &str) {
    vm.stack.push(Value::Integer(level));
    assert_eq!(call(commands, NWNX_PUSH_INTEGER), 0);
    vm.stack.push(Value::String(message.to_string()));
    assert_eq!(call(commands, NWNX_PUSH_STRING), 0);
    vm.stack.push(Value::String("Log".to_string()));
    vm.stack.push(Value::String("NWNRS".to_string()));
    assert_eq!(call(commands, NWNX_CALL), 0);
}

fn call_capability_version(commands: &mut Commands, vm: &mut VirtualMachine, capability: &str) -> i32 {
    vm.stack.push(Value::String(capability.to_string()));
    assert_eq!(call(commands, NWNX_PUSH_STRING), 0);
    call_integer(commands, vm, "GetCapabilityVersion")
}

fn call_has_capability(
    commands: &mut Commands,
    vm: &mut VirtualMachine,
    capability: &str,
    minimum: i32,
) -> i32 {
    vm.stack.push(Value::Integer(minimum));
    assert_eq!(call(commands, NWNX_PUSH_INTEGER), 0);
    vm.stack.push(Value::String(capability.to_string()));
    assert_eq!(call(commands, NWNX_PUSH_STRING), 0);
    call_integer(commands, vm, "HasCapability")
}

fn call_without_result(commands: &mut Commands, vm: &mut VirtualMachine, function: &str) {
    vm.stack.push(Value::String(function.to_string()));
    vm.stack.push(Value::String("NWNRS".to_string()));
    assert_eq!(call(commands, NWNX_CALL), 0);
}

fn call_with_string(commands: &mut Commands, vm: &mut VirtualMachine, function: &str, value: &str) {
    vm.stack.push(Value::String(value.to_string()));
    assert_eq!(call(commands, NWNX_PUSH_STRING), 0);
    call_without_result(commands, vm, function);
}

fn call_with_integer(commands: &mut Commands, vm: &mut VirtualMachine, function: &str, value: i32) {
    vm.stack.push(Value::Integer(value));
    assert_eq!(call(commands, NWNX_PUSH_INTEGER), 0);
    call_without_result(commands, vm, function);
}

fn call_integer_with_integer(
    commands: &mut Commands,
    vm: &mut VirtualMachine,
    function: &str,
    value: i32,
) -> i32 {
    vm.stack.push(Value::Integer(value));
    assert_eq!(call(commands, NWNX_PUSH_INTEGER), 0);
    call_integer(commands, vm, function)
}

fn call_with_two_integers(
    commands: &mut Commands,
    vm: &mut VirtualMachine,
    function: &str,
    first: i32,
    second: i32,
) {
    vm.stack.push(Value::Integer(second));
    assert_eq!(call(commands, NWNX_PUSH_INTEGER), 0);
    vm.stack.push(Value::Integer(first));
    assert_eq!(call(commands, NWNX_PUSH_INTEGER), 0);
    call_without_result(commands, vm, function);
}

fn call_delete_player_character(
    commands: &mut Commands,
    vm: &mut VirtualMachine,
    object_id: u32,
    preserve_backup: bool,
    kick_message: &str,
) {
    vm.stack.push(Value::String(kick_message.to_string()));
    assert_eq!(call(commands, NWNX_PUSH_STRING), 0);
    vm.stack
        .push(Value::Integer(i32::from(preserve_backup)));
    assert_eq!(call(commands, NWNX_PUSH_INTEGER), 0);
    vm.stack.push(Value::Object(object_id));
    assert_eq!(call(commands, NWNX_PUSH_OBJECT), 0);
    call_without_result(commands, vm, "DeletePlayerCharacter");
}

fn call_integer_with_two_strings(
    commands: &mut Commands,
    vm: &mut VirtualMachine,
    function: &str,
    first: &str,
    second: &str,
) -> i32 {
    vm.stack.push(Value::String(second.to_string()));
    assert_eq!(call(commands, NWNX_PUSH_STRING), 0);
    vm.stack.push(Value::String(first.to_string()));
    assert_eq!(call(commands, NWNX_PUSH_STRING), 0);
    call_integer(commands, vm, function)
}

fn observe_current_event(commands: &mut Commands, vm: &mut VirtualMachine) -> ObservedEvent {
    ObservedEvent {
        name:        call_string(commands, vm, "GetCurrentEvent"),
        id:          call_integer(commands, vm, "GetCurrentEventId"),
        script_name: call_string(commands, vm, "GetCurrentEventScript"),
        phase:       call_string(commands, vm, "GetCurrentEventPhase"),
        depth:       call_integer(commands, vm, "GetCurrentEventDepth"),
        is_in_event: call_integer(commands, vm, "GetIsInEvent"),
    }
}

fn empty_exo_string() -> CExoString {
    CExoString {
        string:        std::ptr::null_mut(),
        string_length: 0,
        buffer_length: 0,
    }
}

fn borrowed_exo_string(bytes: &mut [u8]) -> CExoString {
    let string_length = u32::try_from(bytes.len()).expect("fixture script name fits u32");
    CExoString {
        string: bytes.as_mut_ptr(),
        string_length,
        buffer_length: string_length,
    }
}

fn set_event(vm: &mut VirtualMachine, level: usize, id: i32, script_name: &mut [u8]) {
    vm.recursion_level = i32::try_from(level).expect("fixture recursion level fits i32");
    let script = vm.scripts.get_mut(level).expect("fixture script slot exists");
    script.event_id = id;
    script.script_name = borrowed_exo_string(script_name);
}

fn assert_fixture_layout() {
    let expected_stride = if cfg!(target_os = "linux") {
        152
    } else if cfg!(target_os = "windows") {
        160
    } else {
        136
    };
    assert_eq!(std::mem::offset_of!(VirtualMachine, recursion_level), 36);
    assert_eq!(std::mem::offset_of!(VirtualMachine, scripts), 40);
    assert_eq!(std::mem::size_of::<VirtualMachineScript>(), expected_stride);
    assert_eq!(std::mem::offset_of!(VirtualMachineScript, script_name), 24);
    assert_eq!(std::mem::offset_of!(VirtualMachineScript, event_id), 72);
    assert_eq!(std::mem::offset_of!(ServerInfo, module_name), 8);
    assert_eq!(std::mem::offset_of!(ServerInfo, joining), 136);
    assert_eq!(std::mem::offset_of!(ServerInfo, play_options), 252);
    assert_eq!(std::mem::offset_of!(ServerInfo, persistent_world_options), 404);
}

fn main() {
    assert_fixture_layout();
    keep_abi_symbols();
    #[cfg(windows)]
    windows_theme::verify();
    // SAFETY: this only creates linker-visible references to fixture symbols.
    unsafe {
        std::hint::black_box(nwnrs_fixture_admin_keep_symbols());
    }
    let server_vault = std::env::temp_dir().join(format!(
        "nwnrs-runtime-native-fixture-{}",
        std::process::id()
    ));
    fs::create_dir(&server_vault).expect("create isolated fixture server vault");
    let player_vault = server_vault.join("fixture-player");
    fs::create_dir(&player_vault).expect("create fixture player vault");
    let player_character = player_vault.join("fixturechar.bic");
    fs::write(&player_character, b"fixture character").expect("write fixture character");

    let mut module_name_bytes = b"fixture-module\0".to_vec();
    let mut server_info = ServerInfo {
        data_changed_flags: 0,
        server_mode:       0,
        padding:           [0; 2],
        module_name:       CExoString {
            string:        module_name_bytes.as_mut_ptr(),
            string_length: 14,
            buffer_length: 15,
        },
        before_joining:    [0; 112],
        joining:           {
            let mut values = [0; 29];
            values[26] = 1;
            values[27] = 40;
            values
        },
        play_options:      {
            let mut values = [0; 29];
            values[10] = 2;
            values
        },
        before_persistent: [0; 36],
        persistent_world_options: {
            let mut values = [0; 5];
            values[4] = 1;
            values
        },
    };
    let mut player_list = PlayerList {
        elements: std::ptr::null_mut(),
        count:    3,
        capacity: 3,
    };
    let mut net_layer = NetLayer {
        max_players: 64,
        udp_port:    5121,
        session_name: empty_exo_string(),
        player_password: empty_exo_string(),
        dm_password: empty_exo_string(),
    };
    // SAFETY: the C++ fixture initializes the three CExoString fields using
    // the same exact layout asserted by the runtime ABI probe.
    let server_vault_text = server_vault.to_string_lossy();
    unsafe {
        nwnrs_fixture_admin_init(
            (&raw mut net_layer).cast(),
            server_vault_text.as_bytes().as_ptr(),
            server_vault_text.len(),
        );
    }
    let mut server = ServerExoApp {
        server_info: &raw mut server_info,
        player_list: &raw mut player_list,
        net_layer:   &raw mut net_layer,
        internal:    std::ptr::null_mut(),
    };
    let empty_list = || StringList {
        elements: std::ptr::null_mut(),
        count: 0,
        capacity: 0,
    };
    let mut server_internal = ServerInternal {
        banned_ip_addresses: empty_list(),
        banned_cd_keys: empty_list(),
        banned_player_names: empty_list(),
    };
    server.internal = &raw mut server_internal;
    let mut app_manager = AppManager {
        client_exo_app: std::ptr::null_mut(),
        server_exo_app: (&raw mut server).cast(),
    };
    // SAFETY: the fixture owns app_manager until process exit and executes on
    // one thread.
    unsafe {
        nwnrs_fixture_app_manager = (&raw mut app_manager).cast();
    }
    let mut vm = VirtualMachine::default();
    // SAFETY: the fixture owns vm until process exit and executes on one
    // thread.
    unsafe {
        nwnrs_fixture_virtual_machine = (&raw mut vm).cast();
    }
    assert_eq!(nwnrs_fixture_load_module_finish(std::ptr::null_mut()), 1);
    assert_eq!(MODULE_ONLOAD_CALLS.load(Ordering::Relaxed), 1);
    let mut commands = Commands {
        virtual_machine: (&raw mut vm).cast(),
    };

    assert_eq!(call(&mut commands, NWNX_GET_IS_AVAILABLE), 0);
    assert!(matches!(vm.stack.pop(), Some(Value::Integer(1))));

    assert_eq!(call_integer(&mut commands, &mut vm, "GetApiVersion"), 1);
    assert_eq!(call_capability_version(&mut commands, &mut vm, "nwscript_bridge"), 1);
    assert_eq!(call_capability_version(&mut commands, &mut vm, "server_state"), 1);
    assert_eq!(call_capability_version(&mut commands, &mut vm, "administration"), 1);
    assert_eq!(call_capability_version(&mut commands, &mut vm, "event_context"), 2);
    assert_eq!(call_has_capability(&mut commands, &mut vm, "server_state", 1), 1);
    assert_eq!(call_has_capability(&mut commands, &mut vm, "server_state", 2), 0);
    assert_eq!(call_integer(&mut commands, &mut vm, "GetLastErrorCode"), 0);
    call_without_result(&mut commands, &mut vm, "NotRegistered");
    assert_eq!(call_integer(&mut commands, &mut vm, "GetLastErrorCode"), 2);
    assert!(call_string(&mut commands, &mut vm, "GetLastErrorMessage").contains("NotRegistered"));

    assert!(!call_string(&mut commands, &mut vm, "GetRuntimeVersion").is_empty());
    assert_eq!(
        call_string(&mut commands, &mut vm, "GetServerBinarySha256").len(),
        64
    );
    assert_eq!(
        call_string(&mut commands, &mut vm, "GetServerBuild"),
        "fixture"
    );
    assert!(matches!(
        call_string(&mut commands, &mut vm, "GetServerOperatingSystem").as_str(),
        "macos" | "linux" | "windows"
    ));
    assert!(matches!(
        call_string(&mut commands, &mut vm, "GetServerArchitecture").as_str(),
        "aarch64" | "x86_64"
    ));
    assert_eq!(
        call_string(&mut commands, &mut vm, "GetModuleName"),
        "fixture-module"
    );
    assert_eq!(call_integer(&mut commands, &mut vm, "GetPlayerCount"), 3);
    assert_eq!(call_integer(&mut commands, &mut vm, "GetMaxPlayers"), 64);
    assert_eq!(call_integer(&mut commands, &mut vm, "GetServerPort"), 5121);
    assert_eq!(
        call_string(&mut commands, &mut vm, "GetServerName"),
        "fixture server"
    );
    assert_eq!(
        call_integer(&mut commands, &mut vm, "GetIsPlayerPasswordSet"),
        1
    );
    assert_eq!(call_integer(&mut commands, &mut vm, "GetIsDmPasswordSet"), 0);
    assert_eq!(call_integer(&mut commands, &mut vm, "GetMinLevel"), 1);
    assert_eq!(call_integer(&mut commands, &mut vm, "GetMaxLevel"), 40);
    assert_eq!(
        call_integer_with_integer(&mut commands, &mut vm, "GetPlayOption", 10),
        2
    );
    assert_eq!(
        call_integer_with_integer(&mut commands, &mut vm, "GetDebugValue", 1),
        1
    );
    assert_eq!(
        call_string(&mut commands, &mut vm, "GetBannedList"),
        "{\"ip_addresses\":[],\"cd_keys\":[],\"player_names\":[]}"
    );

    call_with_string(&mut commands, &mut vm, "SetServerName", "renamed server");
    assert_eq!(
        call_string(&mut commands, &mut vm, "GetServerName"),
        "renamed server"
    );
    call_without_result(&mut commands, &mut vm, "ClearPlayerPassword");
    assert_eq!(
        call_integer(&mut commands, &mut vm, "GetIsPlayerPasswordSet"),
        0
    );
    call_with_string(&mut commands, &mut vm, "SetDmPassword", "dm secret");
    assert_eq!(call_integer(&mut commands, &mut vm, "GetIsDmPasswordSet"), 1);
    call_with_integer(&mut commands, &mut vm, "SetMinLevel", 5);
    call_with_integer(&mut commands, &mut vm, "SetMaxLevel", 35);
    assert_eq!(call_integer(&mut commands, &mut vm, "GetMinLevel"), 5);
    assert_eq!(call_integer(&mut commands, &mut vm, "GetMaxLevel"), 35);
    call_with_two_integers(&mut commands, &mut vm, "SetPlayOption", 14, 1);
    assert_eq!(
        call_integer_with_integer(&mut commands, &mut vm, "GetPlayOption", 14),
        1
    );
    call_with_two_integers(&mut commands, &mut vm, "SetDebugValue", 0, 1);
    assert_eq!(
        call_integer_with_integer(&mut commands, &mut vm, "GetDebugValue", 0),
        1
    );
    call_with_string(&mut commands, &mut vm, "AddBannedIp", "192.0.2.1");
    call_with_string(&mut commands, &mut vm, "RemoveBannedIp", "192.0.2.1");
    call_with_string(&mut commands, &mut vm, "AddBannedCdKey", "fixture-key");
    call_with_string(
        &mut commands,
        &mut vm,
        "RemoveBannedCdKey",
        "fixture-key",
    );
    call_with_string(
        &mut commands,
        &mut vm,
        "AddBannedPlayerName",
        "fixture-player",
    );
    call_with_string(
        &mut commands,
        &mut vm,
        "RemoveBannedPlayerName",
        "fixture-player",
    );
    call_without_result(&mut commands, &mut vm, "ReloadRules");
    assert_eq!(
        call_integer_with_two_strings(
            &mut commands,
            &mut vm,
            "DeleteTURD",
            "fixture-player",
            "Fixture Character",
        ),
        1
    );
    assert_eq!(
        call_integer_with_two_strings(
            &mut commands,
            &mut vm,
            "DeleteTURD",
            "fixture-player",
            "Fixture Character",
        ),
        0
    );
    // SAFETY: this restores only the fixture-owned linked-list state.
    unsafe {
        nwnrs_fixture_reset_turd();
    }
    call_delete_player_character(&mut commands, &mut vm, 0x0bad_f00d, true, "invalid");
    assert_eq!(call_integer(&mut commands, &mut vm, "GetLastErrorCode"), 5);
    assert!(
        call_string(&mut commands, &mut vm, "GetLastErrorMessage")
            .contains("not controlled by a connected player")
    );
    assert!(player_character.is_file());
    call_delete_player_character(
        &mut commands,
        &mut vm,
        0x0102_0304,
        true,
        "fixture kick",
    );
    assert!(player_character.is_file());
    // The Frida replacement drains deferred administration work before it
    // calls this original fixture main loop.
    let run_main_loop = std::hint::black_box(
        nwnrs_fixture_main_loop as extern "C" fn(*mut c_void) -> i32,
    );
    assert_eq!(run_main_loop((&raw mut server_internal).cast()), 1);
    assert!(!player_character.exists());
    let player_backup = player_vault.join("fixturechar.bic.deleted0");
    assert_eq!(
        fs::read(&player_backup).expect("read preserved fixture character"),
        b"fixture character"
    );
    // SAFETY: the C++ fixture globals are written only on this thread.
    unsafe {
        assert_eq!(
            std::ptr::read_volatile(&raw const nwnrs_fixture_disconnect_count),
            1
        );
        assert_eq!(
            std::ptr::read_volatile(&raw const nwnrs_fixture_disconnect_reason_length),
            12
        );
    }
    assert_eq!(
        call_integer_with_two_strings(
            &mut commands,
            &mut vm,
            "DeleteTURD",
            "fixture-player",
            "Fixture Character",
        ),
        0
    );

    let cd_key_vault = server_vault.join("fixture-key");
    fs::create_dir(&cd_key_vault).expect("create fixture CD-key vault");
    let cd_key_character = cd_key_vault.join("fixturechar.bic");
    fs::write(&cd_key_character, b"second fixture character")
        .expect("write second fixture character");
    server_info.persistent_world_options[4] = 0;
    std::hint::black_box(&server_info.persistent_world_options);
    // SAFETY: these are fixture-owned globals used on this one thread.
    unsafe {
        nwnrs_fixture_reset_turd();
        nwnrs_fixture_disconnect_count = 0;
        nwnrs_fixture_disconnect_reason_length = 0;
    }
    call_delete_player_character(&mut commands, &mut vm, 0x0102_0304, false, "");
    assert!(cd_key_character.is_file());
    assert_eq!(run_main_loop((&raw mut server_internal).cast()), 1);
    assert!(!cd_key_character.exists());
    assert!(!cd_key_vault.join("fixturechar.bic.deleted0").exists());
    // SAFETY: the C++ fixture globals are written only on this thread.
    unsafe {
        assert_eq!(
            std::ptr::read_volatile(&raw const nwnrs_fixture_disconnect_count),
            1
        );
        assert_eq!(
            std::ptr::read_volatile(&raw const nwnrs_fixture_disconnect_reason_length),
            0
        );
    }
    assert_eq!(call_integer(&mut commands, &mut vm, "GetIsInEvent"), 0);
    assert_eq!(call_integer(&mut commands, &mut vm, "GetCurrentEventId"), -1);
    assert_eq!(call_integer(&mut commands, &mut vm, "GetCurrentEventDepth"), 0);
    call_log(&mut commands, &mut vm, 0, "fixture trace message");
    call_log(&mut commands, &mut vm, 1, "fixture debug message");
    call_log(&mut commands, &mut vm, 2, "fixture info message");
    call_log(
        &mut commands,
        &mut vm,
        2,
        "fixture multiline first\nfixture multiline second\nfixture multiline third",
    );
    call_log(&mut commands, &mut vm, 3, "fixture warn message");
    call_log(&mut commands, &mut vm, 4, "fixture error message");

    let mut module_script = b"fixture_event".to_vec();
    let mut area_script = b"area_enter".to_vec();
    let mut creature_script = b"creature_spawn".to_vec();
    set_event(&mut vm, 0, 3002, &mut module_script);
    assert_eq!(
        observe_current_event(&mut commands, &mut vm),
        ObservedEvent {
            name:        "module.on_module_load".to_string(),
            id:          3002,
            script_name: "fixture_event".to_string(),
            phase:       "running".to_string(),
            depth:       1,
            is_in_event: 1,
        }
    );
    set_event(&mut vm, 1, 4002, &mut area_script);
    assert_eq!(
        observe_current_event(&mut commands, &mut vm),
        ObservedEvent {
            name:        "area.on_enter".to_string(),
            id:          4002,
            script_name: "area_enter".to_string(),
            phase:       "running".to_string(),
            depth:       2,
            is_in_event: 1,
        }
    );
    set_event(&mut vm, 0, 5008, &mut creature_script);
    assert_eq!(
        observe_current_event(&mut commands, &mut vm),
        ObservedEvent {
            name:        "creature.on_spawn_in".to_string(),
            id:          5008,
            script_name: "creature_spawn".to_string(),
            phase:       "running".to_string(),
            depth:       1,
            is_in_event: 1,
        }
    );
    call_without_result(&mut commands, &mut vm, "RequestShutdown");
    // SAFETY: the fixture global remains live until process exit.
    assert_eq!(unsafe { nwnrs_fixture_exit_program }, 1);
    fs::remove_dir_all(&server_vault).expect("remove isolated fixture server vault");
}

fn keep_abi_symbols() {
    std::hint::black_box(nwnrs_fixture_stack_pop_integer as *const ());
    std::hint::black_box(nwnrs_fixture_stack_push_integer as *const ());
    std::hint::black_box(nwnrs_fixture_stack_pop_float as *const ());
    std::hint::black_box(nwnrs_fixture_stack_push_float as *const ());
    std::hint::black_box(nwnrs_fixture_stack_pop_object as *const ());
    std::hint::black_box(nwnrs_fixture_stack_push_object as *const ());
    std::hint::black_box(nwnrs_fixture_stack_pop_string as *const ());
    std::hint::black_box(nwnrs_fixture_stack_push_string as *const ());
    std::hint::black_box(nwnrs_fixture_stack_pop_vector as *const ());
    std::hint::black_box(nwnrs_fixture_stack_push_vector as *const ());
    std::hint::black_box(nwnrs_fixture_free_exo_string_buffer as *const ());
    std::hint::black_box(nwnrs_fixture_get_server_info as *const ());
    std::hint::black_box(nwnrs_fixture_get_player_list as *const ());
    std::hint::black_box(nwnrs_fixture_get_net_layer as *const ());
    std::hint::black_box(nwnrs_fixture_get_session_max_players as *const ());
    std::hint::black_box(nwnrs_fixture_get_udp_port as *const ());
    std::hint::black_box(nwnrs_fixture_main_loop as *const ());
    std::hint::black_box(nwnrs_fixture_load_module_finish as *const ());
    std::hint::black_box(nwnrs_fixture_run_script as *const ());
    std::hint::black_box(&raw const nwnrs_fixture_app_manager);
    std::hint::black_box(&raw const nwnrs_fixture_virtual_machine);
    std::hint::black_box(&raw const nwnrs_fixture_enable_combat_debugging);
    std::hint::black_box(&raw const nwnrs_fixture_enable_saving_throw_debugging);
    std::hint::black_box(&raw const nwnrs_fixture_enable_movement_speed_debugging);
    std::hint::black_box(&raw const nwnrs_fixture_enable_hit_die_debugging);
    std::hint::black_box(&raw const nwnrs_fixture_exit_program);
    std::hint::black_box(&raw const nwnrs_fixture_rules);
}
