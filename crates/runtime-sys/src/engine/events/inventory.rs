use std::{collections::BTreeMap, ffi::c_void};

use nwnrs_runtime::{EventObjectId, EventValue};

use super::{
    super::{
        Engine, EventSpec,
        abi::{
            InventoryGuiSetOpen, InventorySelectPanel, InventoryStatus, ModifyGold, PlayerMessage,
            RepositoryAddItem, RepositoryRemoveItem,
        },
        active_engine,
        hook::{NativeHookSpec, NativeOriginal},
    },
    dispatch, native,
};

const ADD_GOLD_HOOK: &str = "inventory_add_gold";
const REMOVE_GOLD_HOOK: &str = "inventory_remove_gold";
const MESSAGE_HOOK: &str = "inventory_message";
const ADD_ITEM_HOOK: &str = "inventory_add_item";
const REMOVE_ITEM_HOOK: &str = "inventory_remove_item";
const STATUS_MINOR: u8 = 0x01;
const SELECT_PANEL_MINOR: u8 = 0x02;

static ADD_GOLD_ORIGINAL: NativeOriginal<ModifyGold> = NativeOriginal::new();
static REMOVE_GOLD_ORIGINAL: NativeOriginal<ModifyGold> = NativeOriginal::new();
static MESSAGE_ORIGINAL: NativeOriginal<PlayerMessage> = NativeOriginal::new();
static ADD_ITEM_ORIGINAL: NativeOriginal<RepositoryAddItem> = NativeOriginal::new();
static REMOVE_ITEM_ORIGINAL: NativeOriginal<RepositoryRemoveItem> = NativeOriginal::new();

pub(super) fn append_hook_specs(engine: &Engine, hooks: &mut Vec<NativeHookSpec>) {
    append(
        engine,
        hooks,
        ADD_GOLD_HOOK,
        "CNWSCreature::AddGold events",
        add_gold_replacement as ModifyGold,
        &ADD_GOLD_ORIGINAL,
    );
    append(
        engine,
        hooks,
        REMOVE_GOLD_HOOK,
        "CNWSCreature::RemoveGold events",
        remove_gold_replacement as ModifyGold,
        &REMOVE_GOLD_ORIGINAL,
    );
    append(
        engine,
        hooks,
        MESSAGE_HOOK,
        "CNWSMessage::HandlePlayerToServerGuiInventoryMessage events",
        message_replacement as PlayerMessage,
        &MESSAGE_ORIGINAL,
    );
    append(
        engine,
        hooks,
        ADD_ITEM_HOOK,
        "CItemRepository::AddItem events",
        add_item_replacement as RepositoryAddItem,
        &ADD_ITEM_ORIGINAL,
    );
    append(
        engine,
        hooks,
        REMOVE_ITEM_HOOK,
        "CItemRepository::RemoveItem events",
        remove_item_replacement as RepositoryRemoveItem,
        &REMOVE_ITEM_ORIGINAL,
    );
}

extern "C" fn message_replacement(message: *mut c_void, player: *mut c_void, minor: u8) -> i32 {
    if player.is_null() {
        return call_message(message, player, minor);
    }
    match minor {
        STATUS_MINOR => inventory_open(message, player, minor),
        SELECT_PANEL_MINOR => inventory_select_panel(message, player, minor),
        _ => call_message(message, player, minor),
    }
}

fn inventory_open(message: *mut c_void, player: *mut c_void, minor: u8) -> i32 {
    let Some(reader) = native::message_reader(message) else {
        return call_message(message, player, minor);
    };
    let Some(target) = reader.u32_le(0).map(|target| target & 0x7fff_ffff) else {
        return call_message(message, player, minor);
    };
    let Some(open) = reader.byte(4).map(|flags| flags & 0x10 != 0) else {
        return call_message(message, player, minor);
    };
    if !open {
        return call_message(message, player, minor);
    }
    let data = BTreeMap::from([object_value("target_inventory", target)]);
    let skipped = dispatch::player(
        player,
        EventSpec::catalog("inventory.open", "before"),
        data.clone(),
    )
    .is_some_and(|frame| frame.skipped());
    let result = if skipped {
        cancel_inventory_open(message, player, target);
        0
    } else {
        call_message(message, player, minor)
    };
    dispatch::player(player, EventSpec::catalog("inventory.open", "after"), data);
    result
}

fn inventory_select_panel(message: *mut c_void, player: *mut c_void, minor: u8) -> i32 {
    let Some(reader) = native::message_reader(message) else {
        return call_message(message, player, minor);
    };
    let Some(selected_panel) = reader.byte(0) else {
        return call_message(message, player, minor);
    };
    if !reader.byte(1).is_some_and(|flags| flags & 0x10 != 0) {
        return call_message(message, player, minor);
    }
    let Some(gui) = native::read_field::<*mut c_void>(player, "player_inventory_gui") else {
        return call_message(message, player, minor);
    };
    let Some(current_panel) = native::read_field::<u8>(gui, "inventory_gui_selected_panel") else {
        return call_message(message, player, minor);
    };
    let data = BTreeMap::from([
        (
            "current_panel".to_string(),
            EventValue::Integer(i32::from(current_panel)),
        ),
        (
            "selected_panel".to_string(),
            EventValue::Integer(i32::from(selected_panel)),
        ),
    ]);
    let skipped = dispatch::player(
        player,
        EventSpec::catalog("inventory.select_panel", "before"),
        data.clone(),
    )
    .is_some_and(|frame| frame.skipped());
    let result = if skipped {
        let _cleared = native::clear_read_message(message);
        if let (Some(player_id), Some(target)) = (
            native::read_field::<u32>(player, "player_id"),
            active_engine().and_then(|engine| {
                engine.event_function::<InventorySelectPanel>("inventory_select_panel")
            }),
        ) {
            let _sent = target.get()(message, player_id, current_panel);
        }
        0
    } else {
        call_message(message, player, minor)
    };
    dispatch::player(
        player,
        EventSpec::catalog("inventory.select_panel", "after"),
        data,
    );
    result
}

fn cancel_inventory_open(message: *mut c_void, player: *mut c_void, target: u32) {
    let _cleared = native::clear_read_message(message);
    if let Some(status) = active_engine()
        .and_then(|engine| engine.event_function::<InventoryStatus>("inventory_status"))
    {
        let _sent = status.get()(message, player, 0, target);
    }
    let own = native::read_field::<u32>(player, "player_object_id") == Some(target);
    let layout = if own {
        "player_inventory_gui"
    } else {
        "player_other_inventory_gui"
    };
    if let (Some(gui), Some(set_open)) = (
        native::read_field::<*mut c_void>(player, layout),
        active_engine().and_then(|engine| {
            engine.event_function::<InventoryGuiSetOpen>("inventory_gui_set_open")
        }),
    ) && !gui.is_null()
    {
        set_open.get()(gui, 0, 0);
    }
}

extern "C" fn add_item_replacement(
    repository: *mut c_void,
    item: *mut *mut c_void,
    x: u8,
    y: u8,
    allow_encumbrance: i32,
    merge_item: i32,
) -> i32 {
    let Some(parent) = repository_event_parent(repository) else {
        return call_add_item(repository, item, x, y, allow_encumbrance, merge_item);
    };
    let item_id = item_pointer_id(item);
    let data = BTreeMap::from([object_value("item", item_id)]);
    let skipped = dispatch::object_id(
        parent,
        EventSpec::catalog("inventory.add_item", "before"),
        data.clone(),
    )
    .is_some_and(|frame| frame.skipped());
    let result = if skipped {
        0
    } else {
        call_add_item(repository, item, x, y, allow_encumbrance, merge_item)
    };
    dispatch::object_id(
        parent,
        EventSpec::catalog("inventory.add_item", "after"),
        data,
    );
    result
}

extern "C" fn remove_item_replacement(repository: *mut c_void, item: *mut c_void) -> i32 {
    let Some(parent) = repository_event_parent(repository) else {
        return call_remove_item(repository, item);
    };
    let item_id = game_object_id(item).unwrap_or(native::OBJECT_INVALID);
    let data = BTreeMap::from([object_value("item", item_id)]);
    dispatch::object_id(
        parent,
        EventSpec::catalog("inventory.remove_item", "before"),
        data.clone(),
    );
    let result = call_remove_item(repository, item);
    dispatch::object_id(
        parent,
        EventSpec::catalog("inventory.remove_item", "after"),
        data,
    );
    result
}

fn repository_event_parent(repository: *mut c_void) -> Option<u32> {
    let parent = native::repository_parent(repository)?;
    let object = native::get_game_object(parent)?;
    matches!(
        native::object_type(object),
        Some(native::OBJECT_TYPE_ITEM) | Some(native::OBJECT_TYPE_PLACEABLE)
    )
    .then_some(parent)
}

fn item_pointer_id(item: *mut *mut c_void) -> u32 {
    if item.is_null() {
        return native::OBJECT_INVALID;
    }
    // SAFETY: the hooked ABI supplies a nullable CNWSItem** argument.
    game_object_id(unsafe { item.read() }).unwrap_or(native::OBJECT_INVALID)
}

fn game_object_id(object: *mut c_void) -> Option<u32> {
    if object.is_null() {
        return None;
    }
    let offset = active_engine()?.event_layout_offset("game_object_id")?;
    // SAFETY: the target pack owns CGameObject::m_idSelf's field offset.
    Some(unsafe {
        object
            .cast::<u8>()
            .add(offset)
            .cast::<u32>()
            .read_unaligned()
    })
}

fn call_message(message: *mut c_void, player: *mut c_void, minor: u8) -> i32 {
    let Some(original) = MESSAGE_ORIGINAL.load() else {
        return 0;
    };
    original(message, player, minor)
}

fn call_add_item(
    repository: *mut c_void,
    item: *mut *mut c_void,
    x: u8,
    y: u8,
    allow_encumbrance: i32,
    merge_item: i32,
) -> i32 {
    let Some(original) = ADD_ITEM_ORIGINAL.load() else {
        return 0;
    };
    original(repository, item, x, y, allow_encumbrance, merge_item)
}

fn call_remove_item(repository: *mut c_void, item: *mut c_void) -> i32 {
    let Some(original) = REMOVE_ITEM_ORIGINAL.load() else {
        return 0;
    };
    original(repository, item)
}

fn object_value(name: &str, value: u32) -> (String, EventValue) {
    (
        name.to_string(),
        EventValue::Object(EventObjectId::new(value)),
    )
}

fn append<Function: Copy>(
    engine: &Engine,
    hooks: &mut Vec<NativeHookSpec>,
    key: &'static str,
    name: &'static str,
    replacement: Function,
    original: &'static NativeOriginal<Function>,
) {
    if let Some(target) = engine.event_hook_target(key) {
        hooks.push(NativeHookSpec::typed_event(
            key,
            name,
            target,
            replacement,
            original,
        ));
    }
}

extern "C" fn add_gold_replacement(creature: *mut c_void, gold: i32, feedback: i32) {
    modify_gold(
        creature,
        gold,
        feedback,
        "inventory.add_gold",
        &ADD_GOLD_ORIGINAL,
    );
}

extern "C" fn remove_gold_replacement(creature: *mut c_void, gold: i32, feedback: i32) {
    modify_gold(
        creature,
        gold,
        feedback,
        "inventory.remove_gold",
        &REMOVE_GOLD_ORIGINAL,
    );
}

fn modify_gold(
    creature: *mut c_void,
    gold: i32,
    feedback: i32,
    name: &'static str,
    original: &'static NativeOriginal<ModifyGold>,
) {
    let data = BTreeMap::from([("gold".to_string(), EventValue::Integer(gold))]);
    let skipped = dispatch::game_object(creature, EventSpec::catalog(name, "before"), data.clone())
        .is_some_and(|frame| frame.skipped());
    if !skipped {
        call_original(original, creature, gold, feedback);
    }
    dispatch::game_object(creature, EventSpec::catalog(name, "after"), data);
}

fn call_original(
    original: &NativeOriginal<ModifyGold>,
    creature: *mut c_void,
    gold: i32,
    feedback: i32,
) {
    if let Some(original) = original.load() {
        original(creature, gold, feedback);
    }
}
