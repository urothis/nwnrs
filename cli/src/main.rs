#![forbid(unsafe_code)]
//! Command-line entrypoint for NWNRS tools.

mod args;
mod compile;
mod convert;
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
        Command::Compile(cmd) => compile::run_compile(cmd),
        Command::Convert(cmd) => convert::run_convert(cmd),
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{Cli, Command, args::InspectCmd, run};

    #[test]
    fn run_propagates_subcommand_errors() {
        let cli = Cli {
            command: Command::Inspect(InspectCmd {
                path: PathBuf::from("unsupported.xyz"),
            }),
        };

        let err = run(cli).expect_err("run should fail");
        assert!(err.contains("unsupported file type"));
    }
}
