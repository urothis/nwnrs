# nwnrs-runtime

Safe runtime configuration, executable identification, exact target-pack
selection, and the typed NWScript call dispatcher shared by the native
launcher and injected runtime.

This directory is the source of truth for the safe runtime crate and its
supporting integration assets:

- `targets/` contains exact server-binary target packs.
- `fixtures/` contains the native injected-runtime fixture host.
- `scripts/` contains the cross-platform fixture runner used by CI.

Frida Gum is supplied to the native boundary by the Cargo-managed
`frida-gum-sys` dependency. Its `auto-download` feature obtains the matching
native devkit for the build target.

The initial dispatcher exposes runtime identity and live server state through
the static `NWNRS` namespace. It reports the module name, current player count,
and maximum players. During native event-script execution it also reports the
stable event name, engine identifier, script resref, phase, and nesting depth.
NWScript can emit trace, debug, info, warning, and error records through the
runtime's structured tracing pipeline. None of this requires a plugin loader,
HTTP API, or metrics service.

Module source includes [`nwnrs.nss`](../../module/nwnrs.nss). The
source-controlled demo module compiles `module/nwnrs_init.nss` into its
module-load event, preserves the stock `x2_mod_def_load` behavior, and writes a
runtime, server, and module-load-event summary at startup.
