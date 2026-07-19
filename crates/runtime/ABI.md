# Runtime ABI provenance

The runtime does not infer Neverwinter Nights engine layouts. Every native
declaration is derived from `nwnxee/unified`, and every address is selected by
the complete SHA-256 of one server executable.

The current schema targets Unified commit
`3d4c4e13c6bf01b032ffe90534fc4a19eb036c03`, whose build declarations are
8193, revision 37, postfix 17.

## Authorities

| Fact | Authority |
| --- | --- |
| Function signature, field type/order, constants | Pinned Unified headers |
| Size, alignment, member offset | `abi-probe.cpp` compiled on the target platform |
| Native symbol or module-relative address | Exact server executable selected by SHA-256 |
| Ownership and runtime behavior | Real NWServer bridge self-test |

The relevant Unified declarations are:

- `NWNXLib/API/nwn_api.hpp`: `ObjectID` and build contract;
- `NWNXLib/API/Constants/Base.hpp`: `OBJECT_INVALID`;
- `NWNXLib/API/API/CExoString.hpp`: engine string layout and ownership;
- `NWNXLib/API/API/CExoBase.hpp`: engine alias-list ownership;
- `NWNXLib/API/API/CExoAliasList.hpp`: `SERVERVAULT` path resolution;
- `NWNXLib/API/API/CExoLocString.hpp`: localized character-name lookup;
- `NWNXLib/API/API/CExoLinkedListInternal.hpp`: TURD list ownership and removal;
- `NWNXLib/API/API/CExoLinkedListNode.hpp`: TURD list traversal;
- `NWNXLib/API/API/CExoArrayList.hpp`: player-list header;
- `NWNXLib/API/API/Vector.hpp`: NWScript vector ABI;
- `NWNXLib/API/API/CVirtualMachineCmdImplementer.hpp`: VM pointer;
- `NWNXLib/API/API/CVirtualMachine.hpp`: recursion and script slots;
- `NWNXLib/API/API/CNWSVirtualMachineCommands.hpp`: NWNX command entry point;
- `NWNXLib/API/API/CVirtualMachineScript.hpp`: event context;
- `NWNXLib/API/API/CAppManager.hpp`: server application pointer;
- `NWNXLib/API/API/CServerInfo.hpp`: module name, joining restrictions, and play options;
- `NWNXLib/API/API/CPersistantWorldOptions.hpp`: server-vault directory policy;
- `NWNXLib/API/API/CJoiningRestrictions.hpp`: minimum and maximum levels;
- `NWNXLib/API/API/CPlayOptions.hpp`: live administration option fields;
- `NWNXLib/API/API/CServerExoApp.hpp`: server accessors;
- `NWNXLib/API/API/CNWSModule.hpp`: module TURD-list location;
- `NWNXLib/API/API/CNWSPlayerTURD.hpp`: player and character identity fields;
- `NWNXLib/API/API/CNWSPlayer.hpp`: player ID, vault resref, and community name;
- `NWNXLib/API/API/CNWSCreature.hpp`: live creature-stat ownership;
- `NWNXLib/API/API/CNWSCreatureStats.hpp`: localized character identity;
- `NWNXLib/API/API/CNetLayer.hpp`: session identity, passwords, player limit,
  UDP listening port, player lookup, and disconnection;
- `NWNXLib/API/API/CNetLayerPlayerInfo.hpp`: active player CD-key data;
- `NWNXLib/API/API/CNetLayerPlayerCDKeyInfo.hpp`: public CD-key field;
- `NWNXLib/API/Globals.hpp`: debug toggles and graceful-exit flag.

The probe compiles exact member-function pointer assertions for every hooked
or called method in addition to emitting object layouts. A signature change in
Unified therefore fails the platform check before Rust is compiled against a
new pack.

## Rules

- C++ engine objects remain opaque outside `nwnrs-runtime-sys`.
- Only trivial ABI values receive Rust `repr(C)` definitions.
- Standard-library containers are never modeled in Rust; only probe-verified
  header fields may be copied while executing on the VM thread.
- Non-trivial `CExoString` returns and by-value parameters cross a compiled C++
  thunk so each target compiler applies its own hidden-return convention.
- Capability blocks are complete and versioned. An absent block means the
  capability is unavailable.
- A target pack with mismatched provenance, layout, platform, API version, or
  binary hash is rejected before hooks are installed.
- New unsafe operations require a Unified declaration, target-pack data, a
  fixture test, and a real-server verification case.
- Destructive player-character work is queued during the NWScript call and
  drained from the verified `CServerExoAppInternal::MainLoop` hook before the
  next engine tick. The queue retains owned values, never engine pointers.

Run the platform probe with:

```bash
crates/runtime/scripts/verify-unified-abi.sh \
  sources/unified \
  target/unified-abi.toml
```

## Real-server acceptance

For every new exact-hash pack, build the source-controlled module and start the
server through `nwnrs run` with that pack. Acceptance requires all of the
following before the pack is released:

- the runtime initializes and the module reaches `Server: Module loaded`;
- `NWNRS_GetApiVersion()` equals `NWNRS_API_VERSION`;
- every declared capability reports its target-pack version;
- the module-load callback reports the correct module, player state, UDP port,
  server name, access restrictions, event ID, script name, phase, and depth
  through `nwnrs::script`;
- Ctrl-C reaches a clean NWServer shutdown.

The native crate's fixture runner exercises the same contract without
proprietary binaries:

```bash
crates/runtime-sys/scripts/test-native-runtime.sh
```
