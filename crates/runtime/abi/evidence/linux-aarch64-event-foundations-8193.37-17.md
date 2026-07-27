# Linux AArch64 event-foundation evidence: NWN 8193.37-17

Validated natively on 2026-07-24 using Debian 13.6 on
`aarch64-unknown-linux-gnu`, Rust 1.97.1, and GCC 14.2.0.

## Exact inputs

- NWServer:
  `Neverwinter Nights/bin/linux-arm64/nwserver-linux`
- SHA-256:
  `69b3705d831a48386e6ce1490a0ecaa521485f68b2351d0b92853d644d4ffdcf`
- Build reported by NWServer: `8193.37-17 [26c6e573]`
- Unified:
  `3d4c4e13c6bf01b032ffe90534fc4a19eb036c03`

The server hash exactly matches the source-controlled Linux AArch64 target
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
- the release subscription-mask benchmark at `18.199 ns` per lookup, below
  the enforced `500 ns` ceiling.

The native Unified ABI probe compiled and ran on the physical AArch64 host.
It used the explicit pinned revision for the copied Unified tree and compiled
Unified's bundled SQLite source. The measured output matched the exact target
pack.

## Real-server acceptance

The source-built `nwnrs-event-validation.mod` was launched through the current
debug launcher and runtime against the exact server above. The runtime
initialized, dispatched `module.load` with event ID `3002`, phase `before`,
script `_nwnrs_onload`, depth `1`, and target `00000000`, and NWServer reached
`Server: Module loaded`. An interactive `quit` then produced
`Server: Exiting` and a successful launcher exit.
