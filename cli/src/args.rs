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
/// convert supported NWN textures and mdl files
pub(crate) struct ConvertCmd {
    #[argh(switch, short = 'f')]
    /// overwrite existing output files
    pub(crate) force: bool,

    #[argh(option, default = "String::from(\"dxt5\")")]
    /// dds block format when OUTPUT ends in .dds: dxt1 or dxt5
    pub(crate) dds_format: String,

    #[argh(positional)]
    /// input file path
    pub(crate) input: PathBuf,

    #[argh(positional)]
    /// output file path
    pub(crate) output: PathBuf,
}

#[derive(FromArgs)]
#[argh(subcommand, name = "inspect")]
/// inspect a single NWN resource file by extension
pub(crate) struct InspectCmd {
    #[argh(switch)]
    /// for .ncs input, render internal opcode/aux names instead of canonical
    /// mnemonics
    pub(crate) internal_names: bool,

    #[argh(option, default = "15")]
    /// for .ncs input, maximum rendered string length before truncation
    pub(crate) max_string_length: usize,

    #[argh(switch)]
    /// for .ncs input, require a sibling .ndb file
    pub(crate) require_ndb: bool,

    #[argh(switch)]
    /// for .ncs input, ignore a sibling .ndb file
    pub(crate) no_ndb: bool,

    #[argh(switch)]
    /// for .ncs input, suppress source weaving even when .ndb and source files
    /// are available
    pub(crate) no_source_weave: bool,

    #[argh(switch)]
    /// for .ncs input, suppress per-function local offsets
    pub(crate) no_local_offsets: bool,

    #[argh(switch)]
    /// for .ncs input, skip synthetic jump labels
    pub(crate) no_labels: bool,

    #[argh(switch)]
    /// for .ncs input, suppress global offsets
    pub(crate) no_offsets: bool,

    #[argh(switch)]
    /// for .ncs input, skip loading nwscript.nss for builtin action names
    pub(crate) no_langspec: bool,

    #[argh(option)]
    /// for .ncs input, explicit nwscript.nss path instead of sibling lookup
    pub(crate) langspec: Option<PathBuf>,

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

    use super::{Cli, Command, InspectCmd, NwsyncCommand};

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

    #[test]
    fn parses_convert_command_for_mdl_paths() {
        let cli = Cli::from_args(
            &["nwnrs"],
            &["convert", "-f", "models/input.mdl", "models/output.mdl"],
        )
        .unwrap_or_else(|error| panic!("parse convert args: {error:?}"));

        let Command::Convert(cmd) = cli.command else {
            panic!("expected convert command");
        };
        assert!(cmd.force);
        assert_eq!(cmd.input, PathBuf::from("models/input.mdl"));
        assert_eq!(cmd.output, PathBuf::from("models/output.mdl"));
    }

    #[test]
    fn parses_inspect_command_with_ncs_disassembly_options() {
        let cli = Cli::from_args(
            &["nwnrs"],
            &[
                "inspect",
                "--internal-names",
                "--max-string-length",
                "42",
                "--require-ndb",
                "--no-source-weave",
                "--no-local-offsets",
                "--no-labels",
                "--no-offsets",
                "--no-langspec",
                "--langspec",
                "specs/custom.nss",
                "scripts/test.ncs",
            ],
        )
        .unwrap_or_else(|error| panic!("parse inspect args: {error:?}"));

        let Command::Inspect(InspectCmd {
            internal_names,
            max_string_length,
            require_ndb,
            no_ndb,
            no_source_weave,
            no_local_offsets,
            no_labels,
            no_offsets,
            no_langspec,
            langspec,
            path,
        }) = cli.command
        else {
            panic!("expected inspect command");
        };

        assert!(internal_names);
        assert_eq!(max_string_length, 42);
        assert!(require_ndb);
        assert!(!no_ndb);
        assert!(no_source_weave);
        assert!(no_local_offsets);
        assert!(no_labels);
        assert!(no_offsets);
        assert!(no_langspec);
        assert_eq!(langspec, Some(PathBuf::from("specs/custom.nss")));
        assert_eq!(path, PathBuf::from("scripts/test.ncs"));
    }
}
