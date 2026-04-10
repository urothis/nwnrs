use bevy::{app::Plugin, asset::AssetApp, prelude::App};

use crate::{NwnMdlAssetLoader, NwnModelAsset};

/// Registers the Bevy-side NWN asset types and loaders.
#[derive(Debug, Default, Clone, Copy)]
pub struct NwnBevyPlugin;

impl Plugin for NwnBevyPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<NwnModelAsset>()
            .init_asset_loader::<NwnMdlAssetLoader>();
    }
}
