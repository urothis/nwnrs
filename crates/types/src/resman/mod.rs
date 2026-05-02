#![forbid(unsafe_code)]
#![doc = include_str!("README.md")]

mod manager;
mod read;
mod resdir;
mod resfile;
mod resmemfile;
mod resref_parse;
mod resref_types;
mod restype_registry;
mod restype_types;
mod types;

pub use manager::*;
pub use read::*;
pub use resdir::*;
pub use resfile::*;
pub use resmemfile::*;
pub use resref_parse::*;
pub use resref_types::*;
pub use restype_registry::*;
pub use restype_types::*;
pub use types::*;

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::resman::{
        CachePolicy, MEMORY_CACHE_THRESHOLD, RESREF_MAX_LENGTH, ReadSeek, RegisterResTypeError,
        Res, ResContainer, ResDir, ResDirError, ResDirResult, ResFile, ResFileError, ResFileResult,
        ResIoSpawner, ResMan, ResManError, ResManResult, ResMemFile, ResMemFileError,
        ResMemFileResult, ResOrigin, ResRef, ResRefError, ResType, ResolvedResRef, SharedReadSeek,
        get_res_ext, get_res_type, is_valid_resref_part1, lookup_res_ext, lookup_res_type,
        new_res_origin, read_resdir, read_resfile, read_resfile_as, read_resmemfile,
        read_resmemfile_arc, register_custom_res_type, res_ext_registered, res_type_registered,
        shared_stream,
    };
}
