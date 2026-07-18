#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

#[cfg(not(any(feature = "supervisor", feature = "tooling")))]
compile_error!("nwnrs requires the supervisor feature, the tooling feature, or both");

mod args;
#[cfg(feature = "tooling")]
mod compile;
#[cfg(all(feature = "supervisor", target_os = "linux"))]
mod container;
#[cfg(feature = "tooling")]
mod convert;
#[cfg(feature = "tooling")]
mod inspect;
mod logging;
#[cfg(feature = "tooling")]
mod nwsync;
#[cfg(feature = "tooling")]
mod pack;
#[cfg(feature = "tooling")]
mod package;
#[cfg(feature = "tooling")]
mod project;
#[cfg(feature = "supervisor")]
mod run;
#[cfg(feature = "tooling")]
mod unpack;
#[cfg(feature = "tooling")]
mod util;

use std::process::ExitCode;

#[cfg(feature = "tooling")]
use args::NwsyncCommand;
use args::{Cli, Command};
use tracing::{error, instrument};

/// Runs the CLI process entrypoint and returns one process exit code.
///
/// # Examples
///
/// ```rust,no_run
/// let _entry: fn() -> std::process::ExitCode = nwnrs::main_entry;
/// ```
pub fn main_entry() -> ExitCode {
    let cli: Cli = argh::from_env();
    let color = match &cli.command {
        #[cfg(feature = "supervisor")]
        Command::Run(command) => command.color,
        #[cfg(feature = "tooling")]
        _ => args::ColorMode::Auto,
    };
    logging::init_tracing(color);
    match run(cli) {
        Ok(code) => code,
        Err(message) => {
            error!(error = %message, "command failed");
            ExitCode::FAILURE
        }
    }
}

#[instrument(level = "info", skip_all)]
fn run(cli: Cli) -> Result<ExitCode, String> {
    match cli.command {
        #[cfg(feature = "tooling")]
        Command::Compile(cmd) => compile::run_compile(cmd).map(|()| ExitCode::SUCCESS),
        #[cfg(feature = "tooling")]
        Command::Convert(cmd) => convert::run_convert(&cmd).map(|()| ExitCode::SUCCESS),
        #[cfg(feature = "tooling")]
        Command::Inspect(cmd) => inspect::run_inspect(&cmd).map(|()| ExitCode::SUCCESS),
        #[cfg(feature = "tooling")]
        Command::Init(cmd) => project::run_init(cmd).map(|()| ExitCode::SUCCESS),
        #[cfg(feature = "tooling")]
        Command::New(cmd) => project::run_new(cmd).map(|()| ExitCode::SUCCESS),
        #[cfg(feature = "tooling")]
        Command::Pack(cmd) => pack::run_pack(cmd).map(|()| ExitCode::SUCCESS),
        #[cfg(feature = "supervisor")]
        Command::Run(cmd) => run::run_server(cmd),
        #[cfg(feature = "tooling")]
        Command::Unpack(cmd) => unpack::run_unpack(cmd).map(|()| ExitCode::SUCCESS),
        #[cfg(feature = "tooling")]
        Command::Nwsync(cmd) => match cmd.command {
            NwsyncCommand::Print(cmd) => nwsync::run_nwsync_print(cmd).map(|()| ExitCode::SUCCESS),
            NwsyncCommand::Fetch(cmd) => nwsync::run_nwsync_fetch(cmd).map(|()| ExitCode::SUCCESS),
            NwsyncCommand::Prune(cmd) => nwsync::run_nwsync_prune(cmd).map(|()| ExitCode::SUCCESS),
            NwsyncCommand::Write(cmd) => nwsync::run_nwsync_write(cmd).map(|()| ExitCode::SUCCESS),
        },
    }
}

#[cfg(all(test, feature = "tooling"))]
mod tests {
    use std::path::PathBuf;

    use super::{Cli, Command, args::InspectCmd, run};

    #[test]
    fn run_propagates_subcommand_errors() {
        let cli = Cli {
            command: Command::Inspect(InspectCmd {
                internal_names:    false,
                max_string_length: 15,
                require_ndb:       false,
                no_ndb:            false,
                no_source_weave:   false,
                no_local_offsets:  false,
                no_labels:         false,
                no_offsets:        false,
                no_langspec:       false,
                langspec:          None,
                root:              None,
                user:              None,
                language:          "english".to_string(),
                load_ovr:          false,
                path:              PathBuf::from("unsupported.xyz"),
            }),
        };

        let err = run(cli).expect_err("run should fail");
        assert!(err.contains("unsupported file type"));
    }
}
