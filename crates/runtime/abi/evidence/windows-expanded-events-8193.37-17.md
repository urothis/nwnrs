# Windows expanded-event evidence: NWN 8193.37-17

This file records the binary-export and compiler-measured layout evidence used
to enable the Object, Feat, Inventory, and Item event expansion on Windows
x86_64.

## Exact binary

The Steam installation at
`Neverwinter Nights\bin\win32\nwserver.exe` has SHA-256
`98951511ae7d06a251355f12d4f3e4b96269a2414fa3058e641afd70098690f6`.
That complete hash exactly matches the source-controlled Windows target-pack
filename.

## Expanded event exports

MSVC `dumpbin /exports` reports every symbol below in the exact executable.
The complete decorated names match the member-function signatures asserted by
the pinned Unified ABI probe.

| Target-pack field | Complete PE export |
| --- | --- |
| `hooks.object_set_experience` | `?SetExperience@CNWSCreatureStats@@QEAAXIH@Z` |
| `hooks.feat_decrement_remaining_uses` | `?DecrementFeatRemainingUses@CNWSCreatureStats@@QEAAXG@Z` |
| `hooks.feat_has` | `?HasFeat@CNWSCreatureStats@@QEAAHG@Z` |
| `hooks.inventory_message` | `?HandlePlayerToServerGuiInventoryMessage@CNWSMessage@@QEAAHPEAVCNWSPlayer@@E@Z` |
| `hooks.inventory_add_item` | `?AddItem@CItemRepository@@QEAAHPEAPEAVCNWSItem@@EEHH@Z` |
| `hooks.inventory_remove_item` | `?RemoveItem@CItemRepository@@QEAAHPEAVCNWSItem@@@Z` |
| `hooks.item_validate_use` | `?CanUseItem@CNWSCreature@@QEAAHPEAVCNWSItem@@H@Z` |
| `hooks.item_ammo_reload` | `?FindItemWithBaseItemId@CItemRepository@@QEAAIIH@Z` |
| `hooks.item_validate_equip` | `?CanEquipItem@CNWSCreature@@QEAAEPEAVCNWSItem@@PEAIHHHPEAVCNWSPlayer@@@Z` |
| `hooks.item_equip` | `?RunEquip@CNWSCreature@@QEAAHIII@Z` |
| `hooks.item_unequip` | `?RunUnequip@CNWSCreature@@QEAAHIIEEHI@Z` |
| `hooks.item_split` | `?SplitItem@CNWSCreature@@QEAAXPEAVCNWSItem@@H@Z` |
| `hooks.item_merge` | `?MergeItem@CNWSCreature@@QEAAXPEAVCNWSItem@@0@Z` |
| `hooks.item_acquire` | `?AcquireItem@CNWSCreature@@QEAAHPEAPEAVCNWSItem@@IIEEHH@Z` |
| `functions.stats_get_feat_remaining_uses` | `?GetFeatRemainingUses@CNWSCreatureStats@@QEAAEG@Z` |
| `functions.server_get_game_object` | `?GetGameObject@CServerExoApp@@QEAAPEAVCGameObject@@I@Z` |
| `functions.server_get_client_object` | `?GetClientObjectByObjectId@CServerExoApp@@QEAAPEAVCNWSPlayer@@I@Z` |
| `functions.server_get_nws_message` | `?GetNWSMessage@CServerExoApp@@QEAAPEAVCNWSMessage@@XZ` |
| `functions.inventory_status` | `?SendPlayerToServerGuiInventory_Status@CNWSMessage@@QEAAHPEAVCNWSPlayer@@HI@Z` |
| `functions.inventory_gui_set_open` | `?SetOpen@CNWSPlayerInventoryGUI@@QEAAXHH@Z` |
| `functions.inventory_select_panel` | `?SendServerToPlayerInventory_SelectPanel@CNWSMessage@@QEAAHIE@Z` |
| `functions.inventory_equip_cancel` | `?SendServerToPlayerInventory_EquipCancel@CNWSMessage@@QEAAHIIIH@Z` |
| `functions.inventory_unequip_cancel` | `?SendServerToPlayerInventory_UnequipCancel@CNWSMessage@@QEAAHIIH@Z` |

## Native layout probe

The ABI probe at Unified commit
`3d4c4e13c6bf01b032ffe90534fc4a19eb036c03` was compiled with MSVC 19.44
and executed natively on Windows x86_64. It measured these expanded-event
fields:

| Target-pack field | Offset |
| --- | ---: |
| `game_object_type_offset` | `12` |
| `item_repository_parent_offset` | `8` |
| `creature_stats_base_creature_offset` | `48` |
| `creature_stats_experience_offset` | `168` |
| `item_base_item_offset` | `64` |
| `item_possessor_offset` | `968` |
| `message_read_buffer_offset` | `56` |
| `message_read_buffer_size_offset` | `64` |
| `message_read_buffer_position_offset` | `68` |
| `message_read_fragments_size_offset` | `80` |
| `message_read_fragments_position_offset` | `84` |
| `message_current_read_bit_offset` | `92` |
| `message_last_byte_bits_offset` | `93` |
| `player_object_id_offset` | `84` |
| `player_inventory_gui_offset` | `152` |
| `player_other_inventory_gui_offset` | `160` |
| `inventory_gui_selected_panel_offset` | `12` |

The Windows `item_possessor_offset` is `968`; the Linux value is `960` and
the macOS ARM64 value is `944`. The Windows pack therefore uses its native
MSVC layout rather than a copied Unix value.

## Runtime acceptance

The native injected-runtime fixture was built with the pinned Rust toolchain,
MSVC 19.44, and LLVM 22.1.8. A `test-native-runtime.ps1 -SkipGui` run
initialized the runtime, subscribed every supported catalog identity,
completed the injection and event assertions, and exited successfully.

The source-controlled `nwnrs` module was also started through `nwnrs` against
the exact Steam executable. The launcher selected this target pack, initialized
the runtime before resuming NWServer, and the generated `_nwnrs_onload`
dispatcher emitted the expected `module.load` payload. After module loading
completed, an interactive `quit` request produced a clean NWServer exit with
status `0`.

The fixture's separate GUI-theme run requires an interactive desktop and
cannot validate its window assertion from an OpenSSH service session. That
limitation is unrelated to event hooks; interactive GUI validation remains a
separate Windows acceptance step.
