use bevy::{
    app::Plugin,
    asset::AssetApp,
    prelude::{App, Update},
};

use crate::{
    NwnAreaWind, NwnMdlAssetLoader, NwnModelAsset,
    animation::{
        animate_nwn_model_materials, animate_nwn_model_meshes, animate_nwn_model_transforms,
        animate_nwn_txi_materials,
    },
    light::animate_nwn_model_lights,
    visibility::update_nwn_tilefade_visibility,
};

/// Registers the Bevy-side NWN asset types and loaders.
#[derive(Debug, Default, Clone, Copy)]
pub struct NwnBevyPlugin;

impl Plugin for NwnBevyPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<NwnModelAsset>()
            .init_resource::<NwnAreaWind>()
            .init_asset_loader::<NwnMdlAssetLoader>()
            .add_systems(
                Update,
                (
                    animate_nwn_model_transforms,
                    animate_nwn_model_meshes,
                    animate_nwn_model_materials,
                    animate_nwn_txi_materials,
                    animate_nwn_model_lights,
                    update_nwn_tilefade_visibility,
                ),
            );
    }
}
