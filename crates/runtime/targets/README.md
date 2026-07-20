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
- versioned capability blocks: `bridge`, optional `server_state`, optional
  `administration`, and optional `events`.

Administration packs declare their shutdown mechanism explicitly. Unix packs
use a verified engine exit-flag address; Windows uses the current engine
thread's message queue and therefore does not invent an address for an absent
global.

An absent optional block means that capability is unavailable. A present block
must be complete and use the one contract version supported by this runtime.
NWScript can inspect these versions with `NWNRS_GetCapabilityVersion` and
`NWNRS_HasCapability`.

Event capability version 2 contains exact addresses for
`CNWSModule::LoadModuleFinish`, global
`g_pVirtualMachine`, and `CVirtualMachine::RunScript`. These implement the
native `_nwnrs_onload` bootstrap without assigning or patching the module's
vanilla `Mod_OnModLoad` field.

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
