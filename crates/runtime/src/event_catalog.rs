//! Authoritative in-progress event catalog shared by the runtime and nwpkg.

/// Projectile-type filter used by `object.broadcast_safe_projectile`.
pub const PROJECTILE_TYPE_ID_WHITELIST: &str = "object.broadcast_safe_projectile.projectile_type";
/// Spell-ID filter used by `object.broadcast_safe_projectile`.
pub const PROJECTILE_SPELL_ID_WHITELIST: &str = "object.broadcast_safe_projectile.spell_id";
/// Feat-ID whitelist required by the high-frequency `feat.has` hook.
pub const FEAT_HAS_ID_WHITELIST: &str = "feat.has.feat_id";

/// The JSON type accepted by an event result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventResultKind {
    /// The event does not accept a replacement result.
    None,
    /// The event accepts a JSON boolean result.
    Boolean,
    /// The event accepts a JSON unsigned 32-bit integer result.
    Unsigned,
    /// The event accepts an eight-digit hexadecimal JSON object-ID string.
    ObjectId,
}

/// Additional target-pack data required by one event implementation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EventRequirements {
    /// Native helper functions required in addition to the hook.
    pub functions:    &'static [&'static str],
    /// Optional platform layouts which must be present.
    pub layouts:      &'static [&'static str],
    /// Whether live `CServerExoApp` access is required.
    pub server_state: bool,
}

const NO_REQUIREMENTS: EventRequirements = EventRequirements {
    functions:    &[],
    layouts:      &[],
    server_state: false,
};

/// One supported NWScript event annotation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EventDefinition {
    /// Identifier used by `#[nwnrs::events(...)]`.
    pub identity:            &'static str,
    /// Logical runtime event name.
    pub name:                &'static str,
    /// Runtime phase (`before` or `after`).
    pub phase:               &'static str,
    /// Physical hook required by the event.
    pub hook:                &'static str,
    /// Optional native helper required to construct the payload.
    pub helper:              Option<&'static str>,
    /// Whether the before-event handler may suppress the original operation.
    pub skippable:           bool,
    /// JSON result type accepted by this event.
    pub result_kind:         EventResultKind,
    /// Additional target data needed to safely implement the event.
    pub requirements:        EventRequirements,
    /// ID whitelist automatically enabled when this event is subscribed.
    pub forced_id_whitelist: Option<&'static str>,
}

macro_rules! event {
    ($identity:literal, $name:literal, $phase:literal, $hook:literal) => {
        EventDefinition {
            identity:            $identity,
            name:                $name,
            phase:               $phase,
            hook:                $hook,
            helper:              None,
            skippable:           false,
            result_kind:         EventResultKind::None,
            requirements:        NO_REQUIREMENTS,
            forced_id_whitelist: None,
        }
    };
    ($identity:literal, $name:literal, $phase:literal, $hook:literal,helper = $helper:literal) => {
        EventDefinition {
            identity:            $identity,
            name:                $name,
            phase:               $phase,
            hook:                $hook,
            helper:              Some($helper),
            skippable:           false,
            result_kind:         EventResultKind::None,
            requirements:        NO_REQUIREMENTS,
            forced_id_whitelist: None,
        }
    };
    ($identity:literal, $name:literal, $phase:literal, $hook:literal,result = $result:ident) => {
        EventDefinition {
            identity:            $identity,
            name:                $name,
            phase:               $phase,
            hook:                $hook,
            helper:              None,
            skippable:           false,
            result_kind:         EventResultKind::$result,
            requirements:        NO_REQUIREMENTS,
            forced_id_whitelist: None,
        }
    };
}

macro_rules! requiring {
    ($definition:expr, $requirements:expr) => {{
        let mut definition = $definition;
        definition.requirements = $requirements;
        definition
    }};
}

macro_rules! forcing_whitelist {
    ($definition:expr, $whitelist:expr) => {{
        let mut definition = $definition;
        definition.forced_id_whitelist = Some($whitelist);
        definition
    }};
}

const STATS_REQUIREMENTS: EventRequirements = EventRequirements {
    functions:    &[],
    layouts:      &["creature_stats_base_creature"],
    server_state: false,
};
const EXPERIENCE_REQUIREMENTS: EventRequirements = EventRequirements {
    functions:    &[],
    layouts:      &["creature_stats_base_creature", "creature_stats_experience"],
    server_state: false,
};
const FEAT_USES_REQUIREMENTS: EventRequirements = EventRequirements {
    functions:    &["stats_get_feat_remaining_uses"],
    layouts:      &["creature_stats_base_creature"],
    server_state: false,
};
const REPOSITORY_REQUIREMENTS: EventRequirements = EventRequirements {
    functions:    &["server_get_game_object"],
    layouts:      &["item_repository_parent", "game_object_type"],
    server_state: true,
};
const INVENTORY_MESSAGE_REQUIREMENTS: EventRequirements = EventRequirements {
    functions:    &[
        "inventory_status",
        "inventory_gui_set_open",
        "inventory_select_panel",
    ],
    layouts:      &[
        "message_read_buffer",
        "message_read_buffer_size",
        "message_read_buffer_position",
        "message_read_fragments_size",
        "message_read_fragments_position",
        "message_current_read_bit",
        "message_last_byte_bits",
        "player_object_id",
        "player_inventory_gui",
        "player_other_inventory_gui",
        "inventory_gui_selected_panel",
    ],
    server_state: false,
};
const AMMO_REQUIREMENTS: EventRequirements = EventRequirements {
    functions:    &["server_get_game_object"],
    layouts:      &[
        "item_repository_parent",
        "game_object_type",
        "item_base_item",
        "item_possessor",
    ],
    server_state: true,
};
const EQUIPMENT_REQUIREMENTS: EventRequirements = EventRequirements {
    functions:    &[
        "server_get_client_object",
        "server_get_nws_message",
        "inventory_equip_cancel",
        "inventory_unequip_cancel",
    ],
    layouts:      &[],
    server_state: true,
};
const OBJECT_LOOKUP_REQUIREMENTS: EventRequirements = EventRequirements {
    functions:    &["server_get_game_object"],
    layouts:      &[],
    server_state: true,
};

macro_rules! skippable_event {
    ($identity:literal, $name:literal, $phase:literal, $hook:literal) => {{
        let mut definition = event!($identity, $name, $phase, $hook);
        definition.skippable = true;
        definition
    }};
    ($identity:literal, $name:literal, $phase:literal, $hook:literal,helper = $helper:literal) => {{
        let mut definition = event!($identity, $name, $phase, $hook, helper = $helper);
        definition.skippable = true;
        definition
    }};
    ($identity:literal, $name:literal, $phase:literal, $hook:literal,result = $result:ident) => {{
        let mut definition = event!($identity, $name, $phase, $hook, result = $result);
        definition.skippable = true;
        definition
    }};
}

/// Complete event catalog currently implemented by the runtime.
pub const EVENT_CATALOG: &[EventDefinition] = &[
    event!("module_load", "module.load", "before", "module_load"),
    event!(
        "associate_add_before",
        "associate.add",
        "before",
        "associate_add"
    ),
    event!(
        "associate_add_after",
        "associate.add",
        "after",
        "associate_add"
    ),
    event!(
        "associate_remove_before",
        "associate.remove",
        "before",
        "associate_remove"
    ),
    event!(
        "associate_remove_after",
        "associate.remove",
        "after",
        "associate_remove"
    ),
    skippable_event!(
        "associate_possess_familiar_before",
        "associate.possess_familiar",
        "before",
        "associate_possess_familiar",
        helper = "associate_get_id"
    ),
    event!(
        "associate_possess_familiar_after",
        "associate.possess_familiar",
        "after",
        "associate_possess_familiar",
        helper = "associate_get_id"
    ),
    skippable_event!(
        "associate_unpossess_familiar_before",
        "associate.unpossess_familiar",
        "before",
        "associate_unpossess_familiar",
        helper = "associate_get_id"
    ),
    event!(
        "associate_unpossess_familiar_after",
        "associate.unpossess_familiar",
        "after",
        "associate_unpossess_familiar",
        helper = "associate_get_id"
    ),
    skippable_event!("object_lock_before", "object.lock", "before", "object_lock"),
    event!("object_lock_after", "object.lock", "after", "object_lock"),
    skippable_event!(
        "object_unlock_before",
        "object.unlock",
        "before",
        "object_unlock"
    ),
    event!(
        "object_unlock_after",
        "object.unlock",
        "after",
        "object_unlock"
    ),
    skippable_event!("object_use_before", "object.use", "before", "object_use"),
    event!("object_use_after", "object.use", "after", "object_use"),
    skippable_event!(
        "placeable_open_before",
        "placeable.open",
        "before",
        "placeable_open"
    ),
    event!(
        "placeable_open_after",
        "placeable.open",
        "after",
        "placeable_open"
    ),
    event!(
        "placeable_close_before",
        "placeable.close",
        "before",
        "placeable_close"
    ),
    event!(
        "placeable_close_after",
        "placeable.close",
        "after",
        "placeable_close"
    ),
    skippable_event!(
        "inventory_add_gold_before",
        "inventory.add_gold",
        "before",
        "inventory_add_gold"
    ),
    event!(
        "inventory_add_gold_after",
        "inventory.add_gold",
        "after",
        "inventory_add_gold"
    ),
    skippable_event!(
        "inventory_remove_gold_before",
        "inventory.remove_gold",
        "before",
        "inventory_remove_gold"
    ),
    event!(
        "inventory_remove_gold_after",
        "inventory.remove_gold",
        "after",
        "inventory_remove_gold"
    ),
    skippable_event!("feat_use_before", "feat.use", "before", "feat_use"),
    event!("feat_use_after", "feat.use", "after", "feat_use"),
    event!(
        "journal_open_before",
        "journal.open",
        "before",
        "journal_message",
        helper = "player_get_game_object"
    ),
    event!(
        "journal_open_after",
        "journal.open",
        "after",
        "journal_message",
        helper = "player_get_game_object"
    ),
    event!(
        "journal_close_before",
        "journal.close",
        "before",
        "journal_message",
        helper = "player_get_game_object"
    ),
    event!(
        "journal_close_after",
        "journal.close",
        "after",
        "journal_message",
        helper = "player_get_game_object"
    ),
    event!(
        "timing_bar_start_before",
        "timing_bar.start",
        "before",
        "timing_bar_send",
        helper = "player_get_game_object"
    ),
    event!(
        "timing_bar_start_after",
        "timing_bar.start",
        "after",
        "timing_bar_send",
        helper = "player_get_game_object"
    ),
    event!(
        "timing_bar_stop_before",
        "timing_bar.stop",
        "before",
        "timing_bar_send",
        helper = "player_get_game_object"
    ),
    event!(
        "timing_bar_stop_after",
        "timing_bar.stop",
        "after",
        "timing_bar_send",
        helper = "player_get_game_object"
    ),
    event!(
        "timing_bar_cancel_before",
        "timing_bar.cancel",
        "before",
        "timing_bar_cancel",
        helper = "player_get_game_object"
    ),
    event!(
        "timing_bar_cancel_after",
        "timing_bar.cancel",
        "after",
        "timing_bar_cancel",
        helper = "player_get_game_object"
    ),
    skippable_event!(
        "object_broadcast_safe_projectile_before",
        "object.broadcast_safe_projectile",
        "before",
        "object_broadcast_safe_projectile"
    ),
    event!(
        "object_broadcast_safe_projectile_after",
        "object.broadcast_safe_projectile",
        "after",
        "object_broadcast_safe_projectile"
    ),
    skippable_event!("skill_use_before", "skill.use", "before", "skill_use"),
    event!("skill_use_after", "skill.use", "after", "skill_use"),
    skippable_event!(
        "item_use_before",
        "item.use",
        "before",
        "item_use",
        result = Boolean
    ),
    event!("item_use_after", "item.use", "after", "item_use"),
    skippable_event!(
        "item_inventory_open_before",
        "item.inventory_open",
        "before",
        "item_inventory_open"
    ),
    event!(
        "item_inventory_open_after",
        "item.inventory_open",
        "after",
        "item_inventory_open"
    ),
    skippable_event!(
        "item_inventory_close_before",
        "item.inventory_close",
        "before",
        "item_inventory_close"
    ),
    event!(
        "item_inventory_close_after",
        "item.inventory_close",
        "after",
        "item_inventory_close"
    ),
    skippable_event!(
        "item_scroll_learn_before",
        "item.scroll_learn",
        "before",
        "item_scroll_learn"
    ),
    event!(
        "item_scroll_learn_after",
        "item.scroll_learn",
        "after",
        "item_scroll_learn"
    ),
    skippable_event!(
        "item_use_lore_before",
        "item.use_lore",
        "before",
        "item_use_lore"
    ),
    event!(
        "item_use_lore_after",
        "item.use_lore",
        "after",
        "item_use_lore"
    ),
    skippable_event!(
        "item_pay_to_identify_before",
        "item.pay_to_identify",
        "before",
        "item_pay_to_identify"
    ),
    event!(
        "item_pay_to_identify_after",
        "item.pay_to_identify",
        "after",
        "item_pay_to_identify"
    ),
    skippable_event!(
        "item_destroy_before",
        "item.destroy",
        "before",
        "item_event_handler"
    ),
    event!(
        "item_destroy_after",
        "item.destroy",
        "after",
        "item_event_handler"
    ),
    skippable_event!(
        "item_decrement_stack_size_before",
        "item.decrement_stack_size",
        "before",
        "item_event_handler"
    ),
    event!(
        "item_decrement_stack_size_after",
        "item.decrement_stack_size",
        "after",
        "item_event_handler"
    ),
    requiring!(
        skippable_event!(
            "object_set_experience_before",
            "object.set_experience",
            "before",
            "object_set_experience",
            result = Unsigned
        ),
        EXPERIENCE_REQUIREMENTS
    ),
    requiring!(
        event!(
            "object_set_experience_after",
            "object.set_experience",
            "after",
            "object_set_experience"
        ),
        EXPERIENCE_REQUIREMENTS
    ),
    requiring!(
        skippable_event!(
            "feat_decrement_remaining_uses_before",
            "feat.decrement_remaining_uses",
            "before",
            "feat_decrement_remaining_uses"
        ),
        FEAT_USES_REQUIREMENTS
    ),
    requiring!(
        event!(
            "feat_decrement_remaining_uses_after",
            "feat.decrement_remaining_uses",
            "after",
            "feat_decrement_remaining_uses"
        ),
        FEAT_USES_REQUIREMENTS
    ),
    requiring!(
        forcing_whitelist!(
            skippable_event!(
                "feat_has_before",
                "feat.has",
                "before",
                "feat_has",
                result = Boolean
            ),
            FEAT_HAS_ID_WHITELIST
        ),
        STATS_REQUIREMENTS
    ),
    requiring!(
        forcing_whitelist!(
            event!("feat_has_after", "feat.has", "after", "feat_has"),
            FEAT_HAS_ID_WHITELIST
        ),
        STATS_REQUIREMENTS
    ),
    requiring!(
        skippable_event!(
            "inventory_open_before",
            "inventory.open",
            "before",
            "inventory_message"
        ),
        INVENTORY_MESSAGE_REQUIREMENTS
    ),
    requiring!(
        event!(
            "inventory_open_after",
            "inventory.open",
            "after",
            "inventory_message"
        ),
        INVENTORY_MESSAGE_REQUIREMENTS
    ),
    requiring!(
        skippable_event!(
            "inventory_select_panel_before",
            "inventory.select_panel",
            "before",
            "inventory_message"
        ),
        INVENTORY_MESSAGE_REQUIREMENTS
    ),
    requiring!(
        event!(
            "inventory_select_panel_after",
            "inventory.select_panel",
            "after",
            "inventory_message"
        ),
        INVENTORY_MESSAGE_REQUIREMENTS
    ),
    requiring!(
        skippable_event!(
            "inventory_add_item_before",
            "inventory.add_item",
            "before",
            "inventory_add_item"
        ),
        REPOSITORY_REQUIREMENTS
    ),
    requiring!(
        event!(
            "inventory_add_item_after",
            "inventory.add_item",
            "after",
            "inventory_add_item"
        ),
        REPOSITORY_REQUIREMENTS
    ),
    requiring!(
        event!(
            "inventory_remove_item_before",
            "inventory.remove_item",
            "before",
            "inventory_remove_item"
        ),
        REPOSITORY_REQUIREMENTS
    ),
    requiring!(
        event!(
            "inventory_remove_item_after",
            "inventory.remove_item",
            "after",
            "inventory_remove_item"
        ),
        REPOSITORY_REQUIREMENTS
    ),
    skippable_event!(
        "item_validate_use_before",
        "item.validate_use",
        "before",
        "item_validate_use",
        result = Boolean
    ),
    event!(
        "item_validate_use_after",
        "item.validate_use",
        "after",
        "item_validate_use",
        result = Boolean
    ),
    requiring!(
        skippable_event!(
            "item_ammo_reload_before",
            "item.ammo_reload",
            "before",
            "item_ammo_reload",
            result = ObjectId
        ),
        AMMO_REQUIREMENTS
    ),
    requiring!(
        event!(
            "item_ammo_reload_after",
            "item.ammo_reload",
            "after",
            "item_ammo_reload",
            result = ObjectId
        ),
        AMMO_REQUIREMENTS
    ),
    skippable_event!(
        "item_validate_equip_before",
        "item.validate_equip",
        "before",
        "item_validate_equip",
        result = Boolean
    ),
    event!(
        "item_validate_equip_after",
        "item.validate_equip",
        "after",
        "item_validate_equip",
        result = Boolean
    ),
    requiring!(
        skippable_event!("item_equip_before", "item.equip", "before", "item_equip"),
        EQUIPMENT_REQUIREMENTS
    ),
    requiring!(
        event!("item_equip_after", "item.equip", "after", "item_equip"),
        EQUIPMENT_REQUIREMENTS
    ),
    requiring!(
        skippable_event!(
            "item_unequip_before",
            "item.unequip",
            "before",
            "item_unequip"
        ),
        EQUIPMENT_REQUIREMENTS
    ),
    requiring!(
        event!(
            "item_unequip_after",
            "item.unequip",
            "after",
            "item_unequip"
        ),
        EQUIPMENT_REQUIREMENTS
    ),
    skippable_event!("item_split_before", "item.split", "before", "item_split"),
    event!("item_split_after", "item.split", "after", "item_split"),
    requiring!(
        skippable_event!("item_merge_before", "item.merge", "before", "item_merge"),
        OBJECT_LOOKUP_REQUIREMENTS
    ),
    requiring!(
        event!("item_merge_after", "item.merge", "after", "item_merge"),
        OBJECT_LOOKUP_REQUIREMENTS
    ),
    skippable_event!(
        "item_acquire_before",
        "item.acquire",
        "before",
        "item_acquire"
    ),
    event!(
        "item_acquire_after",
        "item.acquire",
        "after",
        "item_acquire"
    ),
];

/// Looks up one annotation identity.
#[must_use]
pub fn event_definition(identity: &str) -> Option<&'static EventDefinition> {
    EVENT_CATALOG
        .iter()
        .find(|event| event.identity == identity)
}

/// Looks up one logical event and phase.
#[must_use]
pub fn runtime_event_definition(name: &str, phase: &str) -> Option<&'static EventDefinition> {
    EVENT_CATALOG
        .iter()
        .find(|event| event.name == name && event.phase == phase)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn catalog_identities_and_runtime_pairs_are_unique() {
        let mut identities = BTreeSet::new();
        let mut runtime_pairs = BTreeSet::new();
        for event in EVENT_CATALOG {
            assert!(identities.insert(event.identity));
            assert!(runtime_pairs.insert((event.name, event.phase)));
        }
        assert_eq!(EVENT_CATALOG.len(), 85);
    }
}
