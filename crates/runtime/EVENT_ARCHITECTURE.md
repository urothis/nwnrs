# Event runtime architecture

This document defines the implementation rules for event ports. The
authoritative logical and physical inventories live in `src/event_catalog.rs`;
this document explains how native implementations consume those inventories.

## Logical routes and physical hooks

- An annotation identity is one logical route. Before and after routes have
  independent `EventKey` values and subscription bits.
- A `PhysicalHookDefinition` is one native detour. Every native logical route
  names exactly one physical hook.
- A family replacement multiplexes every logical route produced by that
  detour. The runtime never installs two replacements at the same address.
- Versioned ABI names describe the native signature expected at the target
  address. New nontrivial C++ value types must cross a compiled C++ shim;
  trivially copyable C-compatible signatures may use the typed Rust facade.

## Installation and publication

Module load opens one subscription transaction. Annotation subscriptions and
their forced filters are staged together.

Before publication, the runtime:

1. resolves the required physical-hook set from the staged logical routes;
2. validates that exactly one native specification exists per required hook;
3. installs all missing detours in one Frida transaction; and
4. publishes one immutable dispatch configuration containing subscription
   words and ID filters.

A failed or abandoned update preserves the previously published configuration.
Hook callbacks load the immutable configuration without taking the
administration mutex.

## Native ABI boundary

`NativeHookSpec::typed_event` accepts a replacement and `NativeOriginal` with
the same function type. This makes it impossible to pair a trampoline with a
different declared Rust signature accidentally.

Resolved native helpers are returned as `NativeFunction<Function>`. Raw
address-to-function conversion is contained in the native facade instead of
being repeated in event families.

Functions involving C++ constructors, destructors, references, return-value
slots, or nontrivial by-value classes belong in the compiled native shim.
Rust-facing shim functions use fixed-width scalars, pointers, byte spans, and
plain C data-transfer structures only.

## Controls and results

Each logical route declares:

- whether it observes or may suppress the original operation;
- the semantic role of its result;
- its result type; and
- its reentrancy policy.

Event control decodes a result once into the catalog-declared boolean,
unsigned, signed, finite float, bounded UTF-8 string, or object-ID type.
Native replacements consume typed accessors and do not parse raw JSON
themselves. A new result type must be added to the catalog, bridge validation,
native typed value, and fixture together.

## Reentrancy and cross-hook state

The frame stack owns nesting depth and detects active logical keys.
`SuppressSameRoute` bypasses recursive dispatch of the same route while still
allowing the replacement to invoke its original operation.

State shared by different physical hooks must use an explicit correlation
owner with these properties:

- records are owned values, never borrowed engine pointers;
- begin and finish operations use a monotonically increasing token;
- correlation is LIFO per declared correlation key unless the engine evidence
  proves another ordering;
- unmatched, duplicate, and out-of-order finishes produce diagnostics and
  discard the affected record; and
- module reload and shutdown clear outstanding records.

Family-local globals and boolean recursion guards are not permitted.

The shared correlation owner is implemented in `runtime-sys` and is cleared on
module load and successful shutdown. A new multi-hook family must declare its
correlation key and store only the minimum owned data needed by its finishing
hook.

## Protocol messages

Protocol hooks construct a validated `MessageReader` from the engine cursor.
Readers snapshot bounds and cursor state, preserve the engine's bit cursor,
and expose bounded LSB-first bit reads, explicitly little-endian scalar reads,
signed values, IEEE-754 floats, object IDs, length-prefixed strings, and
resrefs. Event families do not use generic unaligned field peeks.

Message cancellation may advance the engine cursor only through the dedicated
clear operation and must include a fixture assertion for the resulting cursor
state.

## Asynchronous event sources

Resource events use an event pump rather than native detours. Producers send
owned records into a bounded queue; the engine thread drains that queue from
the main-loop boundary and performs normal subscription, filter, frame, and
dispatch processing there.

The pump must define queue capacity, overflow diagnostics, shutdown behavior,
and record ordering before resource watchers are enabled.

The shared pump has a 1,024-record capacity, assigns monotonically increasing
sequence numbers, drains at most 256 records per engine tick in FIFO order,
counts overflow, rejects producers after shutdown, and is reset on module
load. Resource watchers remain responsible for converting OS notifications
into owned, platform-neutral records before enqueueing them.

## Subscription and hot-path policy

Every catalog entry declares its frequency and filter policy. Hot events must
select either an optional integer whitelist or a required integer whitelist;
the catalog test rejects an unspecified hot-path policy. Subscription lookup
uses an immutable dense bit mask and optional filters are published with the
same configuration transaction.

The release-mode `event-hot-path` benchmark is an enforced regression check.
It measures the subscription-mask lookup used by replacements and fails when
the average exceeds 500 ns on a runner.

## Unified behavior decisions

Behavioral choices that Unified does not settle through a declaration are
recorded in `event-decisions.toml` and explained in
`UNIFIED_DECISIONS.md`. Each entry names its source, decision, rationale, and
acceptance requirement. Catalog tests validate the ledger so a known
ambiguity cannot silently return to an unverified state.

## Evidence and fixtures

- Every target-pack address and layout requires evidence for the exact binary
  hash.
- The fixture target generator must exactly cover the physical-hook catalog
  and all helpers required by subscribed logical routes.
- Fixture subscriptions are generated from `EVENT_CATALOG`; hand-maintained
  identity lists are not allowed.
- A port is complete only after the relevant unit tests and injected native
  fixture pass on Linux x86_64, Linux ARM64, Windows x86_64, and macOS ARM64.
