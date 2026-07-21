use std::{
    collections::BTreeMap,
    ffi::c_void,
    ptr,
    sync::atomic::{AtomicPtr, Ordering},
};

use nwnrs_runtime::{EventObjectId, EventValue, EventVector};

use super::{
    super::{
        Engine, EventSpec,
        abi::{
            EngineVector, ItemEventHandler, ItemInventoryClose, ItemInventoryOpen,
            ItemObjectAction, PayToIdentifyItem, UseItem,
        },
        hook::NativeHookSpec,
    },
    dispatch,
};

const USE_HOOK: &str = "item_use";
const OPEN_HOOK: &str = "item_inventory_open";
const CLOSE_HOOK: &str = "item_inventory_close";
const LEARN_SCROLL_HOOK: &str = "item_scroll_learn";
const USE_LORE_HOOK: &str = "item_use_lore";
const PAY_TO_IDENTIFY_HOOK: &str = "item_pay_to_identify";
const EVENT_HANDLER_HOOK: &str = "item_event_handler";

static USE_ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static OPEN_ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static CLOSE_ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static LEARN_SCROLL_ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static USE_LORE_ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static PAY_TO_IDENTIFY_ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static EVENT_HANDLER_ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

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
    let skipped = dispatch::game_object(
        creature,
        EventSpec::skippable("item.use", "before"),
        data.clone(),
    )
    .is_some_and(|frame| frame.skipped());
    let result = if skipped {
        0
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
    dispatch::game_object(creature, EventSpec::read_only("item.use", "after"), after);
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
    let skipped = dispatch::game_object(item, EventSpec::skippable(name, "before"), data.clone())
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
    dispatch::game_object(item, EventSpec::read_only(name, "after"), data);
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
    let skipped =
        dispatch::game_object(creature, EventSpec::skippable(name, "before"), data.clone())
            .is_some_and(|frame| frame.skipped());
    let result = if skipped {
        0
    } else {
        call_object_action(original, creature, object)
    };
    let mut after = data;
    after.insert("action_result".to_string(), EventValue::Integer(result));
    dispatch::game_object(creature, EventSpec::read_only(name, "after"), after);
    result
}

extern "C" fn pay_to_identify_replacement(creature: *mut c_void, item: u32, store: u32) {
    let data = BTreeMap::from([object_value("item", item), object_value("store", store)]);
    let skipped = dispatch::game_object(
        creature,
        EventSpec::skippable("item.pay_to_identify", "before"),
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
        EventSpec::read_only("item.pay_to_identify", "after"),
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
        let skipped =
            dispatch::game_object(item, EventSpec::skippable(name, "before"), data.clone())
                .is_some_and(|frame| frame.skipped());
        if !skipped {
            call_event_handler(item, event_id, caller, script, calendar_day, time_of_day);
        }
        dispatch::game_object(item, EventSpec::read_only(name, "after"), data);
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
