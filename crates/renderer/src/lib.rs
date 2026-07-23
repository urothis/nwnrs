#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

mod assets;
mod dependency;
mod error;
mod loader;
mod packet;
mod scene;

pub use dependency::*;
pub use error::*;
pub use loader::*;
pub use packet::*;
pub use scene::*;

/// Common renderer imports.
pub mod prelude {
    pub use crate::{
        AreaScene, DependencyEdge, DependencyGraph, DependencyKind, DependencyNode,
        DependencyState, ModelScene, RenderAreaObject, RenderDiagnostic, RenderDiagnosticSeverity,
        RenderEnvironment, RenderInstance, RenderInstanceKind, RenderScene, RendererError,
        RendererResult, SceneLoader, ScenePacket, ScenePacketManifest, SceneSource,
    };
}
