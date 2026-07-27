#![forbid(unsafe_code)]
#![doc = include_str!("README.md")]

mod area;
mod git;
mod io;
mod json;
mod merge;
mod types;

pub use area::*;
pub use git::*;
pub use io::*;
pub use json::*;
pub use merge::*;
pub use types::*;

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::gff::{
        ARE_RES_TYPE, AreEnvironment, AreFile, AreTile, GIT_RES_TYPE, GffCExoLocString, GffError,
        GffField, GffFieldKind, GffResult, GffRoot, GffStruct, GffValue, GitAreaProperties,
        GitCreature, GitDoor, GitEncounter, GitError, GitFile, GitPlaceable, GitPoint, GitResult,
        GitSound, GitSoundRef, GitStore, GitTransform, GitTrigger, GitWaypoint, IFO_RES_TYPE,
        ModuleEntryPoint, ModuleInfo, build_git_root, gff_root_from_json, gff_root_from_json_bytes,
        gff_root_to_json, gff_root_to_json_bytes, merge_root_preserving_provenance, parse_are_root,
        parse_git_root, parse_module_info_root, read_are, read_gff_root, read_git,
        read_module_info, write_gff_root, write_git,
    };
}
