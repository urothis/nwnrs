//! Native fixture process used to verify injected NWScript bridge calls.

use std::{
    ffi::c_void,
    fs,
    slice,
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
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

#[unsafe(no_mangle)]
pub static mut nwnrs_fixture_app_manager: *mut c_void = std::ptr::null_mut();

#[unsafe(no_mangle)]
pub static mut nwnrs_fixture_virtual_machine: *mut c_void = std::ptr::null_mut();

static MODULE_ONLOAD_CALLS: AtomicU32 = AtomicU32::new(0);
static SUBSCRIBE_EVENTS: AtomicBool = AtomicBool::new(true);
static ASSOCIATE_EVENT_CALLS: AtomicU32 = AtomicU32::new(0);
static ADD_ASSOCIATE_ORIGINAL_CALLS: AtomicU32 = AtomicU32::new(0);
static REMOVE_ASSOCIATE_ORIGINAL_CALLS: AtomicU32 = AtomicU32::new(0);
static FAMILIAR_EVENT_CALLS: AtomicU32 = AtomicU32::new(0);
static POSSESS_FAMILIAR_ORIGINAL_CALLS: AtomicU32 = AtomicU32::new(0);
static UNPOSSESS_FAMILIAR_ORIGINAL_CALLS: AtomicU32 = AtomicU32::new(0);
static OBJECT_EVENT_CALLS: AtomicU32 = AtomicU32::new(0);
static OBJECT_ORIGINAL_CALLS: AtomicU32 = AtomicU32::new(0);
static INVENTORY_EVENT_CALLS: AtomicU32 = AtomicU32::new(0);
static INVENTORY_ORIGINAL_CALLS: AtomicU32 = AtomicU32::new(0);
static FEAT_EVENT_CALLS: AtomicU32 = AtomicU32::new(0);
static FEAT_ORIGINAL_CALLS: AtomicU32 = AtomicU32::new(0);
static JOURNAL_EVENT_CALLS: AtomicU32 = AtomicU32::new(0);
static JOURNAL_ORIGINAL_CALLS: AtomicU32 = AtomicU32::new(0);
static TIMING_BAR_EVENT_CALLS: AtomicU32 = AtomicU32::new(0);
static TIMING_BAR_ORIGINAL_CALLS: AtomicU32 = AtomicU32::new(0);
static PROJECTILE_EVENT_CALLS: AtomicU32 = AtomicU32::new(0);
static PROJECTILE_ORIGINAL_CALLS: AtomicU32 = AtomicU32::new(0);
static SKILL_EVENT_CALLS: AtomicU32 = AtomicU32::new(0);
static SKILL_ORIGINAL_CALLS: AtomicU32 = AtomicU32::new(0);
static ITEM_EVENT_CALLS: AtomicU32 = AtomicU32::new(0);
static ITEM_ORIGINAL_CALLS: AtomicU32 = AtomicU32::new(0);

const EVENT_IDENTITIES: &[&str] = &[
    "module_load",
    "associate_add_before",
    "associate_add_after",
    "associate_remove_before",
    "associate_remove_after",
    "associate_possess_familiar_before",
    "associate_possess_familiar_after",
    "associate_unpossess_familiar_before",
    "associate_unpossess_familiar_after",
    "object_lock_before",
    "object_lock_after",
    "object_unlock_before",
    "object_unlock_after",
    "object_use_before",
    "object_use_after",
    "placeable_open_before",
    "placeable_open_after",
    "placeable_close_before",
    "placeable_close_after",
    "inventory_add_gold_before",
    "inventory_add_gold_after",
    "inventory_remove_gold_before",
    "inventory_remove_gold_after",
    "feat_use_before",
    "feat_use_after",
    "journal_open_before",
    "journal_open_after",
    "journal_close_before",
    "journal_close_after",
    "timing_bar_start_before",
    "timing_bar_start_after",
    "timing_bar_stop_before",
    "timing_bar_stop_after",
    "timing_bar_cancel_before",
    "timing_bar_cancel_after",
    "object_broadcast_safe_projectile_before",
    "object_broadcast_safe_projectile_after",
    "skill_use_before",
    "skill_use_after",
    "item_use_before",
    "item_use_after",
    "item_inventory_open_before",
    "item_inventory_open_after",
    "item_inventory_close_before",
    "item_inventory_close_after",
    "item_scroll_learn_before",
    "item_scroll_learn_after",
    "item_use_lore_before",
    "item_use_lore_after",
    "item_pay_to_identify_before",
    "item_pay_to_identify_after",
    "item_destroy_before",
    "item_destroy_after",
    "item_decrement_stack_size_before",
    "item_decrement_stack_size_after",
    "object_set_experience_before",
    "object_set_experience_after",
    "feat_decrement_remaining_uses_before",
    "feat_decrement_remaining_uses_after",
    "feat_has_before",
    "feat_has_after",
    "inventory_open_before",
    "inventory_open_after",
    "inventory_select_panel_before",
    "inventory_select_panel_after",
    "inventory_add_item_before",
    "inventory_add_item_after",
    "inventory_remove_item_before",
    "inventory_remove_item_after",
    "item_validate_use_before",
    "item_validate_use_after",
    "item_ammo_reload_before",
    "item_ammo_reload_after",
    "item_validate_equip_before",
    "item_validate_equip_after",
    "item_equip_before",
    "item_equip_after",
    "item_unequip_before",
    "item_unequip_after",
    "item_split_before",
    "item_split_after",
    "item_merge_before",
    "item_merge_after",
    "item_acquire_before",
    "item_acquire_after",
];

#[repr(C)]
struct FixtureGameObject {
    vtable:    *mut c_void,
    object_id: u32,
    object_type: u8,
    padding: [u8; 3],
}

#[repr(C)]
struct FixturePlayer {
    game_object: *mut c_void,
}

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
#[inline(never)]
pub extern "C" fn nwnrs_fixture_add_associate(
    _creature: *mut c_void,
    _associate: u32,
    _associate_type: u16,
) {
    ADD_ASSOCIATE_ORIGINAL_CALLS.fetch_add(1, Ordering::Relaxed);
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn nwnrs_fixture_remove_associate(_creature: *mut c_void, _associate: u32) {
    REMOVE_ASSOCIATE_ORIGINAL_CALLS.fetch_add(1, Ordering::Relaxed);
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn nwnrs_fixture_get_associate_id(
    _creature: *mut c_void,
    associate_type: u16,
    nth: i32,
) -> u32 {
    assert_eq!(associate_type, 3);
    assert_eq!(nth, 1);
    0x0e0f_1011
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn nwnrs_fixture_possess_familiar(_creature: *mut c_void) {
    POSSESS_FAMILIAR_ORIGINAL_CALLS.fetch_add(1, Ordering::Relaxed);
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn nwnrs_fixture_unpossess_familiar(_creature: *mut c_void) {
    UNPOSSESS_FAMILIAR_ORIGINAL_CALLS.fetch_add(1, Ordering::Relaxed);
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn nwnrs_fixture_object_lock(_object: *mut c_void, _door: u32) -> i32 {
    OBJECT_ORIGINAL_CALLS.fetch_add(1, Ordering::Relaxed);
    11
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn nwnrs_fixture_object_unlock(
    _object: *mut c_void,
    _door: u32,
    _thieves_tool: u32,
    _active_property_index: i32,
) -> i32 {
    OBJECT_ORIGINAL_CALLS.fetch_add(1, Ordering::Relaxed);
    12
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn nwnrs_fixture_object_use(_object: *mut c_void, _used: u32) -> i32 {
    OBJECT_ORIGINAL_CALLS.fetch_add(1, Ordering::Relaxed);
    13
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn nwnrs_fixture_placeable_open(_placeable: *mut c_void, _opener: u32) {
    OBJECT_ORIGINAL_CALLS.fetch_add(1, Ordering::Relaxed);
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn nwnrs_fixture_placeable_close(
    _placeable: *mut c_void,
    _closer: u32,
    _update_player: i32,
) {
    OBJECT_ORIGINAL_CALLS.fetch_add(1, Ordering::Relaxed);
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn nwnrs_fixture_inventory_add_gold(
    _creature: *mut c_void,
    _gold: i32,
    _feedback: i32,
) {
    INVENTORY_ORIGINAL_CALLS.fetch_add(1, Ordering::Relaxed);
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn nwnrs_fixture_inventory_remove_gold(
    _creature: *mut c_void,
    _gold: i32,
    _feedback: i32,
) {
    INVENTORY_ORIGINAL_CALLS.fetch_add(1, Ordering::Relaxed);
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn nwnrs_fixture_feat_use(
    _creature: *mut c_void,
    feat: u16,
    subfeat: u16,
    target: u32,
    area: u32,
    position: *const Vector,
) -> i32 {
    assert_eq!((feat, subfeat, target, area), (42, 7, 0x1112_1314, 0x2122_2324));
    assert!(!position.is_null());
    // SAFETY: the fixture call supplies this live vector synchronously.
    let position = unsafe { &*position };
    assert_eq!((position.x, position.y, position.z), (1.5, 2.5, 3.5));
    FEAT_ORIGINAL_CALLS.fetch_add(1, Ordering::Relaxed);
    14
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn nwnrs_fixture_player_get_game_object(player: *mut c_void) -> *mut c_void {
    assert!(!player.is_null());
    // SAFETY: player points to the live FixturePlayer used by the call.
    unsafe { (*player.cast::<FixturePlayer>()).game_object }
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn nwnrs_fixture_journal_message(
    _message: *mut c_void,
    _player: *mut c_void,
    _minor: u8,
) -> i32 {
    JOURNAL_ORIGINAL_CALLS.fetch_add(1, Ordering::Relaxed);
    21
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn nwnrs_fixture_timing_bar_send(
    _message: *mut c_void,
    _player: *mut c_void,
    _starting: i32,
    _event_id: u8,
    _duration: u32,
) -> i32 {
    TIMING_BAR_ORIGINAL_CALLS.fetch_add(1, Ordering::Relaxed);
    22
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn nwnrs_fixture_timing_bar_cancel(
    _message: *mut c_void,
    _player: *mut c_void,
) -> i32 {
    TIMING_BAR_ORIGINAL_CALLS.fetch_add(1, Ordering::Relaxed);
    23
}

#[unsafe(no_mangle)]
#[inline(never)]
#[allow(clippy::too_many_arguments)]
pub extern "C" fn nwnrs_fixture_object_broadcast_safe_projectile(
    _object: *mut c_void,
    _originator: u32,
    _target: u32,
    _originator_position: Vector,
    _target_position: Vector,
    _delta: u32,
    _projectile_type: u8,
    _spell_id: u32,
    _attack_result: u8,
    _projectile_path_type: u8,
) {
    PROJECTILE_ORIGINAL_CALLS.fetch_add(1, Ordering::Relaxed);
}

#[unsafe(no_mangle)]
#[inline(never)]
#[allow(clippy::too_many_arguments)]
pub extern "C" fn nwnrs_fixture_skill_use(
    _creature: *mut c_void,
    skill: u8,
    subskill: u8,
    target: u32,
    position: Vector,
    area: u32,
    used_item: u32,
    active_property_index: i32,
) -> i32 {
    assert_eq!((skill, subskill), (6, 2));
    assert_eq!((target, area, used_item), (0x1112_1314, 0x2122_2324, 0x3132_3334));
    assert_eq!((position.x, position.y, position.z), (4.5, 5.5, 6.5));
    assert_eq!(active_property_index, 8);
    SKILL_ORIGINAL_CALLS.fetch_add(1, Ordering::Relaxed);
    31
}

#[unsafe(no_mangle)]
#[inline(never)]
#[allow(clippy::too_many_arguments)]
pub extern "C" fn nwnrs_fixture_item_use(
    _creature: *mut c_void,
    _item: u32,
    _active_property_index: u8,
    _sub_property_index: u8,
    _target: u32,
    _position: Vector,
    _area: u32,
    _use_charges: i32,
) -> i32 {
    ITEM_ORIGINAL_CALLS.fetch_add(1, Ordering::Relaxed);
    32
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn nwnrs_fixture_item_inventory_open(_item: *mut c_void, _owner: u32) {
    ITEM_ORIGINAL_CALLS.fetch_add(1, Ordering::Relaxed);
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn nwnrs_fixture_item_inventory_close(
    _item: *mut c_void,
    _owner: u32,
    _update_player: i32,
) {
    ITEM_ORIGINAL_CALLS.fetch_add(1, Ordering::Relaxed);
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn nwnrs_fixture_item_scroll_learn(_creature: *mut c_void, _scroll: u32) -> i32 {
    ITEM_ORIGINAL_CALLS.fetch_add(1, Ordering::Relaxed);
    33
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn nwnrs_fixture_item_use_lore(_creature: *mut c_void, _item: u32) -> i32 {
    ITEM_ORIGINAL_CALLS.fetch_add(1, Ordering::Relaxed);
    34
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn nwnrs_fixture_item_pay_to_identify(
    _creature: *mut c_void,
    _item: u32,
    _store: u32,
) {
    ITEM_ORIGINAL_CALLS.fetch_add(1, Ordering::Relaxed);
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn nwnrs_fixture_item_event_handler(
    _item: *mut c_void,
    _event_id: u32,
    _caller: u32,
    _script: *mut c_void,
    _calendar_day: u32,
    _time_of_day: u32,
) {
    ITEM_ORIGINAL_CALLS.fetch_add(1, Ordering::Relaxed);
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn nwnrs_fixture_set_experience(
    _stats: *mut c_void,
    _experience: u32,
    _do_level: i32,
) {
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn nwnrs_fixture_decrement_feat_remaining_uses(
    _stats: *mut c_void,
    _feat: u16,
) {
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn nwnrs_fixture_has_feat(_stats: *mut c_void, _feat: u16) -> i32 {
    1
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn nwnrs_fixture_get_feat_remaining_uses(
    _stats: *mut c_void,
    _feat: u16,
) -> u8 {
    3
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn nwnrs_fixture_inventory_message(
    _message: *mut c_void,
    _player: *mut c_void,
    _minor: u8,
) -> i32 {
    1
}

#[unsafe(no_mangle)]
#[inline(never)]
#[allow(clippy::too_many_arguments)]
pub extern "C" fn nwnrs_fixture_inventory_add_item(
    _repository: *mut c_void,
    _item: *mut *mut c_void,
    _x: u8,
    _y: u8,
    _allow_encumbrance: i32,
    _merge: i32,
) -> i32 {
    1
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn nwnrs_fixture_inventory_remove_item(
    _repository: *mut c_void,
    _item: *mut c_void,
) -> i32 {
    1
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn nwnrs_fixture_item_validate_use(
    _creature: *mut c_void,
    _item: *mut c_void,
    _ignore_identified: i32,
) -> i32 {
    1
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn nwnrs_fixture_item_ammo_reload(
    _repository: *mut c_void,
    _base_item: u32,
    _nth: i32,
) -> u32 {
    0x7f00_0000
}

#[unsafe(no_mangle)]
#[inline(never)]
#[allow(clippy::too_many_arguments)]
pub extern "C" fn nwnrs_fixture_item_validate_equip(
    _creature: *mut c_void,
    _item: *mut c_void,
    _slot: *mut u32,
    _equipping: i32,
    _loading: i32,
    _feedback: i32,
    _player: *mut c_void,
) -> u8 {
    1
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn nwnrs_fixture_item_equip(
    _creature: *mut c_void,
    _item: u32,
    _slot: u32,
    _feedback_player: u32,
) -> i32 {
    1
}

#[unsafe(no_mangle)]
#[inline(never)]
#[allow(clippy::too_many_arguments)]
pub extern "C" fn nwnrs_fixture_item_unequip(
    _creature: *mut c_void,
    _item: u32,
    _repository: u32,
    _x: u8,
    _y: u8,
    _merge: i32,
    _feedback_player: u32,
) -> i32 {
    1
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn nwnrs_fixture_item_split(
    _creature: *mut c_void,
    _item: *mut c_void,
    _amount: i32,
) {
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn nwnrs_fixture_item_merge(
    _creature: *mut c_void,
    _into: *mut c_void,
    _merged: *mut c_void,
) {
}

#[unsafe(no_mangle)]
#[inline(never)]
#[allow(clippy::too_many_arguments)]
pub extern "C" fn nwnrs_fixture_item_acquire(
    _creature: *mut c_void,
    _item: *mut *mut c_void,
    _possessor: u32,
    _repository: u32,
    _x: u8,
    _y: u8,
    _from_script: i32,
    _feedback: i32,
) -> i32 {
    1
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn nwnrs_fixture_get_game_object(
    _server: *mut c_void,
    _object: u32,
) -> *mut c_void {
    std::ptr::null_mut()
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn nwnrs_fixture_get_client_object(
    _server: *mut c_void,
    _object: u32,
) -> *mut c_void {
    std::ptr::null_mut()
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn nwnrs_fixture_get_nws_message(_server: *mut c_void) -> *mut c_void {
    std::ptr::null_mut()
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn nwnrs_fixture_inventory_status(
    _message: *mut c_void,
    _player: *mut c_void,
    _active: i32,
    _inventory: u32,
) -> i32 {
    1
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn nwnrs_fixture_inventory_gui_set_open(
    _gui: *mut c_void,
    _open: i32,
    _client_directed: i32,
) {
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn nwnrs_fixture_inventory_select_panel(
    _message: *mut c_void,
    _player: u32,
    _panel: u8,
) -> i32 {
    1
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn nwnrs_fixture_inventory_equip_cancel(
    _message: *mut c_void,
    _player: u32,
    _item: u32,
    _slot: u32,
    _non_player: i32,
) -> i32 {
    1
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn nwnrs_fixture_inventory_unequip_cancel(
    _message: *mut c_void,
    _player: u32,
    _item: u32,
    _non_player: i32,
) -> i32 {
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
    if vm.is_null() || script.is_null() || owner_is_valid != 1 || event_id != 0 {
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
    let mut commands = Commands {
        virtual_machine: vm,
    };
    // SAFETY: the fixture runtime owns this VM on the current thread for the
    // complete synchronous RunScript call.
    let vm = unsafe { &mut *vm.cast::<VirtualMachine>() };
    assert_eq!(call_integer(&mut commands, vm, "GetIsInEvent"), 1);
    let current = call_string(&mut commands, vm, "GetCurrentEvent");
    if owner == 0 {
        assert_eq!(
            current,
            concat!(
                "{\"name\":\"module.load\",\"id\":3002,",
                "\"script\":\"_nwnrs_onload\",\"phase\":\"before\",",
                "\"depth\":1,\"target\":\"00000000\",",
                "\"controls\":{\"skippable\":false,\"result\":false},\"data\":{}}"
            )
        );
        if SUBSCRIBE_EVENTS.load(Ordering::Relaxed) {
            for identity in EVENT_IDENTITIES {
                vm.stack.push(Value::String((*identity).to_string()));
                assert_eq!(call(&mut commands, NWNX_PUSH_STRING), 0);
                assert_eq!(call_integer(&mut commands, vm, "GetEventSupported"), 1);
                call_with_string(&mut commands, vm, "SubscribeEvent", identity);
            }
        }
        MODULE_ONLOAD_CALLS.fetch_add(1, Ordering::Relaxed);
    } else {
        assert_eq!(owner, 0x0102_0304);
        let associate_events = [
            concat!(
                "{\"name\":\"associate.add\",\"id\":-1,",
                "\"script\":\"_nwnrs_onload\",\"phase\":\"before\",",
                "\"depth\":1,\"target\":\"01020304\",",
                "\"controls\":{\"skippable\":false,\"result\":false},",
                "\"data\":{\"associate\":\"0a0b0c0d\",\"associate_type\":5}}"
            ),
            concat!(
                "{\"name\":\"associate.add\",\"id\":-1,",
                "\"script\":\"_nwnrs_onload\",\"phase\":\"after\",",
                "\"depth\":1,\"target\":\"01020304\",",
                "\"controls\":{\"skippable\":false,\"result\":false},",
                "\"data\":{\"associate\":\"0a0b0c0d\",\"associate_type\":5}}"
            ),
            concat!(
                "{\"name\":\"associate.remove\",\"id\":-1,",
                "\"script\":\"_nwnrs_onload\",\"phase\":\"before\",",
                "\"depth\":1,\"target\":\"01020304\",",
                "\"controls\":{\"skippable\":false,\"result\":false},",
                "\"data\":{\"associate\":\"0a0b0c0d\"}}"
            ),
            concat!(
                "{\"name\":\"associate.remove\",\"id\":-1,",
                "\"script\":\"_nwnrs_onload\",\"phase\":\"after\",",
                "\"depth\":1,\"target\":\"01020304\",",
                "\"controls\":{\"skippable\":false,\"result\":false},",
                "\"data\":{\"associate\":\"0a0b0c0d\"}}"
            ),
        ];
        let familiar_events = [
            concat!(
                "{\"name\":\"associate.possess_familiar\",\"id\":-1,",
                "\"script\":\"_nwnrs_onload\",\"phase\":\"before\",",
                "\"depth\":1,\"target\":\"01020304\",",
                "\"controls\":{\"skippable\":true,\"result\":false},",
                "\"data\":{\"familiar\":\"0e0f1011\"}}"
            ),
            concat!(
                "{\"name\":\"associate.possess_familiar\",\"id\":-1,",
                "\"script\":\"_nwnrs_onload\",\"phase\":\"after\",",
                "\"depth\":1,\"target\":\"01020304\",",
                "\"controls\":{\"skippable\":false,\"result\":false},",
                "\"data\":{\"familiar\":\"0e0f1011\"}}"
            ),
            concat!(
                "{\"name\":\"associate.unpossess_familiar\",\"id\":-1,",
                "\"script\":\"_nwnrs_onload\",\"phase\":\"before\",",
                "\"depth\":1,\"target\":\"01020304\",",
                "\"controls\":{\"skippable\":true,\"result\":false},",
                "\"data\":{\"familiar\":\"0e0f1011\"}}"
            ),
            concat!(
                "{\"name\":\"associate.unpossess_familiar\",\"id\":-1,",
                "\"script\":\"_nwnrs_onload\",\"phase\":\"after\",",
                "\"depth\":1,\"target\":\"01020304\",",
                "\"controls\":{\"skippable\":false,\"result\":false},",
                "\"data\":{\"familiar\":\"0e0f1011\"}}"
            ),
        ];
        if associate_events.contains(&current.as_str()) {
            ASSOCIATE_EVENT_CALLS.fetch_add(1, Ordering::Relaxed);
        } else if familiar_events.contains(&current.as_str()) {
            FAMILIAR_EVENT_CALLS.fetch_add(1, Ordering::Relaxed);
            if current.contains("associate.unpossess_familiar")
                && current.contains("\"phase\":\"before\"")
            {
                call_without_result(&mut commands, vm, "SkipCurrentEvent");
            }
        } else if (current.contains("\"name\":\"object.")
            && !current.contains("\"name\":\"object.broadcast_safe_projectile\""))
            || current.contains("\"name\":\"placeable.")
        {
            assert!(current.contains("\"target\":\"01020304\""));
            assert!(current.contains("\"depth\":1"));
            if current.contains("\"name\":\"object.lock\"") {
                assert!(current.contains("\"door\":\"11121314\""));
                if current.contains("\"phase\":\"after\"") {
                    assert!(current.contains("\"action_result\":11"));
                }
            } else if current.contains("\"name\":\"object.unlock\"") {
                assert!(current.contains("\"active_property_index\":7"));
                assert!(current.contains("\"door\":\"11121314\""));
                assert!(current.contains("\"thieves_tool\":\"15161718\""));
                if current.contains("\"phase\":\"after\"") {
                    assert!(current.contains("\"action_result\":12"));
                }
            } else if current.contains("\"name\":\"object.use\"") {
                assert!(current.contains("\"object\":\"11121314\""));
                if current.contains("\"phase\":\"before\"") {
                    call_without_result(&mut commands, vm, "SkipCurrentEvent");
                } else {
                    assert!(current.contains("\"action_result\":0"));
                }
            } else if current.contains("\"name\":\"placeable.open\"") {
                assert!(current.contains("\"object\":\"11121314\""));
                if current.contains("\"phase\":\"before\"") {
                    call_without_result(&mut commands, vm, "SkipCurrentEvent");
                } else {
                    assert!(current.contains("\"before_skipped\":true"));
                }
            } else {
                assert!(current.contains("\"name\":\"placeable.close\""));
                assert!(current.contains("\"object\":\"11121314\""));
                assert!(current.contains("\"skippable\":false"));
            }
            OBJECT_EVENT_CALLS.fetch_add(1, Ordering::Relaxed);
        } else if current.contains("\"name\":\"inventory.") {
            assert!(current.contains("\"gold\":500"));
            if current.contains("\"name\":\"inventory.remove_gold\"")
                && current.contains("\"phase\":\"before\"")
            {
                call_without_result(&mut commands, vm, "SkipCurrentEvent");
            }
            INVENTORY_EVENT_CALLS.fetch_add(1, Ordering::Relaxed);
        } else if current.contains("\"name\":\"feat.use\"") {
            assert!(current.contains("\"name\":\"feat.use\""));
            assert!(current.contains("\"area\":\"21222324\""));
            assert!(current.contains("\"feat\":42"));
            assert!(current.contains("\"position\":{\"x\":1.5,\"y\":2.5,\"z\":3.5}"));
            assert!(current.contains("\"subfeat\":7"));
            assert!(current.contains("\"target\":\"11121314\""));
            if current.contains("\"phase\":\"after\"") {
                assert!(current.contains("\"action_result\":14"));
            }
            FEAT_EVENT_CALLS.fetch_add(1, Ordering::Relaxed);
        } else if current.contains("\"name\":\"journal.") {
            assert!(
                current.contains("\"name\":\"journal.open\"")
                    || current.contains("\"name\":\"journal.close\"")
            );
            assert!(current.contains("\"skippable\":false"));
            assert!(current.contains("\"data\":{}"));
            JOURNAL_EVENT_CALLS.fetch_add(1, Ordering::Relaxed);
        } else if current.contains("\"name\":\"timing_bar.") {
            assert!(current.contains("\"name\":\"timing_bar."));
            if current.contains("\"name\":\"timing_bar.start\"") {
                assert!(current.contains("\"duration\":4294967295"));
                assert!(current.contains("\"event_id\":9"));
            } else {
                assert!(current.contains("\"data\":{}"));
            }
            TIMING_BAR_EVENT_CALLS.fetch_add(1, Ordering::Relaxed);
        } else if current.contains("\"name\":\"object.broadcast_safe_projectile\"") {
            assert!(current.contains("\"originator\":\"11121314\""));
            assert!(current.contains("\"target\":\"21222324\""));
            assert!(current.contains("\"originator_position\":{\"x\":1.0,\"y\":2.0,\"z\":3.0}"));
            assert!(current.contains("\"target_position\":{\"x\":4.0,\"y\":5.0,\"z\":6.0}"));
            assert!(current.contains("\"delta\":4294967295"));
            assert!(current.contains("\"projectile_type\":7"));
            assert!(current.contains("\"spell_id\":4294967294"));
            assert!(current.contains("\"attack_result\":8"));
            assert!(current.contains("\"projectile_path_type\":9"));
            if current.contains("\"phase\":\"before\"") {
                call_without_result(&mut commands, vm, "SkipCurrentEvent");
            }
            PROJECTILE_EVENT_CALLS.fetch_add(1, Ordering::Relaxed);
        } else if current.contains("\"name\":\"skill.use\"") {
            assert!(current.contains("\"active_property_index\":8"));
            assert!(current.contains("\"area\":\"21222324\""));
            assert!(current.contains("\"position\":{\"x\":4.5,\"y\":5.5,\"z\":6.5}"));
            assert!(current.contains("\"skill\":6"));
            assert!(current.contains("\"subskill\":2"));
            assert!(current.contains("\"target\":\"11121314\""));
            assert!(current.contains("\"used_item\":\"31323334\""));
            if current.contains("\"phase\":\"after\"") {
                assert!(current.contains("\"action_result\":31"));
            }
            SKILL_EVENT_CALLS.fetch_add(1, Ordering::Relaxed);
        } else {
            assert!(current.contains("\"name\":\"item."));
            if current.contains("\"name\":\"item.use\"") {
                assert!(current.contains("\"item\":\"11121314\""));
                assert!(current.contains("\"target\":\"21222324\""));
                assert!(current.contains("\"active_property_index\":3"));
                assert!(current.contains("\"sub_property_index\":4"));
                assert!(current.contains("\"position\":{\"x\":7.5,\"y\":8.5,\"z\":9.5}"));
                assert!(current.contains("\"area\":\"31323334\""));
                assert!(current.contains("\"use_charges\":true"));
                if current.contains("\"phase\":\"after\"") {
                    assert!(current.contains("\"action_result\":32"));
                }
            } else if current.contains("\"name\":\"item.inventory_") {
                assert!(current.contains("\"owner\":\"11121314\""));
                if current.contains("\"name\":\"item.inventory_open\"")
                    && current.contains("\"phase\":\"before\"")
                {
                    call_without_result(&mut commands, vm, "SkipCurrentEvent");
                }
            } else if current.contains("\"name\":\"item.scroll_learn\"") {
                assert!(current.contains("\"scroll\":\"11121314\""));
                if current.contains("\"phase\":\"after\"") {
                    assert!(current.contains("\"action_result\":33"));
                }
            } else if current.contains("\"name\":\"item.use_lore\"") {
                assert!(current.contains("\"item\":\"11121314\""));
                if current.contains("\"phase\":\"before\"") {
                    call_without_result(&mut commands, vm, "SkipCurrentEvent");
                } else {
                    assert!(current.contains("\"action_result\":0"));
                }
            } else if current.contains("\"name\":\"item.pay_to_identify\"") {
                assert!(current.contains("\"item\":\"11121314\""));
                assert!(current.contains("\"store\":\"21222324\""));
            } else {
                assert!(current.contains("\"name\":\"item.destroy\"")
                    || current.contains("\"name\":\"item.decrement_stack_size\""));
                assert!(current.contains("\"data\":{}"));
                if current.contains("\"name\":\"item.decrement_stack_size\"")
                    && current.contains("\"phase\":\"before\"")
                {
                    call_without_result(&mut commands, vm, "SkipCurrentEvent");
                }
            }
            ITEM_EVENT_CALLS.fetch_add(1, Ordering::Relaxed);
        }
    }
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

fn call_has_capability(
    commands: &mut Commands,
    vm: &mut VirtualMachine,
    capability: &str,
) -> i32 {
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

fn call_with_string_and_integer(
    commands: &mut Commands,
    vm: &mut VirtualMachine,
    function: &str,
    name: &str,
    value: i32,
) {
    vm.stack.push(Value::Integer(value));
    assert_eq!(call(commands, NWNX_PUSH_INTEGER), 0);
    vm.stack.push(Value::String(name.to_string()));
    assert_eq!(call(commands, NWNX_PUSH_STRING), 0);
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

fn empty_exo_string() -> CExoString {
    CExoString {
        string:        std::ptr::null_mut(),
        string_length: 0,
        buffer_length: 0,
    }
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
    assert_eq!(std::mem::offset_of!(FixtureGameObject, object_id), 8);
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
    let mut creature = FixtureGameObject {
        vtable: std::ptr::null_mut(),
        object_id: 0x0102_0304,
        object_type: 5,
        padding: [0; 3],
    };
    nwnrs_fixture_add_associate((&raw mut creature).cast(), 0x0a0b_0c0d, 5);
    nwnrs_fixture_remove_associate((&raw mut creature).cast(), 0x0a0b_0c0d);
    nwnrs_fixture_possess_familiar((&raw mut creature).cast());
    nwnrs_fixture_unpossess_familiar((&raw mut creature).cast());
    assert_eq!(ADD_ASSOCIATE_ORIGINAL_CALLS.load(Ordering::Relaxed), 1);
    assert_eq!(REMOVE_ASSOCIATE_ORIGINAL_CALLS.load(Ordering::Relaxed), 1);
    assert_eq!(ASSOCIATE_EVENT_CALLS.load(Ordering::Relaxed), 4);
    assert_eq!(POSSESS_FAMILIAR_ORIGINAL_CALLS.load(Ordering::Relaxed), 1);
    assert_eq!(UNPOSSESS_FAMILIAR_ORIGINAL_CALLS.load(Ordering::Relaxed), 0);
    assert_eq!(FAMILIAR_EVENT_CALLS.load(Ordering::Relaxed), 4);
    assert_eq!(nwnrs_fixture_object_lock((&raw mut creature).cast(), 0x1112_1314), 11);
    assert_eq!(
        nwnrs_fixture_object_unlock(
            (&raw mut creature).cast(),
            0x1112_1314,
            0x1516_1718,
            7,
        ),
        12
    );
    assert_eq!(nwnrs_fixture_object_use((&raw mut creature).cast(), 0x1112_1314), 0);
    nwnrs_fixture_placeable_open((&raw mut creature).cast(), 0x1112_1314);
    nwnrs_fixture_placeable_close((&raw mut creature).cast(), 0x1112_1314, 1);
    assert_eq!(OBJECT_EVENT_CALLS.load(Ordering::Relaxed), 10);
    assert_eq!(OBJECT_ORIGINAL_CALLS.load(Ordering::Relaxed), 3);
    nwnrs_fixture_inventory_add_gold((&raw mut creature).cast(), 500, 1);
    nwnrs_fixture_inventory_remove_gold((&raw mut creature).cast(), 500, 1);
    assert_eq!(INVENTORY_EVENT_CALLS.load(Ordering::Relaxed), 4);
    assert_eq!(INVENTORY_ORIGINAL_CALLS.load(Ordering::Relaxed), 1);
    let position = Vector {
        x: 1.5,
        y: 2.5,
        z: 3.5,
    };
    assert_eq!(
        nwnrs_fixture_feat_use(
            (&raw mut creature).cast(),
            42,
            7,
            0x1112_1314,
            0x2122_2324,
            &raw const position,
        ),
        14
    );
    assert_eq!(FEAT_EVENT_CALLS.load(Ordering::Relaxed), 2);
    assert_eq!(FEAT_ORIGINAL_CALLS.load(Ordering::Relaxed), 1);
    let mut player = FixturePlayer {
        game_object: (&raw mut creature).cast(),
    };
    let player = (&raw mut player).cast();
    assert_eq!(nwnrs_fixture_journal_message(std::ptr::null_mut(), player, 0x0a), 21);
    assert_eq!(nwnrs_fixture_journal_message(std::ptr::null_mut(), player, 0x0b), 21);
    assert_eq!(JOURNAL_EVENT_CALLS.load(Ordering::Relaxed), 4);
    assert_eq!(JOURNAL_ORIGINAL_CALLS.load(Ordering::Relaxed), 2);
    assert_eq!(
        nwnrs_fixture_timing_bar_send(
            std::ptr::null_mut(),
            player,
            1,
            9,
            u32::MAX,
        ),
        22
    );
    assert_eq!(
        nwnrs_fixture_timing_bar_send(std::ptr::null_mut(), player, 0, 0, 0),
        22
    );
    assert_eq!(
        nwnrs_fixture_timing_bar_cancel(std::ptr::null_mut(), player),
        23
    );
    assert_eq!(TIMING_BAR_EVENT_CALLS.load(Ordering::Relaxed), 6);
    assert_eq!(TIMING_BAR_ORIGINAL_CALLS.load(Ordering::Relaxed), 3);
    nwnrs_fixture_object_broadcast_safe_projectile(
        (&raw mut creature).cast(),
        0x1112_1314,
        0x2122_2324,
        Vector { x: 1.0, y: 2.0, z: 3.0 },
        Vector { x: 4.0, y: 5.0, z: 6.0 },
        u32::MAX,
        7,
        u32::MAX - 1,
        8,
        9,
    );
    assert_eq!(PROJECTILE_EVENT_CALLS.load(Ordering::Relaxed), 2);
    assert_eq!(PROJECTILE_ORIGINAL_CALLS.load(Ordering::Relaxed), 0);
    let mut event_commands = Commands {
        virtual_machine: (&raw mut vm).cast(),
    };
    call_with_string_and_integer(
        &mut event_commands,
        &mut vm,
        "ToggleEventIdWhitelist",
        "object.broadcast_safe_projectile.projectile_type",
        1,
    );
    nwnrs_fixture_object_broadcast_safe_projectile(
        (&raw mut creature).cast(),
        0x1112_1314,
        0x2122_2324,
        Vector { x: 1.0, y: 2.0, z: 3.0 },
        Vector { x: 4.0, y: 5.0, z: 6.0 },
        u32::MAX,
        7,
        u32::MAX - 1,
        8,
        9,
    );
    assert_eq!(PROJECTILE_EVENT_CALLS.load(Ordering::Relaxed), 2);
    assert_eq!(PROJECTILE_ORIGINAL_CALLS.load(Ordering::Relaxed), 1);
    call_with_string_and_integer(
        &mut event_commands,
        &mut vm,
        "AddEventIdToWhitelist",
        "object.broadcast_safe_projectile.projectile_type",
        7,
    );
    call_with_string_and_integer(
        &mut event_commands,
        &mut vm,
        "ToggleEventIdWhitelist",
        "object.broadcast_safe_projectile.spell_id",
        1,
    );
    nwnrs_fixture_object_broadcast_safe_projectile(
        (&raw mut creature).cast(),
        0x1112_1314,
        0x2122_2324,
        Vector { x: 1.0, y: 2.0, z: 3.0 },
        Vector { x: 4.0, y: 5.0, z: 6.0 },
        u32::MAX,
        7,
        u32::MAX - 1,
        8,
        9,
    );
    assert_eq!(PROJECTILE_EVENT_CALLS.load(Ordering::Relaxed), 2);
    assert_eq!(PROJECTILE_ORIGINAL_CALLS.load(Ordering::Relaxed), 2);
    call_with_string_and_integer(
        &mut event_commands,
        &mut vm,
        "AddEventIdToWhitelist",
        "object.broadcast_safe_projectile.spell_id",
        -2,
    );
    nwnrs_fixture_object_broadcast_safe_projectile(
        (&raw mut creature).cast(),
        0x1112_1314,
        0x2122_2324,
        Vector { x: 1.0, y: 2.0, z: 3.0 },
        Vector { x: 4.0, y: 5.0, z: 6.0 },
        u32::MAX,
        7,
        u32::MAX - 1,
        8,
        9,
    );
    assert_eq!(PROJECTILE_EVENT_CALLS.load(Ordering::Relaxed), 4);
    assert_eq!(PROJECTILE_ORIGINAL_CALLS.load(Ordering::Relaxed), 2);
    assert_eq!(
        nwnrs_fixture_skill_use(
            (&raw mut creature).cast(),
            6,
            2,
            0x1112_1314,
            Vector { x: 4.5, y: 5.5, z: 6.5 },
            0x2122_2324,
            0x3132_3334,
            8,
        ),
        31
    );
    assert_eq!(SKILL_EVENT_CALLS.load(Ordering::Relaxed), 2);
    assert_eq!(SKILL_ORIGINAL_CALLS.load(Ordering::Relaxed), 1);
    assert_eq!(
        nwnrs_fixture_item_use(
            (&raw mut creature).cast(),
            0x1112_1314,
            3,
            4,
            0x2122_2324,
            Vector { x: 7.5, y: 8.5, z: 9.5 },
            0x3132_3334,
            1,
        ),
        32
    );
    nwnrs_fixture_item_inventory_open((&raw mut creature).cast(), 0x1112_1314);
    nwnrs_fixture_item_inventory_close((&raw mut creature).cast(), 0x1112_1314, 1);
    assert_eq!(nwnrs_fixture_item_scroll_learn((&raw mut creature).cast(), 0x1112_1314), 33);
    assert_eq!(nwnrs_fixture_item_use_lore((&raw mut creature).cast(), 0x1112_1314), 0);
    nwnrs_fixture_item_pay_to_identify(
        (&raw mut creature).cast(),
        0x1112_1314,
        0x2122_2324,
    );
    nwnrs_fixture_item_event_handler(
        (&raw mut creature).cast(),
        11,
        0x1112_1314,
        std::ptr::null_mut(),
        1,
        2,
    );
    nwnrs_fixture_item_event_handler(
        (&raw mut creature).cast(),
        16,
        0x1112_1314,
        std::ptr::null_mut(),
        1,
        2,
    );
    assert_eq!(ITEM_EVENT_CALLS.load(Ordering::Relaxed), 16);
    assert_eq!(ITEM_ORIGINAL_CALLS.load(Ordering::Relaxed), 5);
    let mut commands = Commands {
        virtual_machine: (&raw mut vm).cast(),
    };

    assert_eq!(call(&mut commands, NWNX_GET_IS_AVAILABLE), 0);
    assert!(matches!(vm.stack.pop(), Some(Value::Integer(1))));

    assert_eq!(call_integer(&mut commands, &mut vm, "GetApiVersion"), 1);
    assert_eq!(call_has_capability(&mut commands, &mut vm, "nwscript_bridge"), 1);
    assert_eq!(call_has_capability(&mut commands, &mut vm, "server_state"), 1);
    assert_eq!(call_has_capability(&mut commands, &mut vm, "administration"), 1);
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
    assert_eq!(call_string(&mut commands, &mut vm, "GetCurrentEvent"), "null");
    SUBSCRIBE_EVENTS.store(false, Ordering::Relaxed);
    assert_eq!(nwnrs_fixture_load_module_finish(std::ptr::null_mut()), 1);
    assert_eq!(MODULE_ONLOAD_CALLS.load(Ordering::Relaxed), 2);
    assert_eq!(
        nwnrs_fixture_object_lock((&raw mut creature).cast(), 0x1112_1314),
        11
    );
    assert_eq!(OBJECT_EVENT_CALLS.load(Ordering::Relaxed), 10);
    assert_eq!(OBJECT_ORIGINAL_CALLS.load(Ordering::Relaxed), 4);
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
    std::hint::black_box(nwnrs_fixture_add_associate as *const ());
    std::hint::black_box(nwnrs_fixture_remove_associate as *const ());
    std::hint::black_box(nwnrs_fixture_get_associate_id as *const ());
    std::hint::black_box(nwnrs_fixture_possess_familiar as *const ());
    std::hint::black_box(nwnrs_fixture_unpossess_familiar as *const ());
    std::hint::black_box(nwnrs_fixture_object_lock as *const ());
    std::hint::black_box(nwnrs_fixture_object_unlock as *const ());
    std::hint::black_box(nwnrs_fixture_object_use as *const ());
    std::hint::black_box(nwnrs_fixture_placeable_open as *const ());
    std::hint::black_box(nwnrs_fixture_placeable_close as *const ());
    std::hint::black_box(nwnrs_fixture_inventory_add_gold as *const ());
    std::hint::black_box(nwnrs_fixture_inventory_remove_gold as *const ());
    std::hint::black_box(nwnrs_fixture_feat_use as *const ());
    std::hint::black_box(nwnrs_fixture_player_get_game_object as *const ());
    std::hint::black_box(nwnrs_fixture_journal_message as *const ());
    std::hint::black_box(nwnrs_fixture_timing_bar_send as *const ());
    std::hint::black_box(nwnrs_fixture_timing_bar_cancel as *const ());
    std::hint::black_box(nwnrs_fixture_set_experience as *const ());
    std::hint::black_box(nwnrs_fixture_decrement_feat_remaining_uses as *const ());
    std::hint::black_box(nwnrs_fixture_has_feat as *const ());
    std::hint::black_box(nwnrs_fixture_get_feat_remaining_uses as *const ());
    std::hint::black_box(nwnrs_fixture_inventory_message as *const ());
    std::hint::black_box(nwnrs_fixture_inventory_add_item as *const ());
    std::hint::black_box(nwnrs_fixture_inventory_remove_item as *const ());
    std::hint::black_box(nwnrs_fixture_item_validate_use as *const ());
    std::hint::black_box(nwnrs_fixture_item_ammo_reload as *const ());
    std::hint::black_box(nwnrs_fixture_item_validate_equip as *const ());
    std::hint::black_box(nwnrs_fixture_item_equip as *const ());
    std::hint::black_box(nwnrs_fixture_item_unequip as *const ());
    std::hint::black_box(nwnrs_fixture_item_split as *const ());
    std::hint::black_box(nwnrs_fixture_item_merge as *const ());
    std::hint::black_box(nwnrs_fixture_item_acquire as *const ());
    std::hint::black_box(nwnrs_fixture_get_game_object as *const ());
    std::hint::black_box(nwnrs_fixture_get_client_object as *const ());
    std::hint::black_box(nwnrs_fixture_get_nws_message as *const ());
    std::hint::black_box(nwnrs_fixture_inventory_status as *const ());
    std::hint::black_box(nwnrs_fixture_inventory_gui_set_open as *const ());
    std::hint::black_box(nwnrs_fixture_inventory_select_panel as *const ());
    std::hint::black_box(nwnrs_fixture_inventory_equip_cancel as *const ());
    std::hint::black_box(nwnrs_fixture_inventory_unequip_cancel as *const ());
    std::hint::black_box(&raw const nwnrs_fixture_app_manager);
    std::hint::black_box(&raw const nwnrs_fixture_virtual_machine);
    std::hint::black_box(&raw const nwnrs_fixture_enable_combat_debugging);
    std::hint::black_box(&raw const nwnrs_fixture_enable_saving_throw_debugging);
    std::hint::black_box(&raw const nwnrs_fixture_enable_movement_speed_debugging);
    std::hint::black_box(&raw const nwnrs_fixture_enable_hit_die_debugging);
    std::hint::black_box(&raw const nwnrs_fixture_exit_program);
    std::hint::black_box(&raw const nwnrs_fixture_rules);
}
