# nwnrs-runtime

Safe runtime configuration, executable identification, exact target-pack
selection, and the typed NWScript call dispatcher shared by the native
launcher and injected runtime.

This crate contains no native engine access. It owns the public bridge
contract, validates calls, and requests one typed operation at a time through
`RuntimeHost`. The injected `nwnrs-runtime-sys` crate is the only production
implementation of that interface.

This directory is the source of truth for the safe runtime crate:

- `targets/` contains exact server-binary target packs.
- `abi/` contains the compiler probe for the pinned Unified headers.
- `scripts/` contains target-pack and ABI verification tooling.

Native injected-runtime fixtures and their cross-platform runner belong to
`nwnrs-runtime-sys`, alongside the implementation they exercise.

Frida Gum is supplied to the native boundary by the Cargo-managed
`frida-gum-sys` dependency. Its `auto-download` feature obtains the matching
native devkit for the build target.

The dispatcher exposes runtime identity, live server state, and administration
operations through the static `NWNRS` namespace. Administration calls include
session settings, access restrictions, ban lists, debug toggles, graceful
shutdown, rules reload, TURD recovery, and deferred server-vault character
deletion. Every argument is validated in this safe crate before one typed
`RuntimeHost` operation reaches the native crate. During native event-script execution the
dispatcher also reports the stable event name, engine identifier, script
resref, phase, and nesting depth. NWScript can emit trace, debug, info, warning,
and error records through the runtime's structured tracing pipeline. None of
this requires a plugin loader, HTTP API, or metrics service.

The NWScript contract is integer-versioned and statically registered. Scripts
can query the core, server-state, administration, and event-context capability
versions before using optional functions. Dispatch failures retain a stable
error code and a diagnostic message on the current bridge thread.

Native ABI provenance and regeneration rules are documented in
[`ABI.md`](ABI.md).

Module source includes [`nwnrs.nss`](../../module/nwnrs.nss). The
source-controlled demo module compiles `module/nwnrs_init.nss` into its
module-load event, preserves the stock `x2_mod_def_load` behavior, and writes a
runtime, server, and module-load-event summary at startup.
