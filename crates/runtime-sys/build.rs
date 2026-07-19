//! Builds the portable C++ ABI thunks used by the native engine boundary.

fn main() {
    println!("cargo:rerun-if-changed=native/exo_string.cpp");
    cc::Build::new()
        .cpp(true)
        .std("c++17")
        .warnings(true)
        .warnings_into_errors(true)
        .file("native/exo_string.cpp")
        .compile("nwnrs_engine_exo_string");
}
