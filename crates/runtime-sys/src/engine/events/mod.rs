mod associate;
mod dispatch;
mod feat;
mod inventory;
mod item;
mod journal;
mod module;
mod native;
mod object;
mod skill;
mod timing_bar;

use super::{Engine, hook::NativeHookSpec};
use crate::bridge::BridgeInstallError;

pub(crate) fn hook_specs(engine: &Engine) -> Result<Vec<NativeHookSpec>, BridgeInstallError> {
    unfinished_platform_event_targets(engine);
    let mut hooks = Vec::new();
    module::append_hook_specs(engine, &mut hooks);
    associate::append_hook_specs(engine, &mut hooks)?;
    object::append_hook_specs(engine, &mut hooks);
    inventory::append_hook_specs(engine, &mut hooks);
    feat::append_hook_specs(engine, &mut hooks);
    skill::append_hook_specs(engine, &mut hooks);
    item::append_hook_specs(engine, &mut hooks);
    journal::append_hook_specs(engine, &mut hooks)?;
    timing_bar::append_hook_specs(engine, &mut hooks)?;
    Ok(hooks)
}

#[cfg(target_os = "macos")]
fn unfinished_platform_event_targets(_engine: &Engine) {}

#[cfg(target_os = "linux")]
fn unfinished_platform_event_targets(engine: &Engine) {
    if MACOS_EVENT_HOOKS
        .iter()
        .any(|hook| engine.event_hook_target(hook).is_some())
    {
        todo!("verify and enable the expanded event hooks in the Linux target pack");
    }
}

#[cfg(target_os = "windows")]
fn unfinished_platform_event_targets(engine: &Engine) {
    if MACOS_EVENT_HOOKS
        .iter()
        .any(|hook| engine.event_hook_target(hook).is_some())
    {
        todo!("verify and enable the expanded event hooks in the Windows target pack");
    }
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
const MACOS_EVENT_HOOKS: &[&str] = &[
    "object_set_experience",
    "feat_decrement_remaining_uses",
    "feat_has",
    "inventory_message",
    "inventory_add_item",
    "inventory_remove_item",
    "item_validate_use",
    "item_ammo_reload",
    "item_validate_equip",
    "item_equip",
    "item_unequip",
    "item_split",
    "item_merge",
    "item_acquire",
];

pub(crate) fn bootstrap_hook_specs(
    engine: &Engine,
) -> Result<Vec<NativeHookSpec>, BridgeInstallError> {
    let mut hooks = Vec::new();
    module::append_hook_specs(engine, &mut hooks);
    Ok(hooks)
}
