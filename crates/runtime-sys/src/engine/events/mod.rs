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

pub(crate) fn bootstrap_hook_specs(
    engine: &Engine,
) -> Result<Vec<NativeHookSpec>, BridgeInstallError> {
    let mut hooks = Vec::new();
    module::append_hook_specs(engine, &mut hooks);
    Ok(hooks)
}
