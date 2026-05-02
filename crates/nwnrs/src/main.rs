#![forbid(unsafe_code)]
//! Binary entrypoint for the `nwnrs` command-line tools.

use std::process::ExitCode;

fn main() -> ExitCode {
    nwnrs::main_entry()
}
