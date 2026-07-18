#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

mod fs;
mod kind;
mod lock;
mod manifest;

pub use fs::is_project_control_file;
pub use kind::{ProjectKind, ProjectLayout};
pub use lock::{
    ErfPackMetadata, KeyPackMetadata, ResourcePackMetadata, copy_original_key_set,
    read_erf_pack_metadata, read_key_pack_metadata, read_resource_pack_metadata,
    resolve_existing_key_bif_path, should_copy_original_erf, should_copy_original_key,
    should_copy_original_resource, write_erf_pack_metadata, write_key_pack_metadata,
    write_new_erf_pack_metadata, write_new_key_pack_metadata, write_new_resource_pack_metadata,
    write_resource_pack_metadata,
};
pub use manifest::{ProjectManifest, read_project_manifest, write_project_manifest};

/// Canonical `nwproject.toml` filename.
pub const PROJECT_MANIFEST_FILENAME: &str = "nwproject.toml";

/// Canonical `nwpkg.lock` filename.
pub const PACKAGE_LOCK_FILENAME: &str = "nwpkg.lock";
