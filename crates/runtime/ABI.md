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
- `NWNXLib/API/API/CExoArrayList.hpp`: player-list header;
- `NWNXLib/API/API/Vector.hpp`: NWScript vector ABI;
- `NWNXLib/API/API/CVirtualMachineCmdImplementer.hpp`: VM pointer;
- `NWNXLib/API/API/CVirtualMachine.hpp`: recursion and script slots;
- `NWNXLib/API/API/CNWSVirtualMachineCommands.hpp`: NWNX command entry point;
- `NWNXLib/API/API/CVirtualMachineScript.hpp`: event context;
- `NWNXLib/API/API/CAppManager.hpp`: server application pointer;
- `NWNXLib/API/API/CServerInfo.hpp`: module-name field;
- `NWNXLib/API/API/CServerExoApp.hpp`: server accessors;
- `NWNXLib/API/API/CNetLayer.hpp`: session-player limit and UDP listening port.

The probe compiles exact member-function pointer assertions for every hooked
or called method in addition to emitting object layouts. A signature change in
Unified therefore fails the platform check before Rust is compiled against a
new pack.

## Rules

- C++ engine objects remain opaque outside `nwnrs-runtime-sys`.
- Only trivial ABI values receive Rust `repr(C)` definitions.
- Standard-library containers are never modeled in Rust; only probe-verified
  header fields may be copied while executing on the VM thread.
- Capability blocks are complete and versioned. An absent block means the
  capability is unavailable.
- A target pack with mismatched provenance, layout, platform, API version, or
  binary hash is rejected before hooks are installed.
- New unsafe operations require a Unified declaration, target-pack data, a
  fixture test, and a real-server verification case.

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
  event ID, script name, phase, and depth through `nwnrs::script`;
- Ctrl-C reaches a clean NWServer shutdown.

The fixture runner exercises the same contract without proprietary binaries:

```bash
crates/runtime/scripts/test-native-runtime.sh
```
