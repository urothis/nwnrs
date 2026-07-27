//! Generates uncommitted TypeScript contracts owned by Rust serde types.

use std::{env, fs, io, path::PathBuf};

use nwnrs_types::scene::{
    AreaObjectInspection, SceneAnimationPacketManifest, SceneAreaObject, ScenePacketManifest,
    SceneTexturePacketManifest,
};
use nwnrs_vscode_native::{
    RESOURCE_EDITOR_MUTATION_METHODS,
    resource_contracts::{
        JsonValue, ResourceBackupResult, ResourceBinaryEditResult, ResourceCloseResult,
        ResourceEdit, ResourceEditResult, ResourceGffNodePage, ResourceSaveAsPlan,
        ResourceSaveResult, ResourceSnapshot,
    },
};
use ts_rs::{Config, TS};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = env::args_os().nth(1).map(PathBuf::from).ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "output directory is required")
    })?;
    fs::create_dir_all(&output)?;
    let configuration = Config::default().with_out_dir(&output);
    AreaObjectInspection::export_all(&configuration)?;
    SceneAreaObject::export_all(&configuration)?;
    ScenePacketManifest::export_all(&configuration)?;
    SceneAnimationPacketManifest::export_all(&configuration)?;
    SceneTexturePacketManifest::export_all(&configuration)?;
    JsonValue::export_all(&configuration)?;
    ResourceEdit::export_all(&configuration)?;
    ResourceGffNodePage::export_all(&configuration)?;
    ResourceSnapshot::export_all(&configuration)?;
    ResourceEditResult::export_all(&configuration)?;
    ResourceBinaryEditResult::export_all(&configuration)?;
    ResourceSaveResult::export_all(&configuration)?;
    ResourceSaveAsPlan::export_all(&configuration)?;
    ResourceBackupResult::export_all(&configuration)?;
    ResourceCloseResult::export_all(&configuration)?;

    let mut modules = fs::read_dir(&output)?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            (path.extension().and_then(|value| value.to_str()) == Some("ts"))
                .then(|| path.file_stem()?.to_str().map(str::to_owned))
                .flatten()
        })
        .filter(|name| name != "index" && name != "contract" && !name.contains('.'))
        .collect::<Vec<_>>();
    modules.sort();
    let barrel = modules
        .iter()
        .map(|module| format!("export type {{ {module} }} from './{module}';\n"))
        .collect::<String>();
    fs::write(output.join("index.ts"), barrel)?;
    let mut webview = String::from(
        "// Generated from Rust serde contracts by nwnrs-vscode-native. Do not edit.\n\n",
    );
    for module in &modules {
        let source = fs::read_to_string(output.join(format!("{module}.ts")))?;
        for line in source.lines() {
            if line.starts_with("// This file was generated") || line.starts_with("import type ") {
                continue;
            }
            webview.push_str(line.strip_prefix("export ").unwrap_or(line));
            webview.push('\n');
        }
        webview.push('\n');
    }
    webview.push_str(
        "// Renderer-only name for the effective material binding carried by scene packets.\ntype \
         SceneTextureBinding = SceneMaterialTexture;\n",
    );
    fs::write(output.join("webview.d.ts"), webview)?;
    fs::write(
        output.join("contract.ts"),
        format!(
            "export const EXPECTED_NATIVE_CONTRACT_FINGERPRINT = {:?} as const;\nexport const \
             RESOURCE_EDITOR_MUTATION_METHODS = {:?} as const;\n",
            env!("NWNRS_CONTRACT_FINGERPRINT"),
            RESOURCE_EDITOR_MUTATION_METHODS,
        ),
    )?;
    Ok(())
}
