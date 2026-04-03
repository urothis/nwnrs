use argh::FromArgs;
use std::path::PathBuf;

#[derive(FromArgs)]
/// Neverwinter Nights utility CLI.
pub(crate) struct Cli {
    #[argh(subcommand)]
    pub(crate) command: Command,
}

#[derive(FromArgs)]
#[argh(subcommand)]
pub(crate) enum Command {
    Inspect(InspectCmd),
    Pack(PackCmd),
    Unpack(UnpackCmd),
    Nwsync(NwsyncCmd),
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
/// unpack a resource into a directory or source-like text form
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
/// pack a source file or directory into NWN binary form
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
    pub(crate) data_version: String,
    pub(crate) data_compression: String,
    pub(crate) no_squash: bool,
    pub(crate) no_symlinks: bool,
    pub(crate) force: bool,
    pub(crate) key: String,
    pub(crate) source: PathBuf,
    pub(crate) destination: PathBuf,
}

pub(crate) struct KeyUnpackCmd {
    pub(crate) force: bool,
    pub(crate) key: PathBuf,
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
