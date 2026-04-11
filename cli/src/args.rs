use std::path::PathBuf;

use argh::FromArgs;

#[derive(FromArgs)]
/// Neverwinter Nights utility CLI.
pub(crate) struct Cli {
    #[argh(subcommand)]
    pub(crate) command: Command,
}

#[derive(FromArgs)]
#[argh(subcommand)]
pub(crate) enum Command {
    Compile(CompileCmd),
    Convert(ConvertCmd),
    Inspect(InspectCmd),
    Pack(PackCmd),
    Unpack(UnpackCmd),
    Nwsync(NwsyncCmd),
}

#[derive(FromArgs)]
#[argh(subcommand, name = "compile")]
/// compile one NWScript source file to NCS and optional NDB output
pub(crate) struct CompileCmd {
    #[argh(switch, short = 'f')]
    /// overwrite existing output files
    pub(crate) force: bool,

    #[argh(switch)]
    /// also write debugger output as a sibling .ndb file
    pub(crate) debug: bool,

    #[argh(switch)]
    /// skip main/StartingConditional entrypoint validation
    pub(crate) no_entrypoint_check: bool,

    #[argh(option, short = 'o')]
    /// output NCS file path, defaults to INPUT with .ncs extension
    pub(crate) output: Option<PathBuf>,

    #[argh(option)]
    /// explicit nwscript.nss path; defaults to nwscript.nss beside INPUT
    pub(crate) langspec: Option<PathBuf>,

    #[argh(option)]
    /// extra directory to search for #include files; may be repeated
    pub(crate) include_dir: Vec<PathBuf>,

    #[argh(option, default = "String::from(\"O0\")")]
    /// optimization level: O0, O1, O2, or O3
    pub(crate) optimization: String,

    #[argh(positional)]
    /// input .nss file to compile
    pub(crate) input: PathBuf,
}

#[derive(FromArgs)]
#[argh(subcommand, name = "convert")]
/// convert image assets between supported formats
pub(crate) struct ConvertCmd {
    #[argh(switch, short = 'f')]
    /// overwrite existing output files
    pub(crate) force: bool,

    #[argh(option, default = "String::from(\"dxt5\")")]
    /// dds block format when OUTPUT ends in .dds: dxt1 or dxt5
    pub(crate) dds_format: String,

    #[argh(positional)]
    /// input image path
    pub(crate) input: PathBuf,

    #[argh(positional)]
    /// output image path
    pub(crate) output: PathBuf,
}

#[derive(FromArgs)]
#[argh(subcommand, name = "inspect")]
/// inspect a single NWN resource file by extension
pub(crate) struct InspectCmd {
    #[argh(positional)]
    /// path to the file to inspect
    pub(crate) path: PathBuf,
}

#[derive(FromArgs)]
#[argh(subcommand, name = "unpack")]
/// unpack a resource into a directory
pub(crate) struct UnpackCmd {
    #[argh(option, short = 'd', default = "PathBuf::from(\".\")")]
    /// destination directory
    pub(crate) directory: PathBuf,

    #[argh(switch, short = 'f')]
    /// overwrite existing output files
    pub(crate) force: bool,

    #[argh(positional)]
    /// input file to unpack
    pub(crate) input: PathBuf,
}

#[derive(FromArgs)]
#[argh(subcommand, name = "pack")]
/// pack a directory into NWN binary form
pub(crate) struct PackCmd {
    #[argh(switch, short = 'f')]
    /// overwrite existing output files
    pub(crate) force: bool,

    #[argh(option, default = "String::from(\"V1\")")]
    /// data file version to write (V1 or E1)
    pub(crate) data_version: String,

    #[argh(option, default = "String::from(\"none\")")]
    /// compression for E1 (none, zlib, zstd)
    pub(crate) data_compression: String,

    #[argh(switch)]
    /// do not squash bif files into the same directory as the key
    pub(crate) no_squash: bool,

    #[argh(switch)]
    /// don't follow symlinks
    pub(crate) no_symlinks: bool,

    #[argh(option, short = 'r', default = "2")]
    /// recurse at most N directories when packing archive entries
    pub(crate) recurse: usize,

    #[argh(option, short = 'e')]
    /// override archive header type when packing erf/mod/hak/nwm
    pub(crate) erf_type: Option<String>,

    #[argh(positional)]
    /// source file or directory
    pub(crate) input: PathBuf,

    #[argh(positional)]
    /// output file
    pub(crate) output: PathBuf,
}

pub(crate) struct KeyPackCmd {
    pub(crate) data_version:     String,
    pub(crate) data_compression: String,
    pub(crate) no_squash:        bool,
    pub(crate) no_symlinks:      bool,
    pub(crate) force:            bool,
    pub(crate) key:              String,
    pub(crate) source:           PathBuf,
    pub(crate) destination:      PathBuf,
}

pub(crate) struct KeyUnpackCmd {
    pub(crate) force:       bool,
    pub(crate) key:         PathBuf,
    pub(crate) destination: PathBuf,
}

#[derive(FromArgs)]
#[argh(subcommand, name = "nwsync")]
/// nwsync repository utilities
pub(crate) struct NwsyncCmd {
    #[argh(subcommand)]
    pub(crate) command: NwsyncCommand,
}

#[derive(FromArgs)]
#[argh(subcommand)]
pub(crate) enum NwsyncCommand {
    Print(NwsyncPrintCmd),
    Fetch(NwsyncFetchCmd),
    Prune(NwsyncPruneCmd),
    Write(NwsyncWriteCmd),
}

#[derive(FromArgs)]
#[argh(subcommand, name = "print")]
/// print manifest contents from a manifest file or nwsync repository
pub(crate) struct NwsyncPrintCmd {
    #[argh(option)]
    /// manifest sha1 to print when INPUT is a nwsync repository
    pub(crate) manifest: Option<String>,

    #[argh(positional)]
    /// manifest file or nwsync repository root
    pub(crate) input: PathBuf,
}

#[derive(FromArgs)]
#[argh(subcommand, name = "fetch")]
/// synchronize a manifest server-to-server with aria2c
pub(crate) struct NwsyncFetchCmd {
    #[argh(positional)]
    /// remote manifest URL to fetch
    pub(crate) url: String,

    #[argh(option, short = 'o')]
    /// output directory for downloaded files
    pub(crate) output: Option<PathBuf>,
}

#[derive(FromArgs)]
#[argh(subcommand, name = "prune")]
/// trim unreferenced data from nwsync repository
pub(crate) struct NwsyncPruneCmd {
    #[argh(positional)]
    /// nwsync repository root
    pub(crate) repository: PathBuf,

    #[argh(switch)]
    /// dry run - show what would be removed without actually removing
    pub(crate) dry_run: bool,
}

#[derive(FromArgs)]
#[argh(subcommand, name = "write")]
/// generate a serverside NWSync manifest from directory
pub(crate) struct NwsyncWriteCmd {
    #[argh(positional)]
    /// input directory containing NWN resources
    pub(crate) input: PathBuf,

    #[argh(positional)]
    /// output manifest file path
    pub(crate) output: PathBuf,

    #[argh(switch, short = 'f')]
    /// overwrite existing output file
    pub(crate) force: bool,
}

#[allow(clippy::panic)]
#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use argh::FromArgs;

    use super::{Cli, Command, NwsyncCommand};

    #[test]
    fn parses_compile_command_with_repeated_include_dirs() {
        let cli = Cli::from_args(
            &["nwnrs"],
            &[
                "compile",
                "--debug",
                "--no-entrypoint-check",
                "--include-dir",
                "inc/a",
                "--include-dir",
                "inc/b",
                "--optimization",
                "O2",
                "-o",
                "out/test.ncs",
                "scripts/test.nss",
            ],
        )
        .unwrap_or_else(|error| panic!("parse compile args: {error:?}"));

        let Command::Compile(cmd) = cli.command else {
            panic!("expected compile command");
        };
        assert!(cmd.debug);
        assert!(cmd.no_entrypoint_check);
        assert_eq!(cmd.optimization, "O2");
        assert_eq!(
            cmd.include_dir,
            vec![PathBuf::from("inc/a"), PathBuf::from("inc/b")]
        );
        assert_eq!(cmd.output, Some(PathBuf::from("out/test.ncs")));
        assert_eq!(cmd.input, PathBuf::from("scripts/test.nss"));
    }

    #[test]
    fn parses_nwsync_fetch_command() {
        let cli = Cli::from_args(
            &["nwnrs"],
            &[
                "nwsync",
                "fetch",
                "https://example.invalid/manifest/abcd",
                "-o",
                "repo",
            ],
        )
        .unwrap_or_else(|error| panic!("parse nwsync args: {error:?}"));

        let Command::Nwsync(cmd) = cli.command else {
            panic!("expected nwsync command");
        };
        let NwsyncCommand::Fetch(cmd) = cmd.command else {
            panic!("expected fetch subcommand");
        };
        assert_eq!(cmd.url, "https://example.invalid/manifest/abcd");
        assert_eq!(cmd.output, Some(PathBuf::from("repo")));
    }
}
