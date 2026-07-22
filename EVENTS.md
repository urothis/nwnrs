# Events implementation plan

This is the unversioned working plan for porting the Unified Events plugin into nwnrs. It describes current implementation coverage; it is not an API standard or a compatibility-version document.

## Scope and source of truth

- Unified source: `sources/unified/Plugins/Events/Events/*.cpp` at pinned commit `3d4c4e13c6bf01b032ffe90534fc4a19eb036c03`.
- Current nwnrs coverage: `crates/runtime/src/event_catalog.rs` and the native family modules under `crates/runtime-sys/src/engine/events/`.
- Inventory rule: an event is included when the Unified Events implementation registers or signals it. Generic constants exposed by `nwnx_events.nss` but owned elsewhere are excluded.
- Braced names such as `NWNX_ON_USE_ITEM_{BEFORE,AFTER}` represent the two exact Unified signals ending in `_BEFORE` and `_AFTER`.

## Status legend

| Status | Meaning |
| --- | --- |
| ✅ Ported | Both Unified phases, or the complete one-shot event, are present in the Rust event catalog and native event implementation. |
| 🍎 macOS | Ported and verified for the current macOS target. Linux and Windows target symbols/layouts are still intentionally absent. |
| 🟨 Partial | The family has some implemented events, but still has missing events. |
| ⬜ Pending | No event in the family has been ported yet, or the individual event is still missing. |

## Coverage summary

- **42 of 181 logical Unified events ported on macOS** (84 of 350 concrete Unified signals).
- **27 of 181 are currently backed by all existing platform target packs; the latest 15 are macOS-only until Linux and Windows are verified locally.**
- **8 complete families and 28 pending families on macOS.**
- `module_load` is implemented in nwnrs but is listed separately because it is a runtime bootstrap event, not one of the Unified Events plugin hooks.

| Family | Ported | Remaining | Family status |
| --- | ---: | ---: | --- |
| Ability | 0 / 1 | 1 | ⬜ Pending |
| Associate | 4 / 4 | 0 | ✅ Complete |
| Barter | 0 / 3 | 3 | ⬜ Pending |
| Calendar | 0 / 6 | 6 | ⬜ Pending |
| Client | 0 / 7 | 7 | ⬜ Pending |
| Combat | 0 / 11 | 11 | ⬜ Pending |
| DM action | 0 / 38 | 38 | ⬜ Pending |
| Debug | 0 / 3 | 3 | ⬜ Pending |
| Effect | 0 / 2 | 2 | ⬜ Pending |
| Event script | 0 / 1 | 1 | ⬜ Pending |
| Examine | 0 / 4 | 4 | ⬜ Pending |
| Faction | 0 / 1 | 1 | ⬜ Pending |
| Feat | 3 / 3 | 0 | ✅ Complete (macOS) |
| Healing | 0 / 2 | 2 | ⬜ Pending |
| Input | 0 / 8 | 8 | ⬜ Pending |
| Inventory | 6 / 6 | 0 | ✅ Complete (macOS) |
| Item | 16 / 16 | 0 | ✅ Complete (macOS) |
| Item property | 0 / 2 | 2 | ⬜ Pending |
| Journal | 2 / 2 | 0 | ✅ Complete |
| Level | 0 / 4 | 4 | ⬜ Pending |
| Map | 0 / 3 | 3 | ⬜ Pending |
| Movement | 0 / 5 | 5 | ⬜ Pending |
| Object | 7 / 7 | 0 | ✅ Complete (macOS) |
| PVP | 0 / 1 | 1 | ⬜ Pending |
| Party | 0 / 8 | 8 | ⬜ Pending |
| Polymorph | 0 / 2 | 2 | ⬜ Pending |
| Quick chat | 0 / 1 | 1 | ⬜ Pending |
| Quickbar | 0 / 1 | 1 | ⬜ Pending |
| Resource | 0 / 3 | 3 | ⬜ Pending |
| Skill | 1 / 1 | 0 | ✅ Complete |
| Spell | 0 / 7 | 7 | ⬜ Pending |
| Stealth | 0 / 6 | 6 | ⬜ Pending |
| Store | 0 / 2 | 2 | ⬜ Pending |
| Timing bar | 3 / 3 | 0 | ✅ Complete |
| Trap | 0 / 6 | 6 | ⬜ Pending |
| UUID | 0 / 1 | 1 | ⬜ Pending |
| **Total** | **42 / 181** | **139** | **macOS coverage** |

## Work order

1. Verify the latest 15 event implementations on Linux and Windows, then add their platform-specific hook symbols and layouts. The runtime contains explicit `todo!` guards for those target-enablement paths; subscriptions remain unsupported until the target packs are populated.
2. Port small self-contained families next, using the same catalog, dispatch, target-map, fixture, and warning coverage already required by implemented events.
3. Port cross-cutting gameplay families such as Effect, Spell, Combat, Movement, and Stealth after their shared hook and payload requirements are mapped.
4. Port client/protocol and UI families after validating their message layouts for every supported target.
5. Treat DM action as its own batch: it contains 38 logical events and is the largest remaining family.

## Related tooling: VS Code extension

The macOS Apple Silicon VS Code extension now links the reusable compiler API
in-process and does not spawn or require the `nwnrs` CLI. It checks unsaved NSS
buffers, resolves the owning project and transitive local include dependencies,
publishes multiple source-aware diagnostics, and validates generated event
dispatchers without a package build. Project-wide checks are debounced,
cancellable, dependency-deduplicated, and invalidated by NSS or manifest
watchers. Hover and Go to Definition use the same include graph for functions,
macros, enums, variants, compatibility aliases, and type aliases.

Windows, Linux, Intel macOS, richer symbol kinds, completion, references,
signature help, semantic highlighting, and VS Code-host integration tests
remain tracked in [the editor backlog](./editors/vscode-nwnrs/VSCODE_TODO.md).

## Source discrepancies to resolve during implementation

- Unified signals `NWNX_ON_ABILITY_CHANGE_{BEFORE,AFTER}`, `NWNX_ON_SET_EXPERIENCE_{BEFORE,AFTER}`, and `NWNX_ON_CREATURE_ON_AREA_EDGE_ENTER` in C++ but does not declare matching public constants in `nwnx_events.nss`. They remain in this plan because the implementation is the inventory source of truth.
- Unified declares the Disarm constants, but its lazy-subscription pattern is `NWNX_ON_DISARM_*`; as a regular expression, that does not match the full before/after names as intended. Verify the desired hook behavior rather than copying that registration defect.
- `NWNX_ON_ELC_VALIDATE_CHARACTER_{BEFORE,AFTER}` and `NWNX_ON_WEBHOOK_{SUCCESS,FAILURE}` are present in the shared NWScript constant file but are not implemented by `Plugins/Events/Events/*.cpp`, so they are outside this plan.

## nwnrs bootstrap event

| Status | nwnrs identity | Runtime event | Note |
| --- | --- | --- | --- |
| ✅ Ported | `module_load` | `module.load` / `before` | Runtime bootstrap event; no Unified Events-plugin counterpart. |

## Detailed event inventory

### Ability (0/1)

| Status | Unified signal(s) | nwnrs annotation identities |
| --- | --- | --- |
| ⬜ Pending | `NWNX_ON_ABILITY_CHANGE_{BEFORE,AFTER}` | — |

### Associate (4/4)

| Status | Unified signal(s) | nwnrs annotation identities |
| --- | --- | --- |
| ✅ Ported | `NWNX_ON_ADD_ASSOCIATE_{BEFORE,AFTER}` | `associate_add_before`<br>`associate_add_after` |
| ✅ Ported | `NWNX_ON_POSSESS_FAMILIAR_{BEFORE,AFTER}` | `associate_possess_familiar_before`<br>`associate_possess_familiar_after` |
| ✅ Ported | `NWNX_ON_REMOVE_ASSOCIATE_{BEFORE,AFTER}` | `associate_remove_before`<br>`associate_remove_after` |
| ✅ Ported | `NWNX_ON_UNPOSSESS_FAMILIAR_{BEFORE,AFTER}` | `associate_unpossess_familiar_before`<br>`associate_unpossess_familiar_after` |

### Barter (0/3)

| Status | Unified signal(s) | nwnrs annotation identities |
| --- | --- | --- |
| ⬜ Pending | `NWNX_ON_BARTER_ADD_ITEM_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_BARTER_END_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_BARTER_START_{BEFORE,AFTER}` | — |

### Calendar (0/6)

| Status | Unified signal(s) | nwnrs annotation identities |
| --- | --- | --- |
| ⬜ Pending | `NWNX_ON_CALENDAR_DAWN` | — |
| ⬜ Pending | `NWNX_ON_CALENDAR_DAY` | — |
| ⬜ Pending | `NWNX_ON_CALENDAR_DUSK` | — |
| ⬜ Pending | `NWNX_ON_CALENDAR_HOUR` | — |
| ⬜ Pending | `NWNX_ON_CALENDAR_MONTH` | — |
| ⬜ Pending | `NWNX_ON_CALENDAR_YEAR` | — |

### Client (0/7)

| Status | Unified signal(s) | nwnrs annotation identities |
| --- | --- | --- |
| ⬜ Pending | `NWNX_ON_CHECK_STICKY_PLAYER_NAME_RESERVED_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_CLIENT_CONNECT_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_CLIENT_DISCONNECT_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_CLIENT_EXPORT_CHARACTER_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_CLIENT_SET_DEVICE_PROPERTY_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_SERVER_CHARACTER_SAVE_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_SERVER_SEND_AREA_{BEFORE,AFTER}` | — |

### Combat (0/11)

| Status | Unified signal(s) | nwnrs annotation identities |
| --- | --- | --- |
| ⬜ Pending | `NWNX_ON_AREA_PLAY_BATTLE_MUSIC_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_ATTACK_TARGET_CHANGE_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_BROADCAST_ATTACK_OF_OPPORTUNITY_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_COMBAT_ATTACK_OF_OPPORTUNITY_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_COMBAT_DR_BROKEN_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_COMBAT_ENTER_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_COMBAT_EXIT_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_COMBAT_MODE_OFF` | — |
| ⬜ Pending | `NWNX_ON_COMBAT_MODE_ON` | — |
| ⬜ Pending | `NWNX_ON_DISARM_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_START_COMBAT_ROUND_{BEFORE,AFTER}` | — |

### DM action (0/38)

| Status | Unified signal(s) | nwnrs annotation identities |
| --- | --- | --- |
| ⬜ Pending | `NWNX_ON_DM_APPEAR_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_DM_CHANGE_DIFFICULTY_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_DM_DISABLE_TRAP_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_DM_DISAPPEAR_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_DM_DUMP_LOCALS_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_DM_FORCE_REST_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_DM_GET_FACTION_REPUTATION_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_DM_GET_VARIABLE_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_DM_GIVE_ALIGNMENT_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_DM_GIVE_GOLD_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_DM_GIVE_ITEM_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_DM_GIVE_LEVEL_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_DM_GIVE_XP_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_DM_GOTO_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_DM_HEAL_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_DM_JUMP_ALL_PLAYERS_TO_POINT_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_DM_JUMP_TARGET_TO_POINT_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_DM_JUMP_TO_POINT_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_DM_KILL_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_DM_LIMBO_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_DM_PLAYERDM_LOGIN_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_DM_PLAYERDM_LOGOUT_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_DM_POSSESS_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_DM_POSSESS_FULL_POWER_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_DM_SET_DATE_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_DM_SET_FACTION_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_DM_SET_FACTION_REPUTATION_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_DM_SET_STAT_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_DM_SET_TIME_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_DM_SET_VARIABLE_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_DM_SPAWN_OBJECT_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_DM_SPAWN_TRAP_ON_OBJECT_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_DM_TAKE_ITEM_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_DM_TOGGLE_AI_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_DM_TOGGLE_IMMORTAL_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_DM_TOGGLE_INVULNERABLE_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_DM_TOGGLE_LOCK_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_DM_VIEW_INVENTORY_{BEFORE,AFTER}` | — |

### Debug (0/3)

| Status | Unified signal(s) | nwnrs annotation identities |
| --- | --- | --- |
| ⬜ Pending | `NWNX_ON_DEBUG_PLAY_VISUAL_EFFECT_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_DEBUG_RUN_SCRIPT_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_DEBUG_RUN_SCRIPT_CHUNK_{BEFORE,AFTER}` | — |

### Effect (0/2)

| Status | Unified signal(s) | nwnrs annotation identities |
| --- | --- | --- |
| ⬜ Pending | `NWNX_ON_EFFECT_APPLIED_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_EFFECT_REMOVED_{BEFORE,AFTER}` | — |

### Event script (0/1)

| Status | Unified signal(s) | nwnrs annotation identities |
| --- | --- | --- |
| ⬜ Pending | `NWNX_ON_RUN_EVENT_SCRIPT_{BEFORE,AFTER}` | — |

### Examine (0/4)

| Status | Unified signal(s) | nwnrs annotation identities |
| --- | --- | --- |
| ⬜ Pending | `NWNX_ON_CHARACTER_SHEET_CLOSE_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_CHARACTER_SHEET_OPEN_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_CHARACTER_SHEET_PERMITTED_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_EXAMINE_OBJECT_{BEFORE,AFTER}` | — |

### Faction (0/1)

| Status | Unified signal(s) | nwnrs annotation identities |
| --- | --- | --- |
| ⬜ Pending | `NWNX_ON_SET_NPC_FACTION_REPUTATION_{BEFORE,AFTER}` | — |

### Feat (3/3 on macOS)

| Status | Unified signal(s) | nwnrs annotation identities |
| --- | --- | --- |
| 🍎 macOS | `NWNX_ON_DECREMENT_REMAINING_FEAT_USES_{BEFORE,AFTER}` | `feat_decrement_remaining_uses_before`<br>`feat_decrement_remaining_uses_after` |
| 🍎 macOS | `NWNX_ON_HAS_FEAT_{BEFORE,AFTER}` | `feat_has_before`<br>`feat_has_after` |
| ✅ Ported | `NWNX_ON_USE_FEAT_{BEFORE,AFTER}` | `feat_use_before`<br>`feat_use_after` |

### Healing (0/2)

| Status | Unified signal(s) | nwnrs annotation identities |
| --- | --- | --- |
| ⬜ Pending | `NWNX_ON_HEAL_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_HEALER_KIT_{BEFORE,AFTER}` | — |

### Input (0/8)

| Status | Unified signal(s) | nwnrs annotation identities |
| --- | --- | --- |
| ⬜ Pending | `NWNX_ON_INPUT_ATTACK_OBJECT_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_INPUT_CAST_SPELL_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_INPUT_DROP_ITEM_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_INPUT_EMOTE_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_INPUT_FORCE_MOVE_TO_OBJECT_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_INPUT_KEYBOARD_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_INPUT_TOGGLE_PAUSE_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_INPUT_WALK_TO_WAYPOINT_{BEFORE,AFTER}` | — |

### Inventory (6/6 on macOS)

| Status | Unified signal(s) | nwnrs annotation identities |
| --- | --- | --- |
| ✅ Ported | `NWNX_ON_INVENTORY_ADD_GOLD_{BEFORE,AFTER}` | `inventory_add_gold_before`<br>`inventory_add_gold_after` |
| 🍎 macOS | `NWNX_ON_INVENTORY_ADD_ITEM_{BEFORE,AFTER}` | `inventory_add_item_before`<br>`inventory_add_item_after` |
| 🍎 macOS | `NWNX_ON_INVENTORY_OPEN_{BEFORE,AFTER}` | `inventory_open_before`<br>`inventory_open_after` |
| ✅ Ported | `NWNX_ON_INVENTORY_REMOVE_GOLD_{BEFORE,AFTER}` | `inventory_remove_gold_before`<br>`inventory_remove_gold_after` |
| 🍎 macOS | `NWNX_ON_INVENTORY_REMOVE_ITEM_{BEFORE,AFTER}` | `inventory_remove_item_before`<br>`inventory_remove_item_after` |
| 🍎 macOS | `NWNX_ON_INVENTORY_SELECT_PANEL_{BEFORE,AFTER}` | `inventory_select_panel_before`<br>`inventory_select_panel_after` |

### Item (16/16 on macOS)

| Status | Unified signal(s) | nwnrs annotation identities |
| --- | --- | --- |
| 🍎 macOS | `NWNX_ON_ITEM_ACQUIRE_{BEFORE,AFTER}` | `item_acquire_before`<br>`item_acquire_after` |
| 🍎 macOS | `NWNX_ON_ITEM_AMMO_RELOAD_{BEFORE,AFTER}` | `item_ammo_reload_before`<br>`item_ammo_reload_after` |
| ✅ Ported | `NWNX_ON_ITEM_DECREMENT_STACKSIZE_{BEFORE,AFTER}` | `item_decrement_stack_size_before`<br>`item_decrement_stack_size_after` |
| ✅ Ported | `NWNX_ON_ITEM_DESTROY_OBJECT_{BEFORE,AFTER}` | `item_destroy_before`<br>`item_destroy_after` |
| 🍎 macOS | `NWNX_ON_ITEM_EQUIP_{BEFORE,AFTER}` | `item_equip_before`<br>`item_equip_after` |
| ✅ Ported | `NWNX_ON_ITEM_INVENTORY_CLOSE_{BEFORE,AFTER}` | `item_inventory_close_before`<br>`item_inventory_close_after` |
| ✅ Ported | `NWNX_ON_ITEM_INVENTORY_OPEN_{BEFORE,AFTER}` | `item_inventory_open_before`<br>`item_inventory_open_after` |
| 🍎 macOS | `NWNX_ON_ITEM_MERGE_{BEFORE,AFTER}` | `item_merge_before`<br>`item_merge_after` |
| ✅ Ported | `NWNX_ON_ITEM_PAY_TO_IDENTIFY_{BEFORE,AFTER}` | `item_pay_to_identify_before`<br>`item_pay_to_identify_after` |
| ✅ Ported | `NWNX_ON_ITEM_SCROLL_LEARN_{BEFORE,AFTER}` | `item_scroll_learn_before`<br>`item_scroll_learn_after` |
| 🍎 macOS | `NWNX_ON_ITEM_SPLIT_{BEFORE,AFTER}` | `item_split_before`<br>`item_split_after` |
| 🍎 macOS | `NWNX_ON_ITEM_UNEQUIP_{BEFORE,AFTER}` | `item_unequip_before`<br>`item_unequip_after` |
| ✅ Ported | `NWNX_ON_ITEM_USE_LORE_{BEFORE,AFTER}` | `item_use_lore_before`<br>`item_use_lore_after` |
| ✅ Ported | `NWNX_ON_USE_ITEM_{BEFORE,AFTER}` | `item_use_before`<br>`item_use_after` |
| 🍎 macOS | `NWNX_ON_VALIDATE_ITEM_EQUIP_{BEFORE,AFTER}` | `item_validate_equip_before`<br>`item_validate_equip_after` |
| 🍎 macOS | `NWNX_ON_VALIDATE_USE_ITEM_{BEFORE,AFTER}` | `item_validate_use_before`<br>`item_validate_use_after` |

### Item property (0/2)

| Status | Unified signal(s) | nwnrs annotation identities |
| --- | --- | --- |
| ⬜ Pending | `NWNX_ON_ITEMPROPERTY_EFFECT_APPLIED_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_ITEMPROPERTY_EFFECT_REMOVED_{BEFORE,AFTER}` | — |

### Journal (2/2)

| Status | Unified signal(s) | nwnrs annotation identities |
| --- | --- | --- |
| ✅ Ported | `NWNX_ON_JOURNAL_CLOSE_{BEFORE,AFTER}` | `journal_close_before`<br>`journal_close_after` |
| ✅ Ported | `NWNX_ON_JOURNAL_OPEN_{BEFORE,AFTER}` | `journal_open_before`<br>`journal_open_after` |

### Level (0/4)

| Status | Unified signal(s) | nwnrs annotation identities |
| --- | --- | --- |
| ⬜ Pending | `NWNX_ON_CLIENT_LEVEL_UP_BEGIN_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_LEVEL_DOWN_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_LEVEL_UP_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_LEVEL_UP_AUTOMATIC_{BEFORE,AFTER}` | — |

### Map (0/3)

| Status | Unified signal(s) | nwnrs annotation identities |
| --- | --- | --- |
| ⬜ Pending | `NWNX_ON_MAP_PIN_ADD_PIN_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_MAP_PIN_CHANGE_PIN_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_MAP_PIN_DESTROY_PIN_{BEFORE,AFTER}` | — |

### Movement (0/5)

| Status | Unified signal(s) | nwnrs annotation identities |
| --- | --- | --- |
| ⬜ Pending | `NWNX_ON_CREATURE_JUMP_TO_OBJECT_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_CREATURE_JUMP_TO_POINT_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_CREATURE_ON_AREA_EDGE_ENTER` | — |
| ⬜ Pending | `NWNX_ON_CREATURE_TILE_CHANGE_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_MATERIALCHANGE_{BEFORE,AFTER}` | — |

### Object (7/7 on macOS)

| Status | Unified signal(s) | nwnrs annotation identities |
| --- | --- | --- |
| ✅ Ported | `NWNX_ON_BROADCAST_SAFE_PROJECTILE_{BEFORE,AFTER}` | `object_broadcast_safe_projectile_before`<br>`object_broadcast_safe_projectile_after` |
| ✅ Ported | `NWNX_ON_OBJECT_LOCK_{BEFORE,AFTER}` | `object_lock_before`<br>`object_lock_after` |
| ✅ Ported | `NWNX_ON_OBJECT_UNLOCK_{BEFORE,AFTER}` | `object_unlock_before`<br>`object_unlock_after` |
| ✅ Ported | `NWNX_ON_OBJECT_USE_{BEFORE,AFTER}` | `object_use_before`<br>`object_use_after` |
| ✅ Ported | `NWNX_ON_PLACEABLE_CLOSE_{BEFORE,AFTER}` | `placeable_close_before`<br>`placeable_close_after` |
| ✅ Ported | `NWNX_ON_PLACEABLE_OPEN_{BEFORE,AFTER}` | `placeable_open_before`<br>`placeable_open_after` |
| 🍎 macOS | `NWNX_ON_SET_EXPERIENCE_{BEFORE,AFTER}` | `object_set_experience_before`<br>`object_set_experience_after` |

### PVP (0/1)

| Status | Unified signal(s) | nwnrs annotation identities |
| --- | --- | --- |
| ⬜ Pending | `NWNX_ON_PVP_ATTITUDE_CHANGE_{BEFORE,AFTER}` | — |

### Party (0/8)

| Status | Unified signal(s) | nwnrs annotation identities |
| --- | --- | --- |
| ⬜ Pending | `NWNX_ON_PARTY_ACCEPT_INVITATION_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_PARTY_IGNORE_INVITATION_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_PARTY_INVITE_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_PARTY_KICK_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_PARTY_KICK_HENCHMAN_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_PARTY_LEAVE_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_PARTY_REJECT_INVITATION_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_PARTY_TRANSFER_LEADERSHIP_{BEFORE,AFTER}` | — |

### Polymorph (0/2)

| Status | Unified signal(s) | nwnrs annotation identities |
| --- | --- | --- |
| ⬜ Pending | `NWNX_ON_POLYMORPH_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_UNPOLYMORPH_{BEFORE,AFTER}` | — |

### Quick chat (0/1)

| Status | Unified signal(s) | nwnrs annotation identities |
| --- | --- | --- |
| ⬜ Pending | `NWNX_ON_QUICKCHAT_{BEFORE,AFTER}` | — |

### Quickbar (0/1)

| Status | Unified signal(s) | nwnrs annotation identities |
| --- | --- | --- |
| ⬜ Pending | `NWNX_ON_QUICKBAR_SET_BUTTON_{BEFORE,AFTER}` | — |

### Resource (0/3)

| Status | Unified signal(s) | nwnrs annotation identities |
| --- | --- | --- |
| ⬜ Pending | `NWNX_ON_RESOURCE_ADDED` | — |
| ⬜ Pending | `NWNX_ON_RESOURCE_MODIFIED` | — |
| ⬜ Pending | `NWNX_ON_RESOURCE_REMOVED` | — |

### Skill (1/1)

| Status | Unified signal(s) | nwnrs annotation identities |
| --- | --- | --- |
| ✅ Ported | `NWNX_ON_USE_SKILL_{BEFORE,AFTER}` | `skill_use_before`<br>`skill_use_after` |

### Spell (0/7)

| Status | Unified signal(s) | nwnrs annotation identities |
| --- | --- | --- |
| ⬜ Pending | `NWNX_CLEAR_MEMORIZED_SPELL_SLOT_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_BROADCAST_CAST_SPELL_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_CAST_SPELL_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_DECREMENT_SPELL_COUNT_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_SPELL_FAILED_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_SPELL_INTERRUPTED_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_SET_MEMORIZED_SPELL_SLOT_{BEFORE,AFTER}` | — |

### Stealth (0/6)

| Status | Unified signal(s) | nwnrs annotation identities |
| --- | --- | --- |
| ⬜ Pending | `NWNX_ON_DETECT_ENTER_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_DETECT_EXIT_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_DO_LISTEN_DETECTION_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_DO_SPOT_DETECTION_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_STEALTH_ENTER_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_STEALTH_EXIT_{BEFORE,AFTER}` | — |

### Store (0/2)

| Status | Unified signal(s) | nwnrs annotation identities |
| --- | --- | --- |
| ⬜ Pending | `NWNX_ON_STORE_REQUEST_BUY_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_STORE_REQUEST_SELL_{BEFORE,AFTER}` | — |

### Timing bar (3/3)

| Status | Unified signal(s) | nwnrs annotation identities |
| --- | --- | --- |
| ✅ Ported | `NWNX_ON_TIMING_BAR_CANCEL_{BEFORE,AFTER}` | `timing_bar_cancel_before`<br>`timing_bar_cancel_after` |
| ✅ Ported | `NWNX_ON_TIMING_BAR_START_{BEFORE,AFTER}` | `timing_bar_start_before`<br>`timing_bar_start_after` |
| ✅ Ported | `NWNX_ON_TIMING_BAR_STOP_{BEFORE,AFTER}` | `timing_bar_stop_before`<br>`timing_bar_stop_after` |

### Trap (0/6)

| Status | Unified signal(s) | nwnrs annotation identities |
| --- | --- | --- |
| ⬜ Pending | `NWNX_ON_TRAP_DISARM_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_TRAP_ENTER_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_TRAP_EXAMINE_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_TRAP_FLAG_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_TRAP_RECOVER_{BEFORE,AFTER}` | — |
| ⬜ Pending | `NWNX_ON_TRAP_SET_{BEFORE,AFTER}` | — |

### UUID (0/1)

| Status | Unified signal(s) | nwnrs annotation identities |
| --- | --- | --- |
| ⬜ Pending | `NWNX_ON_UUID_COLLISION_{BEFORE,AFTER}` | — |
