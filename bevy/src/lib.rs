#![forbid(unsafe_code)]
//! Bevy integration for Neverwinter Nights assets.
//!
//! Phase 1 intentionally focuses on a narrow vertical slice:
//!
//! - load NWN `mdl` assets through `nwnrs-mdl`
//! - convert static mesh primitives into Bevy `Mesh` assets
//! - decode NWN `dds` / `tga` textures into Bevy `Image` assets
//! - build basic `StandardMaterial` assets
//! - expose a spawn helper for the loaded model hierarchy

mod appearance;
mod assets;
mod convert;
mod error;
mod install;
mod install_state;
mod loader;
mod plugin;
mod runtime;
mod spawn;

pub use appearance::*;
pub use assets::*;
pub use convert::*;
pub use error::*;
pub use install::*;
pub use loader::*;
pub use plugin::*;
pub use runtime::*;
pub use spawn::*;
