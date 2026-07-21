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
`RuntimeHost` operation reaches the native crate. Native hooks expose one
immutable JSON event snapshot containing common context and event-specific
data. Skip and result changes are separate schema-checked commands. NWScript
can emit trace, debug, info, warning, and error records through the runtime's
structured tracing pipeline. None of this requires a plugin loader, HTTP API,
or metrics service.

The NWScript contract is integer-versioned and statically registered. Scripts
can query the core, server-state, administration, and events capability
versions before using optional functions. Dispatch failures retain a stable
error code and a diagnostic message on the current bridge thread.

Native ABI provenance and regeneration rules are documented in
[`ABI.md`](ABI.md).

The [`nwnrs.nss`](../../include/nwnrs/nwnrs.nss) include is a separate local
`nwpkg` dependency of the source-controlled demo module. Its compiler-only
[`nwnrs_macros.nss`](../../include/nwnrs/nwnrs_macros.nss) procedural macro
collects `#[nwnrs::events(module_load)]` and native event phase functions
during the project preprocessing pass and quotes the always-present
`_nwnrs_onload` dispatcher.
Handlers use the exact `void Handler(json event)` signature. The dispatcher is
then compiled and baked by `nwpkg`; it retrieves and parses the current payload
once and passes the same value only to handlers registered for that event and
phase. The native runtime runs that dispatcher at
`CNWSModule::LoadModuleFinish`, before the original engine function, while
the module's vanilla `Mod_OnModLoad` remains independently assigned to
`x2_mod_def_load`.

The stable event envelope is:

```json
{
  "name": "module.load",
  "id": 3002,
  "script": "_nwnrs_onload",
  "phase": "before",
  "depth": 1,
  "target": "00000000",
  "controls": { "skippable": false, "result": false },
  "data": {}
}
```

Object identifiers are serialized as eight lowercase hexadecimal digits,
vectors as `{ "x", "y", "z" }`, and locations as an area identifier,
position, and facing. Native strings and compound values are copied before the
dispatcher runs; no borrowed engine pointer enters JSON. Event frames are
nested, bounded, and removed by scope cleanup even when dispatch fails.

The first native hook family uses
`#[nwnrs::events(associate_add_before)]`,
`#[nwnrs::events(associate_add_after)]`,
`#[nwnrs::events(associate_remove_before)]`, and
`#[nwnrs::events(associate_remove_after)]`. Familiar possession uses the same
pattern with `associate_possess_familiar_*` and
`associate_unpossess_familiar_*`. Payload names are `associate.add`,
`associate.remove`, `associate.possess_familiar`, and
`associate.unpossess_familiar`. `data.associate` is an object ID,
`data.associate_type` is an integer for add events, and familiar events expose
`data.familiar`. Their before phases are skippable. Object lock, unlock, use,
placeable open/close and safe-projectile hooks live in the separate Object
family. Inventory gold mutation, feat use, skill use, and item operations each
have their own family module. All paired NWScript registrations use stable
`<family>_<operation>_before` and `<family>_<operation>_after` identities.
Journal open/close and timing-bar start/stop/cancel events use a verified
player-to-game-object helper instead of hard-coded `CNWSPlayer` field offsets.
