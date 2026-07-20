# nwnrs-runtime-sys

The isolated native boundary for the injected nwnrs runtime. This crate owns
Frida Gum calls, process-loader initialization, native callbacks, and the small
amount of unsafe code required by those interfaces.

It contains no NWScript API policy: argument validation, function semantics,
return-state management, and administration command construction remain in
the safe `nwnrs-runtime` crate.

The boundary is split into typed engine modules for VM stack transport,
server state, event context, engine strings, native addresses, and VM-thread
access. Resolved addresses are converted to their exact function types once.
Opaque C++ objects never escape this crate, owned engine strings use an RAII
deallocator, and every engine read requires a non-`Send`, non-`Sync` callback
thread token.

At injected startup it resolves the exact target pack's minimal VM ABI and
replaces `ExecuteCommandNWNXFunctionManagement`. Integer, float, object,
string, and vector stack operations are copied across the native boundary;
the safe `nwnrs-runtime` crate owns dispatch and return state. Hash-specific
server accessors implement its `RuntimeHost` contract and read only the value
requested by each call. Mutations execute synchronously, so native failures
belong to the same bridge call. These accessors expose the live module name,
player count, maximum players, and active UDP port, which base NWScript does
not provide. Administration mutations use separately verified engine methods
and field offsets; TURD recovery traverses the engine-owned linked list and
removes only an exact community-name and full-character-name match.
Player-character deletion is prepared during the NWScript callback using only
owned identity and path data, then drained through a separately verified main
loop hook. The deferred operation disconnects the player, creates a unique
byte-identical `.deletedN` hard-link backup when requested, removes the active
BIC, and cleans the matching TURD without retaining an engine pointer between
ticks.

The same exact target pack records the live `CVirtualMachineScript` layout.
During each NWScript bridge call, the runtime copies the current script name,
engine event identifier, and recursion depth directly from that VM slot. This
makes module, area, and object event context visible without adding event
hooks or changing engine event behavior.

Validated NWScript log calls are emitted through `tracing` under the
`nwnrs::script` target. A supervising launcher preserves their requested level;
directly preloaded servers render them from the injected runtime itself.
Multiline messages are emitted as one event per line so every line retains the
`nwnrs::script` target and requested level on every supported platform.

The Windows runtime keeps the NWServer control-panel window hidden by default
while preserving its message loop and native controls for engine compatibility.
The `nwnrs run --gui` flag makes it visible. The runtime observes creation of
the exact control-panel window and applies a native dark theme before it becomes visible. It uses DWM
for the frame, standard control themes and control-color messages for inputs
and lists, and paint-only subclasses for legacy buttons, checkboxes, combo
arrows, and numeric spinners. The subclasses delegate all non-paint messages
to the original controls, preserving normal keyboard, mouse, and command
behavior.

Build and execute the platform interception probe with:

```console
cargo run -p nwnrs-runtime-sys --example frida-probe
```

Run the complete injected-runtime fixture with:

```console
crates/runtime-sys/scripts/test-native-runtime.sh
```

On Windows, the MSVC-built fixture exercises PE target selection, suspended
process startup, DLL initialization, native calling conventions, bridge calls,
administration operations, and clean shutdown:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File `
  crates\runtime-sys\scripts\test-native-runtime.ps1
```
