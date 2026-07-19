//! Configures platform-specific linker behavior for the command-line binary.

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        // The optional Windows launcher dependency carries the Frida devkit's
        // static-CRT directive into this final executable. The launcher and
        // its Rust dependency graph use the dynamic CRT consistently.
        println!("cargo:rustc-link-arg=/NODEFAULTLIB:LIBCMT");
    }
}
