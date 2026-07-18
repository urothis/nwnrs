#![forbid(unsafe_code)]
#![doc = include_str!("README.md")]

mod git;
mod io;
mod json;
mod merge;
mod types;

pub use git::*;
pub use io::*;
pub use json::*;
pub use merge::*;
pub use types::*;

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::gff::{
        GIT_RES_TYPE, GffCExoLocString, GffError, GffField, GffFieldKind, GffResult, GffRoot,
        GffStruct, GffValue, GitAreaProperties, GitCreature, GitDoor, GitEncounter, GitError,
        GitFile, GitPlaceable, GitPoint, GitResult, GitSound, GitSoundRef, GitStore, GitTransform,
        GitTrigger, GitWaypoint, build_git_root, gff_root_from_json, gff_root_from_json_bytes,
        gff_root_to_json, gff_root_to_json_bytes, merge_root_preserving_provenance, parse_git_root,
        read_gff_root, read_git, write_gff_root, write_git,
    };
}
