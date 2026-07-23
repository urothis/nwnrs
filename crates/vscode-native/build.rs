//! Node-API linker setup and generated resource capability tables.

use std::{
    collections::BTreeSet,
    env, fs,
    io::{self, Write as _},
    path::PathBuf,
};

use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResourceCapability {
    extension:     String,
    handler:       String,
    custom_editor: bool,
    viewer:        bool,
    text:          bool,
    writable:      bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    napi_build::setup();
    generate_resource_capabilities()
}

fn generate_resource_capabilities() -> Result<(), Box<dyn std::error::Error>> {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "Cargo did not provide CARGO_MANIFEST_DIR",
        )
    })?);
    let registry = manifest_dir
        .join("..")
        .join("..")
        .join("editors")
        .join("vscode-nwnrs")
        .join("resource-capabilities.json");
    writeln!(
        io::stdout().lock(),
        "cargo:rerun-if-changed={}",
        registry.display()
    )?;
    let bytes = fs::read(&registry)?;
    let capabilities: Vec<ResourceCapability> = serde_json::from_slice(&bytes)?;
    let mut extensions = BTreeSet::new();
    for capability in &capabilities {
        if capability.extension.is_empty()
            || !capability
                .extension
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'.')
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "invalid resource capability extension: {}",
                    capability.extension
                ),
            )
            .into());
        }
        if !extensions.insert(capability.extension.as_str()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "duplicate resource capability extension: {}",
                    capability.extension
                ),
            )
            .into());
        }
        if !matches!(
            capability.handler.as_str(),
            "2da"
                | "tlk"
                | "dds"
                | "tga"
                | "plt"
                | "gff"
                | "erf"
                | "key"
                | "ncs"
                | "ndb"
                | "viewer"
                | "text"
        ) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "resource .{} has unknown handler {}",
                    capability.extension, capability.handler
                ),
            )
            .into());
        }
        if capability.viewer && !capability.custom_editor {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "viewer resource .{} must contribute the custom editor",
                    capability.extension
                ),
            )
            .into());
        }
        if capability.text && capability.custom_editor {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "text resource .{} cannot also contribute the custom editor",
                    capability.extension
                ),
            )
            .into());
        }
        if capability.viewer && capability.writable {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "viewer resource .{} cannot be marked writable",
                    capability.extension
                ),
            )
            .into());
        }
        if matches!(capability.handler.as_str(), "ncs" | "ndb") && capability.writable {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "script workbench resource .{} cannot be marked writable",
                    capability.extension
                ),
            )
            .into());
        }
    }

    let mut generated = String::from(
        "// Generated from editors/vscode-nwnrs/resource-capabilities.json.\n// Do not edit by \
         hand.\n\npub(crate) fn resource_handler(extension: &str) -> Option<&'static str> \
         {\n\tmatch extension {\n",
    );
    for capability in &capabilities {
        generated.push_str(&format!(
            "\t\t{:?} => Some({:?}),\n",
            capability.extension, capability.handler
        ));
    }
    generated.push_str("\t\t_ => None,\n\t}\n}\n\n");
    generated.push_str(
        "pub(crate) fn is_gff_extension(extension: &str) -> bool {\n\tmatches!(extension,\n",
    );
    let mut gff = capabilities
        .iter()
        .filter(|capability| capability.handler == "gff" && !capability.extension.contains('.'))
        .peekable();
    while let Some(capability) = gff.next() {
        generated.push_str(&format!(
            "\t\t{:?}{}\n",
            capability.extension,
            if gff.peek().is_some() { " |" } else { "" }
        ));
    }
    generated.push_str("\t)\n}\n");

    let output =
        PathBuf::from(env::var_os("OUT_DIR").ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "Cargo did not provide OUT_DIR")
        })?)
        .join("resource_capabilities.rs");
    fs::write(output, generated)?;
    Ok(())
}
