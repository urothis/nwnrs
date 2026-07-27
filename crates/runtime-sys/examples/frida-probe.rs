//! Standalone Phase 0 Frida Gum interception probe.

fn main() -> Result<(), nwnrs_runtime_sys::ProbeError> {
    nwnrs_runtime_sys::run_frida_probe()
}
