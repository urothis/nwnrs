use std::{path::PathBuf, str::FromStr};

use argh::FromArgs;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum ColorMode {
    #[default]
    Auto,
    Always,
    Never,
}

impl FromStr for ColorMode {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "auto" => Ok(Self::Auto),
            "always" => Ok(Self::Always),
            "never" => Ok(Self::Never),
            _ => Err(format!(
                "invalid color mode {value:?}; expected auto, always, or never"
            )),
        }
    }
}

#[cfg(feature = "supervisor")]
impl ColorMode {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Always => "always",
            Self::Never => "never",
        }
    }
}

#[derive(FromArgs)]
/// Neverwinter Nights utility CLI.
pub(crate) struct Cli {
    #[argh(subcommand)]
    pub(crate) command: Command,
}

#[derive(FromArgs)]
#[argh(subcommand)]
pub(crate) enum Command {
    #[cfg(feature = "tooling")]
    Compile(CompileCmd),
    #[cfg(feature = "tooling")]
    Convert(ConvertCmd),
    #[cfg(feature = "tooling")]
    Inspect(InspectCmd),
    #[cfg(feature = "tooling")]
    Init(InitCmd),
    #[cfg(feature = "tooling")]
    New(NewCmd),
    #[cfg(feature = "tooling")]
    Pack(PackCmd),
    #[cfg(feature = "supervisor")]
    Run(RunCmd),
    #[cfg(feature = "tooling")]
    Unpack(UnpackCmd),
    #[cfg(feature = "tooling")]
    Nwsync(NwsyncCmd),
}

#[cfg(feature = "supervisor")]
#[derive(FromArgs)]
#[argh(subcommand, name = "run")]
/// start an NWN server under nwnrs supervision
pub(crate) struct RunCmd {
    #[cfg(feature = "tooling")]
    #[argh(switch)]
    /// start the supervised server image through the local Docker CLI
    pub(crate) docker: bool,

    #[cfg(feature = "tooling")]
    #[argh(option, default = "String::from(\"nwserver:local\")")]
    /// container image used by --docker
    pub(crate) docker_image: String,

    #[cfg(feature = "tooling")]
    #[argh(option)]
    /// optional container name used by --docker
    pub(crate) docker_name: Option<String>,

    #[cfg(feature = "tooling")]
    #[argh(option, default = "String::from(\"nwserver-home\")")]
    /// docker volume or host path mounted at /nwn/home
    pub(crate) docker_home: String,

    #[cfg(feature = "tooling")]
    #[argh(option)]
    /// published container port; may repeat and replaces 5121:5121/udp
    pub(crate) docker_publish: Vec<String>,

    #[cfg(feature = "tooling")]
    #[argh(option)]
    /// docker long option without leading dashes; may repeat
    pub(crate) docker_arg: Vec<String>,

    #[argh(option, default = "ColorMode::Auto")]
    /// color output policy: auto, always, or never
    pub(crate) color: ColorMode,

    #[argh(switch)]
    /// start the server without mirroring its log and error files
    pub(crate) no_tail_logs: bool,

    #[argh(option)]
    /// native mode: path to the injected runtime dylib or shared object
    #[cfg(feature = "tooling")]
    pub(crate) runtime: Option<PathBuf>,

    #[argh(option)]
    /// native mode: root directory containing exact runtime target packs
    #[cfg(feature = "tooling")]
    pub(crate) targets: Option<PathBuf>,

    #[cfg(not(feature = "tooling"))]
    #[argh(option)]
    /// path to the injected nwnrs runtime dylib or shared object
    pub(crate) runtime: PathBuf,

    #[cfg(not(feature = "tooling"))]
    #[argh(option)]
    /// root directory containing exact runtime target packs
    pub(crate) targets: PathBuf,

    #[argh(option)]
    /// server working directory; defaults to the server binary directory
    pub(crate) working_directory: Option<PathBuf>,

    #[argh(positional, greedy)]
    /// native server path and arguments, or Docker image command arguments
    pub(crate) arguments: Vec<String>,
}

#[cfg(feature = "tooling")]
#[derive(FromArgs, Clone)]
#[argh(subcommand, name = "compile")]
/// compile one or more NWScript source files
pub(crate) struct CompileCmd {
    #[argh(switch, short = 'f')]
    /// overwrite existing NCS, NDB, and Graphviz output files
    pub(crate) force: bool,

    #[argh(switch, short = 'g')]
    /// write NDB debugger output
    pub(crate) debug: bool,

    #[argh(switch)]
    /// skip main/StartingConditional entrypoint validation
    pub(crate) no_entrypoint_check: bool,

    #[argh(option)]
    /// explicit nwscript.nss path instead of normal resource lookup
    pub(crate) langspec: Option<PathBuf>,

    #[argh(option)]
    /// extra directory to search for #include files; may be repeated
    pub(crate) include_dir: Vec<PathBuf>,

    #[argh(option, default = "String::from(\"O1\")")]
    /// optimization preset: O0, O1, O2, or O3; defaults to safe O1
    pub(crate) optimization: String,

    #[argh(option)]
    /// exact optimization flag set; may repeat and overrides the preset
    pub(crate) optimization_flag: Vec<String>,

    #[argh(option, default = "16")]
    /// maximum recursive include depth from 1 through 200
    pub(crate) max_include_depth: usize,

    #[argh(option)]
    /// write Graphviz syntax-tree images into this directory
    pub(crate) graphviz: Option<PathBuf>,

    #[argh(option, default = "String::from(\"svg\")")]
    /// graphviz output format: svg, png, pdf, or dot; defaults to svg
    pub(crate) graphviz_format: String,

    #[argh(switch)]
    /// retain DOT source alongside rendered Graphviz images
    pub(crate) keep_graphviz_dot: bool,

    #[argh(switch)]
    /// compile and report outcomes without writing artifacts
    pub(crate) simulate: bool,

    #[argh(switch, short = 'y')]
    /// continue compiling remaining inputs after an error
    pub(crate) continue_on_error: bool,

    #[argh(switch, short = 'R')]
    /// recurse into input directories
    pub(crate) recurse: bool,

    #[argh(switch)]
    /// follow symlinks while collecting input directories
    pub(crate) follow_symlinks: bool,

    #[argh(option, short = 'j')]
    /// parallel compile workers; defaults to available CPU parallelism
    pub(crate) jobs: Option<usize>,

    #[argh(option, short = 'o')]
    /// exact NCS output path, relative or absolute; only valid with one input
    pub(crate) output: Option<PathBuf>,

    #[argh(option, short = 'd')]
    /// directory for compiled NCS and NDB artifacts
    pub(crate) directory: Option<PathBuf>,

    #[argh(option)]
    /// explicit Neverwinter Nights installation root
    pub(crate) root: Option<PathBuf>,

    #[argh(option)]
    /// explicit Neverwinter Nights user directory
    pub(crate) user: Option<PathBuf>,

    #[argh(option, default = "String::from(\"english\")")]
    /// installation language used for resource lookup
    pub(crate) language: String,

    #[argh(switch)]
    /// include the installation override directory in resource lookup
    pub(crate) load_ovr: bool,

    #[argh(positional)]
    /// source files or directories to compile
    pub(crate) paths: Vec<PathBuf>,
}

#[cfg(feature = "tooling")]
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

    #[argh(option)]
    /// install root for install-backed conversions such as utc -> obj
    pub(crate) root: Option<PathBuf>,

    #[argh(option)]
    /// user directory for install-backed conversions; defaults to autodetect
    pub(crate) user: Option<PathBuf>,

    #[argh(option, default = "String::from(\"english\")")]
    /// install language for install-backed conversions
    pub(crate) language: String,

    #[argh(switch)]
    /// include the install override directory for install-backed conversions
    pub(crate) load_ovr: bool,

    #[argh(option)]
    /// explicit animation name to snapshot for obj export
    pub(crate) animation: Option<String>,

    #[argh(option)]
    /// animation time in seconds for obj export
    pub(crate) time: Option<f32>,

    #[argh(switch)]
    /// list available animations for mdl or utc input and exit
    pub(crate) list_animations: bool,

    #[argh(switch)]
    /// embed original compiled mdl bytes in decompiled ASCII comments
    pub(crate) preserve_compiled_source: bool,

    #[argh(positional)]
    /// input file path
    pub(crate) input: PathBuf,

    #[argh(positional)]
    /// output file path when converting to a new file
    pub(crate) output: Option<PathBuf>,
}

#[cfg(feature = "tooling")]
#[derive(FromArgs)]
#[argh(subcommand, name = "inspect")]
/// inspect a single NWN resource file by extension
// This struct has 8 bool fields, exceeding clippy::pedantic's threshold of 3.
// They cannot be collapsed into enums or a state machine because argh requires
// bool fields for `#[argh(switch)]`, and each flag is an independent CLI option
// with no mutual exclusivity
#[allow(clippy::struct_excessive_bools)]
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
    /// for .ncs input, explicit nwscript.nss path instead of installation
    /// lookup
    pub(crate) langspec: Option<PathBuf>,

    #[argh(option)]
    /// explicit Neverwinter Nights installation root for resource lookup
    pub(crate) root: Option<PathBuf>,

    #[argh(option)]
    /// explicit Neverwinter Nights user directory for resource lookup
    pub(crate) user: Option<PathBuf>,

    #[argh(option, default = "String::from(\"english\")")]
    /// installation language used for resource lookup
    pub(crate) language: String,

    #[argh(switch)]
    /// include the installation override directory in resource lookup
    pub(crate) load_ovr: bool,

    #[argh(positional)]
    /// path to the file to inspect
    pub(crate) path: PathBuf,
}

#[cfg(feature = "tooling")]
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

#[cfg(feature = "tooling")]
#[derive(FromArgs)]
#[argh(subcommand, name = "init")]
/// initialize an NWN project in an existing directory
pub(crate) struct InitCmd {
    #[argh(option)]
    /// project output kind such as utc, 2da, tlk, ssf, mdl, tga, dds, plt, ncs,
    /// mod, hak, nwm, erf, or key
    pub(crate) kind: Option<String>,

    #[argh(positional)]
    /// directory to initialize; defaults to the current directory
    pub(crate) path: Option<PathBuf>,
}

#[cfg(feature = "tooling")]
#[derive(FromArgs)]
#[argh(subcommand, name = "new")]
/// create a new NWN project directory
pub(crate) struct NewCmd {
    #[argh(option)]
    /// project output kind such as utc, 2da, tlk, ssf, mdl, tga, dds, plt, ncs,
    /// mod, hak, nwm, erf, or key
    pub(crate) kind: Option<String>,

    #[argh(positional)]
    /// directory to create
    pub(crate) path: PathBuf,
}

#[cfg(feature = "tooling")]
#[derive(FromArgs, Clone)]
#[argh(subcommand, name = "pack")]
/// pack resources into NWN binary form, including compiling NWScript source
pub(crate) struct PackCmd {
    #[argh(switch, short = 'f')]
    /// overwrite existing output files
    pub(crate) force: bool,

    #[argh(switch)]
    /// when packing .nss to .ncs, also write debugger output as a sibling .ndb
    /// file
    pub(crate) debug: bool,

    #[argh(switch)]
    /// skip main/StartingConditional entrypoint validation for compiled .nss
    /// inputs
    pub(crate) no_entrypoint_check: bool,

    #[argh(option)]
    /// explicit nwscript.nss path for compiled .nss inputs
    pub(crate) langspec: Option<PathBuf>,

    #[argh(option)]
    /// extra directory to search for #include files in compiled .nss inputs;
    /// may be repeated
    pub(crate) include_dir: Vec<PathBuf>,

    #[argh(option, default = "String::from(\"O1\")")]
    /// optimization level for compiled .nss inputs; defaults to safe O1
    pub(crate) optimization: String,

    #[argh(option)]
    /// exact NWScript optimization flag set; may repeat and overrides preset
    pub(crate) optimization_flag: Vec<String>,

    #[argh(option, short = 'j')]
    /// parallel NWScript compile workers; defaults to available CPU parallelism
    pub(crate) jobs: Option<usize>,

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

    #[argh(option, short = 'e')]
    /// override archive header type when packing erf/mod/hak/nwm
    pub(crate) erf_type: Option<String>,

    #[argh(option)]
    /// package mode: explicit NWN install root override
    pub(crate) root: Option<PathBuf>,

    #[argh(option)]
    /// package mode: explicit NWN user directory override
    pub(crate) user: Option<PathBuf>,

    #[argh(option)]
    /// package mode: language root under root/lang to resolve
    pub(crate) language: Option<String>,

    #[argh(positional)]
    /// explicit pack mode: INPUT OUTPUT; package mode: `KEY_NAME` `OUTPUT_DIR`
    pub(crate) paths: Vec<PathBuf>,
}

#[cfg(feature = "tooling")]
pub(crate) struct KeyPackCmd {
    pub(crate) data_version:        String,
    pub(crate) data_compression:    String,
    pub(crate) no_squash:           bool,
    pub(crate) no_symlinks:         bool,
    pub(crate) force:               bool,
    pub(crate) debug:               bool,
    pub(crate) no_entrypoint_check: bool,
    pub(crate) langspec:            Option<PathBuf>,
    pub(crate) include_dir:         Vec<PathBuf>,
    pub(crate) optimization:        String,
    pub(crate) optimization_flag:   Vec<String>,
    pub(crate) jobs:                Option<usize>,
    pub(crate) key:                 String,
    pub(crate) source:              PathBuf,
    pub(crate) destination:         PathBuf,
}

#[cfg(feature = "tooling")]
pub(crate) struct KeyUnpackCmd {
    pub(crate) force:       bool,
    pub(crate) key:         PathBuf,
    pub(crate) destination: PathBuf,
}

#[cfg(feature = "tooling")]
#[derive(FromArgs)]
#[argh(subcommand, name = "nwsync")]
/// nwsync repository utilities
pub(crate) struct NwsyncCmd {
    #[argh(subcommand)]
    pub(crate) command: NwsyncCommand,
}

#[cfg(feature = "tooling")]
#[derive(FromArgs)]
#[argh(subcommand)]
pub(crate) enum NwsyncCommand {
    Print(NwsyncPrintCmd),
    Fetch(NwsyncFetchCmd),
    Prune(NwsyncPruneCmd),
    Write(NwsyncWriteCmd),
}

#[cfg(feature = "tooling")]
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

#[cfg(feature = "tooling")]
#[derive(FromArgs)]
#[argh(subcommand, name = "fetch")]
/// download a manifest and its resources from a remote nwsync server
pub(crate) struct NwsyncFetchCmd {
    #[argh(positional)]
    /// remote manifest URL to fetch
    pub(crate) url: String,

    #[argh(option, short = 'o')]
    /// output directory for downloaded files
    pub(crate) output: Option<PathBuf>,
}

#[cfg(feature = "tooling")]
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

#[cfg(feature = "tooling")]
#[derive(FromArgs)]
#[argh(subcommand, name = "write")]
/// generate a serverside `NWSync` manifest from directory
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

#[cfg(all(test, feature = "tooling"))]
mod tests {
    use std::path::PathBuf;

    use argh::FromArgs;

    use super::{Cli, Command, InspectCmd, NwsyncCommand};

    #[test]
    #[cfg(feature = "supervisor")]
    fn parses_run_command_and_preserves_server_arguments() {
        let cli = Cli::from_args(
            &["nwnrs"],
            &[
                "run",
                "--runtime",
                "libnwnrs_runtime.so",
                "--targets",
                "crates/runtime/targets",
                "--",
                "/opt/nwn/nwserver",
                "-module",
                "nwnrs",
            ],
        )
        .unwrap_or_else(|error| panic!("parse run args: {error:?}"));

        let Command::Run(command) = cli.command else {
            panic!("expected run command");
        };
        assert_eq!(command.runtime, Some(PathBuf::from("libnwnrs_runtime.so")));
        assert_eq!(
            command.targets,
            Some(PathBuf::from("crates/runtime/targets"))
        );
        assert_eq!(command.color, super::ColorMode::Auto);
        assert!(!command.no_tail_logs);
        assert_eq!(
            command.arguments,
            vec!["/opt/nwn/nwserver", "-module", "nwnrs"]
        );
    }

    #[test]
    #[cfg(feature = "supervisor")]
    fn parses_docker_run_without_native_artifacts() {
        let cli = Cli::from_args(
            &["nwnrs"],
            &[
                "run",
                "--docker",
                "--docker-name",
                "my-server",
                "--docker-publish",
                "127.0.0.1:5121:5121/udp",
                "--docker-arg",
                "pull=always",
                "--",
                "-module",
                "custom",
            ],
        )
        .unwrap_or_else(|error| panic!("parse Docker run args: {error:?}"));

        let Command::Run(command) = cli.command else {
            panic!("expected run command");
        };
        assert!(command.docker);
        assert_eq!(command.docker_name.as_deref(), Some("my-server"));
        assert_eq!(command.docker_publish, vec!["127.0.0.1:5121:5121/udp"]);
        assert_eq!(command.docker_arg, vec!["pull=always"]);
        assert_eq!(command.runtime, None);
        assert_eq!(command.targets, None);
        assert_eq!(command.arguments, vec!["-module", "custom"]);
    }

    #[test]
    fn parses_compile_command_with_compiler_controls() {
        let cli = Cli::from_args(
            &["nwnrs"],
            &[
                "compile",
                "-g",
                "-R",
                "-y",
                "--include-dir",
                "inc",
                "--optimization-flag",
                "remove-dead-branches",
                "--optimization-flag",
                "meld-instructions",
                "--max-include-depth",
                "32",
                "--graphviz",
                "graphs",
                "-d",
                "build",
                "scripts",
            ],
        )
        .unwrap_or_else(|error| panic!("parse compile args: {error:?}"));

        let Command::Compile(cmd) = cli.command else {
            panic!("expected compile command");
        };
        assert!(cmd.debug);
        assert!(cmd.recurse);
        assert!(cmd.continue_on_error);
        assert_eq!(cmd.optimization, "O1");
        assert_eq!(
            cmd.optimization_flag,
            vec!["remove-dead-branches", "meld-instructions"]
        );
        assert_eq!(cmd.max_include_depth, 32);
        assert_eq!(cmd.directory, Some(PathBuf::from("build")));
        assert_eq!(cmd.graphviz, Some(PathBuf::from("graphs")));
        assert_eq!(cmd.graphviz_format, "svg");
        assert_eq!(cmd.paths, vec![PathBuf::from("scripts")]);
    }

    #[test]
    fn parses_pack_command_with_repeated_include_dirs() {
        let cli = Cli::from_args(
            &["nwnrs"],
            &[
                "pack",
                "--debug",
                "--no-entrypoint-check",
                "--include-dir",
                "inc/a",
                "--include-dir",
                "inc/b",
                "--optimization",
                "O2",
                "--optimization-flag",
                "remove-dead-code",
                "-j",
                "4",
                "scripts/test.nss",
                "out/test.ncs",
            ],
        )
        .unwrap_or_else(|error| panic!("parse pack args: {error:?}"));

        let Command::Pack(cmd) = cli.command else {
            panic!("expected pack command");
        };
        assert!(cmd.debug);
        assert!(cmd.no_entrypoint_check);
        assert_eq!(cmd.optimization, "O2");
        assert_eq!(cmd.optimization_flag, vec!["remove-dead-code"]);
        assert_eq!(cmd.jobs, Some(4));
        assert_eq!(
            cmd.include_dir,
            vec![PathBuf::from("inc/a"), PathBuf::from("inc/b")]
        );
        assert_eq!(
            cmd.paths,
            vec![
                PathBuf::from("scripts/test.nss"),
                PathBuf::from("out/test.ncs")
            ]
        );
    }

    #[test]
    fn parses_init_command_with_kind_and_path() {
        let cli = Cli::from_args(&["nwnrs"], &["init", "--kind", "utc", "project"])
            .unwrap_or_else(|error| panic!("parse init args: {error:?}"));

        let Command::Init(cmd) = cli.command else {
            panic!("expected init command");
        };
        assert_eq!(cmd.kind.as_deref(), Some("utc"));
        assert_eq!(cmd.path, Some(PathBuf::from("project")));
    }

    #[test]
    fn parses_new_command_with_kind_and_path() {
        let cli = Cli::from_args(&["nwnrs"], &["new", "--kind", "mod", "my_mod"])
            .unwrap_or_else(|error| panic!("parse new args: {error:?}"));

        let Command::New(cmd) = cli.command else {
            panic!("expected new command");
        };
        assert_eq!(cmd.kind.as_deref(), Some("mod"));
        assert_eq!(cmd.path, PathBuf::from("my_mod"));
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
        assert_eq!(cmd.root, None);
        assert_eq!(cmd.user, None);
        assert_eq!(cmd.language, "english");
        assert!(!cmd.load_ovr);
        assert_eq!(cmd.animation, None);
        assert_eq!(cmd.time, None);
        assert!(!cmd.list_animations);
        assert!(!cmd.preserve_compiled_source);
        assert_eq!(cmd.input, PathBuf::from("models/input.mdl"));
        assert_eq!(cmd.output, Some(PathBuf::from("models/output.mdl")));
    }

    #[test]
    fn parses_convert_compiled_source_opt_in() {
        let cli = Cli::from_args(
            &["nwnrs"],
            &[
                "convert",
                "--preserve-compiled-source",
                "models/input.mdl",
                "models/output.mdl",
            ],
        )
        .unwrap_or_else(|error| panic!("parse convert args: {error:?}"));

        let Command::Convert(cmd) = cli.command else {
            panic!("expected convert command");
        };
        assert!(cmd.preserve_compiled_source);
    }

    #[test]
    fn parses_convert_command_for_animation_listing_without_output() {
        let cli = Cli::from_args(
            &["nwnrs"],
            &[
                "convert",
                "--list-animations",
                "--animation",
                "default",
                "--time",
                "0.0",
                "models/input.mdl",
            ],
        )
        .unwrap_or_else(|error| panic!("parse convert args: {error:?}"));

        let Command::Convert(cmd) = cli.command else {
            panic!("expected convert command");
        };
        assert!(cmd.list_animations);
        assert_eq!(cmd.animation.as_deref(), Some("default"));
        assert_eq!(cmd.time, Some(0.0));
        assert_eq!(cmd.input, PathBuf::from("models/input.mdl"));
        assert_eq!(cmd.output, None);
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
            ..
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

    #[test]
    fn parses_pack_command_for_generic_packing() {
        let cli = Cli::from_args(
            &["nwnrs"],
            &["pack", "--data-version", "E1", "input", "output.key"],
        )
        .unwrap_or_else(|error| panic!("parse pack args: {error:?}"));

        let Command::Pack(cmd) = cli.command else {
            panic!("expected pack command");
        };

        assert_eq!(cmd.data_version, "E1");
        assert_eq!(
            cmd.paths,
            vec![PathBuf::from("input"), PathBuf::from("output.key")]
        );
        assert_eq!(cmd.root, None);
        assert_eq!(cmd.language, None);
    }

    #[test]
    fn parses_pack_command_for_install_packaging() {
        let cli = Cli::from_args(
            &["nwnrs"],
            &[
                "pack",
                "-f",
                "--root",
                "/srv/nwn",
                "--language",
                "en",
                "--data-version",
                "E1",
                "--data-compression",
                "zstd",
                "custom_base.key",
                "docker/data/data",
            ],
        )
        .unwrap_or_else(|error| panic!("parse pack args: {error:?}"));

        let Command::Pack(cmd) = cli.command else {
            panic!("expected pack command");
        };

        assert!(cmd.force);
        assert_eq!(cmd.root, Some(PathBuf::from("/srv/nwn")));
        assert_eq!(cmd.language, Some("en".to_string()));
        assert_eq!(cmd.data_version, "E1");
        assert_eq!(cmd.data_compression, "zstd");
        assert_eq!(
            cmd.paths,
            vec![
                PathBuf::from("custom_base.key"),
                PathBuf::from("docker/data/data")
            ]
        );
    }
}

#[cfg(all(test, feature = "supervisor", not(feature = "tooling")))]
mod supervisor_tests {
    use std::path::PathBuf;

    use argh::FromArgs;

    use super::{Cli, Command};

    #[test]
    fn parses_run_command_and_preserves_server_arguments() {
        let cli = Cli::from_args(
            &["nwnrs"],
            &[
                "run",
                "--runtime",
                "libnwnrs_runtime.so",
                "--targets",
                "crates/runtime/targets",
                "--",
                "/opt/nwn/nwserver",
                "-module",
                "nwnrs",
            ],
        )
        .unwrap_or_else(|error| panic!("parse run args: {error:?}"));

        let Command::Run(command) = cli.command;
        assert_eq!(command.runtime, PathBuf::from("libnwnrs_runtime.so"));
        assert_eq!(command.targets, PathBuf::from("crates/runtime/targets"));
        assert_eq!(command.color, super::ColorMode::Auto);
        assert!(!command.no_tail_logs);
        assert_eq!(
            command.arguments,
            vec!["/opt/nwn/nwserver", "-module", "nwnrs"]
        );
    }

    #[test]
    fn rejects_docker_mode_in_supervisor_only_build() {
        let result = Cli::from_args(&["nwnrs"], &["run", "--docker"]);
        let Err(error) = result else {
            panic!("supervisor-only build unexpectedly accepted --docker");
        };
        assert!(error.output.contains("--docker"));
    }
}
