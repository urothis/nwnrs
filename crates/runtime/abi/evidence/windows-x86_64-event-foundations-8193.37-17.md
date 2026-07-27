# Windows x86_64 event-foundation evidence: NWN 8193.37-17

Validated natively on 2026-07-24 using Windows 10.0.26200.8875 on
`x86_64-pc-windows-msvc`, Rust 1.97.1, and the installed MSVC x64 toolchain.

## Exact inputs

- NWServer:
  `Neverwinter Nights\bin\win32\nwserver.exe`
- SHA-256:
  `98951511ae7d06a251355f12d4f3e4b96269a2414fa3058e641afd70098690f6`
- Build: `8193.37-17 [26c6e573]`
- Unified:
  `3d4c4e13c6bf01b032ffe90534fc4a19eb036c03`

The server hash exactly matches the source-controlled Windows x86_64 target
pack.

## Foundation gate

The following checks passed from the synchronized working tree:

- `cargo fmt --all -- --check`;
- the `nwnrs-runtime` and `nwnrs-runtime-sys` unit and doc tests;
- strict Clippy for both runtime crates and all targets;
- the MSVC-compiled shim version and three-by-value `CExoString` self-test;
- correlation LIFO, out-of-order cleanup, and lifecycle reset tests;
- bit-level bounded message-reader tests;
- bounded event-pump ordering, overflow, shutdown, and reload tests;
- typed event-result and decision-ledger tests;
- the generated injected native runtime fixture in headless NWServer mode; and
- the release subscription-mask benchmark at `1.782 ns` per lookup, below
  the enforced `500 ns` ceiling.

The native Unified ABI probe compiled with MSVC, ran on this host, and matched
the exact target pack.

The fixture's optional GUI visibility/theme assertion is not valid from the
OpenSSH service desktop and remains a separate UI acceptance check. The
headless injection, event, native-shim, administration, and shutdown paths all
passed.

## Real-server acceptance

The source-built `nwnrs-event-validation.mod` was launched through the current
debug launcher and injected runtime against the exact Steam server. Before
the suspended primary thread was resumed, the runtime initialized and selected
the hash-named target pack. `_nwnrs_onload` then dispatched `module.load` with
event ID `3002`, phase `before`, script `_nwnrs_onload`, depth `1`, and target
`00000000`. The launcher issued a graceful shutdown request and NWServer
exited successfully.
