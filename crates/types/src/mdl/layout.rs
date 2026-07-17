//! Shared constants for the BioWare compiled MDL layout.

pub(crate) const FILE_HEADER_SIZE: usize = 12;
pub(crate) const MODEL_HEADER_SIZE: usize = 232;
pub(crate) const NODE_HEADER_SIZE: usize = 112;
pub(crate) const LIGHT_HEADER_SIZE: usize = 92;
pub(crate) const EMITTER_HEADER_SIZE: usize = 216;
pub(crate) const REFERENCE_HEADER_SIZE: usize = 68;
pub(crate) const MESH_HEADER_SIZE: usize = 512;
#[allow(clippy::cast_possible_truncation)]
pub(crate) const MESH_HEADER_SIZE_U32: u32 = MESH_HEADER_SIZE as u32;
pub(crate) const SKIN_HEADER_SIZE: usize = 192;
pub(crate) const ANIM_HEADER_SIZE: usize = 56;
pub(crate) const DANGLY_HEADER_SIZE: usize = 24;
pub(crate) const AABB_HEADER_SIZE: usize = 4;
pub(crate) const CONTROLLER_SIZE: usize = 12;
pub(crate) const FACE_SIZE: usize = 32;
pub(crate) const EVENT_SIZE: usize = 36;
pub(crate) const AABB_ENTRY_SIZE: usize = 40;
