use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The semantic role of a resource dependency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DependencyKind {
    /// Root resource opened by the user.
    Root,
    /// Render model.
    Model,
    /// Tile, door, or placeable walkmesh.
    Walkmesh,
    /// Supermodel providing inherited animations.
    Supermodel,
    /// Referenced child model.
    ReferenceModel,
    /// Blueprint resource.
    Blueprint,
    /// Area definition.
    Area,
    /// Area instance list.
    AreaInstances,
    /// Module information.
    Module,
    /// Tileset catalog.
    Tileset,
    /// Two-dimensional lookup table.
    TwoDa,
    /// Material descriptor.
    Material,
    /// Texture metadata sidecar.
    TextureInfo,
    /// Texture image.
    Texture,
    /// Shader source.
    Shader,
}

/// Resolution state of one dependency node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DependencyState {
    /// Resource resolved successfully.
    Resolved,
    /// Resource is optional and was not present.
    OptionalMissing,
    /// Required resource is missing.
    Missing,
    /// Resource was found but could not be decoded.
    Invalid,
}

/// One resource in a scene dependency graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencyNode {
    /// Stable node index.
    pub id:       usize,
    /// Canonical `resref.ext` identity.
    pub resource: String,
    /// Resource role.
    pub kind:     DependencyKind,
    /// Resolution outcome.
    pub state:    DependencyState,
    /// Container and container-local origin, when resolved.
    pub origin:   Option<String>,
    /// Detailed failure text for missing or invalid resources.
    pub message:  Option<String>,
}

/// A directed dependency relationship.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencyEdge {
    /// Referring resource node.
    pub from:         usize,
    /// Referenced resource node.
    pub to:           usize,
    /// Human-readable relationship such as `texture0` or `supermodel`.
    pub relationship: String,
}

/// Deduplicated scene dependency graph with resolution provenance.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencyGraph {
    /// Resource nodes.
    pub nodes: Vec<DependencyNode>,
    /// Directed relationships.
    pub edges: Vec<DependencyEdge>,
    #[serde(skip)]
    indices:   BTreeMap<String, usize>,
}

impl DependencyGraph {
    /// Returns the stable id of an already-recorded resource.
    #[must_use]
    pub fn id_for(&self, resource: &str) -> Option<usize> {
        self.indices.get(&resource.to_ascii_lowercase()).copied()
    }

    /// Inserts or updates a dependency and returns its stable id.
    pub fn record(
        &mut self,
        resource: impl Into<String>,
        kind: DependencyKind,
        state: DependencyState,
        origin: Option<String>,
        message: Option<String>,
    ) -> usize {
        let resource = resource.into();
        let key = resource.to_ascii_lowercase();
        if let Some(id) = self.indices.get(&key).copied() {
            if let Some(node) = self.nodes.get_mut(id) {
                node.kind = kind;
                node.state = state;
                if origin.is_some() {
                    node.origin = origin;
                }
                if message.is_some() {
                    node.message = message;
                }
            }
            return id;
        }
        let id = self.nodes.len();
        self.nodes.push(DependencyNode {
            id,
            resource,
            kind,
            state,
            origin,
            message,
        });
        self.indices.insert(key, id);
        id
    }

    /// Adds a dependency edge unless an identical relationship already exists.
    pub fn connect(&mut self, from: usize, to: usize, relationship: impl Into<String>) {
        let relationship = relationship.into();
        if !self
            .edges
            .iter()
            .any(|edge| edge.from == from && edge.to == to && edge.relationship == relationship)
        {
            self.edges.push(DependencyEdge {
                from,
                to,
                relationship,
            });
        }
    }
}
