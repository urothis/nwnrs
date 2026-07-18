# nwnrs-runtime-sys

The isolated native boundary for the injected nwnrs runtime. This crate owns
Frida Gum calls, process-loader initialization, native callbacks, and the small
amount of unsafe code required by those interfaces.

At injected startup it resolves the exact target pack's minimal VM ABI and
replaces `ExecuteCommandNWNXFunctionManagement`. Integer, float, object,
string, and vector stack operations are copied across the native boundary;
the safe `nwnrs-runtime` crate owns dispatch and return state. Hash-specific
server accessors supply the live module name, player count, and maximum
players on the same server thread as the NWScript call.

The same exact target pack records the live `CVirtualMachineScript` layout.
During each NWScript bridge call, the runtime copies the current script name,
engine event identifier, and recursion depth directly from that VM slot. This
makes module, area, and object event context visible without adding event
hooks or changing engine event behavior.

Validated NWScript log calls are emitted through `tracing` under the
`nwnrs::script` target. A supervising launcher preserves their requested level;
directly preloaded servers render them from the injected runtime itself.

Build and execute the platform interception probe with:

```console
cargo run -p nwnrs-runtime-sys --example frida-probe
```
