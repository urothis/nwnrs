#![forbid(unsafe_code)]
//! Command-line entrypoint for NWNRS tools.

mod args;
mod inspect;
mod logging;
mod metadata;
mod nwsync;
mod pack;
mod unpack;
mod util;

use std::process::ExitCode;

use args::{Cli, Command, NwsyncCommand};
use tracing::{error, instrument};

fn main() -> ExitCode {
    logging::init_tracing();
    let cli: Cli = argh::from_env();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            error!(error = %message, "command failed");
            ExitCode::FAILURE
        }
    }
}

#[instrument(level = "info", skip_all, err)]
fn run(cli: Cli) -> Result<(), String> {
    match cli.command {
        Command::Inspect(cmd) => inspect::run_inspect(&cmd.path),
        Command::Pack(cmd) => pack::run_pack(cmd),
        Command::Unpack(cmd) => unpack::run_unpack(cmd),
        Command::Nwsync(cmd) => match cmd.command {
            NwsyncCommand::Print(cmd) => nwsync::run_nwsync_print(cmd),
            NwsyncCommand::Fetch(cmd) => nwsync::run_nwsync_fetch(cmd),
            NwsyncCommand::Prune(cmd) => nwsync::run_nwsync_prune(cmd),
            NwsyncCommand::Write(cmd) => nwsync::run_nwsync_write(cmd),
        },
    }
}
