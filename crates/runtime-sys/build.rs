//! Builds the portable C++ ABI thunks used by the native engine boundary.

fn main() {
    println!("cargo:rerun-if-changed=native/exo_string.cpp");
    let windows = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");
    let mut build = cc::Build::new();
    build
        .cpp(true)
        .std("c++17")
        .warnings(true)
        .warnings_into_errors(true)
        .file("native/exo_string.cpp");
    if windows {
        build.static_crt(false);
        // The Frida Windows devkit advertises the static CRT even though the
        // Rust DLL and this C++ boundary use the dynamic CRT. Resolve its CRT
        // references from the already-selected dynamic libraries instead of
        // allowing LINK to choose both runtimes.
        println!("cargo:rustc-link-arg=/NODEFAULTLIB:LIBCMT");
    }
    build.compile("nwnrs_engine_exo_string");
}
