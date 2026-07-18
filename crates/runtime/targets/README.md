# Runtime target packs

Target packs are selected by operating system, architecture, and the exact
SHA-256 of the server executable:

```text
crates/runtime/targets/<os>-<architecture>/<server-sha256>.toml
```

No fallback or nearest-version matching is performed.

Schema 2 also records the complete minimal ABI used by the NWScript bridge:

```toml
schema_version = 2
runtime_api = 2

[server]
sha256 = "<complete lowercase server SHA-256>"
build = "<human-readable server build>"

[server.platform]
os = "linux"
architecture = "x86_64"

[bridge]
virtual_machine_offset = 16

[bridge.function_management]
symbol = "<exact native symbol>"

[bridge.stack_pop_integer]
symbol = "<exact native symbol>"

[bridge.stack_push_integer]
symbol = "<exact native symbol>"

[bridge.stack_pop_float]
symbol = "<exact native symbol>"

[bridge.stack_push_float]
symbol = "<exact native symbol>"

[bridge.stack_pop_object]
symbol = "<exact native symbol>"

[bridge.stack_push_object]
symbol = "<exact native symbol>"

[bridge.stack_pop_string]
symbol = "<exact native symbol>"

[bridge.stack_push_string]
symbol = "<exact native symbol>"

[bridge.stack_pop_vector]
symbol = "<exact native symbol>"

[bridge.stack_push_vector]
symbol = "<exact native symbol>"

[bridge.free_exo_string_buffer]
symbol = "<exact native symbol>"

[server_state]
server_exo_app_offset = 8
server_info_module_name_offset = 8
player_list_count_offset = 8

[server_state.app_manager]
symbol = "<global CAppManager pointer storage>"

[server_state.get_server_info]
symbol = "<exact native symbol>"

[server_state.get_player_list]
symbol = "<exact native symbol>"

[server_state.get_net_layer]
symbol = "<exact native symbol>"

[server_state.get_session_max_players]
symbol = "<exact native symbol>"

[events]
recursion_level_offset = 36
script_array_offset = 40
script_slot_count = 8
script_stride = 152
script_name_offset = 24
script_event_id_offset = 72
```

The example uses the Linux `CVirtualMachineScript` stride of 152 bytes. The
current macOS ARM64 binary uses 136 bytes because its C++ standard-library
container layout differs; this is why the stride remains exact target data.

Each address may instead use `offset = <module-relative-address>` when the
exact binary does not retain a trustworthy symbol. Offsets are relative to the
main executable's runtime load address, not file offsets.

The event offsets locate the active `CVirtualMachineScript` slot, its
`CExoString` script name, and its engine event identifier. The runtime reads
this state only while servicing a bridge call from that same VM thread. It
does not hook, replace, or retain pointers to the engine event functions.

The API declarations in `sources/unified` are the current semantic reference
for function signatures, object types, event identifiers, and class layout.
The executable named by `sha256` is the final authority for every symbol,
address, and offset recorded here.
