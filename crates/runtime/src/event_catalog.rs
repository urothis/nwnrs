//! Authoritative in-progress event catalog shared by the runtime and nwpkg.

/// Projectile-type filter used by `object.broadcast_safe_projectile`.
pub const PROJECTILE_TYPE_ID_WHITELIST: &str = "object.broadcast_safe_projectile.projectile_type";
/// Spell-ID filter used by `object.broadcast_safe_projectile`.
pub const PROJECTILE_SPELL_ID_WHITELIST: &str = "object.broadcast_safe_projectile.spell_id";
/// Feat-ID whitelist required by the high-frequency `feat.has` hook.
pub const FEAT_HAS_ID_WHITELIST: &str = "feat.has.feat_id";

/// Stable numeric position of one logical event in [`EVENT_CATALOG`].
///
/// Keys are process-local runtime identifiers, not persisted target-pack data.
/// They allow native hot paths to use subscription bitsets without hashing
/// event identity strings.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct EventKey(u16);

impl EventKey {
    /// Returns the catalog index represented by this key.
    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// Dense subscription mask used by native event hot paths.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EventSubscriptionMask {
    words: Box<[u64]>,
}

impl Default for EventSubscriptionMask {
    fn default() -> Self {
        Self::empty()
    }
}

impl EventSubscriptionMask {
    /// Creates an empty mask sized for the complete event catalog.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            words: vec![0; EVENT_CATALOG.len().div_ceil(u64::BITS as usize)].into_boxed_slice(),
        }
    }

    /// Enables one stable event key.
    ///
    /// Returns `false` only when a key from a different catalog cannot fit in
    /// this mask.
    pub fn insert(&mut self, key: EventKey) -> bool {
        let word = key.index() / u64::BITS as usize;
        let bit = key.index() % u64::BITS as usize;
        let Some(value) = self.words.get_mut(word) else {
            return false;
        };
        *value |= 1_u64 << bit;
        true
    }

    /// Tests one key without hashing, locking, or allocating.
    #[must_use]
    pub fn contains(&self, key: EventKey) -> bool {
        let word = key.index() / u64::BITS as usize;
        let bit = key.index() % u64::BITS as usize;
        self.words
            .get(word)
            .is_some_and(|word| word & (1_u64 << bit) != 0)
    }
}

/// Mechanism which produces one logical event.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventSourceKind {
    /// A synchronous native engine detour.
    NativeDetour,
    /// An owned record drained onto the engine thread from an asynchronous
    /// producer such as a filesystem watcher.
    EventPump,
}

/// Meaning of a result supplied by an event handler.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventResultRole {
    /// The event accepts no result.
    None,
    /// The result becomes the native return value when the original call is
    /// suppressed.
    CancellationReturn,
    /// The result may replace the native operation's return value.
    ReplaceReturn,
    /// The result replaces one input argument before the original is called.
    ReplaceArgument(&'static str),
}

/// Native-call control semantics exposed by one logical event.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventControlPolicy {
    /// The handler observes the operation without changing whether the
    /// original is called.
    Observe,
    /// Skipping suppresses the original native operation.
    SuppressOriginal,
    /// Skipping uses a typed result according to [`EventResultRole`].
    TypedResult(EventResultRole),
}

/// Behavior when the same logical route is entered recursively.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventReentrancyPolicy {
    /// Nested entry is dispatched normally.
    Allow,
    /// A future route may suppress nested entry into the same logical event.
    ///
    /// This is declared now so ports cannot silently invent ad-hoc recursion
    /// guards; no current event uses it yet.
    SuppressSameRoute,
}

/// Expected invocation frequency before subscription checks are applied.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventFrequency {
    /// Ordinary engine boundary.
    Normal,
    /// Boundary called often enough to require an explicit pre-decode filter.
    Hot,
}

/// Filtering performed before payload construction or script dispatch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventFilterPolicy {
    /// The subscription bit is the only pre-dispatch filter.
    SubscriptionOnly,
    /// Zero or more operator-enabled integer whitelists narrow dispatch.
    OptionalIntegerWhitelists(&'static [&'static str]),
    /// Subscription creates an empty whitelist which must be populated before
    /// any matching native call is dispatched.
    RequiredIntegerWhitelist(&'static str),
}

impl EventFilterPolicy {
    /// Returns the whitelist automatically enabled with the subscription.
    #[must_use]
    pub const fn required_whitelist(self) -> Option<&'static str> {
        match self {
            Self::RequiredIntegerWhitelist(name) => Some(name),
            Self::SubscriptionOnly | Self::OptionalIntegerWhitelists(_) => None,
        }
    }
}

/// Point at which one physical detour is installed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PhysicalHookInstallPolicy {
    /// Installed during runtime bootstrap because it creates the subscription
    /// transaction itself.
    Bootstrap,
    /// Installed in the module-load transaction when its first logical route
    /// is subscribed.
    OnSubscription,
}

/// One native function address which may produce one or more logical events.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PhysicalHookDefinition {
    /// Stable target-pack key.
    pub id:             &'static str,
    /// Versioned native signature understood by the Rust or compiled shim
    /// replacement.
    pub abi:            &'static str,
    /// Installation lifecycle for the detour.
    pub install_policy: PhysicalHookInstallPolicy,
}

macro_rules! physical_hook {
    ($id:literal, $abi:literal) => {
        PhysicalHookDefinition {
            id:             $id,
            abi:            $abi,
            install_policy: PhysicalHookInstallPolicy::OnSubscription,
        }
    };
}

/// Physical detours used by the currently implemented logical event catalog.
///
/// Logical before/after phases and discriminated operations intentionally
/// reference these entries rather than registering independent replacements.
pub const PHYSICAL_HOOK_CATALOG: &[PhysicalHookDefinition] = &[
    PhysicalHookDefinition {
        id:             "module_load",
        abi:            "LoadModuleFinish.v1",
        install_policy: PhysicalHookInstallPolicy::Bootstrap,
    },
    physical_hook!("associate_add", "AddAssociate.v1"),
    physical_hook!("associate_remove", "RemoveAssociate.v1"),
    physical_hook!("associate_possess_familiar", "FamiliarAction.v1"),
    physical_hook!("associate_unpossess_familiar", "FamiliarAction.v1"),
    physical_hook!("object_lock", "ObjectAction.v1"),
    physical_hook!("object_unlock", "UnlockObjectAction.v1"),
    physical_hook!("object_use", "ObjectAction.v1"),
    physical_hook!("placeable_open", "OpenInventory.v1"),
    physical_hook!("placeable_close", "CloseInventory.v1"),
    physical_hook!("inventory_add_gold", "ModifyGold.v1"),
    physical_hook!("inventory_remove_gold", "ModifyGold.v1"),
    physical_hook!("feat_use", "UseFeat.v1"),
    physical_hook!("journal_message", "PlayerMessage.v1"),
    physical_hook!("timing_bar_send", "SendTimingEvent.v1"),
    physical_hook!("timing_bar_cancel", "CancelTimingEvent.v1"),
    physical_hook!(
        "object_broadcast_safe_projectile",
        "BroadcastSafeProjectile.v1"
    ),
    physical_hook!("skill_use", "UseSkill.v1"),
    physical_hook!("item_use", "UseItem.v1"),
    physical_hook!("item_inventory_open", "ItemInventoryOpen.v1"),
    physical_hook!("item_inventory_close", "ItemInventoryClose.v1"),
    physical_hook!("item_scroll_learn", "ItemObjectAction.v1"),
    physical_hook!("item_use_lore", "ItemObjectAction.v1"),
    physical_hook!("item_pay_to_identify", "PayToIdentifyItem.v1"),
    physical_hook!("item_event_handler", "ItemEventHandler.v1"),
    physical_hook!("object_set_experience", "SetExperience.v1"),
    physical_hook!(
        "feat_decrement_remaining_uses",
        "DecrementFeatRemainingUses.v1"
    ),
    physical_hook!("feat_has", "HasFeat.v1"),
    physical_hook!("inventory_message", "PlayerMessage.v1"),
    physical_hook!("inventory_add_item", "RepositoryAddItem.v1"),
    physical_hook!("inventory_remove_item", "RepositoryRemoveItem.v1"),
    physical_hook!("item_validate_use", "ValidateUseItem.v1"),
    physical_hook!("item_ammo_reload", "FindItemWithBaseItemId.v1"),
    physical_hook!("item_validate_equip", "ValidateEquipItem.v1"),
    physical_hook!("item_equip", "RunEquip.v1"),
    physical_hook!("item_unequip", "RunUnequip.v1"),
    physical_hook!("item_split", "SplitItem.v1"),
    physical_hook!("item_merge", "MergeItem.v1"),
    physical_hook!("item_acquire", "AcquireItem.v1"),
];

/// The JSON type accepted by an event result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventResultKind {
    /// The event does not accept a replacement result.
    None,
    /// The event accepts a JSON boolean result.
    Boolean,
    /// The event accepts a JSON unsigned 32-bit integer result.
    Unsigned,
    /// The event accepts a JSON signed 32-bit integer result.
    Signed,
    /// The event accepts a finite JSON 32-bit floating-point result.
    Float,
    /// The event accepts a bounded JSON string result.
    String,
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
    pub identity:       &'static str,
    /// Logical runtime event name.
    pub name:           &'static str,
    /// Runtime phase (`before` or `after`).
    pub phase:          &'static str,
    /// Physical hook required by the event.
    pub hook:           &'static str,
    /// Optional native helper required to construct the payload.
    pub helper:         Option<&'static str>,
    /// Whether the before-event handler may suppress the original operation.
    pub skippable:      bool,
    /// JSON result type accepted by this event.
    pub result_kind:    EventResultKind,
    /// Semantic role of skip and result controls.
    pub control_policy: EventControlPolicy,
    /// Mechanism which produces the event.
    pub source_kind:    EventSourceKind,
    /// Recursive-entry policy for this logical route.
    pub reentrancy:     EventReentrancyPolicy,
    /// Expected native invocation frequency.
    pub frequency:      EventFrequency,
    /// Filtering required before payload construction.
    pub filter_policy:  EventFilterPolicy,
    /// Additional target data needed to safely implement the event.
    pub requirements:   EventRequirements,
}

macro_rules! event {
    ($identity:literal, $name:literal, $phase:literal, $hook:literal) => {
        EventDefinition {
            identity:       $identity,
            name:           $name,
            phase:          $phase,
            hook:           $hook,
            helper:         None,
            skippable:      false,
            result_kind:    EventResultKind::None,
            control_policy: EventControlPolicy::Observe,
            source_kind:    EventSourceKind::NativeDetour,
            reentrancy:     EventReentrancyPolicy::Allow,
            frequency:      EventFrequency::Normal,
            filter_policy:  EventFilterPolicy::SubscriptionOnly,
            requirements:   NO_REQUIREMENTS,
        }
    };
    ($identity:literal, $name:literal, $phase:literal, $hook:literal,helper = $helper:literal) => {
        EventDefinition {
            identity:       $identity,
            name:           $name,
            phase:          $phase,
            hook:           $hook,
            helper:         Some($helper),
            skippable:      false,
            result_kind:    EventResultKind::None,
            control_policy: EventControlPolicy::Observe,
            source_kind:    EventSourceKind::NativeDetour,
            reentrancy:     EventReentrancyPolicy::Allow,
            frequency:      EventFrequency::Normal,
            filter_policy:  EventFilterPolicy::SubscriptionOnly,
            requirements:   NO_REQUIREMENTS,
        }
    };
    ($identity:literal, $name:literal, $phase:literal, $hook:literal,result = $result:ident) => {
        EventDefinition {
            identity:       $identity,
            name:           $name,
            phase:          $phase,
            hook:           $hook,
            helper:         None,
            skippable:      false,
            result_kind:    EventResultKind::$result,
            control_policy: EventControlPolicy::TypedResult(EventResultRole::ReplaceReturn),
            source_kind:    EventSourceKind::NativeDetour,
            reentrancy:     EventReentrancyPolicy::Allow,
            frequency:      EventFrequency::Normal,
            filter_policy:  EventFilterPolicy::SubscriptionOnly,
            requirements:   NO_REQUIREMENTS,
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
        definition.filter_policy = EventFilterPolicy::RequiredIntegerWhitelist($whitelist);
        definition
    }};
}

macro_rules! optional_whitelists {
    ($definition:expr, $whitelists:expr) => {{
        let mut definition = $definition;
        definition.filter_policy = EventFilterPolicy::OptionalIntegerWhitelists($whitelists);
        definition
    }};
}

macro_rules! hot_event {
    ($definition:expr) => {{
        let mut definition = $definition;
        definition.frequency = EventFrequency::Hot;
        definition
    }};
}

macro_rules! replacing_argument {
    ($definition:expr, $argument:literal) => {{
        let mut definition = $definition;
        definition.control_policy =
            EventControlPolicy::TypedResult(EventResultRole::ReplaceArgument($argument));
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
        definition.control_policy = EventControlPolicy::SuppressOriginal;
        definition
    }};
    ($identity:literal, $name:literal, $phase:literal, $hook:literal,helper = $helper:literal) => {{
        let mut definition = event!($identity, $name, $phase, $hook, helper = $helper);
        definition.skippable = true;
        definition.control_policy = EventControlPolicy::SuppressOriginal;
        definition
    }};
    ($identity:literal, $name:literal, $phase:literal, $hook:literal,result = $result:ident) => {{
        let mut definition = event!($identity, $name, $phase, $hook, result = $result);
        definition.skippable = true;
        definition.control_policy =
            EventControlPolicy::TypedResult(EventResultRole::CancellationReturn);
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
    hot_event!(optional_whitelists!(
        skippable_event!(
            "object_broadcast_safe_projectile_before",
            "object.broadcast_safe_projectile",
            "before",
            "object_broadcast_safe_projectile"
        ),
        &[PROJECTILE_TYPE_ID_WHITELIST, PROJECTILE_SPELL_ID_WHITELIST]
    )),
    hot_event!(optional_whitelists!(
        event!(
            "object_broadcast_safe_projectile_after",
            "object.broadcast_safe_projectile",
            "after",
            "object_broadcast_safe_projectile"
        ),
        &[PROJECTILE_TYPE_ID_WHITELIST, PROJECTILE_SPELL_ID_WHITELIST]
    )),
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
        replacing_argument!(
            skippable_event!(
                "object_set_experience_before",
                "object.set_experience",
                "before",
                "object_set_experience",
                result = Unsigned
            ),
            "experience"
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
        hot_event!(forcing_whitelist!(
            skippable_event!(
                "feat_has_before",
                "feat.has",
                "before",
                "feat_has",
                result = Boolean
            ),
            FEAT_HAS_ID_WHITELIST
        )),
        STATS_REQUIREMENTS
    ),
    requiring!(
        hot_event!(forcing_whitelist!(
            event!("feat_has_after", "feat.has", "after", "feat_has"),
            FEAT_HAS_ID_WHITELIST
        )),
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

/// Looks up one annotation identity and its stable runtime key.
#[must_use]
pub fn keyed_event_definition(identity: &str) -> Option<(EventKey, &'static EventDefinition)> {
    EVENT_CATALOG
        .iter()
        .enumerate()
        .find(|(_key, event)| event.identity == identity)
        .and_then(|(key, event)| u16::try_from(key).ok().map(|key| (EventKey(key), event)))
}

/// Looks up one logical event phase and its stable runtime key.
#[must_use]
pub fn keyed_runtime_event_definition(
    name: &str,
    phase: &str,
) -> Option<(EventKey, &'static EventDefinition)> {
    EVENT_CATALOG
        .iter()
        .enumerate()
        .find(|(_key, event)| event.name == name && event.phase == phase)
        .and_then(|(key, event)| u16::try_from(key).ok().map(|key| (EventKey(key), event)))
}

/// Returns the definition represented by a runtime key.
#[must_use]
pub fn event_definition_by_key(key: EventKey) -> Option<&'static EventDefinition> {
    EVENT_CATALOG.get(key.index())
}

/// Looks up one physical native detour.
#[must_use]
pub fn physical_hook_definition(id: &str) -> Option<&'static PhysicalHookDefinition> {
    PHYSICAL_HOOK_CATALOG.iter().find(|hook| hook.id == id)
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
            assert_eq!(
                event.skippable,
                matches!(
                    event.control_policy,
                    EventControlPolicy::SuppressOriginal
                        | EventControlPolicy::TypedResult(
                            EventResultRole::CancellationReturn
                                | EventResultRole::ReplaceArgument(_)
                        )
                )
            );
            assert_eq!(
                event.result_kind == EventResultKind::None,
                !matches!(event.control_policy, EventControlPolicy::TypedResult(_))
            );
            if event.frequency == EventFrequency::Hot {
                assert!(
                    !matches!(event.filter_policy, EventFilterPolicy::SubscriptionOnly),
                    "{} is hot but has no pre-decode filter",
                    event.identity
                );
            }
        }
        assert_eq!(EVENT_CATALOG.len(), 85);
    }

    #[test]
    fn catalog_keys_round_trip() {
        for (index, event) in EVENT_CATALOG.iter().enumerate() {
            let (identity_key, identity) =
                keyed_event_definition(event.identity).expect("identity has a key");
            let (runtime_key, runtime) = keyed_runtime_event_definition(event.name, event.phase)
                .expect("runtime pair has a key");
            assert_eq!(identity_key, runtime_key);
            assert_eq!(identity_key.index(), index);
            assert_eq!(identity, event);
            assert_eq!(runtime, event);
            assert_eq!(event_definition_by_key(identity_key), Some(event));
        }
    }

    #[test]
    fn subscription_mask_tracks_catalog_keys_without_false_positives() {
        let mut mask = EventSubscriptionMask::empty();
        for (index, event) in EVENT_CATALOG.iter().enumerate() {
            let (key, _definition) =
                keyed_event_definition(event.identity).expect("catalog key exists");
            assert!(!mask.contains(key));
            if index == 0 || index % 3 != 0 {
                assert!(mask.insert(key));
                assert!(mask.contains(key));
            }
        }
    }

    #[test]
    fn physical_hooks_are_unique_and_cover_every_native_event() {
        let mut hooks = BTreeSet::new();
        for hook in PHYSICAL_HOOK_CATALOG {
            assert!(hooks.insert(hook.id), "duplicate physical hook {}", hook.id);
            assert!(!hook.abi.is_empty(), "{} has no ABI fingerprint", hook.id);
        }
        for event in EVENT_CATALOG {
            if event.source_kind == EventSourceKind::NativeDetour {
                assert!(
                    physical_hook_definition(event.hook).is_some(),
                    "{} references unknown physical hook {}",
                    event.identity,
                    event.hook
                );
            }
        }
        for hook in PHYSICAL_HOOK_CATALOG {
            assert!(
                EVENT_CATALOG.iter().any(|event| event.hook == hook.id),
                "physical hook {} has no logical routes",
                hook.id
            );
        }
    }

    #[test]
    fn unified_behavior_decisions_are_complete_and_unique() {
        let document: toml::Value =
            toml::from_str(include_str!("../event-decisions.toml")).expect("decision TOML parses");
        assert_eq!(
            document.get("schema").and_then(toml::Value::as_integer),
            Some(1)
        );
        let decisions = document
            .get("decision")
            .and_then(toml::Value::as_array)
            .expect("decision array exists");
        let mut ids = BTreeSet::new();
        for decision in decisions {
            let table = decision.as_table().expect("decision is a table");
            let id = table
                .get("id")
                .and_then(toml::Value::as_str)
                .expect("decision id exists");
            assert!(ids.insert(id), "duplicate Unified decision {id}");
            assert_eq!(
                table.get("status").and_then(toml::Value::as_str),
                Some("decided"),
                "{id} is not decided"
            );
            for field in ["source", "resolution", "rationale", "acceptance"] {
                assert!(
                    table
                        .get(field)
                        .and_then(toml::Value::as_str)
                        .is_some_and(|value| !value.trim().is_empty()),
                    "{id} lacks {field}"
                );
            }
        }
        for required in [
            "inventory-authority",
            "ability-change-missing-public-constants",
            "set-experience-missing-public-constants",
            "area-edge-enter-missing-public-constant",
            "disarm-subscription-regex-defect",
            "elc-events-out-of-scope",
            "webhook-events-out-of-scope",
            "after-result-overrides",
            "typed-results",
            "memorized-spell-slot-spelling",
        ] {
            assert!(
                ids.contains(required),
                "missing Unified decision {required}"
            );
        }
    }
}
