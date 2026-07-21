# Runtime target packs

The runtime selects exactly one pack by platform and the complete SHA-256 of
the NWServer executable:

```text
<os>-<architecture>/<server-sha256>.toml
```

There is no fallback, version range, or nearest-match behavior. Schema 2
contains four kinds of evidence:

- `server`: exact binary identity and human-readable build;
- `source`: the full Unified commit and its NWN build tuple;
- `layouts`: compiler-measured sizes, alignments, and member offsets;
- an unversioned `bridge` block and optional `server_state`, `administration`,
  and `events` target blocks.

Administration packs declare their shutdown mechanism explicitly. Unix packs
use a verified engine exit-flag address; Windows uses the current engine
thread's message queue and therefore does not invent an address for an absent
global.

An absent optional block means that functionality is unavailable.
`NWNRS_HasCapability` reports block presence without a version. Event support
is checked per identity with `NWNRS_GetEventSupported`.

The event target map contains exact addresses for global
`g_pVirtualMachine`, `CVirtualMachine::RunScript`, and a keyed map of physical
engine boundaries under `events.hooks`. The `module_load` hook points at
`CNWSModule::LoadModuleFinish` and implements the native `_nwnrs_onload`
bootstrap without assigning or patching the module's vanilla `Mod_OnModLoad`
field. Every event target map requires that bootstrap hook. Hook and helper
keys must exist in the shared event catalog; unknown keys and duplicate
resolved hook addresses are rejected. Hook keys are stable across platforms while each value is the symbol or
offset for that exact executable; multiple logical event phases sharing one
engine function use one map entry and one detour.
`events.functions` separately records callable engine helpers needed to build
owned payload data; these addresses are never installed as detours. The
machine-probed `layouts.classes.game_object_id_offset` supplies the exact
`CGameObject::m_idSelf` offset. `NWNRS_GetEventSupported` checks an individual
catalog identity against its required hook and helper on the active target.

Addresses are either exact native symbols:

```toml
[bridge.stack_pop_integer]
symbol = "<exact native symbol>"
```

or module-relative runtime offsets:

```toml
[bridge.stack_pop_integer]
offset = 123456
```

Offsets are relative to the loaded main executable, not file offsets. Symbols
and offsets come from the hash-named executable. They are not inferred from
Unified.

Unified is the source of truth for declarations and semantics. The ABI probe
is the source of truth for target-platform layouts. Generate and compare the
probe evidence with:

```bash
crates/runtime/scripts/verify-unified-abi.sh \
  sources/unified \
  target/unified-abi.toml
```

On Windows, use `crates\runtime\scripts\verify-unified-abi.ps1` with the same
two arguments.

See [`../ABI.md`](../ABI.md) for the provenance rules and audited headers.
