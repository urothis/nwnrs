mod associate;
mod dispatch;
mod feat;
mod inventory;
mod item;
mod journal;
mod module;
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

fn require_player_game_object(
    engine: &Engine,
    hook_keys: &[&str],
) -> Result<(), BridgeInstallError> {
    if hook_keys
        .iter()
        .any(|key| engine.event_hook_target(key).is_some())
        && engine
            .event_function_target("player_get_game_object")
            .is_none()
    {
        return Err(BridgeInstallError::new(
            "player event hooks require events.functions.player_get_game_object",
        ));
    }
    Ok(())
}
