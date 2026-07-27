#![doc = include_str!("README.md")]

mod assets;
mod dependency;
mod error;
mod inspection;
mod loader;
mod packet;
mod types;

pub use dependency::*;
pub use error::*;
pub use inspection::*;
pub use loader::*;
pub use packet::*;
pub use types::*;

/// Common scene-system imports.
pub mod prelude {
    pub use crate::scene::{
        AreaInspectionCache, AreaInspector, AreaObjectInspection, DependencyEdge, DependencyGraph,
        DependencyKind, DependencyNode, DependencyState, InspectionField, InspectionSection,
        SceneArea, SceneAreaObject, SceneDiagnostic, SceneDiagnosticSeverity, SceneDocument,
        SceneEnvironment, SceneError, SceneInstance, SceneInstanceKind, SceneLoader, SceneModel,
        ScenePacket, ScenePacketManifest, SceneResult, SceneSource,
    };
}
