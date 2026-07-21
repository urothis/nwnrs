use std::{
    collections::BTreeMap,
    ffi::c_void,
    ptr,
    sync::atomic::{AtomicPtr, Ordering},
};

use nwnrs_runtime::{EventObjectId, EventValue, EventVector};

use super::{
    super::{
        Engine, EventFrame, EventSpec,
        abi::{
            AcquireItem, EngineVector, FindItemWithBaseItemId, InventoryEquipCancel,
            InventoryUnequipCancel, ItemEventHandler, ItemInventoryClose, ItemInventoryOpen,
            ItemObjectAction, MergeItem, PayToIdentifyItem, RunEquip, RunUnequip, SplitItem,
            UseItem, ValidateEquipItem, ValidateUseItem,
        },
        active_engine,
        hook::NativeHookSpec,
    },
    dispatch, native,
};
use crate::write_diagnostic;

const USE_HOOK: &str = "item_use";
const OPEN_HOOK: &str = "item_inventory_open";
const CLOSE_HOOK: &str = "item_inventory_close";
const LEARN_SCROLL_HOOK: &str = "item_scroll_learn";
const USE_LORE_HOOK: &str = "item_use_lore";
const PAY_TO_IDENTIFY_HOOK: &str = "item_pay_to_identify";
const EVENT_HANDLER_HOOK: &str = "item_event_handler";
const VALIDATE_USE_HOOK: &str = "item_validate_use";
const AMMO_RELOAD_HOOK: &str = "item_ammo_reload";
const VALIDATE_EQUIP_HOOK: &str = "item_validate_equip";
const EQUIP_HOOK: &str = "item_equip";
const UNEQUIP_HOOK: &str = "item_unequip";
const SPLIT_HOOK: &str = "item_split";
const MERGE_HOOK: &str = "item_merge";
const ACQUIRE_HOOK: &str = "item_acquire";

static USE_ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static OPEN_ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static CLOSE_ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static LEARN_SCROLL_ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static USE_LORE_ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static PAY_TO_IDENTIFY_ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static EVENT_HANDLER_ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static VALIDATE_USE_ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static AMMO_RELOAD_ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static VALIDATE_EQUIP_ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static EQUIP_ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static UNEQUIP_ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static SPLIT_ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static MERGE_ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static ACQUIRE_ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

pub(super) fn append_hook_specs(engine: &Engine, hooks: &mut Vec<NativeHookSpec>) {
    append(
        engine,
        hooks,
        USE_HOOK,
        "CNWSCreature::UseItem events",
        use_replacement as UseItem as *const () as usize,
        &USE_ORIGINAL,
    );
    append(
        engine,
        hooks,
        OPEN_HOOK,
        "CNWSItem::OpenInventory events",
        open_replacement as ItemInventoryOpen as *const () as usize,
        &OPEN_ORIGINAL,
    );
    append(
        engine,
        hooks,
        CLOSE_HOOK,
        "CNWSItem::CloseInventory events",
        close_replacement as ItemInventoryClose as *const () as usize,
        &CLOSE_ORIGINAL,
    );
    append(
        engine,
        hooks,
        LEARN_SCROLL_HOOK,
        "CNWSCreature::LearnScroll events",
        learn_scroll_replacement as ItemObjectAction as *const () as usize,
        &LEARN_SCROLL_ORIGINAL,
    );
    append(
        engine,
        hooks,
        USE_LORE_HOOK,
        "CNWSCreature::UseLoreOnItem events",
        use_lore_replacement as ItemObjectAction as *const () as usize,
        &USE_LORE_ORIGINAL,
    );
    append(
        engine,
        hooks,
        PAY_TO_IDENTIFY_HOOK,
        "CNWSCreature::PayToIdentifyItem events",
        pay_to_identify_replacement as PayToIdentifyItem as *const () as usize,
        &PAY_TO_IDENTIFY_ORIGINAL,
    );
    append(
        engine,
        hooks,
        EVENT_HANDLER_HOOK,
        "CNWSItem::EventHandler events",
        event_handler_replacement as ItemEventHandler as *const () as usize,
        &EVENT_HANDLER_ORIGINAL,
    );
    append(
        engine,
        hooks,
        VALIDATE_USE_HOOK,
        "CNWSCreature::CanUseItem events",
        validate_use_replacement as ValidateUseItem as *const () as usize,
        &VALIDATE_USE_ORIGINAL,
    );
    append(
        engine,
        hooks,
        AMMO_RELOAD_HOOK,
        "CItemRepository::FindItemWithBaseItemId events",
        ammo_reload_replacement as FindItemWithBaseItemId as *const () as usize,
        &AMMO_RELOAD_ORIGINAL,
    );
    append(
        engine,
        hooks,
        VALIDATE_EQUIP_HOOK,
        "CNWSCreature::CanEquipItem events",
        validate_equip_replacement as ValidateEquipItem as *const () as usize,
        &VALIDATE_EQUIP_ORIGINAL,
    );
    append(
        engine,
        hooks,
        EQUIP_HOOK,
        "CNWSCreature::RunEquip events",
        equip_replacement as RunEquip as *const () as usize,
        &EQUIP_ORIGINAL,
    );
    append(
        engine,
        hooks,
        UNEQUIP_HOOK,
        "CNWSCreature::RunUnequip events",
        unequip_replacement as RunUnequip as *const () as usize,
        &UNEQUIP_ORIGINAL,
    );
    append(
        engine,
        hooks,
        SPLIT_HOOK,
        "CNWSCreature::SplitItem events",
        split_replacement as SplitItem as *const () as usize,
        &SPLIT_ORIGINAL,
    );
    append(
        engine,
        hooks,
        MERGE_HOOK,
        "CNWSCreature::MergeItem events",
        merge_replacement as MergeItem as *const () as usize,
        &MERGE_ORIGINAL,
    );
    append(
        engine,
        hooks,
        ACQUIRE_HOOK,
        "CNWSCreature::AcquireItem events",
        acquire_replacement as AcquireItem as *const () as usize,
        &ACQUIRE_ORIGINAL,
    );
}

fn append(
    engine: &Engine,
    hooks: &mut Vec<NativeHookSpec>,
    key: &str,
    name: &'static str,
    replacement: usize,
    original: &'static AtomicPtr<c_void>,
) {
    if let Some(target) = engine.event_hook_target(key) {
        hooks.push(NativeHookSpec::new(name, target, replacement, original));
    }
}

#[allow(clippy::too_many_arguments)]
extern "C" fn use_replacement(
    creature: *mut c_void,
    item: u32,
    active_property_index: u8,
    sub_property_index: u8,
    target: u32,
    position: EngineVector,
    area: u32,
    use_charges: i32,
) -> i32 {
    let data = BTreeMap::from([
        object_value("item", item),
        object_value("target", target),
        (
            "active_property_index".to_string(),
            EventValue::Integer(i32::from(active_property_index)),
        ),
        (
            "sub_property_index".to_string(),
            EventValue::Integer(i32::from(sub_property_index)),
        ),
        (
            "position".to_string(),
            EventValue::Vector(EventVector {
                x: position.x,
                y: position.y,
                z: position.z,
            }),
        ),
        object_value("area", area),
        (
            "use_charges".to_string(),
            EventValue::Boolean(use_charges != 0),
        ),
    ]);
    let frame = dispatch::game_object(
        creature,
        EventSpec::catalog("item.use", "before"),
        data.clone(),
    );
    let result = if frame.as_ref().is_some_and(EventFrame::skipped) {
        frame
            .as_ref()
            .and_then(EventFrame::result)
            .and_then(|result| serde_json::from_slice::<bool>(result).ok())
            .map_or(0, i32::from)
    } else {
        call_use(
            creature,
            item,
            active_property_index,
            sub_property_index,
            target,
            position,
            area,
            use_charges,
        )
    };
    let mut after = data;
    after.insert("action_result".to_string(), EventValue::Integer(result));
    dispatch::game_object(creature, EventSpec::catalog("item.use", "after"), after);
    result
}

extern "C" fn open_replacement(item: *mut c_void, owner: u32) {
    inventory(item, owner, 0, "item.inventory_open", &OPEN_ORIGINAL, true);
}

extern "C" fn close_replacement(item: *mut c_void, owner: u32, update_player: i32) {
    inventory(
        item,
        owner,
        update_player,
        "item.inventory_close",
        &CLOSE_ORIGINAL,
        false,
    );
}

fn inventory(
    item: *mut c_void,
    owner: u32,
    update_player: i32,
    name: &'static str,
    original: &AtomicPtr<c_void>,
    open: bool,
) {
    let data = BTreeMap::from([object_value("owner", owner)]);
    let skipped = dispatch::game_object(item, EventSpec::catalog(name, "before"), data.clone())
        .is_some_and(|frame| frame.skipped());
    if !skipped {
        let pointer = original.load(Ordering::Acquire);
        if !pointer.is_null() {
            if open {
                // SAFETY: Gum published the CNWSItem::OpenInventory trampoline.
                let original =
                    unsafe { std::mem::transmute::<*mut c_void, ItemInventoryOpen>(pointer) };
                original(item, owner);
            } else {
                // SAFETY: Gum published the CNWSItem::CloseInventory trampoline.
                let original =
                    unsafe { std::mem::transmute::<*mut c_void, ItemInventoryClose>(pointer) };
                original(item, owner, update_player);
            }
        }
    }
    dispatch::game_object(item, EventSpec::catalog(name, "after"), data);
}

extern "C" fn learn_scroll_replacement(creature: *mut c_void, scroll: u32) -> i32 {
    object_action(
        creature,
        scroll,
        "scroll",
        "item.scroll_learn",
        &LEARN_SCROLL_ORIGINAL,
    )
}

extern "C" fn use_lore_replacement(creature: *mut c_void, item: u32) -> i32 {
    object_action(creature, item, "item", "item.use_lore", &USE_LORE_ORIGINAL)
}

fn object_action(
    creature: *mut c_void,
    object: u32,
    key: &str,
    name: &'static str,
    original: &AtomicPtr<c_void>,
) -> i32 {
    let data = BTreeMap::from([object_value(key, object)]);
    let skipped = dispatch::game_object(creature, EventSpec::catalog(name, "before"), data.clone())
        .is_some_and(|frame| frame.skipped());
    let result = if skipped {
        0
    } else {
        call_object_action(original, creature, object)
    };
    let mut after = data;
    after.insert("action_result".to_string(), EventValue::Integer(result));
    dispatch::game_object(creature, EventSpec::catalog(name, "after"), after);
    result
}

extern "C" fn pay_to_identify_replacement(creature: *mut c_void, item: u32, store: u32) {
    let data = BTreeMap::from([object_value("item", item), object_value("store", store)]);
    let skipped = dispatch::game_object(
        creature,
        EventSpec::catalog("item.pay_to_identify", "before"),
        data.clone(),
    )
    .is_some_and(|frame| frame.skipped());
    if !skipped {
        let original = PAY_TO_IDENTIFY_ORIGINAL.load(Ordering::Acquire);
        if !original.is_null() {
            // SAFETY: Gum published the PayToIdentifyItem trampoline.
            let original =
                unsafe { std::mem::transmute::<*mut c_void, PayToIdentifyItem>(original) };
            original(creature, item, store);
        }
    }
    dispatch::game_object(
        creature,
        EventSpec::catalog("item.pay_to_identify", "after"),
        data,
    );
}

extern "C" fn event_handler_replacement(
    item: *mut c_void,
    event_id: u32,
    caller: u32,
    script: *mut c_void,
    calendar_day: u32,
    time_of_day: u32,
) {
    let name = match event_id {
        11 => Some("item.destroy"),
        16 => Some("item.decrement_stack_size"),
        _ => None,
    };
    if let Some(name) = name {
        let data = BTreeMap::new();
        let skipped = dispatch::game_object(item, EventSpec::catalog(name, "before"), data.clone())
            .is_some_and(|frame| frame.skipped());
        if !skipped {
            call_event_handler(item, event_id, caller, script, calendar_day, time_of_day);
        }
        dispatch::game_object(item, EventSpec::catalog(name, "after"), data);
    } else {
        call_event_handler(item, event_id, caller, script, calendar_day, time_of_day);
    }
}

fn object_value(name: &str, value: u32) -> (String, EventValue) {
    (
        name.to_string(),
        EventValue::Object(EventObjectId::new(value)),
    )
}

extern "C" fn validate_use_replacement(
    creature: *mut c_void,
    item: *mut c_void,
    ignore_identified: i32,
) -> i32 {
    let item_id = game_object_id(item).unwrap_or(native::OBJECT_INVALID);
    let data = BTreeMap::from([object_value("item_object_id", item_id)]);
    let before = dispatch::game_object(
        creature,
        EventSpec::catalog("item.validate_use", "before"),
        data.clone(),
    );
    let mut result = if before.as_ref().is_some_and(EventFrame::skipped) {
        i32::from(
            native::parse_boolean_result(before.as_ref().and_then(EventFrame::result))
                .unwrap_or(false),
        )
    } else {
        call_validate_use(creature, item, ignore_identified)
    };
    let mut after_data = data;
    after_data.insert(
        "before_result".to_string(),
        EventValue::Boolean(result != 0),
    );
    let after = dispatch::game_object(
        creature,
        EventSpec::catalog("item.validate_use", "after"),
        after_data,
    );
    if let Some(replacement) =
        native::parse_boolean_result(after.as_ref().and_then(EventFrame::result))
    {
        result = i32::from(replacement);
    }
    result
}

extern "C" fn ammo_reload_replacement(repository: *mut c_void, base_item: u32, nth: i32) -> u32 {
    let Some(parent) = native::repository_parent(repository) else {
        return call_ammo_reload(repository, base_item, nth);
    };
    let Some(parent_object) = native::get_game_object(parent) else {
        return call_ammo_reload(repository, base_item, nth);
    };
    if native::object_type(parent_object) != Some(native::OBJECT_TYPE_CREATURE) {
        return call_ammo_reload(repository, base_item, nth);
    }
    if nth > 255 {
        return native::OBJECT_INVALID;
    }
    let data = BTreeMap::from([
        ("base_item_id".to_string(), EventValue::Unsigned(base_item)),
        ("base_item_nth".to_string(), EventValue::Integer(nth)),
    ]);
    let before = dispatch::object_id(
        parent,
        EventSpec::catalog("item.ammo_reload", "before"),
        data.clone(),
    );
    if before.as_ref().is_some_and(EventFrame::skipped)
        && let Some(replacement) =
            native::parse_object_result(before.as_ref().and_then(EventFrame::result))
    {
        if ammo_result_is_valid(replacement, base_item, parent) {
            return replacement;
        }
        write_diagnostic("item.ammo_reload before returned an invalid item; using original");
    }
    let result = call_ammo_reload(repository, base_item, nth);
    let mut after_data = data;
    after_data.insert("action_result".to_string(), object_value_only(result));
    let after = dispatch::object_id(
        parent,
        EventSpec::catalog("item.ammo_reload", "after"),
        after_data,
    );
    if let Some(replacement) =
        native::parse_object_result(after.as_ref().and_then(EventFrame::result))
    {
        if ammo_result_is_valid(replacement, base_item, parent) {
            return replacement;
        }
        write_diagnostic("item.ammo_reload after returned an invalid item; using engine result");
    }
    result
}

extern "C" fn validate_equip_replacement(
    creature: *mut c_void,
    item: *mut c_void,
    slot: *mut u32,
    equipping: i32,
    loading: i32,
    display_feedback: i32,
    feedback_player: *mut c_void,
) -> u8 {
    let item_id = game_object_id(item).unwrap_or(native::OBJECT_INVALID);
    let slot_id = if slot.is_null() {
        0
    } else {
        // SAFETY: the hooked ABI supplies a live uint32_t slot pointer.
        unsafe { slot.read().trailing_zeros() }
    };
    let data = BTreeMap::from([
        object_value("item_object_id", item_id),
        ("slot".to_string(), EventValue::Unsigned(slot_id)),
    ]);
    let before = dispatch::game_object(
        creature,
        EventSpec::catalog("item.validate_equip", "before"),
        data.clone(),
    );
    let mut result = if before.as_ref().is_some_and(EventFrame::skipped) {
        u8::from(
            native::parse_boolean_result(before.as_ref().and_then(EventFrame::result))
                .unwrap_or(false),
        )
    } else {
        call_validate_equip(
            creature,
            item,
            slot,
            equipping,
            loading,
            display_feedback,
            feedback_player,
        )
    };
    let mut after_data = data;
    after_data.insert(
        "before_result".to_string(),
        EventValue::Boolean(result != 0),
    );
    let after = dispatch::game_object(
        creature,
        EventSpec::catalog("item.validate_equip", "after"),
        after_data,
    );
    if let Some(replacement) =
        native::parse_boolean_result(after.as_ref().and_then(EventFrame::result))
    {
        result = u8::from(replacement);
    }
    result
}

extern "C" fn equip_replacement(
    creature: *mut c_void,
    item: u32,
    slot: u32,
    feedback_player: u32,
) -> i32 {
    let data = BTreeMap::from([
        object_value("item", item),
        (
            "slot".to_string(),
            EventValue::Unsigned(slot.trailing_zeros()),
        ),
    ]);
    let skipped = dispatch::game_object(
        creature,
        EventSpec::catalog("item.equip", "before"),
        data.clone(),
    )
    .is_some_and(|frame| frame.skipped());
    let result = if skipped {
        send_equipment_cancel(creature, item, slot, feedback_player, true);
        0
    } else {
        call_equip(creature, item, slot, feedback_player)
    };
    let mut after_data = data;
    after_data.insert("action_result".to_string(), EventValue::Integer(result));
    dispatch::game_object(
        creature,
        EventSpec::catalog("item.equip", "after"),
        after_data,
    );
    result
}

#[allow(clippy::too_many_arguments)]
extern "C" fn unequip_replacement(
    creature: *mut c_void,
    item: u32,
    target_repository: u32,
    x: u8,
    y: u8,
    merge: i32,
    feedback_player: u32,
) -> i32 {
    let data = BTreeMap::from([object_value("item", item)]);
    let skipped = dispatch::game_object(
        creature,
        EventSpec::catalog("item.unequip", "before"),
        data.clone(),
    )
    .is_some_and(|frame| frame.skipped());
    let result = if skipped {
        send_equipment_cancel(creature, item, 0, feedback_player, false);
        0
    } else {
        call_unequip(
            creature,
            item,
            target_repository,
            x,
            y,
            merge,
            feedback_player,
        )
    };
    let mut after_data = data;
    after_data.insert("action_result".to_string(), EventValue::Integer(result));
    dispatch::game_object(
        creature,
        EventSpec::catalog("item.unequip", "after"),
        after_data,
    );
    result
}

extern "C" fn split_replacement(creature: *mut c_void, item: *mut c_void, amount: i32) {
    let item_id = game_object_id(item).unwrap_or(native::OBJECT_INVALID);
    let data = BTreeMap::from([
        object_value("item", item_id),
        ("number_split_off".to_string(), EventValue::Integer(amount)),
    ]);
    let skipped = dispatch::game_object(
        creature,
        EventSpec::catalog("item.split", "before"),
        data.clone(),
    )
    .is_some_and(|frame| frame.skipped());
    if !skipped {
        call_split(creature, item, amount);
    }
    dispatch::game_object(creature, EventSpec::catalog("item.split", "after"), data);
}

extern "C" fn merge_replacement(creature: *mut c_void, into: *mut c_void, merged: *mut c_void) {
    let into_id = game_object_id(into).unwrap_or(native::OBJECT_INVALID);
    let merged_id = game_object_id(merged).unwrap_or(native::OBJECT_INVALID);
    let before_data = BTreeMap::from([
        object_value("item_to_merge_into", into_id),
        object_value("item_to_merge", merged_id),
    ]);
    let skipped = dispatch::game_object(
        creature,
        EventSpec::catalog("item.merge", "before"),
        before_data,
    )
    .is_some_and(|frame| frame.skipped());
    if !skipped {
        call_merge(creature, into, merged);
    }
    let surviving_merged = if native::get_game_object(merged_id).is_some() {
        merged_id
    } else {
        native::OBJECT_INVALID
    };
    dispatch::game_object(
        creature,
        EventSpec::catalog("item.merge", "after"),
        BTreeMap::from([
            object_value("item_to_merge_into", into_id),
            object_value("item_to_merge", surviving_merged),
        ]),
    );
}

#[allow(clippy::too_many_arguments)]
extern "C" fn acquire_replacement(
    creature: *mut c_void,
    item: *mut *mut c_void,
    possessor: u32,
    target_repository: u32,
    x: u8,
    y: u8,
    from_script: i32,
    display_feedback: i32,
) -> i32 {
    if item.is_null() {
        return call_acquire(
            creature,
            item,
            possessor,
            target_repository,
            x,
            y,
            from_script,
            display_feedback,
        );
    }
    // SAFETY: the hooked ABI supplies a nullable CNWSItem** argument.
    if unsafe { item.read() }.is_null() {
        return call_acquire(
            creature,
            item,
            possessor,
            target_repository,
            x,
            y,
            from_script,
            display_feedback,
        );
    }
    let data = BTreeMap::from([
        object_value("item", indirect_item_id(item)),
        object_value("giver", possessor),
        ("result".to_string(), EventValue::Integer(0)),
    ]);
    let skipped =
        dispatch::game_object(creature, EventSpec::catalog("item.acquire", "before"), data)
            .is_some_and(|frame| frame.skipped());
    let result = if skipped {
        0
    } else {
        call_acquire(
            creature,
            item,
            possessor,
            target_repository,
            x,
            y,
            from_script,
            display_feedback,
        )
    };
    dispatch::game_object(
        creature,
        EventSpec::catalog("item.acquire", "after"),
        BTreeMap::from([
            object_value("item", indirect_item_id(item)),
            object_value("giver", possessor),
            ("result".to_string(), EventValue::Integer(result)),
        ]),
    );
    result
}

fn game_object_id(object: *mut c_void) -> Option<u32> {
    native::read_field(object, "game_object_id")
}

fn indirect_item_id(item: *mut *mut c_void) -> u32 {
    if item.is_null() {
        return native::OBJECT_INVALID;
    }
    // SAFETY: the hooked ABI supplies a nullable CNWSItem** argument.
    game_object_id(unsafe { item.read() }).unwrap_or(native::OBJECT_INVALID)
}

fn object_value_only(value: u32) -> EventValue {
    EventValue::Object(EventObjectId::new(value))
}

fn ammo_result_is_valid(item: u32, base_item: u32, owner: u32) -> bool {
    if item == native::OBJECT_INVALID {
        return true;
    }
    let Some(object) = native::get_game_object(item) else {
        return false;
    };
    if native::object_type(object) != Some(native::OBJECT_TYPE_ITEM)
        || native::read_field::<u32>(object, "item_base_item") != Some(base_item)
    {
        return false;
    }
    let Some(possessor) = native::read_field::<u32>(object, "item_possessor") else {
        return false;
    };
    if possessor == owner {
        return true;
    }
    let Some(container) = native::get_game_object(possessor) else {
        return false;
    };
    native::object_type(container) == Some(native::OBJECT_TYPE_ITEM)
        && native::read_field::<u32>(container, "item_possessor") == Some(owner)
}

fn send_equipment_cancel(
    creature: *mut c_void,
    item: u32,
    slot: u32,
    feedback_player: u32,
    equip: bool,
) {
    let creature_id = game_object_id(creature).unwrap_or(native::OBJECT_INVALID);
    let player_object = if feedback_player == native::OBJECT_INVALID {
        creature_id
    } else {
        feedback_player
    };
    let Some(player) = native::get_client_object(player_object) else {
        return;
    };
    let Some(message) = native::get_nws_message() else {
        return;
    };
    let Some(player_id) = native::read_field::<u32>(player, "player_id") else {
        return;
    };
    let non_player = i32::from(feedback_player != native::OBJECT_INVALID);
    if equip {
        if let Some(target) = active_engine()
            .and_then(|engine| engine.event_function_target("inventory_equip_cancel"))
        {
            // SAFETY: the target pack binds this helper to the equip-cancel sender.
            let function = unsafe { std::mem::transmute::<usize, InventoryEquipCancel>(target) };
            let _sent = function(message, player_id, item, slot, non_player);
        }
    } else if let Some(target) =
        active_engine().and_then(|engine| engine.event_function_target("inventory_unequip_cancel"))
    {
        // SAFETY: the target pack binds this helper to the unequip-cancel sender.
        let function = unsafe { std::mem::transmute::<usize, InventoryUnequipCancel>(target) };
        let _sent = function(message, player_id, item, non_player);
    }
}

fn call_validate_use(creature: *mut c_void, item: *mut c_void, ignore: i32) -> i32 {
    let original = VALIDATE_USE_ORIGINAL.load(Ordering::Acquire);
    if original.is_null() {
        return 0;
    }
    // SAFETY: Gum published the CNWSCreature::CanUseItem trampoline.
    let original = unsafe { std::mem::transmute::<*mut c_void, ValidateUseItem>(original) };
    original(creature, item, ignore)
}

fn call_ammo_reload(repository: *mut c_void, base_item: u32, nth: i32) -> u32 {
    let original = AMMO_RELOAD_ORIGINAL.load(Ordering::Acquire);
    if original.is_null() {
        return native::OBJECT_INVALID;
    }
    // SAFETY: Gum published the CItemRepository::FindItemWithBaseItemId trampoline.
    let original = unsafe { std::mem::transmute::<*mut c_void, FindItemWithBaseItemId>(original) };
    original(repository, base_item, nth)
}

#[allow(clippy::too_many_arguments)]
fn call_validate_equip(
    creature: *mut c_void,
    item: *mut c_void,
    slot: *mut u32,
    equipping: i32,
    loading: i32,
    display_feedback: i32,
    feedback_player: *mut c_void,
) -> u8 {
    let original = VALIDATE_EQUIP_ORIGINAL.load(Ordering::Acquire);
    if original.is_null() {
        return 0;
    }
    // SAFETY: Gum published the CNWSCreature::CanEquipItem trampoline.
    let original = unsafe { std::mem::transmute::<*mut c_void, ValidateEquipItem>(original) };
    original(
        creature,
        item,
        slot,
        equipping,
        loading,
        display_feedback,
        feedback_player,
    )
}

fn call_equip(creature: *mut c_void, item: u32, slot: u32, feedback: u32) -> i32 {
    let original = EQUIP_ORIGINAL.load(Ordering::Acquire);
    if original.is_null() {
        return 0;
    }
    // SAFETY: Gum published the CNWSCreature::RunEquip trampoline.
    let original = unsafe { std::mem::transmute::<*mut c_void, RunEquip>(original) };
    original(creature, item, slot, feedback)
}

#[allow(clippy::too_many_arguments)]
fn call_unequip(
    creature: *mut c_void,
    item: u32,
    target_repository: u32,
    x: u8,
    y: u8,
    merge: i32,
    feedback: u32,
) -> i32 {
    let original = UNEQUIP_ORIGINAL.load(Ordering::Acquire);
    if original.is_null() {
        return 0;
    }
    // SAFETY: Gum published the CNWSCreature::RunUnequip trampoline.
    let original = unsafe { std::mem::transmute::<*mut c_void, RunUnequip>(original) };
    original(creature, item, target_repository, x, y, merge, feedback)
}

fn call_split(creature: *mut c_void, item: *mut c_void, amount: i32) {
    let original = SPLIT_ORIGINAL.load(Ordering::Acquire);
    if original.is_null() {
        return;
    }
    // SAFETY: Gum published the CNWSCreature::SplitItem trampoline.
    let original = unsafe { std::mem::transmute::<*mut c_void, SplitItem>(original) };
    original(creature, item, amount);
}

fn call_merge(creature: *mut c_void, into: *mut c_void, merged: *mut c_void) {
    let original = MERGE_ORIGINAL.load(Ordering::Acquire);
    if original.is_null() {
        return;
    }
    // SAFETY: Gum published the CNWSCreature::MergeItem trampoline.
    let original = unsafe { std::mem::transmute::<*mut c_void, MergeItem>(original) };
    original(creature, into, merged);
}

#[allow(clippy::too_many_arguments)]
fn call_acquire(
    creature: *mut c_void,
    item: *mut *mut c_void,
    possessor: u32,
    target_repository: u32,
    x: u8,
    y: u8,
    from_script: i32,
    display_feedback: i32,
) -> i32 {
    let original = ACQUIRE_ORIGINAL.load(Ordering::Acquire);
    if original.is_null() {
        return 0;
    }
    // SAFETY: Gum published the CNWSCreature::AcquireItem trampoline.
    let original = unsafe { std::mem::transmute::<*mut c_void, AcquireItem>(original) };
    original(
        creature,
        item,
        possessor,
        target_repository,
        x,
        y,
        from_script,
        display_feedback,
    )
}

#[allow(clippy::too_many_arguments)]
fn call_use(
    creature: *mut c_void,
    item: u32,
    active: u8,
    sub: u8,
    target: u32,
    position: EngineVector,
    area: u32,
    charges: i32,
) -> i32 {
    let original = USE_ORIGINAL.load(Ordering::Acquire);
    if original.is_null() {
        return 0;
    }
    // SAFETY: Gum published the UseItem trampoline with this exact ABI.
    let original = unsafe { std::mem::transmute::<*mut c_void, UseItem>(original) };
    original(creature, item, active, sub, target, position, area, charges)
}

fn call_object_action(original: &AtomicPtr<c_void>, creature: *mut c_void, object: u32) -> i32 {
    let original = original.load(Ordering::Acquire);
    if original.is_null() {
        return 0;
    }
    // SAFETY: Gum published the matching one-object trampoline.
    let original = unsafe { std::mem::transmute::<*mut c_void, ItemObjectAction>(original) };
    original(creature, object)
}

fn call_event_handler(
    item: *mut c_void,
    event_id: u32,
    caller: u32,
    script: *mut c_void,
    calendar_day: u32,
    time_of_day: u32,
) {
    let original = EVENT_HANDLER_ORIGINAL.load(Ordering::Acquire);
    if original.is_null() {
        return;
    }
    // SAFETY: Gum published the CNWSItem::EventHandler trampoline.
    let original = unsafe { std::mem::transmute::<*mut c_void, ItemEventHandler>(original) };
    original(item, event_id, caller, script, calendar_day, time_of_day);
}
