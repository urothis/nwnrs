# macOS AArch64 event-foundation evidence: NWN 8193.37-17

Validated natively on 2026-07-24 using macOS 26.5.2 (`25F84`) on
`aarch64-apple-darwin`, Rust 1.97.1, and Apple Clang 17.0.0.

## Exact inputs

- NWServer:
  `Neverwinter Nights/bin/macos/nwserver-macos`
- SHA-256:
  `75475f71906482c3b52bd3e14c5266c0532dadce69ffce5f5bae343a23ccb803`
- Build reported by NWServer: `8193.37-17 [26c6e573]`
- Unified:
  `3d4c4e13c6bf01b032ffe90534fc4a19eb036c03`

The server hash exactly matches the source-controlled macOS AArch64 target
pack.

## Foundation gate

The following checks passed from the synchronized working tree:

- `cargo fmt --all -- --check`;
- the `nwnrs-runtime` and `nwnrs-runtime-sys` unit and doc tests;
- strict Clippy for both runtime crates and all targets;
- the compiled C++ shim version and three-by-value `CExoString` self-test;
- correlation LIFO, out-of-order cleanup, and lifecycle reset tests;
- bit-level bounded message-reader tests;
- bounded event-pump ordering, overflow, shutdown, and reload tests;
- typed event-result and decision-ledger tests;
- the generated injected native runtime fixture; and
- the release subscription-mask benchmark at `3.079 ns` per lookup, below
  the enforced `500 ns` ceiling.

The native Unified ABI probe compiled and ran on this host. Its output matched
the exact target pack, including the message cursor layouts required by the
shared reader and the compiler-specific nontrivial C++ boundary.

## Real-server acceptance

The source-built `nwnrs-event-validation.mod` was launched through the current
debug launcher and runtime against the exact server above. The runtime
initialized, dispatched `module.load` with event ID `3002`, phase `before`,
script `_nwnrs_onload`, depth `1`, and target `00000000`, and NWServer reached
`Server: Module loaded`. An interactive `quit` then produced
`Server: Exiting` and a successful launcher exit.
