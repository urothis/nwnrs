//! Typed in-process Node-API services for the nwnrs editor.

mod language_service;
pub mod resource_contracts;
mod resource_editor;
mod viewer;

pub use resource_editor::RESOURCE_EDITOR_MUTATION_METHODS;

mod resource_capabilities {
    include!(concat!(env!("OUT_DIR"), "/resource_capabilities.rs"));
}
