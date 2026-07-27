# Unified event behavior decisions

The machine-checked decision ledger is
[`event-decisions.toml`](event-decisions.toml). These decisions remove the
remaining ambiguity between the pinned Unified C++ implementation, its shared
NWScript constants, and the native nwnrs API.

## Governing rules

1. `Plugins/Events/Events/*.cpp` at the pinned commit is the inventory and
   behavior authority.
2. A public Unified constant without an Events-plugin producer is not included.
3. A C++ signal without a public constant is included under an nwnrs semantic
   identity.
4. nwnrs corrects subscription-registration defects rather than reproducing
   them.
5. Skip and result behavior is declared per logical route. After-event results
   are mutable only where Unified consumes the result.
6. Unified's string transport is translated into a catalog-declared typed
   result. Native event families never parse result JSON.
7. Every exceptional decision has a concrete fixture and real-server
   acceptance statement in the ledger.

## Port checklist

Before adding a logical route, record:

- exact Unified source file, function, signal, and pinned commit;
- physical hook and versioned native ABI;
- before/after or one-shot phase behavior;
- target object and typed payload fields;
- observe, suppress, replacement-return, or replacement-argument control;
- result kind and whether an after result may replace the native return;
- reentrancy policy and any cross-hook correlation owner/key;
- normal or hot frequency and the pre-decode filtering policy;
- required native helpers and compiler-probed layouts;
- fixture stimulus and real-server stimulus;
- evidence for Linux x86_64, Linux ARM64, macOS ARM64, and Windows x86_64.

An event is not marked ported until all checklist fields and all four target
results are present.
