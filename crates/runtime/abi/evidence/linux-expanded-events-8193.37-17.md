# Linux expanded-event evidence: NWN 8193.37-17

This file records the binary-symbol and compiler-measured layout evidence used
to enable the Object, Feat, Inventory, and Item event expansion on both Linux
server architectures.

## Exact binaries

Both binaries were extracted from
`ghcr.io/urothis/nwserver:8193.37.17`, OCI index digest
`sha256:3adffb77ad86aeff6abef56300f797189fede8aa83d348a15c971e633e974121`.

| Platform | OCI manifest | NWServer SHA-256 | ELF build ID |
| --- | --- | --- | --- |
| `linux/amd64` | `sha256:3803f78a7f7a37e777f28ea9d9b246523d9531fdd4f7ba4d57aa39ac71ce44d2` | `d6d952826b671fc62c376dca912e87f1aa45d4aa81c620e912ceb4d8e051111d` | `3f0beeb9434ae21995d4b03c8b20989a3d09d132` |
| `linux/arm64` | `sha256:1efc0423880a9ac1be803757b56b8317221e2ccfdfe9e755bddee266f5807a16` | `69b3705d831a48386e6ce1490a0ecaa521485f68b2351d0b92853d644d4ffdcf` | `a4100dcbe5ee04f1403854048ed05305bf4c1a72` |

`file` and `readelf -h` identify the first binary as ELF64 x86-64 and the
second as ELF64 AArch64. Their complete SHA-256 values exactly match the
source-controlled target-pack filenames.

## Expanded event symbols

`nm -D --defined-only` reports every symbol below in both exact binaries. The
complete mangled names are identical across the two Linux architectures and
match the member-function signatures asserted by the pinned Unified ABI probe.

| Target-pack field | Complete ELF symbol |
| --- | --- |
| `hooks.object_set_experience` | `_ZN17CNWSCreatureStats13SetExperienceEji` |
| `hooks.feat_decrement_remaining_uses` | `_ZN17CNWSCreatureStats26DecrementFeatRemainingUsesEt` |
| `hooks.feat_has` | `_ZN17CNWSCreatureStats7HasFeatEt` |
| `hooks.inventory_message` | `_ZN11CNWSMessage39HandlePlayerToServerGuiInventoryMessageEP10CNWSPlayerh` |
| `hooks.inventory_add_item` | `_ZN15CItemRepository7AddItemEPP8CNWSItemhhii` |
| `hooks.inventory_remove_item` | `_ZN15CItemRepository10RemoveItemEP8CNWSItem` |
| `hooks.item_validate_use` | `_ZN12CNWSCreature10CanUseItemEP8CNWSItemi` |
| `hooks.item_ammo_reload` | `_ZN15CItemRepository22FindItemWithBaseItemIdEji` |
| `hooks.item_validate_equip` | `_ZN12CNWSCreature12CanEquipItemEP8CNWSItemPjiiiP10CNWSPlayer` |
| `hooks.item_equip` | `_ZN12CNWSCreature8RunEquipEjjj` |
| `hooks.item_unequip` | `_ZN12CNWSCreature10RunUnequipEjjhhij` |
| `hooks.item_split` | `_ZN12CNWSCreature9SplitItemEP8CNWSItemi` |
| `hooks.item_merge` | `_ZN12CNWSCreature9MergeItemEP8CNWSItemS1_` |
| `hooks.item_acquire` | `_ZN12CNWSCreature11AcquireItemEPP8CNWSItemjjhhii` |
| `functions.stats_get_feat_remaining_uses` | `_ZN17CNWSCreatureStats20GetFeatRemainingUsesEt` |
| `functions.server_get_game_object` | `_ZN13CServerExoApp13GetGameObjectEj` |
| `functions.server_get_client_object` | `_ZN13CServerExoApp25GetClientObjectByObjectIdEj` |
| `functions.server_get_nws_message` | `_ZN13CServerExoApp13GetNWSMessageEv` |
| `functions.inventory_status` | `_ZN11CNWSMessage37SendPlayerToServerGuiInventory_StatusEP10CNWSPlayerij` |
| `functions.inventory_gui_set_open` | `_ZN22CNWSPlayerInventoryGUI7SetOpenEii` |
| `functions.inventory_select_panel` | `_ZN11CNWSMessage39SendServerToPlayerInventory_SelectPanelEjh` |
| `functions.inventory_equip_cancel` | `_ZN11CNWSMessage39SendServerToPlayerInventory_EquipCancelEjjji` |
| `functions.inventory_unequip_cancel` | `_ZN11CNWSMessage41SendServerToPlayerInventory_UnequipCancelEjji` |

## Native layout probes

The ABI probe at Unified commit
`3d4c4e13c6bf01b032ffe90534fc4a19eb036c03` was compiled and executed in
native-architecture Debian 12 containers for `linux/amd64` and
`linux/arm64`. Both snapshots measured these expanded-event fields:

| Target-pack field | Offset |
| --- | ---: |
| `game_object_type_offset` | `12` |
| `item_repository_parent_offset` | `8` |
| `creature_stats_base_creature_offset` | `48` |
| `creature_stats_experience_offset` | `168` |
| `item_base_item_offset` | `64` |
| `item_possessor_offset` | `960` |
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

The Linux `item_possessor_offset` is `960`; the macOS AArch64 value is `944`.
The Linux packs therefore use independently measured layouts rather than
copied macOS values.

## Runtime acceptance

The native injected-runtime fixture passed on both Linux architectures. Each
run generated a target pack for its native fixture executable, subscribed all
supported catalog identities, dispatched both phases of the 15 expanded
events, and verified all 15 original native calls.

The source-controlled `nwnrs` module was also started against both exact
NWServer binaries from the image above using release runtime artifacts built
in the repository's pinned Debian 12 builder stage. On `linux/amd64` and
`linux/arm64`:

- the runtime selected the hash-named source-controlled target pack and
  initialized successfully;
- the generated `_nwnrs_onload` script emitted the expected `module.load`
  payload;
- NWServer reached `Server: Module loaded`;
- a graceful shutdown reached `Server: Exiting` and the launcher reported a
  successful exit.

The ARM64 fixture and real-server runs used Docker's registered AArch64
emulation on the x86_64 development host. CI also runs the same native fixture
on its `ubuntu-24.04-arm` runner.
